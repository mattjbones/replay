pub mod commands;
pub mod notifications;

use std::sync::Arc;

use tauri::Manager;
use recap_core::config;
use recap_core::db;
use recap_core::sync;
use commands::AppState;

pub fn run() {
    tracing_subscriber::fmt::init();

    tracing::info!("recap starting...");

    let config = config::AppConfig::load();
    tracing::info!("config loaded from {:?}", config::AppConfig::config_dir());

    let db_path = config::AppConfig::db_path();
    tracing::info!("opening database at {:?}", db_path);
    let db = Arc::new(
        db::Database::new(&db_path).expect("failed to open database"),
    );
    tracing::info!("database opened");

    let state = AppState {
        db: db.clone(),
        config: std::sync::Mutex::new(config.clone()),
    };

    tracing::info!("building tauri app...");

    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(state)
        .setup(move |app| {
            tracing::info!("tauri setup callback running");

            #[cfg(target_os = "macos")]
            {
                tracing::info!("setting macOS activation policy to Regular");
                app.set_activation_policy(tauri::ActivationPolicy::Regular);
            }

            if let Some(window) = app.get_webview_window("main") {
                tracing::info!("main window ready");
                let _ = window.set_focus();
            }

            // Start background sync.
            let sync_db = db.clone();
            let sync_config = config.clone();
            tauri::async_runtime::spawn(async move {
                let scheduler = sync::SyncScheduler::new(sync_db, sync_config);
                scheduler.start().await;
            });

            // Start daily reminder notifications.
            let reminder_handle = app.handle().clone();
            let reminder_db = db.clone();
            let reminder_config = config.clone();
            notifications::start_daily_reminder(reminder_handle, reminder_db, reminder_config);

            tracing::info!("setup complete");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::get_digest,
            commands::get_auth_status,
            commands::save_token,
            commands::save_slack_refresh_token,
            commands::save_anthropic_key,
            commands::exchange_slack_refresh_token,
            commands::get_all_activities,
            commands::clear_cache,
            commands::trigger_sync,
            commands::get_config,
            commands::update_config,
            commands::get_llm_summary,
            commands::get_chart_data,
            commands::get_feature_breakdown,
            commands::get_standup,
            commands::get_open_tickets,
            commands::get_open_prs,
            commands::get_github_issues,
            commands::get_trends_data,
            commands::get_trends_ai_summary,
            commands::get_heatmap_activities,
        ]);

    tracing::info!("calling tauri::Builder::run()...");

    builder
        .run(tauri::generate_context!())
        .expect("error while running recap");

    tracing::info!("tauri app exited");
}
