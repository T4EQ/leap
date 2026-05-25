//! Implements the provisioning state machine for the LEAP server.
//!
//! Provisioning is a multi-step process implemented using the type-state pattern to ensure
//! that steps are performed in the correct order. The process consists of:
//!
//! 1. **Storage**: Formatting the external storage used for LEAP.
//! 2. **Networking**: Configuring the networking of LEAP (Wired/Wireless, credentials, DHCP/manual).
//! 3. **Configuration**: Creating and validating the `config.toml` file for LEAP.
//! 4. **Completion**: Triggering a system reboot and saving the provisioning record.
//!
//! ### Type-safe API
//! The [`Provision`] struct uses the type-state pattern to enforce the provisioning sequence
//! at compile time. Each step transitions the machine to a new state.
//!
//! ```rust,no_run
//! # async fn provision_flow() -> anyhow::Result<()> {
//! # let path = todo!();
//! # let network_config = todo!();
//! # let leap_config = todo!();
//! use leap_server::provision::{Provision, StorageStep};
//! // Example of using the type-safe API (pseudo-code)
//! let provision = Provision::<StorageStep>::new().await?;
//! let provision = provision.configure_storage(path).await.unwrap();
//! let provision = provision.configure_network(&network_config).await.unwrap();
//! let provision = provision.configure_leap(&leap_config).await.unwrap();
//! provision.finish().await?;
//! # Ok(())
//! # }
//! ```
//!
//! ### Dynamic API
//! For cases where the state is not known at compile time (e.g., when handling a provisioning
//! process that might have been interrupted), the [`DynProvision`] struct provides a
//! dynamic representation of the state machine.
//!
//! ```rust,no_run
//! # async fn provision_flow() -> anyhow::Result<()> {
//! # let path = todo!();
//! # let network_config = todo!();
//! # let leap_config = todo!();
//! use leap_server::provision::DynProvision;
//! // Example of using the dynamic API
//! let mut provision = DynProvision::new().await?;
//! provision.configure_storage(path).await?;
//! provision.configure_network(&network_config).await?;
//! provision.configure_leap(&leap_config).await?;
//! provision.finish().await?;
//! # Ok(())
//! # }
//! ```

use crate::cfg::LeapConfig;
use leap_api::types::NetworkConfig;
use leap_api::types::ProvisionStatus;

use std::path::{Path, PathBuf};

mod cfg;
mod network;
mod storage;

const MOUNT_PATH: &str = "/var/lib/leap";
const RUNTIME_PATH: &str = "/var/lib/leap/runtime_path";
const CONTENT_PATH: &str = "/var/lib/leap/content_path";
const PROVISION_FILE_PATH: &str = "/var/lib/leap/provision.json";

pub use storage::{BlockDevice, BlockDeviceType};

/// Implementation of the storage step
#[derive(Debug)]
pub struct StorageStep {}

/// Implementation of the network step
#[derive(Debug)]
pub struct NetworkStep {
    storage_node: PathBuf,
}

/// Implementation of the LEAP configuration step
#[derive(Debug)]
pub struct LeapConfigStep {
    storage_node: PathBuf,
    network_config: NetworkConfig,
}

/// Implementation of the completion step
#[derive(Debug, serde::Serialize)]
pub struct CompleteStep {
    storage_node: PathBuf,
    network_config: NetworkConfig,
    configuration: LeapConfig,
}

/// Sealed marker trait
mod private {
    pub trait ProvisionStep {}
}
use private::ProvisionStep;

impl ProvisionStep for NetworkStep {}
impl ProvisionStep for StorageStep {}
impl ProvisionStep for LeapConfigStep {}
impl ProvisionStep for CompleteStep {}

/// A type-safe state machine for the provisioning process.
///
/// The `Step` type parameter represents the current state of the provisioning process.
/// This ensures that only valid transitions can be made at compile time.
#[derive(Debug)]
pub struct Provision<Step: ProvisionStep> {
    inner: Step,
}

impl<S: ProvisionStep> Provision<S> {
    /// Constructs the Provision in its default state. Initialize the network for provisioning
    pub async fn new() -> anyhow::Result<Provision<StorageStep>> {
        network::start_provision_network().await?;
        Ok(Provision {
            inner: StorageStep {},
        })
    }

    /// Listing the block devices can be done at any step
    pub async fn list_blockdevs(&self) -> anyhow::Result<Vec<storage::BlockDevice>> {
        storage::list_blockdevs().await
    }
}

impl Provision<StorageStep> {
    /// Formats the external storage used for LEAP.
    pub async fn configure_storage(
        self,
        path: &Path,
    ) -> Result<Provision<NetworkStep>, (anyhow::Error, Provision<StorageStep>)> {
        if let Err(err) = storage::prepare_storage_medium(path).await {
            Err((err, self))
        } else {
            Ok(Provision {
                inner: NetworkStep {
                    storage_node: path.to_path_buf(),
                },
            })
        }
    }
}

