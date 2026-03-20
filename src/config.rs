use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default = "default_schedule")]
    pub schedule: ScheduleConfig,
    #[serde(default)]
    pub ttl: TtlConfig,
    #[serde(default)]
    pub github: GitHubConfig,
    #[serde(default)]
    pub linear: LinearConfig,
    #[serde(default)]
    pub slack: SlackConfig,
    #[serde(default)]
    pub notion: NotionConfig,
    #[serde(default)]
    pub llm: LlmConfig,
    #[serde(default)]
    pub dashboard_layout: HashMap<String, CardPosition>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CardPosition {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

fn default_schedule() -> ScheduleConfig {
    ScheduleConfig::default()
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schedule: ScheduleConfig::default(),
            ttl: TtlConfig::default(),
            github: GitHubConfig::default(),
            linear: LinearConfig::default(),
            slack: SlackConfig::default(),
            notion: NotionConfig::default(),
            llm: LlmConfig::default(),
            dashboard_layout: HashMap::new(),
        }
    }
}

impl AppConfig {
    /// Returns the configuration directory: ~/.config/recap/
    pub fn config_dir() -> PathBuf {
        dirs::config_dir()
            .expect("could not determine config directory")
            .join("recap")
    }

    /// Returns the database path: ~/.config/recap/recap.db
    pub fn db_path() -> PathBuf {
        Self::config_dir().join("recap.db")
    }

    /// Load configuration from ~/.config/recap/config.toml.
    /// Creates a default config file if it does not exist.
    pub fn load() -> Self {
        let config_dir = Self::config_dir();
        let config_path = config_dir.join("config.toml");

        if !config_path.exists() {
            let default_config = AppConfig::default();
            default_config.save();
            return default_config;
        }

        match std::fs::read_to_string(&config_path) {
            Ok(contents) => match toml::from_str::<AppConfig>(&contents) {
                Ok(config) => config,
                Err(e) => {
                    tracing::warn!(
                        "failed to parse config at {}: {e}, using defaults",
                        config_path.display()
                    );
                    AppConfig::default()
                }
            },
            Err(e) => {
                tracing::warn!(
                    "failed to read config at {}: {e}, using defaults",
                    config_path.display()
                );
                AppConfig::default()
            }
        }
    }

    /// Save the current configuration to ~/.config/recap/config.toml.
    pub fn save(&self) {
        let config_dir = Self::config_dir();
        if let Err(e) = std::fs::create_dir_all(&config_dir) {
            tracing::error!("failed to create config directory: {e}");
            return;
        }

        let config_path = config_dir.join("config.toml");
        match toml::to_string_pretty(self) {
            Ok(contents) => {
                if let Err(e) = std::fs::write(&config_path, contents) {
                    tracing::error!("failed to write config to {}: {e}", config_path.display());
                }
            }
            Err(e) => {
                tracing::error!("failed to serialize config: {e}");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Schedule
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduleConfig {
    #[serde(default = "default_sync_interval_minutes")]
    pub sync_interval_minutes: u64,
    #[serde(default = "default_daily_reminder_time")]
    pub daily_reminder_time: String,
    #[serde(default = "default_weekly_reminder_day")]
    pub weekly_reminder_day: String,
}

impl Default for ScheduleConfig {
    fn default() -> Self {
        Self {
            sync_interval_minutes: default_sync_interval_minutes(),
            daily_reminder_time: default_daily_reminder_time(),
            weekly_reminder_day: default_weekly_reminder_day(),
        }
    }
}

fn default_sync_interval_minutes() -> u64 {
    5
}

fn default_daily_reminder_time() -> String {
    "17:00".to_string()
}

fn default_weekly_reminder_day() -> String {
    "Friday".to_string()
}

// ---------------------------------------------------------------------------
// TTL
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtlConfig {
    #[serde(default = "default_hot_minutes")]
    pub hot_minutes: i64,
    #[serde(default = "default_warm_minutes")]
    pub warm_minutes: i64,
    #[serde(default = "default_cold_minutes")]
    pub cold_minutes: i64,
}

impl Default for TtlConfig {
    fn default() -> Self {
        Self {
            hot_minutes: default_hot_minutes(),
            warm_minutes: default_warm_minutes(),
            cold_minutes: default_cold_minutes(),
        }
    }
}

fn default_hot_minutes() -> i64 {
    5
}

fn default_warm_minutes() -> i64 {
    60
}

fn default_cold_minutes() -> i64 {
    1440
}

// ---------------------------------------------------------------------------
// Integration configs
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GitHubConfig {
    #[serde(default)]
    pub username: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LinearConfig {
    // Tokens are stored in the system keychain, not in config.
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackConfig {
    #[serde(default)]
    pub user_id: Option<String>,
    #[serde(default)]
    pub ignored_channels: Vec<String>,
    /// Slack app client ID (needed for token rotation/refresh).
    #[serde(default)]
    pub client_id: Option<String>,
    /// Slack app client secret (needed for token rotation/refresh).
    #[serde(default)]
    pub client_secret: Option<String>,
}

impl Default for SlackConfig {
    fn default() -> Self {
        Self {
            user_id: None,
            ignored_channels: Vec::new(),
            client_id: None,
            client_secret: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NotionConfig {
    // Tokens are stored in the system keychain, not in config.
}

// ---------------------------------------------------------------------------
// LLM
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_llm_model")]
    pub model: String,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            model: default_llm_model(),
        }
    }
}

fn default_llm_model() -> String {
    "claude-haiku-4-5-20251001".to_string()
}
