use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum Source {
    #[serde(rename = "linear")]
    Linear,
    #[serde(rename = "github")]
    GitHub,
    #[serde(rename = "slack")]
    Slack,
    #[serde(rename = "notion")]
    Notion,
}

impl std::fmt::Display for Source {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Source::Linear => write!(f, "linear"),
            Source::GitHub => write!(f, "github"),
            Source::Slack => write!(f, "slack"),
            Source::Notion => write!(f, "notion"),
        }
    }
}

impl std::str::FromStr for Source {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "linear" => Ok(Source::Linear),
            "github" => Ok(Source::GitHub),
            "slack" => Ok(Source::Slack),
            "notion" => Ok(Source::Notion),
            other => Err(format!("unknown source: {other}")),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ActivityKind {
    // Linear
    IssueCreated,
    IssueCompleted,
    IssueCommented,
    IssuePrioritized,
    IssueUpdated,
    // GitHub
    CommitPushed,
    PrOpened,
    PrMerged,
    PrReviewed,
    IssueOpened,
    IssueClosed,
    // Slack
    MessageSent,
    ThreadReplied,
    ReactionAdded,
    // Notion
    PageCreated,
    PageEdited,
    DatabaseUpdated,
}

impl std::fmt::Display for ActivityKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = serde_json::to_string(self).unwrap_or_default();
        // Remove surrounding quotes from JSON serialization
        write!(f, "{}", s.trim_matches('"'))
    }
}

impl std::str::FromStr for ActivityKind {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let quoted = format!("\"{s}\"");
        serde_json::from_str(&quoted).map_err(|e| format!("unknown activity kind: {e}"))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Activity {
    pub id: String,
    pub source: Source,
    pub source_id: String,
    pub kind: ActivityKind,
    pub title: String,
    pub description: Option<String>,
    pub url: String,
    pub project: Option<String>,
    pub occurred_at: DateTime<Utc>,
    pub metadata: serde_json::Value,
    pub synced_at: DateTime<Utc>,
}

impl Activity {
    pub fn new(
        source: Source,
        source_id: String,
        kind: ActivityKind,
        title: String,
        url: String,
        occurred_at: DateTime<Utc>,
    ) -> Self {
        Self {
            id: ulid::Ulid::new().to_string(),
            source,
            source_id,
            kind,
            title,
            description: None,
            url,
            project: None,
            occurred_at,
            metadata: serde_json::Value::Null,
            synced_at: Utc::now(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Period {
    Day(chrono::NaiveDate),
    Week(chrono::NaiveDate), // start of week
    Month(chrono::NaiveDate), // start of month
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct DigestStats {
    pub total_activities: usize,
    pub by_source: std::collections::HashMap<String, usize>,
    pub by_kind: std::collections::HashMap<String, usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Digest {
    pub period: Period,
    pub activities: Vec<Activity>,
    pub stats: DigestStats,
    pub llm_summary: Option<String>,
}
