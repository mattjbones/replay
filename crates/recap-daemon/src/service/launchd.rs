use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;

const PLIST_LABEL: &str = "com.recap.daemon";

/// Returns the path to the LaunchAgent plist file.
fn plist_path() -> anyhow::Result<PathBuf> {
    Ok(dirs::home_dir()
        .context("could not determine home directory")?
        .join("Library/LaunchAgents")
        .join(format!("{PLIST_LABEL}.plist")))
}

/// Returns the path to the log directory: `<config dir>/recap/`, which on
/// macOS is `~/Library/Application Support/recap/`.
fn log_dir() -> anyhow::Result<PathBuf> {
    Ok(dirs::config_dir()
        .context("could not determine config directory")?
        .join("recap"))
}

/// Install the LaunchAgent plist and load it via launchctl.
pub fn install() -> anyhow::Result<()> {
    let exe = std::env::current_exe()?;
    let exe_str = exe.display().to_string();
    let log_dir = log_dir()?;
    let stdout_log = log_dir.join("daemon.log");
    let stderr_log = log_dir.join("daemon.err.log");
    let plist = plist_path()?;

    // Ensure the LaunchAgents directory exists
    if let Some(parent) = plist.parent() {
        std::fs::create_dir_all(parent)?;
    }

    // Ensure the log directory exists
    std::fs::create_dir_all(&log_dir)?;

    let plist_content = render_plist(&exe_str, &stdout_log.display().to_string(), &stderr_log.display().to_string());

    std::fs::write(&plist, &plist_content)?;
    tracing::info!("wrote plist to {}", plist.display());

    // `launchctl load` fails if the label is already loaded, which would leave
    // the previous binary running against the new plist. Unload first;
    // a failure here just means nothing was loaded.
    let _ = Command::new("launchctl")
        .args(["unload", &plist.display().to_string()])
        .output();

    let output = Command::new("launchctl")
        .args(["load", &plist.display().to_string()])
        .output()?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::error!("launchctl load failed: {stderr}");
        anyhow::bail!("launchctl load failed: {}", stderr.trim());
    }

    tracing::info!("launchctl load succeeded");
    println!("Daemon installed and loaded.");
    println!("  Plist: {}", plist.display());
    println!("  Logs:  {}", stdout_log.display());

    Ok(())
}

/// Unload the LaunchAgent and delete the plist file.
pub fn uninstall() -> anyhow::Result<()> {
    let plist = plist_path()?;

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
    let plist = plist_path()?;
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
            if is_running(&stdout) {
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

/// Render the LaunchAgent plist. Paths are XML-escaped; the plist is a
/// property list, so a `&` in a path would otherwise make it unparseable.
fn render_plist(exe: &str, stdout_log: &str, stderr_log: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{PLIST_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe}</string>
        <string>service</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>ThrottleInterval</key>
    <integer>60</integer>
    <key>StandardOutPath</key>
    <string>{stdout}</string>
    <key>StandardErrorPath</key>
    <string>{stderr}</string>
</dict>
</plist>
"#,
        exe = xml_escape(exe),
        stdout = xml_escape(stdout_log),
        stderr = xml_escape(stderr_log),
    )
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Interpret `launchctl list <label>` output. launchd prints a `{ ... }`
/// dictionary whenever the job is *loaded*; the `"PID"` key is present only
/// while a process is actually running.
fn is_running(launchctl_list_output: &str) -> bool {
    launchctl_list_output
        .lines()
        .any(|line| line.trim_start().starts_with("\"PID\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plist_contains_label_program_and_logs() {
        let p = render_plist("/usr/local/bin/recap-daemon", "/tmp/out.log", "/tmp/err.log");
        assert!(p.contains("<string>com.recap.daemon</string>"));
        assert!(p.contains("<string>/usr/local/bin/recap-daemon</string>\n        <string>service</string>"));
        assert!(p.contains("<key>StandardOutPath</key>\n    <string>/tmp/out.log</string>"));
        assert!(p.contains("<key>StandardErrorPath</key>\n    <string>/tmp/err.log</string>"));
        assert!(p.contains("<key>KeepAlive</key>\n    <true/>"));
        assert!(p.contains("<key>ThrottleInterval</key>\n    <integer>60</integer>"));
    }

    #[test]
    fn plist_escapes_xml_special_chars_in_paths() {
        let p = render_plist("/Users/a&b/<recap>", "/tmp/o.log", "/tmp/e.log");
        assert!(p.contains("<string>/Users/a&amp;b/&lt;recap&gt;</string>"));
        assert!(!p.contains("a&b"));
    }

    #[test]
    fn loaded_but_not_running_is_not_running() {
        let out = "{\n\t\"Label\" = \"com.recap.daemon\";\n\t\"LastExitStatus\" = 256;\n};\n";
        assert!(!is_running(out));
    }

    #[test]
    fn running_job_has_pid_key() {
        let out = "{\n\t\"Label\" = \"com.recap.daemon\";\n\t\"PID\" = 4242;\n\t\"Program\" = \"/x\";\n};\n";
        assert!(is_running(out));
    }

    #[test]
    fn empty_output_is_not_running() {
        assert!(!is_running(""));
    }
}
