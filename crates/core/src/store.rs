use crate::{
    models::{
        DailyConsolidationJob, LogEventInput, LogQuery, LogQueryResult, MarkReviewedResult,
        SourceSummary, StagedEventGroup, StoredLogEvent, VaultLinkRule,
    },
    redaction::{redact_metadata, redact_text},
};
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::{collections::BTreeMap, fs, path::PathBuf};
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
        let conn = self.connect()?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.execute_batch(
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
            "#,
        )?;
        Ok(())
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

        let conn = self.connect()?;
        conn.execute(
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

        self.get_event(&id)?.context("inserted event missing")
    }

    pub fn prune_old_events(&self, retention_days: u64) -> Result<usize> {
        let cutoff = Utc::now() - Duration::days(retention_days as i64);
        let conn = self.connect()?;
        let changed = conn.execute(
            "DELETE FROM log_events WHERE timestamp < ?1",
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

    fn connect(&self) -> Result<Connection> {
        Connection::open(&self.db_path)
            .with_context(|| format!("opening {}", self.db_path.display()))
    }
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
