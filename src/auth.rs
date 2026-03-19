use serde::{Deserialize, Serialize};

use crate::models::Source;

const SERVICE_NAME: &str = "com.recap.app";

pub struct AuthManager;

impl AuthManager {
    /// Store a token for a given source in the system keychain.
    pub fn set_token(source: &Source, token: &str) -> Result<(), String> {
        let key = format!("{}_token", source);
        let entry = keyring::Entry::new(SERVICE_NAME, &key).map_err(|e| e.to_string())?;
        entry.set_password(token).map_err(|e| e.to_string())
    }

    /// Retrieve a token for a given source from the system keychain.
    /// Returns `Ok(None)` if no token is stored.
    pub fn get_token(source: &Source) -> Result<Option<String>, String> {
        let key = format!("{}_token", source);
        let entry = keyring::Entry::new(SERVICE_NAME, &key).map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(password) => Ok(Some(password)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Delete a token for a given source from the system keychain.
    pub fn delete_token(source: &Source) -> Result<(), String> {
        let key = format!("{}_token", source);
        let entry = keyring::Entry::new(SERVICE_NAME, &key).map_err(|e| e.to_string())?;
        match entry.delete_credential() {
            Ok(()) => Ok(()),
            Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Check which sources have tokens configured.
    pub fn get_auth_status() -> AuthStatus {
        let has_token = |source: &Source| -> bool {
            if matches!(source, Source::GitHub) {
                Self::get_github_token()
                    .ok()
                    .flatten()
                    .is_some()
            } else {
                Self::get_token(source)
                    .ok()
                    .flatten()
                    .is_some()
            }
        };

        let anthropic = Self::get_anthropic_key()
            .ok()
            .flatten()
            .is_some();

        AuthStatus {
            github: has_token(&Source::GitHub),
            linear: has_token(&Source::Linear),
            slack: has_token(&Source::Slack),
            notion: has_token(&Source::Notion),
            anthropic,
        }
    }

    /// Retrieve the Anthropic API key from the system keychain.
    /// Returns `Ok(None)` if no key is stored.
    pub fn get_anthropic_key() -> Result<Option<String>, String> {
        let entry =
            keyring::Entry::new(SERVICE_NAME, "anthropic_api_key").map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(password) => Ok(Some(password)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Try to get a GitHub token from the `gh` CLI first, then fall back to the
    /// system keychain.
    pub fn get_github_token() -> Result<Option<String>, String> {
        // Attempt to read the token from `gh auth token`.
        if let Ok(output) = std::process::Command::new("gh")
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

        // Fall back to the keychain.
        Self::get_token(&Source::GitHub)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuthStatus {
    pub github: bool,
    pub linear: bool,
    pub slack: bool,
    pub notion: bool,
    pub anthropic: bool,
}
