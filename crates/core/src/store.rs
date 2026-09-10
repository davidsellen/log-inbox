use crate::{
    auth::{SessionCredentials, normalize_scopes, token_digest, token_matches},
    daily::render_daily_path,
    models::{
        BackupVerification, DailyConsolidationJob, DashboardSession, IgnoredLinkIdentity,
        LogEventInput, LogQuery, LogQueryResult, MarkReviewedResult, MigrationJournalEntry,
        SourceSummary, StagedEventGroup, StoredLogEvent, VaultLinkRule, WorkspaceProfile,
    },
    redaction::{redact_metadata, redact_text},
};
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, OptionalExtension, Row, backup::Backup, params};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Duration as StdDuration,
};
use uuid::Uuid;

#[cfg(test)]
use serde_json::{Map, Value};

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 500;
const MAX_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_METADATA_BYTES: usize = 512 * 1024;
const MAX_SOURCE_BYTES: usize = 512;
const MAX_FINGERPRINT_BYTES: usize = 1024;

#[derive(Debug, Clone)]
pub struct Store {
    db_path: PathBuf,
}

impl Store {
    pub fn database_path(&self) -> &Path {
        &self.db_path
    }

    pub fn open(db_path: PathBuf) -> Result<Self> {
        if let Some(parent) = db_path.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating data dir {}", parent.display()))?;
        }

