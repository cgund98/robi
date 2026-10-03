//! An OS notification when a turn is waiting on approval.
//!
//! On macOS the banner waits for a click, then focuses the window. In dev,
//! Notification Center delivers that banner as Terminal; the click handler
//! still runs in this process.

use tauri::{AppHandle, Emitter, Manager};

#[tauri::command]
pub fn show_approval_notice(
    app: AppHandle,
    body: String,
    session_id: String,
) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let identifier = app.config().identifier.clone();
        let bundle = if tauri::is_dev() {
            "com.apple.Terminal"
        } else {
            identifier.as_str()
        };
        // The bundle can only be set once. A later pause keeps the first one.
        let _ = mac_notification_sys::set_application(bundle);
        std::thread::spawn(move || show_macos(app, body, session_id));
        Ok(())
    }

    #[cfg(not(target_os = "macos"))]
    {
        show_other(app, body, session_id)
    }
}

#[cfg(target_os = "macos")]
fn show_macos(app: AppHandle, body: String, session_id: String) {
    let mut notification = mac_notification_sys::Notification::new();
    notification
        .title("Robi needs approval")
        .message(&body)
        .wait_for_click(true);
    if matches!(
        notification.send(),
        Ok(mac_notification_sys::NotificationResponse::Click)
    ) {
        open_session(&app, &session_id);
    }
}

#[cfg(not(target_os = "macos"))]
fn show_other(app: AppHandle, body: String, session_id: String) -> Result<(), String> {
    let mut notification = notify_rust::Notification::new();
    notification.summary("Robi needs approval").body(&body);
    let handle = notification.show().map_err(|err| err.to_string())?;
    std::thread::spawn(move || {
        handle.wait_for_action(|action| {
            if action == "default" {
                open_session(&app, &session_id);
            }
        });
    });
    Ok(())
}

fn open_session(app: &AppHandle, session_id: &str) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
    let _ = app.emit("approval-notice-open", session_id);
}
