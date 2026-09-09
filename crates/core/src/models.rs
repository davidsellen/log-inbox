use chrono::{DateTime, NaiveDate, Utc};
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DashboardSession {
    pub token_digest: String,
    pub csrf_digest: String,
    pub scopes: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
    pub idle_expires_at: DateTime<Utc>,
    pub absolute_expires_at: DateTime<Utc>,
    pub revoked_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DailyDay {
    pub workspace_id: String,
    pub local_date: NaiveDate,
    pub timezone: String,
    pub start_utc: DateTime<Utc>,
    pub end_utc: DateTime<Utc>,
    pub destination_path: String,
    pub template_revision: Option<String>,
    pub block_id: String,
    pub generation_status: String,
    pub review_status: String,
    pub freshness: String,
    pub current_revision_id: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EvidenceSnapshot {
    pub id: String,
    pub workspace_id: String,
    pub local_date: NaiveDate,
    pub snapshot_digest: String,
    pub event_ids: Vec<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SnapshotEvidence {
    pub event_id: String,
    pub position: u64,
    pub event_digest: String,
    pub disposition: Option<String>,
    pub related_event_id: Option<String>,
    pub decision_actor: Option<String>,
    pub decision_reason: Option<String>,
    pub decided_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManualDailyEntry {
    pub id: String,
    pub workspace_id: String,
    pub local_date: NaiveDate,
    pub text: String,
    pub references: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DailyRevisionContent {
    pub schema_version: u64,
    #[serde(default)]
    pub workstreams: Vec<DailyWorkstream>,
    #[serde(default)]
    pub manual_entry_ids: Vec<String>,
    #[serde(default)]
    pub open_questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DailyWorkstream {
    pub id: String,
    pub title: String,
    pub evidence_event_ids: Vec<String>,
    #[serde(default)]
    pub canonical_links: Vec<String>,
    #[serde(default)]
    pub outcome: Vec<DailyFact>,
    #[serde(default)]
    pub decision: Vec<DailyFact>,
    #[serde(default)]
    pub trade_off: Vec<DailyFact>,
    #[serde(default)]
    pub validation: Vec<DailyFact>,
    #[serde(default)]
    pub blocker: Vec<DailyFact>,
    #[serde(default)]
    pub follow_up: Vec<DailyFact>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct DailyFact {
    pub text: String,
    pub evidence_event_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApplyOperation {
    pub id: String,
    pub workspace_id: String,
    pub local_date: NaiveDate,
    pub revision_id: String,
    pub revision_content_hash: String,
    pub destination_path: String,
    pub expected_old_block_hash: Option<String>,
    pub intended_new_block_hash: String,
    pub recovery_payload: Option<Vec<u8>>,
    pub recovery_path: Option<String>,
    pub state: String,
    pub failure_reason: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareApplyOperation {
    pub id: String,
    pub workspace_id: String,
    pub local_date: NaiveDate,
    pub revision_id: String,
    pub revision_content_hash: String,
    pub destination_path: String,
    pub expected_old_block_hash: Option<String>,
    pub intended_new_block_hash: String,
    pub recovery_payload: Option<Vec<u8>>,
    pub recovery_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ProposalRevision {
    pub id: String,
    pub workspace_id: String,
    pub local_date: NaiveDate,
    pub snapshot_id: Option<String>,
    pub revision_number: u64,
    pub origin: String,
    pub content: Value,
    pub content_hash: String,
    pub created_at: DateTime<Utc>,
}
