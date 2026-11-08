//! Native notifications, sent from a worker thread (the Linux D-Bus call blocks).
//!
//! On Windows the sender shows as "Clocky" only when the `AppUserModelId` is registered under
//! HKCU; without it Windows drops the toast silently (M0 report). No installer is needed: the
//! key is written right before a notification is shown. Failures only log a warning.

/// `AppUserModelId` used for the notifications on Windows.
#[cfg_attr(not(windows), allow(dead_code))]
pub const APP_ID: &str = "Clocky.Clocky";

/// Builds the notification with the app name and, on Windows, the `AppUserModelId`.
pub fn build(summary: &str, body: &str) -> notify_rust::Notification {
    let mut notification = notify_rust::Notification::new();
    notification.appname("Clocky").summary(summary).body(body);
    #[cfg(windows)]
    notification.app_id(APP_ID);
    notification
}

/// Shows a notification without blocking the UI thread.
pub fn show(summary: &str, body: &str) {
    let (summary, body) = (summary.to_owned(), body.to_owned());
    let spawned = std::thread::Builder::new()
        .name("notify".into())
        .spawn(move || {
            #[cfg(windows)]
            if let Err(e) = register_app_id() {
                tracing::warn!("failed to register the AppUserModelId (toasts may not show): {e}");
            }
            match build(&summary, &body).show() {
                Ok(_) => tracing::info!("notification shown: {summary}"),
                Err(e) => tracing::warn!("failed to show a notification: {e}"),
            }
        });
    if let Err(e) = spawned {
        tracing::warn!("failed to start the notification thread: {e}");
    }
}

/// Registry key (under HKCU) that names the notification sender.
#[cfg(windows)]
pub fn app_id_key() -> String {
    format!(r"Software\Classes\AppUserModelId\{APP_ID}")
}

/// Writes `HKCU\<app_id_key>\DisplayName = "Clocky"` (idempotent).
#[cfg(windows)]
fn register_app_id() -> std::io::Result<()> {
    use winreg::RegKey;
    use winreg::enums::HKEY_CURRENT_USER;
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(app_id_key())?;
    key.set_value("DisplayName", &"Clocky")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notification_carries_summary_and_body() {
        let n = build("Foco terminou", "connect › autenticação");
        assert_eq!(n.summary, "Foco terminou");
        assert_eq!(n.body, "connect › autenticação");
    }

    #[cfg(windows)]
    #[test]
    fn app_id_key_is_under_classes() {
        assert_eq!(
            app_id_key(),
            r"Software\Classes\AppUserModelId\Clocky.Clocky"
        );
    }
}
