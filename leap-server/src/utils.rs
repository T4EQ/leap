//! Utilities for the `leap-server`.
//!
//! This module provides helper functions for system-level tasks, such as
//! checking if the system time is synchronized with NTP.

use std::time::Duration;

/// Checks if the system time is synchronized with NTP.
///
/// This function uses the `timedatectl` systemd command to check the time synchronization status.
///
/// Returns `Ok(true)` if synchronized, `Ok(false)` if not, or an error if `timedatectl` does not
/// exit with a zero status code.
pub async fn check_timesync() -> anyhow::Result<bool> {
    let output = tokio::process::Command::new("timedatectl")
        .arg("show")
        .arg("-P")
        .arg("NTPSynchronized")
        .output()
        .await?;
    if !output.status.success() {
        tracing::error!("Failure checking time synchronization {output:?}");
        anyhow::bail!("Failure checking time synchronization {output:?}");
    }

    Ok(output.stdout == b"yes\n")
}

/// Waits for the system time to be synchronized with NTP.
///
/// This function polls `check_timesync` periodically until it returns `true` or the timeout is reached.
///
/// # Errors
///
/// Returns an error if the timeout is reached before synchronization is confirmed.
pub async fn wait_timesync(timeout: Duration) -> anyhow::Result<()> {
    let start = std::time::Instant::now();
    while std::time::Instant::now() - start < timeout {
        if check_timesync().await? {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    anyhow::bail!("Timeout while waiting for time synchronization");
}
