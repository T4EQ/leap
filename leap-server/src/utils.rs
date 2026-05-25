use std::time::Duration;

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
