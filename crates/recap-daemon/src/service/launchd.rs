use std::path::PathBuf;
use std::process::Command;

const PLIST_LABEL: &str = "com.recap.daemon";

/// Returns the path to the LaunchAgent plist file.
fn plist_path() -> PathBuf {
    dirs::home_dir()
        .expect("could not determine home directory")
        .join("Library/LaunchAgents")
        .join(format!("{PLIST_LABEL}.plist"))
}

/// Returns the path to the log directory (~/.config/recap/).
fn log_dir() -> PathBuf {
    dirs::config_dir()
        .expect("could not determine config directory")
        .join("recap")
}

/// Install the LaunchAgent plist and load it via launchctl.
pub fn install() -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let exe_str = exe.display().to_string();
    let log_dir = log_dir();
    let stdout_log = log_dir.join("daemon.log");
    let stderr_log = log_dir.join("daemon.err.log");
    let plist = plist_path();

    // Ensure the LaunchAgents directory exists
    if let Some(parent) = plist.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Ensure the log directory exists
    std::fs::create_dir_all(&log_dir)?;

    let plist_content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{PLIST_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe_str}</string>
        <string>service</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{stdout}</string>
    <key>StandardErrorPath</key>
    <string>{stderr}</string>
</dict>
</plist>
"#,
        stdout = stdout_log.display(),
        stderr = stderr_log.display(),
    );

    std::fs::write(&plist, &plist_content)?;
    tracing::info!("wrote plist to {}", plist.display());

    let output = Command::new("launchctl")
        .args(["load", &plist.display().to_string()])
        .output()?;

    if output.status.success() {
        tracing::info!("launchctl load succeeded");
        println!("Daemon installed and loaded.");
        println!("  Plist: {}", plist.display());
        println!("  Logs:  {}", stdout_log.display());
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!("launchctl load failed: {stderr}");
        eprintln!("launchctl load failed: {stderr}");
    }

    Ok(())
}

/// Unload the LaunchAgent and delete the plist file.
pub fn uninstall() -> anyhow::Result<()> {
    let plist = plist_path();

    if !plist.exists() {
        println!("No plist found at {}; nothing to uninstall.", plist.display());
        return Ok(());
    }

    let output = Command::new("launchctl")
        .args(["unload", &plist.display().to_string()])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::warn!("launchctl unload returned error: {stderr}");
        // Continue to delete the plist anyway
    }

    std::fs::remove_file(&plist)?;
    tracing::info!("removed plist at {}", plist.display());
    println!("Daemon uninstalled.");
    println!("  Removed: {}", plist.display());

    Ok(())
}

/// Print status information about the daemon.
pub fn status() -> anyhow::Result<()> {
    let plist = plist_path();
    let plist_installed = plist.exists();

    println!("=== Recap Daemon Status ===\n");

    // Plist status
    if plist_installed {
        println!("Plist:   installed ({})", plist.display());
    } else {
        println!("Plist:   not installed");
    }

    // launchctl status
    let launchctl_output = Command::new("launchctl")
        .args(["list", PLIST_LABEL])
        .output();

    match launchctl_output {
        Ok(output) if output.status.success() => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            // Parse PID from launchctl output
            let running = stdout.lines().any(|line| {
                line.contains("PID") || line.starts_with('{')
            });
            if running {
                println!("Process: running");
            } else {
                println!("Process: loaded (not currently running)");
            }
        }
        _ => {
            println!("Process: not loaded");
        }
    }

    // Database path
    let db_path = recap_core::config::AppConfig::db_path();
    println!("\nDatabase: {}", db_path.display());
    let db_exists = db_path.exists();
    if !db_exists {
        println!("  (database file does not exist yet)");
    }

    // Auth status
    let auth = recap_core::auth::AuthManager::get_auth_status();
    println!("\nAuth Status:");
    println!("  GitHub:    {}", if auth.github { "connected" } else { "not connected" });
    println!("  Linear:    {}", if auth.linear { "connected" } else { "not connected" });
    println!("  Slack:     {}", if auth.slack { "connected" } else { "not connected" });
    println!("  Notion:    {}", if auth.notion { "connected" } else { "not connected" });
    println!("  Anthropic: {}", if auth.anthropic { "configured" } else { "not configured" });

    // Last sync times
    if db_exists {
        match recap_core::db::Database::new(&db_path) {
            Ok(db) => {
                match recap_core::db::get_all_sync_cursors(&db) {
                    Ok(cursors) => {
                        if cursors.is_empty() {
                            println!("\nSync History: no syncs recorded yet");
                        } else {
                            println!("\nSync History:");
                            for (source, _cursor, last_sync) in &cursors {
                                println!("  {source}: last synced {last_sync}");
                            }
                        }
                    }
                    Err(e) => {
                        println!("\nSync History: error reading cursors: {e}");
                    }
                }
            }
            Err(e) => {
                println!("\nSync History: could not open database: {e}");
            }
        }
    }

    Ok(())
}