impl Provision<NetworkStep> {
    /// Reverts to the previous step.
    pub async fn revert(self) -> Provision<StorageStep> {
        // TODO: need to roll back some storage action?
        Provision {
            inner: StorageStep {},
        }
    }

    /// Configures the network for LEAP.
    pub async fn configure_network(
        self,
        network_config: &NetworkConfig,
    ) -> Result<Provision<LeapConfigStep>, (anyhow::Error, Provision<NetworkStep>)> {
        match network::test_and_create_network_config(network_config).await {
            Ok(()) => Ok(Provision {
                inner: LeapConfigStep {
                    storage_node: self.inner.storage_node,
                    network_config: network_config.clone(),
                },
            }),
            Err(err) => Err((err, self)),
        }
    }
}

impl Provision<LeapConfigStep> {
    /// Reverts to the previous step.
    pub async fn revert(self) -> Provision<NetworkStep> {
        Provision {
            inner: NetworkStep {
                storage_node: self.inner.storage_node,
            },
        }
    }

    /// Saves the LEAP configuration to disk after validation.
    pub async fn configure_leap(
        self,
        config: &leap_api::provision::config::post::LeapConfig,
    ) -> Result<Provision<CompleteStep>, (anyhow::Error, Provision<LeapConfigStep>)> {
        match cfg::provision_configuration(config).await {
            Ok(configuration) => Ok(Provision {
                inner: CompleteStep {
                    network_config: self.inner.network_config,
                    storage_node: self.inner.storage_node,
                    configuration,
                },
            }),
            Err(error) => Err((error, self)),
        }
    }
}

impl Provision<CompleteStep> {
    /// Completes the provisioning process, triggering a reboot.
    pub async fn finish(self) -> anyhow::Result<()> {
        // Save a record of the provisioning process
        tokio::fs::write(PROVISION_FILE_PATH, &serde_json::to_vec(&self.inner)?).await?;

        // Try to guarantee that outstandring writes are completed and flushed to disk. Note that
        // posix specifies that `sync` schedules the writes, but does not wait for them. Linux
        // manpages, however, suggest that the I/O is done synchronously. Two syncs are kept just
        // to be on the safe side.
        nix::unistd::sync();
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        nix::unistd::sync();
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        loop {
            nix::sys::reboot::reboot(nix::sys::reboot::RebootMode::RB_AUTOBOOT)?;
        }
    }
}

#[allow(
    clippy::large_enum_variant,
    reason = "No improvement would be made here if we box each variant"
)]
enum DynProvisionImpl {
    Network(Provision<NetworkStep>),
    Storage(Provision<StorageStep>),
    LeapConfig(Provision<LeapConfigStep>),
    Complete(Provision<CompleteStep>),
}

trait ProvisionVariant: ProvisionStep + Sized {
    const CONSTRUCTOR: fn(Provision<Self>) -> DynProvisionImpl;
    const NAME: &str;
}

impl<S> From<Provision<S>> for DynProvisionImpl
where
    S: ProvisionVariant + ProvisionStep,
{
    fn from(value: Provision<S>) -> Self {
        <S as ProvisionVariant>::CONSTRUCTOR(value)
    }
}

impl ProvisionVariant for NetworkStep {
    const CONSTRUCTOR: fn(Provision<Self>) -> DynProvisionImpl = DynProvisionImpl::Network;
    const NAME: &str = "NetworkStep";
}

impl ProvisionVariant for StorageStep {
    const CONSTRUCTOR: fn(Provision<Self>) -> DynProvisionImpl = DynProvisionImpl::Storage;
    const NAME: &str = "StorageStep";
}

impl ProvisionVariant for LeapConfigStep {
    const CONSTRUCTOR: fn(Provision<Self>) -> DynProvisionImpl = DynProvisionImpl::LeapConfig;
    const NAME: &str = "LeapConfigStep";
}

impl ProvisionVariant for CompleteStep {
    const CONSTRUCTOR: fn(Provision<Self>) -> DynProvisionImpl = DynProvisionImpl::Complete;
    const NAME: &str = "CompleteStep";
}

/// A dynamic representation of the provisioning state machine.
///
/// This wraps the type-state machine [`Provision`] in an enum to allow for
/// runtime state transitions. This is useful when the current state of
/// provisioning is not known at compile time.
pub struct DynProvision(Option<DynProvisionImpl>);

impl DynProvision {
    /// Constructs the state machine starting with the storage state.
    pub async fn new() -> anyhow::Result<Self> {
        Ok(DynProvision(Some(DynProvisionImpl::Storage(
            Provision::<StorageStep>::new().await?,
        ))))
    }

