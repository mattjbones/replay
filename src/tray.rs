use tauri::{
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, Runtime,
};

/// Set up the system tray icon and its click handler.
///
/// Left-clicking toggles the "main" webview window: show it if hidden, hide it if
/// visible, or create it if it does not yet exist.
pub fn setup_tray<R: Runtime>(app: &tauri::App<R>) -> Result<(), Box<dyn std::error::Error>> {
    TrayIconBuilder::new()
        .tooltip("Recap")
        .on_tray_icon_event(|tray_icon, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app_handle = tray_icon.app_handle();

                if let Some(window) = app_handle.get_webview_window("main") {
                    // Window exists -- toggle visibility.
                    if window.is_visible().unwrap_or(false) {
                        let _ = window.hide();
                    } else {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                } else {
                    // Window does not exist yet -- create it.
                    match tauri::WebviewWindowBuilder::new(
                        app_handle,
                        "main",
                        tauri::WebviewUrl::App("index.html".into()),
                    )
                    .title("Recap")
                    .inner_size(400.0, 600.0)
                    .decorations(true)
                    .center()
                    .skip_taskbar(true)
                    .build()
                    {
                        Ok(_) => {}
                        Err(e) => {
                            tracing::error!("failed to create main window: {e}");
                        }
                    }
                }
            }
        })
        .build(app)?;

    Ok(())
}
