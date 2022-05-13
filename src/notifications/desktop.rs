#[cfg(feature = "notifications")]
use notify_rust::Notification;

#[cfg(not(feature = "notify"))]
pub async fn notify(_content: &str) { }

#[cfg(feature = "notify")]
pub async fn notify(content: &str) {
    Notification::new()
        .summary("Rudevil>>")
        .body(content)
        .icon("dialog-info")
        .show()?;
    Ok(())
}
