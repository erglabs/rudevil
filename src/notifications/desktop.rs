#[cfg(feature = "notifications")]
use notify_rust::Notification;

#[cfg(not(feature = "notifications"))]
pub async fn notify(_content: &str) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(feature = "notifications")]
pub async fn notify(content: &str) -> anyhow::Result<()> {
    Notification::new()
        .summary("Rudevil>>")
        .body(content)
        .icon("dialog-info")
        .show()?;
    Ok(())
}
