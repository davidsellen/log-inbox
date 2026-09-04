use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEventInput {
    pub source: String,
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    pub timestamp: Option<DateTime<Utc>>,
    pub message: String,
    #[serde(default)]
    pub metadata: Option<Map<String, Value>>,
    #[serde(default)]
    pub fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredLogEvent {
    pub id: String,
    pub received_at: DateTime<Utc>,
    pub timestamp: DateTime<Utc>,
    pub source: String,
    pub level: String,
    pub message: String,
    pub metadata: Map<String, Value>,
    pub fingerprint: Option<String>,
    pub truncated: bool,
    pub reviewed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceSummary {
    pub source: String,
    pub event_count: u64,
    pub latest_timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogQuery {
    pub source: Option<String>,
    pub since: Option<DateTime<Utc>>,
    pub level: Option<String>,
    pub query: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogQueryResult {
    pub events: Vec<StoredLogEvent>,
    pub truncated: bool,
    pub limit: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarkReviewedResult {
    pub reviewed_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagedEventGroup {
    pub proposal_id: String,
    pub staged_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyConsolidationJob {
    pub id: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub target_note: String,
    pub status: String,
    pub event_count: usize,
    pub proposal_id: Option<String>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LinkSelector {
    pub field: String,
    pub operator: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VaultLinkRule {
    pub id: String,
    pub selectors: Vec<LinkSelector>,
    pub target_note_id: String,
    pub enabled: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}
