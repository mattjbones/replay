use recap_core::config::AppConfig;

#[test]
fn default_config_has_expected_values() {
    let config = AppConfig::default();

    assert_eq!(config.schedule.sync_interval_minutes, 5);
    assert_eq!(config.schedule.daily_reminder_time, "17:00");
    assert_eq!(config.schedule.weekly_reminder_day, "Friday");
    assert_eq!(config.ttl.hot_minutes, 5);
    assert_eq!(config.ttl.warm_minutes, 60);
    assert_eq!(config.ttl.cold_minutes, 1440);
    assert!(!config.llm.enabled);
    assert_eq!(config.working_hours.work_start, "09:00");
    assert_eq!(config.working_hours.work_end, "17:00");
    assert_eq!(config.working_hours.working_days.len(), 5);
    assert!(config.dashboard_layout.is_empty());
}

#[test]
fn config_serializes_to_toml_and_back() {
    let config = AppConfig::default();
    let toml_str = toml::to_string_pretty(&config).expect("should serialize to TOML");
    let parsed: AppConfig = toml::from_str(&toml_str).expect("should parse back from TOML");

    assert_eq!(parsed.schedule.sync_interval_minutes, config.schedule.sync_interval_minutes);
    assert_eq!(parsed.ttl.warm_minutes, config.ttl.warm_minutes);
    assert_eq!(parsed.llm.enabled, config.llm.enabled);
    assert_eq!(parsed.working_hours.timezone, config.working_hours.timezone);
}

#[test]
fn config_serializes_to_json_and_back() {
    // The frontend receives config as JSON via IPC
    let config = AppConfig::default();
    let json_str = serde_json::to_string(&config).expect("should serialize to JSON");
    let parsed: AppConfig = serde_json::from_str(&json_str).expect("should parse back from JSON");

    assert_eq!(parsed.schedule.sync_interval_minutes, config.schedule.sync_interval_minutes);
    assert_eq!(parsed.github.username, config.github.username);
}

#[test]
fn config_deserializes_with_missing_fields() {
    // Minimal TOML should use defaults for all missing fields
    let minimal = "";
    let config: AppConfig = toml::from_str(minimal).expect("empty TOML should use defaults");
    assert_eq!(config.schedule.sync_interval_minutes, 5);
    assert!(!config.llm.enabled);
}

#[test]
fn config_deserializes_partial_toml() {
    let partial = r#"
[schedule]
sync_interval_minutes = 15

[llm]
enabled = true
"#;
    let config: AppConfig = toml::from_str(partial).expect("partial TOML should work");
    assert_eq!(config.schedule.sync_interval_minutes, 15);
    assert!(config.llm.enabled);
    // Other fields should have defaults
    assert_eq!(config.ttl.warm_minutes, 60);
}

#[test]
fn github_workflow_enum_serialization() {
    use recap_core::config::GitHubWorkflow;

    let pr = GitHubWorkflow::Pr;
    let json = serde_json::to_string(&pr).unwrap();
    assert_eq!(json, r#""pr""#);

    let trunk = GitHubWorkflow::Trunk;
    let json = serde_json::to_string(&trunk).unwrap();
    assert_eq!(json, r#""trunk""#);
}
