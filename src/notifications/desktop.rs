#[cfg(feature = "notifications")]
use notify_rust::Notification;

#[allow(unused_variables)]
#[allow(unreachable_code)]
pub fn notify(content: &str) -> anyhow::Result<()> {
  #[cfg(not(feature = "notifications"))]
  {
    return Ok(());
  }

  #[cfg(feature = "notifications")]
  {
    Notification::new()
      .summary("Rudevil>>")
      .body(content)
      .icon("dialog-info")
      .show()?;
  }
  Ok(())
}
