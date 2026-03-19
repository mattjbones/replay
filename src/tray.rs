use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, Runtime,
};

/// Set up the system tray icon and its click handler.
///
/// Left-clicking toggles the "main" webview window: show it if hidden, hide it if
/// visible, or create it if it does not yet exist.
/// Right-clicking opens a context menu with Open, Sync Now, and Quit items.
pub fn setup_tray<R: Runtime>(app: &tauri::App<R>) -> Result<(), Box<dyn std::error::Error>> {
    let open_item = MenuItemBuilder::with_id("open", "Open Recap").build(app)?;
    let sync_item = MenuItemBuilder::with_id("sync", "Sync Now").build(app)?;
    let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;

    let menu = MenuBuilder::new(app)
        .item(&open_item)
        .item(&sync_item)
        .separator()
        .item(&quit_item)
        .build()?;

    TrayIconBuilder::new()
        .tooltip("Recap")
        .menu(&menu)
        .on_menu_event(|app_handle, event| match event.id().as_ref() {
            "open" => {
                show_or_create_window(app_handle);
            }
            "sync" => {
                show_or_create_window(app_handle);
            }
            "quit" => {
                std::process::exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray_icon, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let app_handle = tray_icon.app_handle();
                show_or_create_window(app_handle);
            }
        })
        .build(app)?;

    Ok(())
}

/// Show the main window if it exists, or create it. Toggles visibility if already visible.
fn show_or_create_window<R: Runtime>(app_handle: &tauri::AppHandle<R>) {
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