        let store = Self { db_path };
        store.initialize()?;
        Ok(store)
    }

    pub fn initialize(&self) -> Result<()> {
        let mut conn = self.connect()?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS schema_migrations (
                version INTEGER PRIMARY KEY,
                name TEXT NOT NULL,
                applied_at TEXT NOT NULL
            );
            "#,
        )?;

        let current = schema_version(&conn)?;
        if current < 1 {
            let transaction = conn.transaction()?;
            transaction.execute_batch(
                r#"
            CREATE TABLE IF NOT EXISTS log_events (
                id TEXT PRIMARY KEY,
                received_at TEXT NOT NULL,
                timestamp TEXT NOT NULL,
                source TEXT NOT NULL,
                level TEXT NOT NULL,
                message TEXT NOT NULL,
                metadata_json TEXT NOT NULL,
                fingerprint TEXT,
                truncated INTEGER NOT NULL DEFAULT 0
            );

            CREATE INDEX IF NOT EXISTS idx_log_events_source_timestamp
                ON log_events(source, timestamp);
            CREATE INDEX IF NOT EXISTS idx_log_events_level_timestamp
                ON log_events(level, timestamp);
            CREATE INDEX IF NOT EXISTS idx_log_events_timestamp
                ON log_events(timestamp);

            CREATE TABLE IF NOT EXISTS review_state (
                event_id TEXT PRIMARY KEY,
                reviewed_at TEXT NOT NULL,
                reviewed_by TEXT NOT NULL,
                note TEXT NOT NULL,
                FOREIGN KEY(event_id) REFERENCES log_events(id)
            );

            CREATE TABLE IF NOT EXISTS proposal_state (
                event_id TEXT PRIMARY KEY,
                proposal_id TEXT NOT NULL,
                staged_at TEXT NOT NULL,
                FOREIGN KEY(event_id) REFERENCES log_events(id)
            );

            CREATE INDEX IF NOT EXISTS idx_proposal_state_proposal
                ON proposal_state(proposal_id);

            CREATE TABLE IF NOT EXISTS app_preferences (
                key TEXT PRIMARY KEY,
                value TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS daily_consolidation_jobs (
                id TEXT PRIMARY KEY,
                snapshot_key TEXT NOT NULL UNIQUE,
                start TEXT NOT NULL,
                end TEXT NOT NULL,
                target_note TEXT NOT NULL,
                status TEXT NOT NULL,
                event_count INTEGER NOT NULL,
                proposal_id TEXT,
                error TEXT,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_daily_consolidation_jobs_updated
                ON daily_consolidation_jobs(updated_at DESC);

            CREATE TABLE IF NOT EXISTS daily_consolidation_job_events (
                job_id TEXT NOT NULL,
                event_id TEXT NOT NULL,
                position INTEGER NOT NULL,
                PRIMARY KEY(job_id, event_id),
                FOREIGN KEY(job_id) REFERENCES daily_consolidation_jobs(id),
                FOREIGN KEY(event_id) REFERENCES log_events(id)
            );

            CREATE TABLE IF NOT EXISTS vault_link_rules (
                id TEXT PRIMARY KEY,
                selectors_json TEXT NOT NULL,
                target_note_id TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                created_at TEXT NOT NULL,
                updated_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_vault_link_rules_target
                ON vault_link_rules(target_note_id);

            CREATE TABLE IF NOT EXISTS ignored_link_identities (
                id TEXT PRIMARY KEY,
                field TEXT NOT NULL,
                value TEXT NOT NULL,
                normalized_value TEXT NOT NULL,
                created_at TEXT NOT NULL,
                UNIQUE(field, normalized_value)
            );
            "#,
            )?;
            transaction.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (1, 'legacy baseline', ?1)",
                params![Utc::now().to_rfc3339()],
            )?;
            transaction.commit()?;
        }

        if current < 2 {
            let transaction = conn.transaction()?;
            transaction.execute_batch(
                r#"
                CREATE TABLE migration_journal (
                    operation_id TEXT PRIMARY KEY,
                    migration_name TEXT NOT NULL,
                    source_identity TEXT NOT NULL,
                    status TEXT NOT NULL CHECK(status IN ('started', 'completed', 'failed')),
                    details_json TEXT NOT NULL,
                    started_at TEXT NOT NULL,
                    completed_at TEXT
                );
                "#,
            )?;
            transaction.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (2, 'migration journal', ?1)",
                params![Utc::now().to_rfc3339()],
            )?;
            transaction.commit()?;
        }

        if current < 3 {
            let transaction = conn.transaction()?;
            transaction.execute_batch(
                r#"
                CREATE TABLE workspace_profiles (
                    id TEXT PRIMARY KEY,
                    status TEXT NOT NULL CHECK(status IN ('pending_review', 'active', 'disabled')),
                    root_binding TEXT NOT NULL,
                    timezone TEXT NOT NULL,
                    daily_root TEXT NOT NULL,
                    daily_pattern TEXT NOT NULL,
                    template_path TEXT,
                    link_style TEXT NOT NULL CHECK(link_style IN ('markdown', 'wikilink')),
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE UNIQUE INDEX idx_workspace_profiles_one_active
                    ON workspace_profiles(status) WHERE status = 'active';
                "#,
            )?;
            transaction.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (3, 'workspace profiles', ?1)",
                params![Utc::now().to_rfc3339()],
            )?;
            transaction.commit()?;
        }

        if current < 4 {
            let transaction = conn.transaction()?;
            transaction.execute_batch(
                r#"
                CREATE TABLE owner_security (
                    singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                    secret_hash TEXT NOT NULL,
                    updated_at TEXT NOT NULL
                );

                CREATE TABLE dashboard_sessions (
                    token_digest TEXT PRIMARY KEY,
                    csrf_digest TEXT NOT NULL,
                    scopes_json TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    last_seen_at TEXT NOT NULL,
                    idle_expires_at TEXT NOT NULL,
                    absolute_expires_at TEXT NOT NULL,
                    revoked_at TEXT
                );

                CREATE INDEX idx_dashboard_sessions_expiry
                    ON dashboard_sessions(idle_expires_at, absolute_expires_at);
                "#,
            )?;
            transaction.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (4, 'dashboard authentication', ?1)",
                params![Utc::now().to_rfc3339()],
            )?;
            transaction.commit()?;
        }

        if current < 5 {
            let transaction = conn.transaction()?;
            transaction.execute_batch(
                r#"
                CREATE TABLE daily_days (
                    workspace_id TEXT NOT NULL,
                    local_date TEXT NOT NULL,
                    timezone TEXT NOT NULL,
                    start_utc TEXT NOT NULL,
                    end_utc TEXT NOT NULL,
                    destination_path TEXT NOT NULL,
                    template_revision TEXT,
                    block_id TEXT NOT NULL,
                    generation_status TEXT NOT NULL CHECK(generation_status IN ('none', 'queued', 'running', 'ready', 'failed')),
                    review_status TEXT NOT NULL CHECK(review_status IN ('unresolved', 'in_review', 'ready_to_apply', 'dismissed', 'applied')),
                    freshness TEXT NOT NULL CHECK(freshness IN ('current', 'update_available')),
                    current_revision_id TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    PRIMARY KEY(workspace_id, local_date),
                    FOREIGN KEY(workspace_id) REFERENCES workspace_profiles(id)
                );

                CREATE TABLE evidence_snapshots (
                    id TEXT PRIMARY KEY,
                    workspace_id TEXT NOT NULL,
                    local_date TEXT NOT NULL,
                    snapshot_digest TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    UNIQUE(workspace_id, local_date, snapshot_digest),
                    FOREIGN KEY(workspace_id, local_date) REFERENCES daily_days(workspace_id, local_date)
                );

                CREATE TABLE evidence_snapshot_events (
                    snapshot_id TEXT NOT NULL,
                    event_id TEXT NOT NULL,
                    position INTEGER NOT NULL,
                    event_digest TEXT NOT NULL,
                    disposition TEXT CHECK(disposition IN ('include', 'omit', 'duplicate_of', 'superseded_by')),
                    related_event_id TEXT,
                    decision_actor TEXT,
                    decision_reason TEXT,
                    decided_at TEXT,
                    PRIMARY KEY(snapshot_id, event_id),
                    UNIQUE(snapshot_id, position),
                    FOREIGN KEY(snapshot_id) REFERENCES evidence_snapshots(id)
                );

                CREATE TABLE proposal_revisions (
                    id TEXT PRIMARY KEY,
                    workspace_id TEXT NOT NULL,
                    local_date TEXT NOT NULL,
                    snapshot_id TEXT,
                    revision_number INTEGER NOT NULL,
                    origin TEXT NOT NULL CHECK(origin IN ('generated', 'structured_edit', 'manual', 'regenerated', 'advanced_markdown')),
                    content_json TEXT NOT NULL,
                    content_hash TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    UNIQUE(workspace_id, local_date, revision_number),
                    FOREIGN KEY(workspace_id, local_date) REFERENCES daily_days(workspace_id, local_date),
                    FOREIGN KEY(snapshot_id) REFERENCES evidence_snapshots(id)
                );

                CREATE INDEX idx_daily_days_status
                    ON daily_days(workspace_id, review_status, freshness, local_date DESC);
                CREATE INDEX idx_evidence_snapshots_day
                    ON evidence_snapshots(workspace_id, local_date, created_at DESC);
                "#,
            )?;
            transaction.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (5, 'immutable daily records', ?1)",
                params![Utc::now().to_rfc3339()],
            )?;
            transaction.commit()?;
        }
        if current < 6 {
            let transaction = conn.transaction()?;
            transaction.execute_batch(
                r#"
                CREATE TABLE manual_daily_entries (
                    id TEXT PRIMARY KEY,
                    workspace_id TEXT NOT NULL,
                    local_date TEXT NOT NULL,
                    text TEXT NOT NULL,
                    references_json TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    FOREIGN KEY(workspace_id, local_date) REFERENCES daily_days(workspace_id, local_date)
                );

                CREATE INDEX idx_manual_daily_entries_day
                    ON manual_daily_entries(workspace_id, local_date, created_at);
                "#,
            )?;
            transaction.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (6, 'trusted manual daily entries', ?1)",
                params![Utc::now().to_rfc3339()],
            )?;
            transaction.commit()?;
        }
        if current < 7 {
            let transaction = conn.transaction()?;
            transaction.execute_batch(
                r#"
                DELETE FROM review_state
                WHERE event_id NOT IN (SELECT id FROM log_events);
                DELETE FROM proposal_state
                WHERE event_id NOT IN (SELECT id FROM log_events);
                DELETE FROM daily_consolidation_job_events
                WHERE job_id NOT IN (SELECT id FROM daily_consolidation_jobs)
                   OR event_id NOT IN (SELECT id FROM log_events);

                CREATE TABLE evidence_snapshot_events_v7 (
                    snapshot_id TEXT NOT NULL,
                    event_id TEXT NOT NULL,
                    live_event_id TEXT,
                    position INTEGER NOT NULL,
                    event_digest TEXT NOT NULL,
                    disposition TEXT CHECK(disposition IN ('include', 'omit', 'duplicate_of', 'superseded_by')),
                    related_event_id TEXT,
                    decision_actor TEXT,
                    decision_reason TEXT,
                    decided_at TEXT,
                    PRIMARY KEY(snapshot_id, event_id),
                    UNIQUE(snapshot_id, position),
                    CHECK(live_event_id IS NULL OR live_event_id = event_id),
                    FOREIGN KEY(snapshot_id) REFERENCES evidence_snapshots(id),
                    FOREIGN KEY(live_event_id) REFERENCES log_events(id) ON DELETE SET NULL,
                    FOREIGN KEY(snapshot_id, related_event_id)
                        REFERENCES evidence_snapshot_events_v7(snapshot_id, event_id)
                );

                INSERT INTO evidence_snapshot_events_v7
                    (snapshot_id, event_id, live_event_id, position, event_digest,
                     disposition, related_event_id, decision_actor, decision_reason, decided_at)
                SELECT snapshot_id,
                       event_id,
                       CASE WHEN EXISTS (SELECT 1 FROM log_events WHERE id = event_id)
                            THEN event_id ELSE NULL END,
                       position,
                       event_digest,
                       disposition,
                       related_event_id,
                       decision_actor,
                       decision_reason,
                       decided_at
                FROM evidence_snapshot_events;

                DROP TABLE evidence_snapshot_events;
                ALTER TABLE evidence_snapshot_events_v7 RENAME TO evidence_snapshot_events;

                CREATE TRIGGER cleanup_legacy_event_state_before_delete
                BEFORE DELETE ON log_events
                BEGIN
                    DELETE FROM review_state WHERE event_id = OLD.id;
                    DELETE FROM proposal_state WHERE event_id = OLD.id;
                    DELETE FROM daily_consolidation_job_events WHERE event_id = OLD.id;
                END;
                "#,
            )?;
            transaction.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (7, 'enforced foreign key integrity', ?1)",
                params![Utc::now().to_rfc3339()],
            )?;
            transaction.commit()?;
        }
        if current < 8 {
            let transaction = conn.transaction()?;
            transaction.execute_batch(
                r#"
                CREATE TABLE apply_operations (
                    id TEXT PRIMARY KEY,
                    workspace_id TEXT NOT NULL,
                    local_date TEXT NOT NULL,
                    revision_id TEXT NOT NULL,
                    revision_content_hash TEXT NOT NULL,
                    destination_path TEXT NOT NULL,
                    expected_old_block_hash TEXT,
                    intended_new_block_hash TEXT NOT NULL,
                    recovery_payload BLOB,
                    recovery_path TEXT,
                    state TEXT NOT NULL CHECK(state IN (
                        'prepared', 'writing', 'written', 'finalized', 'failed',
                        'reconciliation_required'
                    )),
                    failure_reason TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    CHECK((recovery_payload IS NOT NULL) <> (recovery_path IS NOT NULL)),
                    FOREIGN KEY(workspace_id, local_date)
                        REFERENCES daily_days(workspace_id, local_date),
                    FOREIGN KEY(revision_id) REFERENCES proposal_revisions(id)
                );

                CREATE INDEX idx_apply_operations_day
                    ON apply_operations(workspace_id, local_date, created_at DESC);
                CREATE INDEX idx_apply_operations_state
                    ON apply_operations(state, updated_at);

                CREATE TRIGGER apply_operations_immutable_inputs
                BEFORE UPDATE OF id, workspace_id, local_date, revision_id,
                    revision_content_hash, destination_path, expected_old_block_hash,
                    intended_new_block_hash, recovery_payload, recovery_path
                ON apply_operations
                BEGIN
                    SELECT RAISE(ABORT, 'apply operation inputs are immutable');
                END;
                "#,
            )?;
            transaction.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (8, 'apply operation journal', ?1)",
                params![Utc::now().to_rfc3339()],
            )?;
            transaction.commit()?;
        }
        if current < 9 {
            let transaction = conn.transaction()?;
            transaction.execute_batch(
                r#"
                ALTER TABLE apply_operations ADD COLUMN expected_target_exists INTEGER
                    CHECK(expected_target_exists IN (0, 1));
                ALTER TABLE apply_operations ADD COLUMN expected_original_content_hash TEXT;
                ALTER TABLE apply_operations ADD COLUMN intended_updated_content_hash TEXT;
                ALTER TABLE apply_operations ADD COLUMN temporary_name TEXT;

                UPDATE apply_operations
                SET state = 'reconciliation_required',
                    failure_reason = 'legacy Apply journal lacks exact file identity; review manually',
                    updated_at = strftime('%Y-%m-%dT%H:%M:%fZ', 'now')
                WHERE state != 'finalized';

                DROP TRIGGER apply_operations_immutable_inputs;
                CREATE TRIGGER apply_operations_immutable_inputs
                BEFORE UPDATE OF id, workspace_id, local_date, revision_id,
                    revision_content_hash, destination_path, expected_old_block_hash,
                    intended_new_block_hash, recovery_payload, recovery_path,
                    expected_target_exists, expected_original_content_hash,
                    intended_updated_content_hash, temporary_name
                ON apply_operations
                BEGIN
                    SELECT RAISE(ABORT, 'apply operation inputs are immutable');
                END;

                CREATE TRIGGER apply_operations_complete_identity
                BEFORE INSERT ON apply_operations
                WHEN NEW.expected_target_exists IS NULL
                    OR NEW.expected_original_content_hash IS NULL
                    OR NEW.intended_updated_content_hash IS NULL
                    OR NEW.temporary_name IS NULL
                BEGIN
                    SELECT RAISE(ABORT, 'apply operation requires complete file identity');
                END;
                "#,
            )?;
            transaction.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (9, 'complete apply operation identity', ?1)",
                params![Utc::now().to_rfc3339()],
            )?;
            transaction.commit()?;
        }
        if current < 10 {
            let transaction = conn.transaction()?;
            transaction.execute_batch(
                r#"
                CREATE TABLE daily_template_snapshots (
                    workspace_id TEXT NOT NULL,
                    local_date TEXT NOT NULL,
                    template_path TEXT NOT NULL,
                    content BLOB NOT NULL,
                    content_hash TEXT NOT NULL,
                    created_at TEXT NOT NULL,
                    PRIMARY KEY(workspace_id, local_date),
                    FOREIGN KEY(workspace_id, local_date)
                        REFERENCES daily_days(workspace_id, local_date) ON DELETE CASCADE
                );

                CREATE TRIGGER daily_template_snapshots_immutable
                BEFORE UPDATE ON daily_template_snapshots
                BEGIN
                    SELECT RAISE(ABORT, 'daily template snapshots are immutable');
                END;
                "#,
            )?;
            transaction.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (10, 'frozen daily templates', ?1)",
                params![Utc::now().to_rfc3339()],
            )?;
            transaction.commit()?;
        }
        if current < 11 {
            let transaction = conn.transaction()?;
            transaction.execute_batch(
                r#"
                CREATE TABLE context_mappings (
                    id TEXT PRIMARY KEY,
                    workspace_id TEXT NOT NULL,
                    selectors_json TEXT NOT NULL,
                    canonical_note_path TEXT NOT NULL,
                    enabled INTEGER NOT NULL DEFAULT 1,
                    source_identity TEXT,
                    source_digest TEXT,
                    created_at TEXT NOT NULL,
                    updated_at TEXT NOT NULL,
                    UNIQUE(workspace_id, selectors_json),
                    FOREIGN KEY(workspace_id) REFERENCES workspace_profiles(id) ON DELETE CASCADE
                );
                CREATE INDEX idx_context_mappings_workspace_note
                    ON context_mappings(workspace_id, canonical_note_path);

                CREATE TABLE ignored_context_identities (
                    id TEXT PRIMARY KEY,
                    workspace_id TEXT NOT NULL,
                    field TEXT NOT NULL,
                    value TEXT NOT NULL,
                    normalized_value TEXT NOT NULL,
                    source_identity TEXT,
                    source_digest TEXT,
                    created_at TEXT NOT NULL,
                    UNIQUE(workspace_id, field, normalized_value),
                    FOREIGN KEY(workspace_id) REFERENCES workspace_profiles(id) ON DELETE CASCADE
                );

                CREATE TABLE migration_items (
                    operation_id TEXT NOT NULL,
                    item_kind TEXT NOT NULL,
                    source_identity TEXT NOT NULL,
                    source_digest TEXT NOT NULL,
                    status TEXT NOT NULL CHECK(status IN (
                        'inventoried', 'imported', 'cleanup_pending', 'cleaned',
                        'preserved', 'error'
                    )),
                    details_json TEXT NOT NULL DEFAULT '{}',
                    updated_at TEXT NOT NULL,
                    PRIMARY KEY(operation_id, item_kind, source_identity),
                    FOREIGN KEY(operation_id) REFERENCES migration_journal(operation_id) ON DELETE CASCADE
                );
                "#,
            )?;
            transaction.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (11, 'workspace scoped context and migration items', ?1)",
                params![Utc::now().to_rfc3339()],
            )?;
            transaction.commit()?;
        }
        if current < 12 {
            let transaction = conn.transaction()?;
            transaction.execute_batch(
                r#"
                CREATE TABLE legacy_migration_artifacts (
                    operation_id TEXT NOT NULL,
                    artifact_kind TEXT NOT NULL,
                    source_identity TEXT NOT NULL,
                    source_digest TEXT NOT NULL,
                    content BLOB NOT NULL,
                    parse_status TEXT NOT NULL CHECK(parse_status IN ('valid', 'unparseable')),
                    details_json TEXT NOT NULL DEFAULT '{}',
                    created_at TEXT NOT NULL,
                    PRIMARY KEY(operation_id, artifact_kind, source_identity),
                    FOREIGN KEY(operation_id, artifact_kind, source_identity)
                        REFERENCES migration_items(operation_id, item_kind, source_identity)
                        ON DELETE CASCADE
                );

                CREATE TABLE legacy_manual_event_imports (
                    event_id TEXT PRIMARY KEY,
                    operation_id TEXT NOT NULL,
                    workspace_id TEXT NOT NULL,
                    manual_entry_id TEXT NOT NULL UNIQUE,
                    source_digest TEXT NOT NULL,
                    imported_at TEXT NOT NULL,
                    FOREIGN KEY(event_id) REFERENCES log_events(id) ON DELETE RESTRICT,
                    FOREIGN KEY(operation_id) REFERENCES migration_journal(operation_id) ON DELETE RESTRICT,
                    FOREIGN KEY(workspace_id) REFERENCES workspace_profiles(id) ON DELETE RESTRICT,
                    FOREIGN KEY(manual_entry_id) REFERENCES manual_daily_entries(id) ON DELETE RESTRICT
                );
                "#,
            )?;
            transaction.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (12, 'preserved legacy migration sources', ?1)",
                params![Utc::now().to_rfc3339()],
            )?;
            transaction.commit()?;
        }
        if current < 13 {
            let transaction = conn.transaction()?;
            transaction.execute_batch(
                r#"
                CREATE TABLE daily_automation_settings (
                    workspace_id TEXT PRIMARY KEY,
                    enabled INTEGER NOT NULL CHECK(enabled IN (0, 1)),
                    generation_time TEXT NOT NULL,
                    catch_up_days INTEGER NOT NULL CHECK(catch_up_days BETWEEN 1 AND 90),
                    raw_retention_days INTEGER NOT NULL CHECK(raw_retention_days BETWEEN 1 AND 3650),
                    audit_retention_days INTEGER NOT NULL CHECK(audit_retention_days BETWEEN 1 AND 3650),
                    recovery_retention_days INTEGER NOT NULL CHECK(recovery_retention_days BETWEEN 1 AND 3650),
                    updated_at TEXT NOT NULL,
                    FOREIGN KEY(workspace_id) REFERENCES workspace_profiles(id) ON DELETE CASCADE
                );

                CREATE TABLE daily_schedule_runs (
                    workspace_id TEXT NOT NULL,
                    local_date TEXT NOT NULL,
                    state TEXT NOT NULL CHECK(state IN ('pending', 'claimed', 'completed', 'failed', 'dismissed')),
                    attempts INTEGER NOT NULL DEFAULT 0,
                    next_attempt_at TEXT NOT NULL,
                    claimed_at TEXT,
                    completed_at TEXT,
                    last_error TEXT,
                    updated_at TEXT NOT NULL,
                    PRIMARY KEY(workspace_id, local_date),
                    FOREIGN KEY(workspace_id) REFERENCES workspace_profiles(id) ON DELETE CASCADE
                );

                CREATE INDEX idx_daily_schedule_runs_due
                    ON daily_schedule_runs(state, next_attempt_at);
                "#,
            )?;
            transaction.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (13, 'daily automation schedule', ?1)",
                params![Utc::now().to_rfc3339()],
            )?;
            transaction.commit()?;
        }
        if current < 14 {
            let transaction = conn.transaction()?;
            transaction.execute_batch(
                r#"
                ALTER TABLE daily_schedule_runs ADD COLUMN scheduled_at TEXT;
                ALTER TABLE daily_schedule_runs ADD COLUMN timezone TEXT;
                ALTER TABLE daily_schedule_runs ADD COLUMN settings_revision TEXT;
                ALTER TABLE daily_schedule_runs ADD COLUMN claim_token TEXT;
                ALTER TABLE daily_schedule_runs ADD COLUMN lease_expires_at TEXT;

                UPDATE daily_schedule_runs
                SET scheduled_at = next_attempt_at,
                    timezone = 'UTC',
                    settings_revision = 'legacy-v13'
                WHERE scheduled_at IS NULL;

                CREATE UNIQUE INDEX idx_daily_schedule_runs_claim_token
                    ON daily_schedule_runs(claim_token) WHERE claim_token IS NOT NULL;
                "#,
            )?;
            transaction.execute(
                "INSERT INTO schema_migrations (version, name, applied_at) VALUES (14, 'safe Daily schedule claims', ?1)",
                params![Utc::now().to_rfc3339()],
            )?;
            transaction.commit()?;
        }
        ensure_foreign_key_integrity(&conn)?;
        Ok(())
    }

    pub fn schema_version(&self) -> Result<i64> {
        schema_version(&self.connect()?)
    }

    pub fn create_verified_backup(&self, destination: &Path) -> Result<BackupVerification> {
        anyhow::ensure!(
            !destination.exists(),
            "backup destination already exists: {}",
            destination.display()
        );
        if let Some(parent) = destination.parent() {
            fs::create_dir_all(parent)
                .with_context(|| format!("creating backup directory {}", parent.display()))?;
        }

        let source = self.connect()?;
        let source_event_count = event_count(&source)?;
        let mut target = Connection::open(destination)
            .with_context(|| format!("creating backup {}", destination.display()))?;
        let backup_result = (|| -> Result<()> {
            let backup = Backup::new(&source, &mut target)?;
            backup.run_to_completion(128, StdDuration::from_millis(10), None)?;
            Ok(())
        })();
        drop(target);
        if let Err(error) = backup_result {
            let _ = fs::remove_file(destination);
            return Err(error).context("backing up SQLite database");
        }

        let verification = verify_backup(destination)?;
        anyhow::ensure!(
            verification.event_count == source_event_count,
            "backup event count mismatch: expected {}, found {}",
            source_event_count,
            verification.event_count
        );
        Ok(verification)
    }

    pub fn create_pending_workspace_profile(
        &self,
        root_binding: &str,
        timezone: &str,
        daily_root: &str,
        daily_pattern: &str,
        template_path: Option<&str>,
        link_style: &str,
    ) -> Result<WorkspaceProfile> {
        validate_workspace_profile(
            root_binding,
            timezone,
            daily_root,
            daily_pattern,
            template_path,
            link_style,
        )?;
        let id = format!("workspace_{}", Uuid::new_v4().simple());
        let now = Utc::now().to_rfc3339();
        let conn = self.connect()?;
        conn.execute(
            r#"
            INSERT INTO workspace_profiles
                (id, status, root_binding, timezone, daily_root, daily_pattern,
                 template_path, link_style, created_at, updated_at)
            VALUES (?1, 'pending_review', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
            "#,
            params![
                id,
                root_binding.trim(),
                timezone,
                daily_root.trim(),
                daily_pattern.trim(),
                template_path.map(str::trim),
                link_style,
                now
            ],
        )?;
        self.workspace_profile(&id)?
            .context("created workspace profile missing")
    }

    pub fn set_owner_secret_hash(&self, secret_hash: &str) -> Result<()> {
        anyhow::ensure!(
            !secret_hash.trim().is_empty(),
            "owner secret hash is required"
        );
        let conn = self.connect()?;
        conn.execute(
            r#"
            INSERT INTO owner_security (singleton, secret_hash, updated_at)
            VALUES (1, ?1, ?2)
            ON CONFLICT(singleton) DO UPDATE SET
                secret_hash = excluded.secret_hash,
                updated_at = excluded.updated_at
            "#,
            params![secret_hash, Utc::now().to_rfc3339()],
        )?;
        self.revoke_all_dashboard_sessions()?;
        Ok(())
    }

    pub fn owner_secret_hash(&self) -> Result<Option<String>> {
        let conn = self.connect()?;
        conn.query_row(
            "SELECT secret_hash FROM owner_security WHERE singleton = 1",
            [],
            |row| row.get(0),
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn create_dashboard_session(
        &self,
        credentials: &SessionCredentials,
        scopes: &[String],
        now: DateTime<Utc>,
        idle_ttl: Duration,
        absolute_ttl: Duration,
    ) -> Result<DashboardSession> {
        anyhow::ensure!(idle_ttl > Duration::zero(), "idle TTL must be positive");
        anyhow::ensure!(absolute_ttl >= idle_ttl, "absolute TTL must cover idle TTL");
        let scopes = normalize_scopes(scopes)?;
        let session = DashboardSession {
            token_digest: token_digest(&credentials.session_token),
            csrf_digest: token_digest(&credentials.csrf_token),
            scopes,
            created_at: now,
            last_seen_at: now,
            idle_expires_at: now + idle_ttl,
            absolute_expires_at: now + absolute_ttl,
            revoked_at: None,
        };
        let conn = self.connect()?;
        conn.execute(
            r#"
            INSERT INTO dashboard_sessions
                (token_digest, csrf_digest, scopes_json, created_at, last_seen_at,
                 idle_expires_at, absolute_expires_at, revoked_at)
            VALUES (?1, ?2, ?3, ?4, ?4, ?5, ?6, NULL)
            "#,
            params![
                session.token_digest,
                session.csrf_digest,
                serde_json::to_string(&session.scopes)?,
                now.to_rfc3339(),
                session.idle_expires_at.to_rfc3339(),
                session.absolute_expires_at.to_rfc3339()
            ],
        )?;
        Ok(session)
    }

    pub fn authenticate_dashboard_session(
        &self,
        session_token: &str,
        csrf_token: Option<&str>,
        required_scope: &str,
        now: DateTime<Utc>,
        idle_ttl: Duration,
    ) -> Result<DashboardSession> {
        let digest = token_digest(session_token);
        let mut session = self
            .dashboard_session(&digest)?
            .context("dashboard session is not valid")?;
        anyhow::ensure!(session.revoked_at.is_none(), "dashboard session is revoked");
        anyhow::ensure!(
            now < session.idle_expires_at,
            "dashboard session idle timeout expired"
        );
        anyhow::ensure!(
            now < session.absolute_expires_at,
            "dashboard session expired"
        );
        anyhow::ensure!(
            session.scopes.iter().any(|scope| scope == required_scope),
            "dashboard session lacks required scope"
        );
        if let Some(csrf_token) = csrf_token {
            anyhow::ensure!(
                token_matches(csrf_token, &session.csrf_digest),
                "CSRF token does not match the dashboard session"
            );
        }
        session.last_seen_at = now;
        session.idle_expires_at = (now + idle_ttl).min(session.absolute_expires_at);
        let conn = self.connect()?;
        conn.execute(
            "UPDATE dashboard_sessions SET last_seen_at = ?1, idle_expires_at = ?2 WHERE token_digest = ?3",
            params![
                session.last_seen_at.to_rfc3339(),
                session.idle_expires_at.to_rfc3339(),
                session.token_digest
            ],
        )?;
        Ok(session)
    }

    pub fn revoke_dashboard_session(&self, session_token: &str) -> Result<bool> {
        let conn = self.connect()?;
        let changed = conn.execute(
            "UPDATE dashboard_sessions SET revoked_at = ?1 WHERE token_digest = ?2 AND revoked_at IS NULL",
            params![Utc::now().to_rfc3339(), token_digest(session_token)],
        )?;
        Ok(changed == 1)
    }

    pub fn revoke_all_dashboard_sessions(&self) -> Result<usize> {
        let conn = self.connect()?;
        conn.execute(
            "UPDATE dashboard_sessions SET revoked_at = ?1 WHERE revoked_at IS NULL",
            params![Utc::now().to_rfc3339()],
        )
        .map_err(Into::into)
    }

    fn dashboard_session(&self, digest: &str) -> Result<Option<DashboardSession>> {
        let conn = self.connect()?;
        conn.query_row(
            "SELECT token_digest, csrf_digest, scopes_json, created_at, last_seen_at, idle_expires_at, absolute_expires_at, revoked_at FROM dashboard_sessions WHERE token_digest = ?1",
            params![digest],
            dashboard_session_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn activate_workspace_profile(&self, id: &str) -> Result<WorkspaceProfile> {
        let mut conn = self.connect()?;
        let transaction = conn.transaction()?;
        let exists = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM workspace_profiles WHERE id = ?1)",
            params![id],
            |row| row.get::<_, bool>(0),
        )?;
        anyhow::ensure!(exists, "workspace profile not found: {id}");
        let now = Utc::now().to_rfc3339();
        transaction.execute(
            "UPDATE workspace_profiles SET status = 'disabled', updated_at = ?1 WHERE status = 'active' AND id <> ?2",
            params![now, id],
        )?;
        transaction.execute(
            "UPDATE workspace_profiles SET status = 'active', updated_at = ?1 WHERE id = ?2",
            params![now, id],
        )?;
        transaction.commit()?;
        self.workspace_profile(id)?
            .context("activated workspace profile missing")
    }

    pub fn active_workspace_profile(&self) -> Result<Option<WorkspaceProfile>> {
        let conn = self.connect()?;
        conn.query_row(
            "SELECT id, status, root_binding, timezone, daily_root, daily_pattern, template_path, link_style, created_at, updated_at FROM workspace_profiles WHERE status = 'active'",
            [],
            workspace_profile_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn save_active_workspace_profile(
        &self,
        root_binding: &str,
        timezone: &str,
        daily_root: &str,
        daily_pattern: &str,
        template_path: Option<&str>,
        link_style: &str,
        expected_active: Option<(&str, DateTime<Utc>)>,
    ) -> Result<WorkspaceProfile> {
        validate_workspace_profile(
            root_binding,
            timezone,
            daily_root,
            daily_pattern,
            template_path,
            link_style,
        )?;
        let mut conn = self.connect()?;
        let transaction = conn.transaction()?;
        let active = transaction
            .query_row(
                "SELECT id, status, root_binding, timezone, daily_root, daily_pattern, template_path, link_style, created_at, updated_at FROM workspace_profiles WHERE status = 'active'",
                [],
                workspace_profile_from_row,
            )
            .optional()?;
        let now = Utc::now();
        let id = if let Some(active) = active {
            let (expected_id, expected_updated_at) = expected_active
                .context("active workspace update requires its expected ID and update timestamp")?;
            anyhow::ensure!(
                active.id == expected_id && active.updated_at == expected_updated_at,
                "active workspace changed; reload it before saving"
            );
            let changed = transaction.execute(
                r#"UPDATE workspace_profiles
                   SET root_binding = ?1, timezone = ?2, daily_root = ?3,
                       daily_pattern = ?4, template_path = ?5, link_style = ?6,
                       updated_at = ?7
                   WHERE id = ?8 AND status = 'active' AND updated_at = ?9"#,
                params![
                    root_binding.trim(),
                    timezone,
                    daily_root.trim(),
                    daily_pattern.trim(),
                    template_path.map(str::trim),
                    link_style,
                    now.to_rfc3339(),
                    expected_id,
                    expected_updated_at.to_rfc3339()
                ],
            )?;
            anyhow::ensure!(changed == 1, "active workspace changed while saving");
            active.id
        } else {
            anyhow::ensure!(
                expected_active.is_none(),
                "expected active workspace does not exist"
            );
            let id = format!("workspace_{}", Uuid::new_v4().simple());
            transaction.execute(
                r#"INSERT INTO workspace_profiles
                    (id, status, root_binding, timezone, daily_root, daily_pattern,
                     template_path, link_style, created_at, updated_at)
                   VALUES (?1, 'active', ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)"#,
                params![
                    id,
                    root_binding.trim(),
                    timezone,
                    daily_root.trim(),
                    daily_pattern.trim(),
                    template_path.map(str::trim),
                    link_style,
                    now.to_rfc3339()
                ],
            )?;
            id
        };
        transaction.commit()?;
        self.workspace_profile(&id)?
            .context("saved active workspace profile is missing")
    }

    pub fn begin_migration_operation(
        &self,
        operation_id: &str,
        migration_name: &str,
        source_identity: &str,
        details: &serde_json::Value,
    ) -> Result<MigrationJournalEntry> {
        anyhow::ensure!(!operation_id.trim().is_empty(), "operation ID is required");
        anyhow::ensure!(
            !migration_name.trim().is_empty(),
            "migration name is required"
        );
        anyhow::ensure!(
            !source_identity.trim().is_empty(),
            "source identity is required"
        );
        let conn = self.connect()?;
        conn.execute(
            r#"
            INSERT INTO migration_journal
                (operation_id, migration_name, source_identity, status, details_json, started_at)
            VALUES (?1, ?2, ?3, 'started', ?4, ?5)
            ON CONFLICT(operation_id) DO NOTHING
            "#,
            params![
                operation_id,
                migration_name,
                source_identity,
                serde_json::to_string(details)?,
                Utc::now().to_rfc3339()
            ],
        )?;
        let entry = self
            .migration_operation(operation_id)?
            .context("migration operation missing")?;
        anyhow::ensure!(
            entry.migration_name == migration_name && entry.source_identity == source_identity,
            "operation ID already belongs to a different migration"
        );
        Ok(entry)
    }

    pub fn finish_migration_operation(
        &self,
        operation_id: &str,
        status: &str,
        details: &serde_json::Value,
    ) -> Result<MigrationJournalEntry> {
        anyhow::ensure!(
            matches!(status, "completed" | "failed"),
            "terminal migration status must be completed or failed"
        );
        let conn = self.connect()?;
        let changed = conn.execute(
            "UPDATE migration_journal SET status = ?1, details_json = ?2, completed_at = ?3 WHERE operation_id = ?4",
            params![
                status,
                serde_json::to_string(details)?,
                Utc::now().to_rfc3339(),
                operation_id
            ],
        )?;
        anyhow::ensure!(
            changed == 1,
            "migration operation not found: {operation_id}"
        );
        self.migration_operation(operation_id)?
            .context("finished migration operation missing")
    }

    fn workspace_profile(&self, id: &str) -> Result<Option<WorkspaceProfile>> {
        let conn = self.connect()?;
        conn.query_row(
            "SELECT id, status, root_binding, timezone, daily_root, daily_pattern, template_path, link_style, created_at, updated_at FROM workspace_profiles WHERE id = ?1",
            params![id],
            workspace_profile_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn migration_operation(&self, operation_id: &str) -> Result<Option<MigrationJournalEntry>> {
        let conn = self.connect()?;
        conn.query_row(
            "SELECT operation_id, migration_name, source_identity, status, details_json, started_at, completed_at FROM migration_journal WHERE operation_id = ?1",
            params![operation_id],
            migration_journal_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn latest_migration_operation(
        &self,
        migration_name: &str,
        source_identity: &str,
    ) -> Result<Option<MigrationJournalEntry>> {
        self.connect()?
            .query_row(
                "SELECT operation_id, migration_name, source_identity, status, details_json, started_at, completed_at FROM migration_journal WHERE migration_name = ?1 AND source_identity = ?2 ORDER BY started_at DESC LIMIT 1",
                params![migration_name, source_identity],
                migration_journal_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn insert_event(&self, input: LogEventInput) -> Result<StoredLogEvent> {
        validate_event(&input)?;

        let now = Utc::now();
        let id = format!("evt_{}", Uuid::new_v4().simple());
        let timestamp = input.timestamp.unwrap_or(now);
        let level = normalize_level(input.level.as_deref());
        let message = redact_text(input.message.trim());
        let metadata = redact_metadata(input.metadata.unwrap_or_default());
        let metadata_json = serde_json::to_string(&metadata)?;

        let mut conn = self.connect()?;
        let transaction = conn.transaction()?;
        transaction.execute(
            r#"
            INSERT INTO log_events
                (id, received_at, timestamp, source, level, message, metadata_json, fingerprint, truncated)
            VALUES
                (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            params![
                id,
                now.to_rfc3339(),
                timestamp.to_rfc3339(),
                input.source.trim(),
                level,
                message,
                metadata_json,
                input.fingerprint,
                0,
            ],
        )?;

        transaction.execute(
            r#"UPDATE daily_days
               SET freshness = 'update_available', updated_at = ?1
               WHERE current_revision_id IS NOT NULL
                 AND start_utc <= ?2 AND end_utc > ?2
                 AND NOT EXISTS (
                     SELECT 1
                     FROM proposal_revisions AS revision
                     JOIN evidence_snapshot_events AS evidence
                       ON evidence.snapshot_id = revision.snapshot_id
                     WHERE revision.id = daily_days.current_revision_id
                       AND evidence.event_id = ?3
                 )"#,
            params![now.to_rfc3339(), timestamp.to_rfc3339(), id],
        )?;
        transaction.commit()?;

        self.get_event(&id)?.context("inserted event missing")
    }

    pub fn prune_old_events(&self, retention_days: u64) -> Result<usize> {
        let cutoff = Utc::now() - Duration::days(retention_days as i64);
        let conn = self.connect()?;
        let changed = conn.execute(
            "DELETE FROM log_events WHERE received_at < ?1",
            params![cutoff.to_rfc3339()],
        )?;
        Ok(changed)
    }

    pub fn list_sources(&self, since: Option<DateTime<Utc>>) -> Result<Vec<SourceSummary>> {
        let conn = self.connect()?;
        let sql = match since {
            Some(_) => {
                r#"
                SELECT source, COUNT(*) AS event_count, MAX(timestamp) AS latest_timestamp
                FROM log_events
                WHERE timestamp >= ?1
                GROUP BY source
                ORDER BY latest_timestamp DESC
                "#
            }
            None => {
                r#"
                SELECT source, COUNT(*) AS event_count, MAX(timestamp) AS latest_timestamp
                FROM log_events
                GROUP BY source
                ORDER BY latest_timestamp DESC
                "#
            }
        };

        let mut stmt = conn.prepare(sql)?;
        let rows = if let Some(since) = since {
            stmt.query_map(params![since.to_rfc3339()], source_summary_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            stmt.query_map([], source_summary_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        Ok(rows)
    }

    pub fn query_logs(&self, query: LogQuery) -> Result<LogQueryResult> {
        let limit = query.limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        let conn = self.connect()?;
        let mut sql = String::from(
            r#"
            SELECT e.id, e.received_at, e.timestamp, e.source, e.level, e.message,
                   e.metadata_json, e.fingerprint, e.truncated,
                   CASE WHEN r.event_id IS NULL THEN 0 ELSE 1 END AS reviewed
            FROM log_events e
            LEFT JOIN review_state r ON r.event_id = e.id
            WHERE 1 = 1
            "#,
        );
        let mut values = Vec::new();

        if let Some(source) = query.source {
            sql.push_str(" AND e.source = ?");
            values.push(source);
        }
        if let Some(since) = query.since {
            sql.push_str(" AND e.timestamp >= ?");
            values.push(since.to_rfc3339());
        }
        if let Some(level) = query.level {
            sql.push_str(" AND e.level = ?");
            values.push(normalize_level(Some(&level)));
        }
        if let Some(search) = query.query {
            sql.push_str(
                " AND (e.message LIKE ? ESCAPE '\\' OR e.metadata_json LIKE ? ESCAPE '\\')",
            );
            let escaped = search
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_");
            let pattern = format!("%{escaped}%");
            values.push(pattern.clone());
            values.push(pattern);
        }

        sql.push_str(" ORDER BY e.timestamp DESC, e.received_at DESC LIMIT ?");
        values.push((limit + 1).to_string());

        let mut stmt = conn.prepare(&sql)?;
        let rows = stmt
            .query_map(rusqlite::params_from_iter(values), stored_event_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let truncated = rows.len() > limit;
        Ok(LogQueryResult {
            events: rows.into_iter().take(limit).collect(),
            truncated,
            limit,
        })
    }

    pub fn get_events_between(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        limit: usize,
    ) -> Result<LogQueryResult> {
        let limit = limit.clamp(1, MAX_LIMIT);
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT e.id, e.received_at, e.timestamp, e.source, e.level, e.message,
                   e.metadata_json, e.fingerprint, e.truncated,
                   CASE WHEN r.event_id IS NULL THEN 0 ELSE 1 END AS reviewed
            FROM log_events e
            LEFT JOIN review_state r ON r.event_id = e.id
            WHERE e.timestamp >= ?1 AND e.timestamp < ?2
            ORDER BY e.timestamp ASC, e.received_at ASC
            LIMIT ?3
            "#,
        )?;
        let rows = stmt
            .query_map(
                params![start.to_rfc3339(), end.to_rfc3339(), (limit + 1) as i64],
                stored_event_from_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let truncated = rows.len() > limit;
        Ok(LogQueryResult {
            events: rows.into_iter().take(limit).collect(),
            truncated,
            limit,
        })
    }

    pub fn get_log_window(
        &self,
        event_id: &str,
        before: Duration,
        after: Duration,
        limit: Option<usize>,
    ) -> Result<LogQueryResult> {
        let anchor = self
            .get_event(event_id)?
            .with_context(|| format!("event {event_id} not found"))?;
        let start = anchor.timestamp - before;
        let end = anchor.timestamp + after;
        let max = limit.unwrap_or(DEFAULT_LIMIT).clamp(1, MAX_LIMIT);
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT e.id, e.received_at, e.timestamp, e.source, e.level, e.message,
                   e.metadata_json, e.fingerprint, e.truncated,
                   CASE WHEN r.event_id IS NULL THEN 0 ELSE 1 END AS reviewed
            FROM log_events e
            LEFT JOIN review_state r ON r.event_id = e.id
            WHERE e.source = ?1 AND e.timestamp >= ?2 AND e.timestamp <= ?3
            ORDER BY e.timestamp ASC, e.received_at ASC
            LIMIT ?4
            "#,
        )?;
        let rows = stmt
            .query_map(
                params![
                    anchor.source,
                    start.to_rfc3339(),
                    end.to_rfc3339(),
                    (max + 1) as i64
                ],
                stored_event_from_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let truncated = rows.len() > max;
        Ok(LogQueryResult {
            events: rows.into_iter().take(max).collect(),
            truncated,
            limit: max,
        })
    }

    pub fn get_events_by_ids(&self, event_ids: &[String]) -> Result<Vec<StoredLogEvent>> {
        event_ids
            .iter()
            .map(|event_id| {
                self.get_event(event_id)?
                    .with_context(|| format!("event {event_id} not found"))
            })
            .collect()
    }

    pub fn get_unstaged_events(
        &self,
        received_before: DateTime<Utc>,
        limit: usize,
    ) -> Result<Vec<StoredLogEvent>> {
        let limit = limit.clamp(1, MAX_LIMIT);
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT e.id, e.received_at, e.timestamp, e.source, e.level, e.message,
                   e.metadata_json, e.fingerprint, e.truncated,
                   CASE WHEN r.event_id IS NULL THEN 0 ELSE 1 END AS reviewed
            FROM log_events e
            LEFT JOIN review_state r ON r.event_id = e.id
            LEFT JOIN proposal_state p ON p.event_id = e.id
            WHERE r.event_id IS NULL
              AND p.event_id IS NULL
              AND e.received_at <= ?1
              AND COALESCE(json_extract(e.metadata_json, '$.entry_kind'), '') != 'manual'
            ORDER BY e.received_at ASC
            LIMIT ?2
            "#,
        )?;
        stmt.query_map(
            params![received_before.to_rfc3339(), limit as i64],
            stored_event_from_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
    }

    pub fn mark_staged(&self, event_ids: &[String], proposal_id: &str) -> Result<StagedEventGroup> {
        let now = Utc::now().to_rfc3339();
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let mut count = 0;
        for event_id in event_ids {
            count += tx.execute(
                r#"
                INSERT INTO proposal_state (event_id, proposal_id, staged_at)
                VALUES (?1, ?2, ?3)
                ON CONFLICT(event_id) DO NOTHING
                "#,
                params![event_id, proposal_id, now],
            )?;
        }
        tx.commit()?;
        Ok(StagedEventGroup {
            proposal_id: proposal_id.to_owned(),
            staged_count: count,
        })
    }

    pub fn mark_reviewed(
        &self,
        event_ids: &[String],
        note: &str,
        reviewed_by: &str,
    ) -> Result<MarkReviewedResult> {
        let now = Utc::now().to_rfc3339();
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let mut count = 0;
        for event_id in event_ids {
            count += tx.execute(
                r#"
                INSERT INTO review_state (event_id, reviewed_at, reviewed_by, note)
                VALUES (?1, ?2, ?3, ?4)
                ON CONFLICT(event_id) DO UPDATE SET
                    reviewed_at = excluded.reviewed_at,
                    reviewed_by = excluded.reviewed_by,
                    note = excluded.note
                "#,
                params![event_id, now, reviewed_by, note],
            )?;
        }
        tx.commit()?;
        Ok(MarkReviewedResult {
            reviewed_count: count,
        })
    }

    pub fn get_preferences(&self) -> Result<BTreeMap<String, String>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare("SELECT key, value FROM app_preferences ORDER BY key")?;
        stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
            .collect::<rusqlite::Result<BTreeMap<_, _>>>()
            .map_err(Into::into)
    }

    pub fn set_preferences(&self, preferences: &BTreeMap<String, String>) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        for (key, value) in preferences {
            tx.execute(
                r#"
                INSERT INTO app_preferences (key, value, updated_at)
                VALUES (?1, ?2, ?3)
                ON CONFLICT(key) DO UPDATE SET
                    value = excluded.value,
                    updated_at = excluded.updated_at
                "#,
                params![key, value, now],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    pub fn delete_preference(&self, key: &str) -> Result<bool> {
        let conn = self.connect()?;
        Ok(conn.execute("DELETE FROM app_preferences WHERE key = ?1", params![key])? > 0)
    }

    pub fn list_link_rules(&self) -> Result<Vec<VaultLinkRule>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT id, selectors_json, target_note_id, enabled, created_at, updated_at FROM vault_link_rules ORDER BY updated_at DESC",
        )?;
        stmt.query_map([], |row| {
            let selectors_json: String = row.get(1)?;
            let selectors = serde_json::from_str(&selectors_json).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    selectors_json.len(),
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            Ok(VaultLinkRule {
                id: row.get(0)?,
                selectors,
                target_note_id: row.get(2)?,
                enabled: row.get::<_, i64>(3)? != 0,
                created_at: parse_utc(row.get::<_, String>(4)?),
                updated_at: parse_utc(row.get::<_, String>(5)?),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
    }

    pub fn save_link_rule(&self, rule: &VaultLinkRule) -> Result<()> {
        let selectors = serde_json::to_string(&rule.selectors)?;
        let conn = self.connect()?;
        conn.execute(
            r#"
            INSERT INTO vault_link_rules
                (id, selectors_json, target_note_id, enabled, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(id) DO UPDATE SET
                selectors_json = excluded.selectors_json,
                target_note_id = excluded.target_note_id,
                enabled = excluded.enabled,
                updated_at = excluded.updated_at
            "#,
            params![
                rule.id,
                selectors,
                rule.target_note_id,
                rule.enabled as i64,
                rule.created_at.to_rfc3339(),
                rule.updated_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn delete_link_rule(&self, id: &str) -> Result<bool> {
        let conn = self.connect()?;
        Ok(conn.execute("DELETE FROM vault_link_rules WHERE id = ?1", params![id])? > 0)
    }

    pub fn list_ignored_link_identities(&self) -> Result<Vec<IgnoredLinkIdentity>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            "SELECT id, field, value, normalized_value, created_at FROM ignored_link_identities ORDER BY created_at DESC",
        )?;
        stmt.query_map([], |row| {
            Ok(IgnoredLinkIdentity {
                id: row.get(0)?,
                field: row.get(1)?,
                value: row.get(2)?,
                normalized_value: row.get(3)?,
                created_at: parse_utc(row.get::<_, String>(4)?),
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
    }

    pub fn ignore_link_identity(
        &self,
        field: &str,
        value: &str,
        normalized_value: &str,
    ) -> Result<IgnoredLinkIdentity> {
        let conn = self.connect()?;
        let id = format!("ignored_{}", Uuid::new_v4().simple());
        let created_at = Utc::now();
        conn.execute(
            r#"
            INSERT INTO ignored_link_identities (id, field, value, normalized_value, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(field, normalized_value) DO NOTHING
            "#,
            params![id, field, value, normalized_value, created_at.to_rfc3339()],
        )?;
        conn.query_row(
            "SELECT id, field, value, normalized_value, created_at FROM ignored_link_identities WHERE field = ?1 AND normalized_value = ?2",
            params![field, normalized_value],
            |row| {
                Ok(IgnoredLinkIdentity {
                    id: row.get(0)?,
                    field: row.get(1)?,
                    value: row.get(2)?,
                    normalized_value: row.get(3)?,
                    created_at: parse_utc(row.get::<_, String>(4)?),
                })
            },
        ).map_err(Into::into)
    }

    pub fn restore_ignored_link_identity(&self, id: &str) -> Result<bool> {
        let conn = self.connect()?;
        Ok(conn.execute(
            "DELETE FROM ignored_link_identities WHERE id = ?1",
            params![id],
        )? > 0)
    }

    pub fn restore_matching_ignored_identity(
        &self,
        field: &str,
        normalized_value: &str,
    ) -> Result<bool> {
        let conn = self.connect()?;
        Ok(conn.execute(
            "DELETE FROM ignored_link_identities WHERE field = ?1 AND normalized_value = ?2",
            params![field, normalized_value],
        )? > 0)
    }

    pub fn all_events(&self) -> Result<Vec<StoredLogEvent>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT e.id, e.received_at, e.timestamp, e.source, e.level, e.message,
                   e.metadata_json, e.fingerprint, e.truncated,
                   CASE WHEN r.event_id IS NULL THEN 0 ELSE 1 END AS reviewed
            FROM log_events e
            LEFT JOIN review_state r ON r.event_id = e.id
            ORDER BY e.timestamp DESC, e.received_at DESC
            "#,
        )?;
        stmt.query_map([], stored_event_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn enqueue_daily_consolidation(
        &self,
        start: DateTime<Utc>,
        end: DateTime<Utc>,
        target_note: &str,
        context_revision: &str,
        event_ids: &[String],
    ) -> Result<DailyConsolidationJob> {
        let last_event_id = event_ids.last().map(String::as_str).unwrap_or_default();
        let snapshot_key = format!(
            "{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}\u{1f}{}",
            start.to_rfc3339(),
            end.to_rfc3339(),
            target_note,
            context_revision,
            event_ids.len(),
            last_event_id
        );
        let now = Utc::now();
        let id = format!("consolidation_{}", Uuid::new_v4().simple());
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        tx.execute(
            r#"
            INSERT OR IGNORE INTO daily_consolidation_jobs
                (id, snapshot_key, start, end, target_note, status, event_count, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, ?7, ?7)
            "#,
            params![
                id,
                snapshot_key,
                start.to_rfc3339(),
                end.to_rfc3339(),
                target_note,
                event_ids.len() as i64,
                now.to_rfc3339(),
            ],
        )?;
        let job_id: String = tx.query_row(
            "SELECT id FROM daily_consolidation_jobs WHERE snapshot_key = ?1",
            params![snapshot_key],
            |row| row.get(0),
        )?;
        if job_id == id {
            for (position, event_id) in event_ids.iter().enumerate() {
                tx.execute(
                    "INSERT INTO daily_consolidation_job_events (job_id, event_id, position) VALUES (?1, ?2, ?3)",
                    params![job_id, event_id, position as i64],
                )?;
            }
        }
        tx.commit()?;
        self.get_daily_consolidation_job(&job_id)?
            .context("queued daily consolidation disappeared")
    }

    pub fn list_daily_consolidations(&self, limit: usize) -> Result<Vec<DailyConsolidationJob>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT id, start, end, target_note, status, event_count, proposal_id, error,
                   created_at, updated_at
            FROM daily_consolidation_jobs
            ORDER BY updated_at DESC
            LIMIT ?1
            "#,
        )?;
        stmt.query_map(
            params![limit.clamp(1, 50) as i64],
            consolidation_job_from_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(Into::into)
    }

    pub fn claim_next_daily_consolidation(&self) -> Result<Option<DailyConsolidationJob>> {
        let now = Utc::now().to_rfc3339();
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let id = tx
            .query_row(
                "SELECT id FROM daily_consolidation_jobs WHERE status = 'pending' ORDER BY created_at ASC LIMIT 1",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(id) = id else {
            tx.commit()?;
            return Ok(None);
        };
        let changed = tx.execute(
            "UPDATE daily_consolidation_jobs SET status = 'running', updated_at = ?2 WHERE id = ?1 AND status = 'pending'",
            params![id, now],
        )?;
        tx.commit()?;
        if changed == 0 {
            return Ok(None);
        }
        self.get_daily_consolidation_job(&id)
    }

    pub fn get_daily_consolidation_events(&self, job_id: &str) -> Result<Vec<StoredLogEvent>> {
        let conn = self.connect()?;
        let mut stmt = conn.prepare(
            r#"
            SELECT e.id, e.received_at, e.timestamp, e.source, e.level, e.message,
                   e.metadata_json, e.fingerprint, e.truncated,
                   CASE WHEN r.event_id IS NULL THEN 0 ELSE 1 END AS reviewed
            FROM daily_consolidation_job_events j
            JOIN log_events e ON e.id = j.event_id
            LEFT JOIN review_state r ON r.event_id = e.id
            WHERE j.job_id = ?1
            ORDER BY j.position ASC
            "#,
        )?;
        stmt.query_map(params![job_id], stored_event_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn request_daily_consolidation_cancel(
        &self,
        job_id: &str,
    ) -> Result<Option<DailyConsolidationJob>> {
        let now = Utc::now().to_rfc3339();
        let conn = self.connect()?;
        conn.execute(
            r#"
            UPDATE daily_consolidation_jobs
            SET status = CASE status WHEN 'pending' THEN 'cancelled' ELSE 'cancel_requested' END,
                updated_at = ?2
            WHERE id = ?1 AND status IN ('pending', 'running')
            "#,
            params![job_id, now],
        )?;
        self.get_daily_consolidation_job(job_id)
    }

    pub fn requeue_daily_consolidation(
        &self,
        job_id: &str,
    ) -> Result<Option<DailyConsolidationJob>> {
        let now = Utc::now().to_rfc3339();
        let conn = self.connect()?;
        conn.execute(
            r#"
            UPDATE daily_consolidation_jobs
            SET status = 'pending', proposal_id = NULL, error = NULL, updated_at = ?2
            WHERE id = ?1 AND status IN ('completed', 'failed', 'cancelled')
            "#,
            params![job_id, now],
        )?;
        self.get_daily_consolidation_job(job_id)
    }

    pub fn daily_consolidation_cancel_requested(&self, job_id: &str) -> Result<bool> {
        let conn = self.connect()?;
        Ok(conn
            .query_row(
                "SELECT status = 'cancel_requested' FROM daily_consolidation_jobs WHERE id = ?1",
                params![job_id],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(false))
    }

    pub fn finish_daily_consolidation(
        &self,
        job_id: &str,
        status: &str,
        proposal_id: Option<&str>,
        error: Option<&str>,
    ) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.connect()?;
        conn.execute(
            r#"
            UPDATE daily_consolidation_jobs
            SET status = ?2, proposal_id = ?3, error = ?4, updated_at = ?5
            WHERE id = ?1
            "#,
            params![job_id, status, proposal_id, error, now],
        )?;
        Ok(())
    }

    pub fn recover_daily_consolidations(&self) -> Result<()> {
        let now = Utc::now().to_rfc3339();
        let conn = self.connect()?;
        conn.execute(
            "UPDATE daily_consolidation_jobs SET status = 'pending', updated_at = ?1 WHERE status = 'running'",
            params![now],
        )?;
        conn.execute(
            "UPDATE daily_consolidation_jobs SET status = 'cancelled', updated_at = ?1 WHERE status = 'cancel_requested'",
            params![now],
        )?;
        Ok(())
    }

    pub fn get_daily_consolidation_job(
        &self,
        job_id: &str,
    ) -> Result<Option<DailyConsolidationJob>> {
        let conn = self.connect()?;
        conn.query_row(
            r#"
            SELECT id, start, end, target_note, status, event_count, proposal_id, error,
                   created_at, updated_at
            FROM daily_consolidation_jobs
            WHERE id = ?1
            "#,
            params![job_id],
            consolidation_job_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    fn get_event(&self, event_id: &str) -> Result<Option<StoredLogEvent>> {
        let conn = self.connect()?;
        conn.query_row(
            r#"
            SELECT e.id, e.received_at, e.timestamp, e.source, e.level, e.message,
                   e.metadata_json, e.fingerprint, e.truncated,
                   CASE WHEN r.event_id IS NULL THEN 0 ELSE 1 END AS reviewed
            FROM log_events e
            LEFT JOIN review_state r ON r.event_id = e.id
            WHERE e.id = ?1
            "#,
            params![event_id],
            stored_event_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub(crate) fn connect(&self) -> Result<Connection> {
        let conn = Connection::open(&self.db_path)
            .with_context(|| format!("opening {}", self.db_path.display()))?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        Ok(conn)
    }
}

fn validate_workspace_profile(
    root_binding: &str,
    timezone: &str,
    daily_root: &str,
    daily_pattern: &str,
    template_path: Option<&str>,
    link_style: &str,
) -> Result<()> {
    anyhow::ensure!(!root_binding.trim().is_empty(), "root binding is required");
    timezone
        .parse::<chrono_tz::Tz>()
        .with_context(|| format!("invalid IANA timezone: {timezone}"))?;
    validate_relative_workspace_path(daily_root, true)?;
    validate_relative_workspace_path(daily_pattern, false)?;
    render_daily_path(
        daily_root,
        daily_pattern,
        chrono::NaiveDate::from_ymd_opt(2000, 1, 2).expect("validation date is valid"),
    )?;
    if let Some(path) = template_path {
        validate_relative_workspace_path(path, false)?;
    }
    anyhow::ensure!(
        matches!(link_style, "markdown" | "wikilink"),
        "link style must be markdown or wikilink"
    );
    Ok(())
}

fn validate_relative_workspace_path(path: &str, allow_empty: bool) -> Result<()> {
    let trimmed = path.trim();
    anyhow::ensure!(
        allow_empty || !trimmed.is_empty(),
        "relative path is required"
    );
    let candidate = Path::new(trimmed);
    anyhow::ensure!(!candidate.is_absolute(), "workspace path must be relative");
    anyhow::ensure!(
        candidate
            .components()
            .all(|component| matches!(component, std::path::Component::Normal(_))),
        "workspace path cannot contain traversal or platform prefixes"
    );
    Ok(())
}

fn workspace_profile_from_row(row: &Row<'_>) -> rusqlite::Result<WorkspaceProfile> {
    Ok(WorkspaceProfile {
        id: row.get(0)?,
        status: row.get(1)?,
        root_binding: row.get(2)?,
        timezone: row.get(3)?,
        daily_root: row.get(4)?,
        daily_pattern: row.get(5)?,
        template_path: row.get(6)?,
        link_style: row.get(7)?,
        created_at: parse_utc(row.get(8)?),
        updated_at: parse_utc(row.get(9)?),
    })
}

fn migration_journal_from_row(row: &Row<'_>) -> rusqlite::Result<MigrationJournalEntry> {
    let details_json: String = row.get(4)?;
    let details = serde_json::from_str(&details_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(MigrationJournalEntry {
        operation_id: row.get(0)?,
        migration_name: row.get(1)?,
        source_identity: row.get(2)?,
        status: row.get(3)?,
        details,
        started_at: parse_utc(row.get(5)?),
        completed_at: row.get::<_, Option<String>>(6)?.map(parse_utc),
    })
}

fn dashboard_session_from_row(row: &Row<'_>) -> rusqlite::Result<DashboardSession> {
    let scopes_json: String = row.get(2)?;
    let scopes = serde_json::from_str(&scopes_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(DashboardSession {
        token_digest: row.get(0)?,
        csrf_digest: row.get(1)?,
        scopes,
        created_at: parse_utc(row.get(3)?),
        last_seen_at: parse_utc(row.get(4)?),
        idle_expires_at: parse_utc(row.get(5)?),
        absolute_expires_at: parse_utc(row.get(6)?),
        revoked_at: row.get::<_, Option<String>>(7)?.map(parse_utc),
    })
}

fn schema_version(conn: &Connection) -> Result<i64> {
    conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_migrations",
        [],
        |row| row.get(0),
    )
    .map_err(Into::into)
}

fn event_count(conn: &Connection) -> Result<u64> {
    conn.query_row("SELECT COUNT(*) FROM log_events", [], |row| row.get(0))
        .map_err(Into::into)
}

pub fn verify_backup(path: &Path) -> Result<BackupVerification> {
    let conn =
        Connection::open(path).with_context(|| format!("opening backup {}", path.display()))?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    let integrity_check: String = conn.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
    anyhow::ensure!(
        integrity_check == "ok",
        "backup integrity check failed: {integrity_check}"
    );
    ensure_foreign_key_integrity(&conn).context("backup foreign key check failed")?;
    Ok(BackupVerification {
        path: path.to_path_buf(),
        schema_version: schema_version(&conn)?,
        event_count: event_count(&conn)?,
        integrity_check,
    })
}

fn ensure_foreign_key_integrity(conn: &Connection) -> Result<()> {
    let violation = conn
        .prepare("PRAGMA foreign_key_check")?
        .query_row([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<i64>>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .optional()?;
    anyhow::ensure!(violation.is_none(), "foreign key violation: {violation:?}");
    Ok(())
}

fn validate_event(input: &LogEventInput) -> Result<()> {
    anyhow::ensure!(!input.source.trim().is_empty(), "source is required");
    anyhow::ensure!(!input.message.trim().is_empty(), "message is required");
    anyhow::ensure!(
        input.source.len() <= MAX_SOURCE_BYTES,
        "source exceeds {MAX_SOURCE_BYTES} bytes"
    );
    anyhow::ensure!(
        input.message.len() <= MAX_MESSAGE_BYTES,
        "message exceeds {MAX_MESSAGE_BYTES} bytes; split it into ordered events with a shared task_id or session_id"
    );
    if let Some(fingerprint) = &input.fingerprint {
        anyhow::ensure!(
            fingerprint.len() <= MAX_FINGERPRINT_BYTES,
            "fingerprint exceeds {MAX_FINGERPRINT_BYTES} bytes"
        );
    }
    if let Some(metadata) = &input.metadata {
        let encoded = serde_json::to_vec(metadata)?;
        anyhow::ensure!(
            encoded.len() <= MAX_METADATA_BYTES,
            "metadata exceeds {MAX_METADATA_BYTES} bytes; move large content into ordered message events or reference a local artifact"
        );
    }
    Ok(())
}

fn normalize_level(level: Option<&str>) -> String {
    match level
        .unwrap_or("unknown")
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "trace" | "debug" | "info" | "warn" | "warning" | "error" | "fatal" => {
            if level.unwrap_or_default().eq_ignore_ascii_case("warning") {
                "warn".to_owned()
            } else {
                level.unwrap_or("unknown").trim().to_ascii_lowercase()
            }
        }
        _ => "unknown".to_owned(),
    }
}

fn stored_event_from_row(row: &Row<'_>) -> rusqlite::Result<StoredLogEvent> {
    let metadata_json: String = row.get(6)?;
    Ok(StoredLogEvent {
        id: row.get(0)?,
        received_at: parse_utc(row.get::<_, String>(1)?),
        timestamp: parse_utc(row.get::<_, String>(2)?),
        source: row.get(3)?,
        level: row.get(4)?,
        message: row.get(5)?,
        metadata: serde_json::from_str(&metadata_json).unwrap_or_default(),
        fingerprint: row.get(7)?,
        truncated: row.get::<_, i64>(8)? != 0,
        reviewed: row.get::<_, i64>(9)? != 0,
    })
}

fn source_summary_from_row(row: &Row<'_>) -> rusqlite::Result<SourceSummary> {
    Ok(SourceSummary {
        source: row.get(0)?,
        event_count: row.get::<_, i64>(1)? as u64,
        latest_timestamp: parse_utc(row.get::<_, String>(2)?),
    })
}

fn consolidation_job_from_row(row: &Row<'_>) -> rusqlite::Result<DailyConsolidationJob> {
    Ok(DailyConsolidationJob {
        id: row.get(0)?,
        start: parse_utc(row.get::<_, String>(1)?),
        end: parse_utc(row.get::<_, String>(2)?),
        target_note: row.get(3)?,
        status: row.get(4)?,
        event_count: row.get::<_, i64>(5)? as usize,
        proposal_id: row.get(6)?,
        error: row.get(7)?,
        created_at: parse_utc(row.get::<_, String>(8)?),
        updated_at: parse_utc(row.get::<_, String>(9)?),
    })
}

fn parse_utc(value: String) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(&value)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_store() -> Store {
        let path = std::env::temp_dir().join(format!("log-inbox-test-{}.sqlite3", Uuid::new_v4()));
        Store::open(path).expect("store opens")
    }

    #[test]
    fn applies_versioned_schema_migrations_idempotently() {
        let store = temp_store();
        assert_eq!(store.schema_version().expect("version reads"), 14);

        store.initialize().expect("reinitialization succeeds");
        assert_eq!(store.schema_version().expect("version remains"), 14);
    }

    #[test]
    fn enables_and_enforces_foreign_keys_on_every_store_connection() {
        let store = temp_store();
        let conn = store.connect().expect("database opens");
        let enabled: bool = conn
            .query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            .expect("foreign key setting reads");
        assert!(enabled);
        assert!(
            conn.execute(
                "INSERT INTO review_state (event_id, reviewed_at, reviewed_by, note) VALUES ('missing-event', ?1, 'test', 'invalid')",
                params![Utc::now().to_rfc3339()],
            )
            .is_err()
        );
    }

    #[test]
    fn retention_uses_receipt_time_for_historical_events() {
        let store = temp_store();
        let event = store
            .insert_event(LogEventInput {
                source: "manual/dashboard".to_owned(),
                level: None,
                timestamp: Some(Utc::now() - Duration::days(90)),
                message: "A late note for an older day.".to_owned(),
                metadata: None,
                fingerprint: None,
            })
            .expect("historical event stores");

        assert_eq!(store.prune_old_events(30).expect("retention runs"), 0);
        assert!(store.get_event(&event.id).expect("event reads").is_some());

        store
            .connect()
            .unwrap()
            .execute(
                "UPDATE log_events SET received_at = ?1 WHERE id = ?2",
                params![(Utc::now() - Duration::days(31)).to_rfc3339(), event.id],
            )
            .expect("receipt time ages");
        assert_eq!(store.prune_old_events(30).expect("retention reruns"), 1);
        assert!(store.get_event(&event.id).expect("event reads").is_none());
    }

    #[test]
    fn creates_and_verifies_a_complete_online_backup() {
        let store = temp_store();
        store
            .insert_event(LogEventInput {
                source: "codex/test".to_owned(),
                level: Some("info".to_owned()),
                timestamp: None,
                message: "backup evidence".to_owned(),
                metadata: None,
                fingerprint: None,
            })
            .expect("event stores");
        let backup_path =
            std::env::temp_dir().join(format!("log-inbox-backup-test-{}.sqlite3", Uuid::new_v4()));

        let verification = store
            .create_verified_backup(&backup_path)
            .expect("backup succeeds");
        assert_eq!(verification.schema_version, 14);
        assert_eq!(verification.event_count, 1);
        assert_eq!(verification.integrity_check, "ok");
        assert!(store.create_verified_backup(&backup_path).is_err());

        let restored = Store::open(backup_path.clone()).expect("backup restores as a store");
        let restored_events = restored.all_events().expect("restored events read");
        assert_eq!(restored_events.len(), 1);
        assert_eq!(restored_events[0].message, "backup evidence");
        drop(restored);

        fs::remove_file(backup_path).expect("test backup removed");
    }

    #[test]
    fn backup_verification_rejects_foreign_key_violations() {
        let store = temp_store();
        let backup_path = std::env::temp_dir().join(format!(
            "log-inbox-invalid-backup-{}.sqlite3",
            Uuid::new_v4()
        ));
        store
            .create_verified_backup(&backup_path)
            .expect("valid backup succeeds");

        let conn = Connection::open(&backup_path).expect("backup opens directly");
        conn.pragma_update(None, "foreign_keys", "OFF")
            .expect("test connection disables enforcement");
        conn.execute(
            "INSERT INTO review_state (event_id, reviewed_at, reviewed_by, note) VALUES ('missing-event', ?1, 'test', 'invalid')",
            params![Utc::now().to_rfc3339()],
        )
        .expect("foreign keys are connection-local and disabled on the raw connection");
        drop(conn);

        let error = verify_backup(&backup_path).expect_err("invalid backup is rejected");
        assert!(error.to_string().contains("foreign key check failed"));
        fs::remove_file(backup_path).expect("test backup removed");
    }

    #[test]
    fn keeps_one_stable_active_workspace_profile() {
        let store = temp_store();
        let first = store
            .create_pending_workspace_profile(
                "binding-one",
                "Europe/Stockholm",
                "Work Log",
                "{year}/{month}/Daily {date}.md",
                Some("Templates/Daily.md"),
                "wikilink",
            )
            .expect("first profile stores");
        assert_eq!(first.status, "pending_review");
        let active = store
            .activate_workspace_profile(&first.id)
            .expect("first profile activates");
        assert_eq!(active.id, first.id);
        assert_eq!(
            store.active_workspace_profile().expect("active reads"),
            Some(active)
        );

        let second = store
            .create_pending_workspace_profile(
                "binding-two",
                "America/New_York",
                "Daily",
                "{date}.md",
                None,
                "markdown",
            )
            .expect("second profile stores");
        store
            .activate_workspace_profile(&second.id)
            .expect("second profile activates");
        assert_eq!(
            store
                .active_workspace_profile()
                .expect("replacement reads")
                .expect("replacement exists")
                .id,
            second.id
        );
        assert!(
            store
                .create_pending_workspace_profile(
                    "binding-three",
                    "not/a timezone",
                    "Daily",
                    "{date}.md",
                    None,
                    "markdown",
                )
                .is_err()
        );
        assert!(
            store
                .create_pending_workspace_profile(
                    "binding-three",
                    "UTC",
                    "../outside",
                    "{date}.md",
                    None,
                    "markdown",
                )
                .is_err()
        );
    }

    #[test]
    fn saves_the_first_active_workspace_and_updates_it_optimistically_in_place() {
        let store = temp_store();
        let first = store
            .save_active_workspace_profile(
                "binding-one",
                "UTC",
                "Daily",
                "{date}.md",
                None,
                "markdown",
                None,
            )
            .expect("first active workspace stores");
        assert_eq!(first.status, "active");

        assert!(
            store
                .save_active_workspace_profile(
                    "binding-two",
                    "Europe/Stockholm",
                    "Work Log",
                    "{year}/{date}.md",
                    Some("Templates/Daily.md"),
                    "wikilink",
                    None,
                )
                .is_err(),
            "updates require an optimistic version"
        );
        let updated = store
            .save_active_workspace_profile(
                "binding-two",
                "Europe/Stockholm",
                "Work Log",
                "{year}/{date}.md",
                Some("Templates/Daily.md"),
                "wikilink",
                Some((&first.id, first.updated_at)),
            )
            .expect("active workspace updates");
        assert_eq!(updated.id, first.id);
        assert_eq!(updated.created_at, first.created_at);
        assert_eq!(updated.root_binding, "binding-two");
        assert_eq!(updated.daily_root, "Work Log");

        assert!(
            store
                .save_active_workspace_profile(
                    "binding-three",
                    "UTC",
                    "Daily",
                    "{date}.md",
                    None,
                    "markdown",
                    Some((&first.id, first.updated_at)),
                )
                .is_err(),
            "a stale version cannot overwrite the active workspace"
        );
    }

    #[test]
    fn journals_migration_operations_idempotently() {
        let store = temp_store();
        let started = store
            .begin_migration_operation(
                "cutover-1",
                "legacy-cutover",
                "legacy-database",
                &serde_json::json!({"phase": "inventory"}),
            )
            .expect("migration starts");
        assert_eq!(started.status, "started");
        let repeated = store
            .begin_migration_operation(
                "cutover-1",
                "legacy-cutover",
                "legacy-database",
                &serde_json::json!({"phase": "ignored retry"}),
            )
            .expect("retry resolves existing operation");
        assert_eq!(repeated.details, serde_json::json!({"phase": "inventory"}));
        assert!(
            store
                .begin_migration_operation(
                    "cutover-1",
                    "different-cutover",
                    "legacy-database",
                    &serde_json::Value::Null,
                )
                .is_err()
        );

        let completed = store
            .finish_migration_operation(
                "cutover-1",
                "completed",
                &serde_json::json!({"imported": 12}),
            )
            .expect("migration completes");
        assert_eq!(completed.status, "completed");
        assert!(completed.completed_at.is_some());
    }

    #[test]
    fn scopes_expires_and_revokes_dashboard_sessions() {
        use crate::auth::{generate_session_credentials, hash_owner_secret};

        let store = temp_store();
        let owner_hash = hash_owner_secret("owner-secret-with-enough-bytes").expect("owner hashes");
        store
            .set_owner_secret_hash(&owner_hash)
            .expect("owner hash stores");
        assert_eq!(
            store.owner_secret_hash().expect("owner reads"),
            Some(owner_hash)
        );

        let credentials = generate_session_credentials();
        let now = Utc::now();
        let session = store
            .create_dashboard_session(
                &credentials,
                &["logs:read".to_owned(), "review:write".to_owned()],
                now,
                Duration::minutes(30),
                Duration::hours(8),
            )
            .expect("session stores");
        assert!(!session.token_digest.contains(&credentials.session_token));
        assert!(
            store
                .authenticate_dashboard_session(
                    &credentials.session_token,
                    None,
                    "logs:read",
                    now + Duration::minutes(1),
                    Duration::minutes(30),
                )
                .is_ok()
        );
        assert!(
            store
                .authenticate_dashboard_session(
                    &credentials.session_token,
                    Some("wrong-csrf"),
                    "review:write",
                    now + Duration::minutes(1),
                    Duration::minutes(30),
                )
                .is_err()
        );
        assert!(
            store
                .authenticate_dashboard_session(
                    &credentials.session_token,
                    Some(&credentials.csrf_token),
                    "vault:write",
                    now + Duration::minutes(1),
                    Duration::minutes(30),
                )
                .is_err()
        );
        assert!(
            store
                .revoke_dashboard_session(&credentials.session_token)
                .expect("revokes")
        );
        assert!(
            store
                .authenticate_dashboard_session(
                    &credentials.session_token,
                    None,
                    "logs:read",
                    now + Duration::minutes(2),
                    Duration::minutes(30),
                )
                .is_err()
        );

        let expiring = generate_session_credentials();
        store
            .create_dashboard_session(
                &expiring,
                &["logs:read".to_owned()],
                now,
                Duration::minutes(5),
                Duration::minutes(10),
            )
            .expect("expiring session stores");
        assert!(
            store
                .authenticate_dashboard_session(
                    &expiring.session_token,
                    None,
                    "logs:read",
                    now + Duration::minutes(6),
                    Duration::minutes(5),
                )
                .is_err()
        );
    }

    #[test]
    fn stores_immutable_daily_snapshots_and_revisions() {
        let store = temp_store();
        let profile = store
            .create_pending_workspace_profile(
                "binding-daily",
                "Europe/Stockholm",
                "Work Log",
                "{date}.md",
                None,
                "markdown",
            )
            .expect("profile stores");
        let profile = store
            .activate_workspace_profile(&profile.id)
            .expect("profile activates");
        let conn = store.connect().expect("database opens");
        let now = Utc::now().to_rfc3339();
        conn.execute(
            r#"INSERT INTO daily_days
                (workspace_id, local_date, timezone, start_utc, end_utc,
                 destination_path, block_id, generation_status, review_status,
                 freshness, created_at, updated_at)
               VALUES (?1, '2026-09-07', 'Europe/Stockholm',
                       '2026-09-06T22:00:00Z', '2026-09-07T22:00:00Z',
                       'Work Log/2026-09-07.md', 'day-2026-09-07', 'ready',
                       'in_review', 'current', ?2, ?2)"#,
            params![profile.id, now],
        )
        .expect("day stores");
        conn.execute(
            "INSERT INTO evidence_snapshots (id, workspace_id, local_date, snapshot_digest, created_at) VALUES ('snapshot-1', ?1, '2026-09-07', 'digest-1', ?2)",
            params![profile.id, now],
        )
        .expect("snapshot stores");
        conn.execute(
            "INSERT INTO evidence_snapshot_events (snapshot_id, event_id, position, event_digest, disposition) VALUES ('snapshot-1', 'evt-1', 0, 'event-digest-1', 'include')",
            [],
        )
        .expect("snapshot evidence stores");
        conn.execute(
            "INSERT INTO proposal_revisions (id, workspace_id, local_date, snapshot_id, revision_number, origin, content_json, content_hash, created_at) VALUES ('revision-1', ?1, '2026-09-07', 'snapshot-1', 1, 'generated', '{}', 'content-digest-1', ?2)",
            params![profile.id, now],
        )
        .expect("revision stores");
        assert!(
            conn.execute(
                "INSERT INTO proposal_revisions (id, workspace_id, local_date, snapshot_id, revision_number, origin, content_json, content_hash, created_at) VALUES ('revision-2', ?1, '2026-09-07', 'snapshot-1', 1, 'generated', '{}', 'content-digest-2', ?2)",
                params![profile.id, now],
            )
            .is_err(),
            "revision numbers are immutable and unique within a day"
        );
    }

    #[test]
    fn stores_searches_and_marks_events() {
        let store = temp_store();
        let inserted = store
            .insert_event(LogEventInput {
                source: "windows/iis".to_owned(),
                level: Some("ERROR".to_owned()),
                timestamp: None,
                message: "Request failed with Bearer secret-token".to_owned(),
                metadata: Some(Map::from_iter([("status".to_owned(), Value::from(500))])),
                fingerprint: None,
            })
            .expect("event inserted");

        assert_eq!(inserted.level, "error");
        assert!(!inserted.message.contains("secret-token"));

        let result = store
            .query_logs(LogQuery {
                source: Some("windows/iis".to_owned()),
                since: None,
                level: Some("error".to_owned()),
                query: Some("failed".to_owned()),
                limit: Some(10),
            })
            .expect("query succeeds");
        assert_eq!(result.events.len(), 1);

        let reviewed = store
            .mark_reviewed(&[inserted.id], "Summarized in daily note", "test")
            .expect("mark reviewed succeeds");
        assert_eq!(reviewed.reviewed_count, 1);
    }

    #[test]
    fn persists_updates_and_deletes_generic_link_rules() {
        let store = temp_store();
        let now = Utc::now();
        let mut rule = VaultLinkRule {
            id: "rule_test".to_owned(),
            selectors: vec![crate::models::LinkSelector {
                field: "repo".to_owned(),
                operator: "exact".to_owned(),
                value: "portal-api".to_owned(),
            }],
            target_note_id: "Knowledge/Customer Portal".to_owned(),
            enabled: true,
            created_at: now,
            updated_at: now,
        };
        store.save_link_rule(&rule).expect("rule saved");
        assert_eq!(
            store.list_link_rules().expect("rules load"),
            vec![rule.clone()]
        );

        rule.enabled = false;
        rule.updated_at = now + Duration::seconds(1);
        store.save_link_rule(&rule).expect("rule updated");
        assert!(!store.list_link_rules().unwrap()[0].enabled);
        assert!(store.delete_link_rule(&rule.id).expect("rule deleted"));
        assert!(store.list_link_rules().unwrap().is_empty());
    }

    #[test]
    fn ignores_identities_idempotently_and_restores_them() {
        let store = temp_store();
        let ignored = store
            .ignore_link_identity("repo", "SweetOne", "sweetone")
            .expect("identity ignored");
        let duplicate = store
            .ignore_link_identity("repo", "sweet-one", "sweetone")
            .expect("duplicate ignore returns existing row");

        assert_eq!(ignored.id, duplicate.id);
        assert_eq!(store.list_ignored_link_identities().unwrap().len(), 1);
        assert!(
            store
                .restore_matching_ignored_identity("repo", "sweetone")
                .expect("identity restored")
        );
        assert!(store.list_ignored_link_identities().unwrap().is_empty());

        let ignored = store
            .ignore_link_identity("source", "codex/fedora", "codexfedora")
            .expect("identity ignored again");
        assert!(
            store
                .restore_ignored_link_identity(&ignored.id)
                .expect("identity restored by ID")
        );
    }

    #[test]
    fn preserves_large_accepted_messages_and_tracks_staging() {
        let store = temp_store();
        let message = "x".repeat(32 * 1024);
        let inserted = store
            .insert_event(LogEventInput {
                source: "codex/test".to_owned(),
                level: Some("info".to_owned()),
                timestamp: None,
                message: message.clone(),
                metadata: Some(Map::from_iter([
                    ("task_id".to_owned(), Value::from("task_123")),
                    ("sequence".to_owned(), Value::from(1)),
                ])),
                fingerprint: None,
            })
            .expect("event inserted");

        assert_eq!(inserted.message, message);
        assert!(!inserted.truncated);

        let now = Utc::now();
        let unstaged = store
            .get_unstaged_events(now, 10)
            .expect("unstaged events load");
        assert_eq!(unstaged.len(), 1);

        let staged = store
            .mark_staged(&[inserted.id], "proposal_test")
            .expect("staging state stored");
        assert_eq!(staged.staged_count, 1);
        assert!(
            store
                .get_unstaged_events(Utc::now(), 10)
                .expect("unstaged events reload")
                .is_empty()
        );
    }

    #[test]
    fn manual_entries_wait_for_daily_consolidation_instead_of_auto_staging() {
        let store = temp_store();
        store
            .insert_event(LogEventInput {
                source: "manual/dashboard".to_owned(),
                level: Some("info".to_owned()),
                timestamp: None,
                message: "Documented the design decision".to_owned(),
                metadata: Some(Map::from_iter([(
                    "entry_kind".to_owned(),
                    Value::from("manual"),
                )])),
                fingerprint: None,
            })
            .expect("manual event inserted");

        assert!(
            store
                .get_unstaged_events(Utc::now(), 10)
                .expect("unstaged automatic events load")
                .is_empty()
        );
    }

    #[test]
    fn searches_literal_underscores_in_metadata() {
        let store = temp_store();
        store
            .insert_event(LogEventInput {
                source: "codex/test".to_owned(),
                level: Some("info".to_owned()),
                timestamp: None,
                message: "Completed a demonstration".to_owned(),
                metadata: Some(Map::from_iter([(
                    "task_id".to_owned(),
                    Value::from("demo_task_123"),
                )])),
                fingerprint: None,
            })
            .expect("event inserted");

        let result = store
            .query_logs(LogQuery {
                source: None,
                since: None,
                level: None,
                query: Some("demo_task_123".to_owned()),
                limit: Some(10),
            })
            .expect("query succeeds");

        assert_eq!(result.events.len(), 1);
    }

    #[test]
    fn persists_application_preferences() {
        let store = temp_store();
        let preferences = BTreeMap::from([
            ("agent_name".to_owned(), "codex".to_owned()),
            ("ingest_url".to_owned(), "http://127.0.0.1:8787".to_owned()),
        ]);

        store
            .set_preferences(&preferences)
            .expect("preferences save");

        assert_eq!(
            store.get_preferences().expect("preferences load"),
            preferences
        );
    }

    #[test]
    fn reads_a_complete_bounded_event_day_in_time_order() {
        let store = temp_store();
        let day = Utc::now().date_naive();
        let start = day.and_hms_opt(0, 0, 0).unwrap().and_utc();
        for seconds in [20, 10, 30] {
            store
                .insert_event(LogEventInput {
                    source: "codex/test".to_owned(),
                    level: Some("info".to_owned()),
                    timestamp: Some(start + Duration::seconds(seconds)),
                    message: format!("event {seconds}"),
                    metadata: None,
                    fingerprint: None,
                })
                .expect("event stores");
        }

        let result = store
            .get_events_between(start, start + Duration::days(1), 2)
            .expect("daily events load");

        assert!(result.truncated);
        assert_eq!(result.events.len(), 2);
        assert_eq!(result.events[0].message, "event 10");
        assert_eq!(result.events[1].message, "event 20");
    }

    #[test]
    fn persists_and_deduplicates_daily_consolidation_jobs() {
        let store = temp_store();
        let start = Utc::now();
        let event = store
            .insert_event(LogEventInput {
                source: "codex/test".to_owned(),
                level: Some("info".to_owned()),
                timestamp: Some(start),
                message: "durable work".to_owned(),
                metadata: None,
                fingerprint: None,
            })
            .expect("event stores");
        let event_ids = vec![event.id];
        let first = store
            .enqueue_daily_consolidation(
                start,
                start + Duration::days(1),
                "Configured daily note",
                "context-1",
                &event_ids,
            )
            .expect("job queues");
        let duplicate = store
            .enqueue_daily_consolidation(
                start,
                start + Duration::days(1),
                "Configured daily note",
                "context-1",
                &event_ids,
            )
            .expect("duplicate resolves");

        assert_eq!(duplicate.id, first.id);
        let changed_context = store
            .enqueue_daily_consolidation(
                start,
                start + Duration::days(1),
                "Configured daily note",
                "context-2",
                &event_ids,
            )
            .expect("changed context queues a replacement");
        assert_ne!(changed_context.id, first.id);
        let running = store
            .claim_next_daily_consolidation()
            .expect("job claims")
            .expect("job exists");
        assert_eq!(running.status, "running");
        assert_eq!(
            store
                .get_daily_consolidation_events(&running.id)
                .expect("snapshot loads")
                .len(),
            1
        );
        let cancelling = store
            .request_daily_consolidation_cancel(&running.id)
            .expect("cancel stores")
            .expect("job remains");
        assert_eq!(cancelling.status, "cancel_requested");
        assert!(
            store
                .daily_consolidation_cancel_requested(&running.id)
                .expect("cancel reads")
        );
        store
            .recover_daily_consolidations()
            .expect("interrupted state recovers");
        let cancelled = store
            .get_daily_consolidation_job(&running.id)
            .expect("job reads")
            .expect("job remains");
        assert_eq!(cancelled.status, "cancelled");
        let pending = store
            .requeue_daily_consolidation(&running.id)
            .expect("job requeues")
            .expect("job remains");
        assert_eq!(pending.status, "pending");
    }

    #[test]
    fn rejects_instead_of_truncating_oversized_messages() {
        let store = temp_store();
        let error = store
            .insert_event(LogEventInput {
                source: "codex/test".to_owned(),
                level: None,
                timestamp: None,
                message: "x".repeat(MAX_MESSAGE_BYTES + 1),
                metadata: None,
                fingerprint: None,
            })
            .expect_err("oversized event rejected");

        assert!(error.to_string().contains("split it into ordered events"));
    }
}