    /// Listing the block devices, which can be done at any step
    pub async fn list_blockdevs(&self) -> anyhow::Result<Vec<storage::BlockDevice>> {
        match self.0.as_ref() {
            Some(DynProvisionImpl::Network(p)) => p.list_blockdevs().await,
            Some(DynProvisionImpl::Storage(p)) => p.list_blockdevs().await,
            Some(DynProvisionImpl::LeapConfig(p)) => p.list_blockdevs().await,
            Some(DynProvisionImpl::Complete(p)) => p.list_blockdevs().await,
            None => {
                anyhow::bail!("BUG: No inner DynProvisionImpl");
            }
        }
    }

    /// Configures the network for LEAP. Only allowed in the Network and LeapConfig states.
    pub async fn configure_network(
        &mut self,
        network_config: &NetworkConfig,
    ) -> anyhow::Result<()> {
        match self.0.take() {
            Some(DynProvisionImpl::Network(p)) => {
                self.handle_retval(p.configure_network(network_config).await)?
            }
            Some(DynProvisionImpl::LeapConfig(p)) => {
                self.handle_retval(p.revert().await.configure_network(network_config).await)?;
            }
            Some(v) => {
                self.0.replace(v);
                anyhow::bail!("configure_network called on invalid state");
            }
            None => {
                anyhow::bail!("BUG: No inner DynProvisionImpl");
            }
        }
        Ok(())
    }

    /// Formats the external storage used for LEAP. Only allowed in the Storage and Network steps.
    pub async fn configure_storage(&mut self, device_path: &Path) -> anyhow::Result<()> {
        match self.0.take() {
            Some(DynProvisionImpl::Storage(p)) => {
                self.handle_retval(p.configure_storage(device_path).await)?;
            }
            Some(DynProvisionImpl::Network(p)) => {
                self.handle_retval(p.revert().await.configure_storage(device_path).await)?;
            }
            Some(v) => {
                self.0.replace(v);
                anyhow::bail!("configure_storage called on invalid state");
            }
            None => {
                anyhow::bail!("BUG: No inner DynProvisionImpl");
            }
        }
        Ok(())
    }

    /// Saves the LEAP configuration to disk after validation. Only allowed in the LEAP
    /// configuration state.
    pub async fn configure_leap(
        &mut self,
        config: &leap_api::provision::config::post::LeapConfig,
    ) -> anyhow::Result<()> {
        match self.0.take() {
            Some(DynProvisionImpl::LeapConfig(p)) => {
                self.handle_retval(p.configure_leap(config).await)?;
            }
            Some(v) => {
                self.0.replace(v);
                anyhow::bail!("configure_leap called on invalid state");
            }
            None => {
                anyhow::bail!("BUG: No inner DynProvisionImpl");
            }
        }
        Ok(())
    }

    /// Completes the provisioning process, triggering a reboot.
    pub async fn finish(&mut self) -> anyhow::Result<()> {
        match self.0.take() {
            Some(DynProvisionImpl::Complete(p)) => {
                p.finish().await?;
                unreachable!("Finish should have rebooted the system");
            }
            Some(v) => {
                self.0.replace(v);
                anyhow::bail!("finish called on invalid state");
            }
            None => {
                anyhow::bail!("BUG: No inner DynProvisionImpl");
            }
        }
    }

    /// Returns the current status of the provisioning process.
    pub fn status(&self) -> anyhow::Result<ProvisionStatus> {
        match self.0.as_ref() {
            Some(DynProvisionImpl::Network(_)) => Ok(ProvisionStatus::NetworkConfig),
            Some(DynProvisionImpl::Storage(_)) => Ok(ProvisionStatus::StorageConfig),
            Some(DynProvisionImpl::LeapConfig(_)) => Ok(ProvisionStatus::LeapConfig),
            Some(DynProvisionImpl::Complete(_)) => Ok(ProvisionStatus::Completed),
            None => {
                anyhow::bail!("BUG: No inner DynProvisionImpl");
            }
        }
    }

    fn handle_retval<Src: ProvisionVariant, Dst: ProvisionVariant>(
        &mut self,
        result: Result<Provision<Dst>, (anyhow::Error, Provision<Src>)>,
    ) -> anyhow::Result<()> {
        match result {
            Ok(dst) => {
                tracing::info!("Successful transition from {} to {}", Src::NAME, Dst::NAME);
                self.0.replace(DynProvisionImpl::from(dst));
                Ok(())
            }
            Err((err, src)) => {
                tracing::info!(
                    "Failed to transition from {} to {}. Staying in {}",
                    Src::NAME,
                    Dst::NAME,
                    Src::NAME
                );
                self.0.replace(DynProvisionImpl::from(src));
                Err(err)
            }
        }
    }
}

impl std::fmt::Debug for DynProvision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.0 {
            Some(DynProvisionImpl::Network(p)) => write!(f, "{p:?}"),
            Some(DynProvisionImpl::Storage(p)) => write!(f, "{p:?}"),
            Some(DynProvisionImpl::LeapConfig(p)) => write!(f, "{p:?}"),
            Some(DynProvisionImpl::Complete(p)) => write!(f, "{p:?}"),
            None => write!(f, "ProvisionInvalidState"),
        }
    }
}
