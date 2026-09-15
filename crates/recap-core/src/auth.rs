use serde::{Deserialize, Serialize};

use crate::models::Source;

/// Token storage backed by SQLite (the `tokens` table).
/// Avoids macOS Keychain prompts during development — the binary signature
/// changes on every recompile, which invalidates Keychain "Always Allow".
pub struct AuthManager;

impl AuthManager {
    /// Store a token.
    pub fn set_token(source: &Source, token: &str) -> Result<(), String> {
        let key = format!("{}_token", source);
        Self::db_set(&key, token)
    }

    /// Retrieve a token.
    pub fn get_token(source: &Source) -> Result<Option<String>, String> {
        let key = format!("{}_token", source);
        Self::db_get(&key)
    }

    /// Delete a token.
    pub fn delete_token(source: &Source) -> Result<(), String> {
        let key = format!("{}_token", source);
        Self::db_delete(&key)
    }

    /// Check which sources have tokens configured.
    pub fn get_auth_status() -> AuthStatus {
        let has_token = |source: &Source| -> bool {
            if matches!(source, Source::GitHub) {
                Self::get_github_token().ok().flatten().is_some()
            } else {
                Self::get_token(source).ok().flatten().is_some()
            }
        };

        AuthStatus {
            github: has_token(&Source::GitHub),
            linear: has_token(&Source::Linear),
            slack: true,  // Slack disabled — requires full OAuth
            notion: true, // Notion disabled
            anthropic: Self::get_anthropic_key().ok().flatten().is_some(),
        }
    }

    /// Retrieve the Anthropic API key.
    pub fn get_anthropic_key() -> Result<Option<String>, String> {
        Self::db_get("anthropic_api_key")
    }

    /// Store a Slack refresh token.
    pub fn set_slack_refresh_token(token: &str) -> Result<(), String> {
        Self::db_set("slack_refresh_token", token)
    }

    /// Retrieve the Slack refresh token.
    pub fn get_slack_refresh_token() -> Result<Option<String>, String> {
        Self::db_get("slack_refresh_token")
    }

    /// GitHub: try `gh` CLI first, then fall back to the token store.
    pub fn get_github_token() -> Result<Option<String>, String> {
        if let Some(gh_path) = find_gh_binary() {
            if let Ok(output) = std::process::Command::new(&gh_path)
                .args(["auth", "token"])
                .output()
            {
                if output.status.success() {
                    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if !token.is_empty() {
                        return Ok(Some(token));
                    }
                }
            }
        }
        Self::get_token(&Source::GitHub)
    }

    /// Store the Anthropic API key.
    pub fn set_anthropic_key(key: &str) -> Result<(), String> {
        Self::db_set("anthropic_api_key", key)
    }

    // -- SQLite helpers --

    fn db_path() -> std::path::PathBuf {
        crate::config::AppConfig::db_path()
    }

    /// Open a connection with WAL mode and a busy timeout so concurrent
    /// access from the daemon and the app doesn't produce SQLITE_BUSY.
    fn open_conn() -> Result<rusqlite::Connection, String> {
        let conn = rusqlite::Connection::open(Self::db_path())
            .map_err(|e| e.to_string())?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")
            .map_err(|e| e.to_string())?;
        Ok(conn)
    }

    fn db_get(key: &str) -> Result<Option<String>, String> {
        let conn = Self::open_conn()?;
        let mut stmt = conn
            .prepare("SELECT value FROM tokens WHERE key = ?1")
            .map_err(|e| e.to_string())?;
        let result = stmt
            .query_row(rusqlite::params![key], |row| row.get::<_, String>(0))
            .ok();
        Ok(result)
    }

    fn db_set(key: &str, value: &str) -> Result<(), String> {
        let conn = Self::open_conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO tokens (key, value) VALUES (?1, ?2)",
            rusqlite::params![key, value],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn db_delete(key: &str) -> Result<(), String> {
        let conn = Self::open_conn()?;
        conn.execute("DELETE FROM tokens WHERE key = ?1", rusqlite::params![key])
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

/// Find the `gh` binary, checking common paths since bundled .app doesn't inherit shell PATH.
pub fn find_gh_binary() -> Option<String> {
    if let Ok(output) = std::process::Command::new("which").arg("gh").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Some(path);
            }
        }
    }
    let candidates = [
        "/usr/local/bin/gh",
        "/opt/homebrew/bin/gh",
    ];
    for path in &candidates {
        if std::path::Path::new(path).exists() {
            return Some(path.to_string());
        }
    }
    None
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthStatus {
    pub github: bool,
    pub linear: bool,
    pub slack: bool,
    pub notion: bool,
    pub anthropic: bool,
}
