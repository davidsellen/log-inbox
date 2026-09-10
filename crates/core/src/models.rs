use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, path::PathBuf};

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
pub struct ContextMapping {
    pub id: String,
    pub workspace_id: String,
    pub selectors: Vec<LinkSelector>,
    pub canonical_note_path: String,
    pub enabled: bool,
    pub source_identity: Option<String>,
    pub source_digest: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IgnoredContextIdentity {
    pub id: String,
    pub workspace_id: String,
    pub field: String,
    pub value: String,
    pub normalized_value: String,
    pub source_identity: Option<String>,
    pub source_digest: Option<String>,
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
pub struct MigrationItem {
    pub operation_id: String,
    pub item_kind: String,
    pub source_identity: String,
    pub source_digest: String,
    pub status: String,
    pub details: Value,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LegacyMigrationArtifact {
    pub operation_id: String,
    pub artifact_kind: String,
    pub source_identity: String,
    pub source_digest: String,
    pub content: Vec<u8>,
    pub parse_status: String,
    pub details: Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct LegacyManualEventImport {
    pub event_id: String,
    pub local_date: NaiveDate,
    pub text: String,
    pub references: Vec<String>,
    pub source_digest: String,
}

#[derive(Debug, Clone)]
pub struct LegacyCutoverImport {
    pub operation_id: String,
    pub source_identity: String,
    pub report_digest: String,
    pub workspace_id: String,
    pub items: Vec<MigrationItem>,
    pub mappings: Vec<ContextMapping>,
    pub ignored: Vec<IgnoredContextIdentity>,
    pub artifacts: Vec<LegacyMigrationArtifact>,
    pub manual_events: Vec<LegacyManualEventImport>,
    pub obsolete_preferences: BTreeMap<String, String>,
    pub backup_path: String,
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
pub struct DailyAutomationSettings {
    pub workspace_id: String,
    pub enabled: bool,
    pub generation_time: String,
    pub catch_up_days: u16,
    pub raw_retention_days: u16,
    pub audit_retention_days: u16,
    pub recovery_retention_days: u16,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DailyScheduleRun {
    pub workspace_id: String,
    pub local_date: NaiveDate,
    pub state: String,
    pub attempts: u16,
    pub scheduled_at: DateTime<Utc>,
    pub timezone: String,
    pub settings_revision: String,
    pub next_attempt_at: DateTime<Utc>,
    pub claim_token: Option<String>,
    pub lease_expires_at: Option<DateTime<Utc>>,
    pub claimed_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
    pub last_error: Option<String>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct DailyOverviewFacts {
    pub local_date: NaiveDate,
    pub day: Option<DailyDay>,
    pub revision: Option<ProposalRevision>,
    pub apply_operation: Option<ApplyOperation>,
    pub schedule_run: Option<DailyScheduleRun>,
    pub event_count: u64,
    pub manual_entry_count: u64,
    pub manual_entries_changed: bool,
    pub new_evidence_count: u64,
    pub expired_evidence_count: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DailyTemplateSnapshot {
    pub workspace_id: String,
    pub local_date: NaiveDate,
    pub template_path: String,
    pub content: Vec<u8>,
    pub content_hash: String,
    pub created_at: DateTime<Utc>,
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
    pub available: bool,
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
    pub expected_target_exists: Option<bool>,
    pub expected_original_content_hash: Option<String>,
    pub intended_updated_content_hash: Option<String>,
    pub temporary_name: Option<String>,
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
    pub expected_target_exists: bool,
    pub expected_original_content_hash: String,
    pub intended_updated_content_hash: String,
    pub temporary_name: String,
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
