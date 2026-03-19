pub mod auth;
pub mod commands;
pub mod config;
pub mod db;
pub mod digest;
pub mod integrations;
pub mod models;
pub mod sync;
pub mod tray;

use std::sync::Arc;

use commands::AppState;

pub fn run() {
    tracing_subscriber::fmt::init();

    let config = config::AppConfig::load();
    let db = Arc::new(
        db::Database::new(&config::AppConfig::db_path()).expect("failed to open database"),
    );

    let state = AppState {
        db: db.clone(),
        config: config.clone(),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .manage(state)
        .setup(move |app| {
            tray::setup_tray(app)?;

            // Start background sync.
            let sync_db = db.clone();
            let sync_config = config.clone();
            tauri::async_runtime::spawn(async move {
                let scheduler = sync::SyncScheduler::new(sync_db, sync_config);
                scheduler.start().await;
            });

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_digest,
            commands::get_auth_status,
            commands::save_token,
            commands::trigger_sync,
            commands::get_config,
            commands::update_config,
            commands::get_llm_summary,
        ])
        .run(tauri::generate_context!())
        .expect("error while running recap");
}
