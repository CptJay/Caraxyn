//! Desktop integration: the tray icon, bringing the window back, and notifications.

use std::panic::AssertUnwindSafe;

use tauri::{
    AppHandle, Manager,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};
use tauri_plugin_notification::NotificationExt;

/// Shows the main window again and brings it to the front.
pub fn show_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        // Each can only fail when the window is already gone; then there is nothing to show.
        window.unminimize().ok();
        window.show().ok();
        window.set_focus().ok();
    }
}

/// Adds the tray icon and says whether it worked. Without one the app still runs,
/// and closing the window quits it (see main).
pub fn add_tray(app: &AppHandle) -> bool {
    // On Linux the tray library is loaded at run time and panics when it is missing
    // (an AppImage on a system without AppIndicator). Tauri builds the tray right here,
    // on this thread, so the panic can be caught.
    match std::panic::catch_unwind(AssertUnwindSafe(|| build_tray(app))) {
        Ok(Ok(())) => true,
        Ok(Err(e)) => {
            tracing::warn!(%e, "no tray icon");
            false
        }
        Err(_) => {
            tracing::warn!("no tray icon: the system tray library is missing");
            false
        }
    }
}

/// The tray icon. Its menu has Open and Quit; a left click also opens the
/// window where the platform reports clicks (not on Linux, where the menu does it).
fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Caraxyn", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut tray = TrayIconBuilder::new()
        .tooltip("Caraxyn")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_window(app),
            // Ends the run loop; the node is shut down on the way out (see main).
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}

/// Shows a desktop notification, unless the window is in front: then the app
/// shows the news itself.
pub fn notify(app: &AppHandle, title: &str, body: &str) {
    let in_front = app.get_webview_window("main").is_some_and(|window| {
        window.is_visible().unwrap_or(false) && window.is_focused().unwrap_or(false)
    });
    if in_front {
        return;
    }
    if let Err(e) = app.notification().builder().title(title).body(body).show() {
        tracing::warn!(%e, "could not show a notification");
    }
}

/// "700.0 MB": the same units as the UI (1024-based, one decimal).
pub fn size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let (mut value, mut unit) = (bytes as f64, 0);
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::size;

    #[test]
    fn sizes_read_like_the_ui() {
        assert_eq!(size(512), "512 B");
        assert_eq!(size(734_003_200), "700.0 MB");
        assert_eq!(size(3 << 30), "3.0 GB");
    }
}
