//! This module takes care of the configuration of the storage device used by LEAP.
//!
//! Public functions:
//!  - [`list_blockdevs`] returns a list of block devices currently attached to the system.
//!  - [`prepare_storage_medium`] formats the requested block device as ext4 and makes sure it is
//!    mounted.

use std::path::{Path, PathBuf};

use anyhow::Context;
use serde::{Deserialize, Serialize};
use tokio::process::Command;

use crate::provision::MOUNT_PATH;

fn default_children_node() -> Vec<BlockDevice> {
    vec![]
}

/// Represents the type of a block device.
#[derive(Deserialize, Serialize, Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockDeviceType {
    /// A physical disk.
    #[serde(rename = "disk")]
    Disk,

    /// A partition on a disk.
    #[serde(rename = "part")]
    Partition,
}

/// Represents a block device and its associated metadata.
///
/// This struct is used to represent the information returned by `lsblk`.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct BlockDevice {
    /// The kernel name of the block device (e.g., `/dev/sda`).
    #[serde(rename = "kname")]
    pub name: String,

    /// Whether the device is removable.
    #[serde(rename = "rm")]
    pub removable: bool,

    /// Whether the device is read-only.
    #[serde(rename = "ro")]
    pub read_only: bool,

    /// The type of the block device.
    #[serde(rename = "type")]
    pub ty: BlockDeviceType,

    /// The size of the device in bytes.
    pub size: u64,

    /// The mount points where this device is currently mounted.
    pub mountpoints: Vec<String>,

    /// The subsystems that this device belongs to.
    pub subsystems: String,

    /// The child block devices (e.g., partitions of a disk).
    #[serde(default = "default_children_node")]
    pub children: Vec<BlockDevice>,
}

/// The output of the `lsblk` command in JSON format.
#[derive(Deserialize, Serialize, Debug, Clone)]
pub struct LsblkOutput {
    blockdevices: Vec<BlockDevice>,
}

/// Lists all available block devices in the system. Note that block devices may have children.
/// Only disks are present at the root of the hierarchy.
/// Skips internal disks or ephimeral disks like zram, ram, and loop devs.
pub async fn list_blockdevs() -> anyhow::Result<Vec<BlockDevice>> {
    let result = Command::new("lsblk")
        .arg("--json")
        .arg("--bytes")
        .arg("--tree")
        .arg("--paths")
        .arg("-o")
        .arg("kname,type,ro,rm,mountpoints,size,subsystems")
        .output()
        .await?;

    let output: LsblkOutput = serde_json::from_slice(&result.stdout)?;

    // Filter devices
    let output: Vec<_> = output
        .blockdevices
        .into_iter()
        .filter(|d| {
            // RAM and CD-ROMs are not relevant storage mediums
            // SD cards and loop devices are also ignored.
            !d.name.starts_with("/dev/sr")
                && !d.name.starts_with("/dev/ram")
                && !d.name.starts_with("/dev/zram")
                && !d.name.starts_with("/dev/boot")
                && !d.name.starts_with("/dev/mmcblk")
                && !d.name.starts_with("/dev/loop")
        })
        .collect();

    Ok(output)
}

async fn unmount_block_dev(block_dev: &BlockDevice) -> anyhow::Result<()> {
    for child in &block_dev.children {
        // This future is boxed and pinned because of recursion.
        let fut = Box::pin(unmount_block_dev(child));
        fut.await?;
    }

    for mount_point in &block_dev.mountpoints {
        let path: PathBuf = mount_point.into();
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            tracing::info!("Unmounting: {}", path.display());
            nix::mount::umount(&path)
                .with_context(|| format!("Unable to unmount {}", path.display()))?;
            Ok(())
        })
        .await??;
    }
    Ok(())
}

/// Formats the given storage device at `path` as ext4 and makes sure that it is mounted.
pub async fn prepare_storage_medium(path: &Path) -> anyhow::Result<()> {
    let all_block_devs = list_blockdevs().await?;

    tracing::info!("Preparing storage medium: {}", path.display());

    let block_dev = all_block_devs
        .iter()
        .find(|b| b.name == *path)
        .with_context(|| format!("Block device not found: {}", path.display()))?;

    unmount_block_dev(block_dev).await?;

    tracing::info!("Create ext4 fs for: {}", path.display());

    // Create filesystem
    let mke2fs_result = tokio::process::Command::new("mke2fs")
        .arg("-L")
        // Note that this label is used by the systemd var-lib-leap.mount to know which device to
        // mount to /var/lib/leap. Do NOT change it.
        .arg("LEAP_DATA")
        .arg("-t")
        .arg("ext4")
        .arg(path)
        .output()
        .await?;
    if !mke2fs_result.status.success() || mke2fs_result.status.code() != Some(0) {
        tracing::error!("Failure creating ext4 fs: {mke2fs_result:?}");
        anyhow::bail!("Failure to format storage {mke2fs_result:?}");
    }

    tracing::info!("Ext4 fs created");
    tracing::info!("Mounting file system");

    let path = path.to_owned();
    tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
        nix::mount::mount(
            Some(&path),
            MOUNT_PATH,
            Some("ext4"),
            nix::mount::MsFlags::MS_NOATIME,
            Option::<&str>::None,
        )?;
        Ok(())
    })
    .await??;

    tracing::info!("File system mounted");
    Ok(())
}
