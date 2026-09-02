use crate::{
    models::{
        LogEventInput, LogQuery, LogQueryResult, MarkReviewedResult, SourceSummary, StoredLogEvent,
    },
    redaction::{redact_metadata, redact_text},
};
use anyhow::{Context, Result};
use chrono::{DateTime, Duration, Utc};
use rusqlite::{Connection, OptionalExtension, Row, params};
use serde_json::{Map, Value};
use std::{fs, path::PathBuf};
use uuid::Uuid;

const DEFAULT_LIMIT: usize = 100;
const MAX_LIMIT: usize = 500;
const MAX_MESSAGE_BYTES: usize = 16 * 1024;
const MAX_METADATA_BYTES: usize = 64 * 1024;

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
        let (message, message_truncated) =
            truncate_string(redact_text(input.message.trim()), MAX_MESSAGE_BYTES);
        let metadata = redact_metadata(input.metadata.unwrap_or_default());
        let (metadata_json, metadata_truncated) = encode_metadata(metadata)?;
        let truncated = message_truncated || metadata_truncated;

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
                truncated as i64,
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
            sql.push_str(" AND (e.message LIKE ? OR e.metadata_json LIKE ?)");
            let pattern = format!("%{}%", search.replace('%', "\\%").replace('_', "\\_"));
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

fn truncate_string(mut value: String, max_bytes: usize) -> (String, bool) {
    if value.len() <= max_bytes {
        return (value, false);
    }
    while value.len() > max_bytes {
        value.pop();
    }
    (value, true)
}

fn encode_metadata(metadata: Map<String, Value>) -> Result<(String, bool)> {
    let json = serde_json::to_string(&metadata)?;
    if json.len() <= MAX_METADATA_BYTES {
        return Ok((json, false));
    }

    let mut replacement = Map::new();
    replacement.insert("truncated".to_owned(), Value::Bool(true));
    replacement.insert(
        "reason".to_owned(),
        Value::String("metadata exceeded max size".to_owned()),
    );
    Ok((serde_json::to_string(&replacement)?, true))
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
                source: "examplewin/iis".to_owned(),
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
                source: Some("examplewin/iis".to_owned()),
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
}
