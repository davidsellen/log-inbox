use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::PathBuf;

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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IgnoredLinkIdentity {
    pub id: String,
    pub field: String,
    pub value: String,
    pub normalized_value: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BackupVerification {
    pub path: PathBuf,
    pub schema_version: i64,
    pub event_count: u64,
    pub integrity_check: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkspaceProfile {
    pub id: String,
    pub status: String,
    pub root_binding: String,
    pub timezone: String,
    pub daily_root: String,
    pub daily_pattern: String,
    pub template_path: Option<String>,
    pub link_style: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MigrationJournalEntry {
    pub operation_id: String,
    pub migration_name: String,
    pub source_identity: String,
    pub status: String,
    pub details: Value,
    pub started_at: DateTime<Utc>,
    pub completed_at: Option<DateTime<Utc>>,
}
