use tauri::{
    menu::{MenuBuilder, MenuItemBuilder},
    tray::TrayIconBuilder,
    Manager, Runtime,
};

/// Set up the system tray icon.
///
/// On macOS, clicking the tray icon opens the menu. "Open Recap" creates/shows
/// the webview window. This is the standard macOS tray UX — menus, not direct clicks.
pub fn setup_tray<R: Runtime>(app: &tauri::App<R>) -> Result<(), Box<dyn std::error::Error>> {
    tracing::info!("setting up system tray...");

    let open_item = MenuItemBuilder::with_id("open", "Open Recap").build(app)?;
    let sync_item = MenuItemBuilder::with_id("sync", "Sync Now").build(app)?;
    let quit_item = MenuItemBuilder::with_id("quit", "Quit").build(app)?;

    let menu = MenuBuilder::new(app)
        .item(&open_item)
        .item(&sync_item)
        .separator()
        .item(&quit_item)
        .build()?;

    tracing::info!("tray menu built");

    // Generate a 22x22 RGBA icon: black "R" on transparent background.
    // macOS template mode auto-inverts for dark/light menubar.
    let (w, h) = (22u32, 22u32);
    let glyph: &[u32] = &[
        0b0000000000000000000000,
        0b0000000000000000000000,
        0b0000000000000000000000,
        0b0001111111100000000000,
        0b0001100000110000000000,
        0b0001100000011000000000,
        0b0001100000011000000000,
        0b0001100000011000000000,
        0b0001100000110000000000,
        0b0001111111100000000000,
        0b0001111110000000000000,
        0b0001100111000000000000,
        0b0001100011100000000000,
        0b0001100001110000000000,
        0b0001100000111000000000,
        0b0001100000011100000000,
        0b0001100000001110000000,
        0b0000000000000000000000,
        0b0000000000000000000000,
        0b0000000000000000000000,
        0b0000000000000000000000,
        0b0000000000000000000000,
    ];
    let mut rgba = Vec::with_capacity((w * h * 4) as usize);
    for y in 0..h {
        for x in 0..w {
            let bit = (glyph[y as usize] >> (w - 1 - x)) & 1;
            if bit == 1 {
                rgba.extend_from_slice(&[0, 0, 0, 255]);
            } else {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }
    let icon = tauri::image::Image::new_owned(rgba, w, h);

    tracing::info!("tray icon generated");

    TrayIconBuilder::new()
        .icon(icon)
        .icon_as_template(true)
        .tooltip("Recap")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app_handle, event| {
            tracing::info!("tray menu event: {:?}", event.id());
            match event.id().as_ref() {
                "open" => {
                    show_or_create_window(app_handle);
                }
                "sync" => {
                    show_or_create_window(app_handle);
                }
                "quit" => {
                    app_handle.exit(0);
                }
                _ => {}
            }
        })
        .build(app)?;

    tracing::info!("system tray built successfully");
    Ok(())
}

/// Show the main window if it exists, or create it. Toggles visibility if already visible.
fn show_or_create_window<R: Runtime>(app_handle: &tauri::AppHandle<R>) {
    tracing::info!("show_or_create_window called");

    if let Some(window) = app_handle.get_webview_window("main") {
        let visible = window.is_visible().unwrap_or(false);
        tracing::info!("existing window found, visible={visible}");
        if visible {
            if let Err(e) = window.set_focus() {
                tracing::error!("failed to focus window: {e}");
            }
        } else {
            if let Err(e) = window.show() {
                tracing::error!("failed to show window: {e}");
            }
            if let Err(e) = window.set_focus() {
                tracing::error!("failed to focus window: {e}");
            }
        }
    } else {
        tracing::info!("no existing window, creating new one");
        match tauri::WebviewWindowBuilder::new(
            app_handle,
            "main",
            tauri::WebviewUrl::App("index.html".into()),
        )
        .title("Recap")
        .inner_size(400.0, 600.0)
        .decorations(true)
        .center()
        .build()
        {
            Ok(w) => {
                tracing::info!("window created successfully, label={}", w.label());
            }
            Err(e) => {
                tracing::error!("failed to create main window: {e}");
            }
        }
    }
}
