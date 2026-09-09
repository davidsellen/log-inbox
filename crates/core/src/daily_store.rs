use crate::{
    daily::resolve_day,
    models::{
        DailyDay, DailyRevisionContent, DailyWorkstream, EvidenceSnapshot, ManualDailyEntry,
        ProposalRevision,
    },
    store::Store,
};
use anyhow::{Context, Result};
use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::{OptionalExtension, Row, params};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

impl Store {
    pub fn ensure_daily_day(
        &self,
        local_date: NaiveDate,
        destination_path: &str,
        template_revision: Option<&str>,
    ) -> Result<DailyDay> {
        let profile = self
            .active_workspace_profile()?
            .context("an active workspace profile is required")?;
        validate_relative_path(destination_path)?;
        let resolved = resolve_day(local_date, &profile.timezone)?;
        let block_id = format!(
            "day_{}",
            &digest(format!("{}:{local_date}", profile.id).as_bytes())[..24]
        );
        let now = Utc::now().to_rfc3339();
        let conn = self.connect()?;
        conn.execute(
            r#"INSERT INTO daily_days
                (workspace_id, local_date, timezone, start_utc, end_utc,
                 destination_path, template_revision, block_id, generation_status,
                 review_status, freshness, created_at, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8,
                       'none', 'unresolved', 'current', ?9, ?9)
               ON CONFLICT(workspace_id, local_date) DO NOTHING"#,
            params![
                profile.id,
                local_date.to_string(),
                resolved.timezone,
                resolved.start_utc.to_rfc3339(),
                resolved.end_utc.to_rfc3339(),
                destination_path,
                template_revision,
                block_id,
                now
            ],
        )?;
        self.daily_day(&profile.id, local_date)?
            .context("daily day missing after creation")
    }

    pub fn daily_day(&self, workspace_id: &str, local_date: NaiveDate) -> Result<Option<DailyDay>> {
        let conn = self.connect()?;
        conn.query_row(
            "SELECT workspace_id, local_date, timezone, start_utc, end_utc, destination_path, template_revision, block_id, generation_status, review_status, freshness, current_revision_id, created_at, updated_at FROM daily_days WHERE workspace_id = ?1 AND local_date = ?2",
            params![workspace_id, local_date.to_string()],
            daily_day_from_row,
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn current_proposal_revision(
        &self,
        workspace_id: &str,
        local_date: NaiveDate,
    ) -> Result<Option<ProposalRevision>> {
        let Some(day) = self.daily_day(workspace_id, local_date)? else {
            return Ok(None);
        };
        let Some(revision_id) = day.current_revision_id else {
            return Ok(None);
        };
        self.proposal_revision(&revision_id)
    }

    pub fn set_daily_generation_status(
        &self,
        workspace_id: &str,
        local_date: NaiveDate,
        status: &str,
    ) -> Result<()> {
        anyhow::ensure!(
            matches!(status, "none" | "queued" | "running" | "ready" | "failed"),
            "invalid daily generation status"
        );
        let changed = self.connect()?.execute(
            "UPDATE daily_days SET generation_status = ?1, updated_at = ?2 WHERE workspace_id = ?3 AND local_date = ?4",
            params![status, Utc::now().to_rfc3339(), workspace_id, local_date.to_string()],
        )?;
        anyhow::ensure!(changed == 1, "daily day was not found");
        Ok(())
    }

    pub fn create_manual_daily_entry(
        &self,
        workspace_id: &str,
        local_date: NaiveDate,
        text: &str,
        references: &[String],
    ) -> Result<ManualDailyEntry> {
        let text = text.trim();
        anyhow::ensure!(!text.is_empty(), "manual entry text is required");
        anyhow::ensure!(text.len() <= 16 * 1024, "manual entry is too large");
        anyhow::ensure!(
            references.len() <= 20,
            "manual entry has too many references"
        );
        anyhow::ensure!(
            references.iter().all(|reference| {
                let authority = reference
                    .strip_prefix("https://")
                    .or_else(|| reference.strip_prefix("http://"));
                reference.len() <= 2048
                    && !reference.chars().any(char::is_whitespace)
                    && authority.is_some_and(|value| !value.is_empty() && !value.starts_with('/'))
            }),
            "manual references must be absolute HTTP(S) URLs"
        );
        self.daily_day(workspace_id, local_date)?
            .context("daily day is required before a manual entry")?;
        let id = format!("manual_{}", Uuid::new_v4().simple());
        let now = Utc::now().to_rfc3339();
        let conn = self.connect()?;
        conn.execute(
            "INSERT INTO manual_daily_entries (id, workspace_id, local_date, text, references_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
            params![
                id,
                workspace_id,
                local_date.to_string(),
                text,
                serde_json::to_string(references)?,
                now
            ],
        )?;
        self.manual_daily_entry(&id)?
            .context("manual entry missing after creation")
    }

    pub fn manual_daily_entries(
        &self,
        workspace_id: &str,
        local_date: NaiveDate,
    ) -> Result<Vec<ManualDailyEntry>> {
        let conn = self.connect()?;
        let mut statement = conn.prepare(
            "SELECT id, workspace_id, local_date, text, references_json, created_at, updated_at FROM manual_daily_entries WHERE workspace_id = ?1 AND local_date = ?2 ORDER BY created_at, id",
        )?;
        statement
            .query_map(
                params![workspace_id, local_date.to_string()],
                manual_entry_from_row,
            )?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    fn manual_daily_entry(&self, id: &str) -> Result<Option<ManualDailyEntry>> {
        self.connect()?
            .query_row(
                "SELECT id, workspace_id, local_date, text, references_json, created_at, updated_at FROM manual_daily_entries WHERE id = ?1",
                params![id],
                manual_entry_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn create_evidence_snapshot(
        &self,
        workspace_id: &str,
        local_date: NaiveDate,
        event_ids: &[String],
    ) -> Result<EvidenceSnapshot> {
        anyhow::ensure!(!event_ids.is_empty(), "evidence snapshot cannot be empty");
        let unique = event_ids.iter().collect::<HashSet<_>>();
        anyhow::ensure!(
            unique.len() == event_ids.len(),
            "evidence IDs must be unique"
        );
        let day = self
            .daily_day(workspace_id, local_date)?
            .context("daily day is required before its evidence snapshot")?;
        let events = self.get_events_by_ids(event_ids)?;
        anyhow::ensure!(
            events.len() == event_ids.len(),
            "one or more evidence events do not exist"
        );
        anyhow::ensure!(
            events
                .iter()
                .all(|event| event.timestamp >= day.start_utc && event.timestamp < day.end_utc),
            "evidence event is outside the frozen daily boundary"
        );
        let by_id = events
            .into_iter()
            .map(|event| (event.id.clone(), event))
            .collect::<HashMap<_, _>>();
        let event_digests = event_ids
            .iter()
            .map(|id| {
                let event = by_id.get(id).context("evidence event missing")?;
                Ok((id.clone(), evidence_event_digest(event)?))
            })
            .collect::<Result<Vec<_>>>()?;
        let snapshot_digest = digest(&serde_json::to_vec(&event_digests)?);
        let mut conn = self.connect()?;
        let transaction = conn.transaction()?;
        let existing = transaction
            .query_row(
                "SELECT id FROM evidence_snapshots WHERE workspace_id = ?1 AND local_date = ?2 AND snapshot_digest = ?3",
                params![workspace_id, local_date.to_string(), snapshot_digest],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let id = existing.unwrap_or_else(|| format!("snapshot_{}", Uuid::new_v4().simple()));
        let now = Utc::now().to_rfc3339();
        transaction.execute(
            "INSERT OR IGNORE INTO evidence_snapshots (id, workspace_id, local_date, snapshot_digest, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![id, workspace_id, local_date.to_string(), snapshot_digest, now],
        )?;
        for (position, (event_id, event_digest)) in event_digests.iter().enumerate() {
            transaction.execute(
                "INSERT OR IGNORE INTO evidence_snapshot_events (snapshot_id, event_id, position, event_digest) VALUES (?1, ?2, ?3, ?4)",
                params![id, event_id, position as i64, event_digest],
            )?;
        }
        transaction.commit()?;
        self.evidence_snapshot(&id)?
            .context("evidence snapshot missing after creation")
    }

    pub fn create_proposal_revision(
        &self,
        workspace_id: &str,
        local_date: NaiveDate,
        snapshot_id: Option<&str>,
        origin: &str,
        content: &Value,
    ) -> Result<ProposalRevision> {
        anyhow::ensure!(
            matches!(
                origin,
                "generated" | "structured_edit" | "manual" | "regenerated" | "advanced_markdown"
            ),
            "invalid proposal revision origin"
        );
        self.daily_day(workspace_id, local_date)?
            .context("daily day is required before a proposal revision")?;
        let snapshot = if let Some(snapshot_id) = snapshot_id {
            let snapshot = self
                .evidence_snapshot(snapshot_id)?
                .context("proposal evidence snapshot does not exist")?;
            anyhow::ensure!(
                snapshot.workspace_id == workspace_id && snapshot.local_date == local_date,
                "proposal evidence snapshot belongs to a different day"
            );
            Some(snapshot)
        } else {
            None
        };
        if origin != "advanced_markdown" {
            let structured: DailyRevisionContent = serde_json::from_value(content.clone())
                .context("proposal revision does not match the structured daily schema")?;
            self.validate_daily_revision_content(
                workspace_id,
                local_date,
                origin,
                &structured,
                snapshot.as_ref(),
            )?;
        }
        let content_json = serde_json::to_string(content)?;
        let content_hash = digest(content_json.as_bytes());
        let mut conn = self.connect()?;
        let transaction = conn.transaction()?;
        let revision_number: u64 = transaction.query_row(
            "SELECT COALESCE(MAX(revision_number), 0) + 1 FROM proposal_revisions WHERE workspace_id = ?1 AND local_date = ?2",
            params![workspace_id, local_date.to_string()],
            |row| row.get(0),
        )?;
        let id = format!("revision_{}", Uuid::new_v4().simple());
        let now = Utc::now().to_rfc3339();
        transaction.execute(
            "INSERT INTO proposal_revisions (id, workspace_id, local_date, snapshot_id, revision_number, origin, content_json, content_hash, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![id, workspace_id, local_date.to_string(), snapshot_id, revision_number, origin, content_json, content_hash, now],
        )?;
        transaction.execute(
            "UPDATE daily_days SET current_revision_id = ?1, generation_status = 'ready', review_status = 'in_review', updated_at = ?2 WHERE workspace_id = ?3 AND local_date = ?4",
            params![id, now, workspace_id, local_date.to_string()],
        )?;
        transaction.commit()?;
        self.proposal_revision(&id)?
            .context("proposal revision missing after creation")
    }

    pub fn decide_snapshot_evidence(
        &self,
        snapshot_id: &str,
        event_id: &str,
        disposition: &str,
        related_event_id: Option<&str>,
        actor: &str,
        reason: Option<&str>,
    ) -> Result<()> {
        anyhow::ensure!(
            matches!(
                disposition,
                "include" | "omit" | "duplicate_of" | "superseded_by"
            ),
            "invalid evidence disposition"
        );
        anyhow::ensure!(!actor.trim().is_empty(), "decision actor is required");
        anyhow::ensure!(
            matches!(disposition, "include" | "omit") || related_event_id.is_some(),
            "related evidence is required for duplicate or superseded decisions"
        );
        let conn = self.connect()?;
        if let Some(related_event_id) = related_event_id {
            let related_exists = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM evidence_snapshot_events WHERE snapshot_id = ?1 AND event_id = ?2)",
                params![snapshot_id, related_event_id],
                |row| row.get::<_, bool>(0),
            )?;
            anyhow::ensure!(related_exists, "related evidence is outside this snapshot");
        }
        let changed = conn.execute(
            "UPDATE evidence_snapshot_events SET disposition = ?1, related_event_id = ?2, decision_actor = ?3, decision_reason = ?4, decided_at = ?5 WHERE snapshot_id = ?6 AND event_id = ?7",
            params![disposition, related_event_id, actor, reason, Utc::now().to_rfc3339(), snapshot_id, event_id],
        )?;
        anyhow::ensure!(changed == 1, "snapshot evidence was not found");
        Ok(())
    }

    pub fn evidence_snapshot(&self, id: &str) -> Result<Option<EvidenceSnapshot>> {
        let conn = self.connect()?;
        let header = conn.query_row(
            "SELECT id, workspace_id, local_date, snapshot_digest, created_at FROM evidence_snapshots WHERE id = ?1",
            params![id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get::<_, String>(2)?, row.get(3)?, row.get::<_, String>(4)?)),
        ).optional()?;
        let Some((id, workspace_id, local_date, snapshot_digest, created_at)) = header else {
            return Ok(None);
        };
        let mut statement = conn.prepare("SELECT event_id FROM evidence_snapshot_events WHERE snapshot_id = ?1 ORDER BY position")?;
        let event_ids = statement
            .query_map(params![id], |row| row.get(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(Some(EvidenceSnapshot {
            id,
            workspace_id,
            local_date: parse_date(&local_date)?,
            snapshot_digest,
            event_ids,
            created_at: parse_time(&created_at)?,
        }))
    }

    fn validate_daily_revision_content(
        &self,
        workspace_id: &str,
        local_date: NaiveDate,
        origin: &str,
        content: &DailyRevisionContent,
        snapshot: Option<&EvidenceSnapshot>,
    ) -> Result<()> {
        anyhow::ensure!(
            content.schema_version == 1,
            "unsupported daily revision schema"
        );
        let manual_ids = content.manual_entry_ids.iter().collect::<HashSet<_>>();
        anyhow::ensure!(
            manual_ids.len() == content.manual_entry_ids.len(),
            "manual entry IDs must be unique"
        );
        let available_manual_ids = self
            .manual_daily_entries(workspace_id, local_date)?
            .into_iter()
            .map(|entry| entry.id)
            .collect::<HashSet<_>>();
        anyhow::ensure!(
            manual_ids
                .iter()
                .all(|entry_id| available_manual_ids.contains(entry_id.as_str())),
            "proposal contains a manual entry from a different day"
        );
        if origin == "manual" {
            anyhow::ensure!(
                snapshot.is_none(),
                "manual revision cannot have automated evidence"
            );
            anyhow::ensure!(
                content.workstreams.is_empty(),
                "manual revision cannot have workstreams"
            );
            anyhow::ensure!(
                !manual_ids.is_empty(),
                "manual revision requires a manual entry"
            );
            return Ok(());
        }
        let Some(snapshot) = snapshot else {
            anyhow::ensure!(
                content.workstreams.is_empty() && !manual_ids.is_empty(),
                "revision without a snapshot must contain only manual entries"
            );
            return Ok(());
        };
        validate_workstream_evidence(&content.workstreams, &snapshot.event_ids)
    }

    fn proposal_revision(&self, id: &str) -> Result<Option<ProposalRevision>> {
        self.connect()?.query_row(
            "SELECT id, workspace_id, local_date, snapshot_id, revision_number, origin, content_json, content_hash, created_at FROM proposal_revisions WHERE id = ?1",
            params![id],
            proposal_revision_from_row,
        ).optional().map_err(Into::into)
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn evidence_event_digest(event: &crate::models::StoredLogEvent) -> Result<String> {
    let immutable_envelope = serde_json::json!({
        "id": event.id,
        "received_at": event.received_at,
        "timestamp": event.timestamp,
        "source": event.source,
        "level": event.level,
        "message": event.message,
        "metadata": event.metadata,
        "fingerprint": event.fingerprint,
        "truncated": event.truncated,
    });
    Ok(digest(&serde_json::to_vec(&immutable_envelope)?))
}

fn validate_relative_path(path: &str) -> Result<()> {
    let candidate = std::path::Path::new(path);
    anyhow::ensure!(
        !path.trim().is_empty() && !candidate.is_absolute(),
        "daily destination must be a relative path"
    );
    anyhow::ensure!(
        candidate
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_))),
        "daily destination cannot contain traversal"
    );
    Ok(())
}

fn parse_date(value: &str) -> Result<NaiveDate> {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").map_err(Into::into)
}
fn parse_time(value: &str) -> Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(value)?.with_timezone(&Utc))
}

fn daily_day_from_row(row: &Row<'_>) -> rusqlite::Result<DailyDay> {
    let parse = |index, value: String| {
        parse_time(&value).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                index,
                rusqlite::types::Type::Text,
                error.into(),
            )
        })
    };
    let local_date: String = row.get(1)?;
    Ok(DailyDay {
        workspace_id: row.get(0)?,
        local_date: NaiveDate::parse_from_str(&local_date, "%Y-%m-%d").map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(1, rusqlite::types::Type::Text, error.into())
        })?,
        timezone: row.get(2)?,
        start_utc: parse(3, row.get(3)?)?,
        end_utc: parse(4, row.get(4)?)?,
        destination_path: row.get(5)?,
        template_revision: row.get(6)?,
        block_id: row.get(7)?,
        generation_status: row.get(8)?,
        review_status: row.get(9)?,
        freshness: row.get(10)?,
        current_revision_id: row.get(11)?,
        created_at: parse(12, row.get(12)?)?,
        updated_at: parse(13, row.get(13)?)?,
    })
}

fn proposal_revision_from_row(row: &Row<'_>) -> rusqlite::Result<ProposalRevision> {
    let local_date: String = row.get(2)?;
    let content_json: String = row.get(6)?;
    let created: String = row.get(8)?;
    Ok(ProposalRevision {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        local_date: parse_date(&local_date).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, e.into())
        })?,
        snapshot_id: row.get(3)?,
        revision_number: row.get(4)?,
        origin: row.get(5)?,
        content: serde_json::from_str(&content_json).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, e.into())
        })?,
        content_hash: row.get(7)?,
        created_at: parse_time(&created).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, e.into())
        })?,
    })
}

fn manual_entry_from_row(row: &Row<'_>) -> rusqlite::Result<ManualDailyEntry> {
    let local_date: String = row.get(2)?;
    let references: String = row.get(4)?;
    let created_at: String = row.get(5)?;
    let updated_at: String = row.get(6)?;
    Ok(ManualDailyEntry {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        local_date: parse_date(&local_date).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, error.into())
        })?,
        text: row.get(3)?,
        references: serde_json::from_str(&references).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, error.into())
        })?,
        created_at: parse_time(&created_at).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, error.into())
        })?,
        updated_at: parse_time(&updated_at).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, error.into())
        })?,
    })
}

fn validate_workstream_evidence(
    workstreams: &[DailyWorkstream],
    expected_event_ids: &[String],
) -> Result<()> {
    anyhow::ensure!(
        !workstreams.is_empty(),
        "automated revision requires a workstream"
    );
    let expected = expected_event_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut covered = HashSet::new();
    let mut workstream_ids = HashSet::new();
    for workstream in workstreams {
        anyhow::ensure!(
            !workstream.id.trim().is_empty() && !workstream.title.trim().is_empty(),
            "workstream ID and title are required"
        );
        anyhow::ensure!(
            workstream_ids.insert(&workstream.id),
            "workstream IDs must be unique"
        );
        let workstream_evidence = workstream
            .evidence_event_ids
            .iter()
            .map(String::as_str)
            .collect::<HashSet<_>>();
        anyhow::ensure!(
            workstream_evidence.len() == workstream.evidence_event_ids.len()
                && !workstream_evidence.is_empty(),
            "workstream evidence must be nonempty and unique"
        );
        anyhow::ensure!(
            workstream_evidence
                .iter()
                .all(|event_id| expected.contains(event_id)),
            "workstream contains evidence outside its snapshot"
        );
        anyhow::ensure!(
            workstream_evidence
                .iter()
                .all(|event_id| covered.insert(*event_id)),
            "snapshot evidence appears in multiple workstreams"
        );
        let facts = [
            &workstream.outcome,
            &workstream.decision,
            &workstream.trade_off,
            &workstream.validation,
            &workstream.blocker,
            &workstream.follow_up,
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        anyhow::ensure!(!facts.is_empty(), "workstream requires a factual field");
        let mut supported = HashSet::new();
        for fact in facts {
            anyhow::ensure!(
                !fact.text.trim().is_empty() && !fact.evidence_event_ids.is_empty(),
                "daily fact requires text and evidence"
            );
            for event_id in &fact.evidence_event_ids {
                anyhow::ensure!(
                    workstream_evidence.contains(event_id.as_str()),
                    "daily fact cites evidence outside its workstream"
                );
                supported.insert(event_id.as_str());
            }
        }
        anyhow::ensure!(
            supported == workstream_evidence,
            "workstream evidence must support a factual field"
        );
    }
    anyhow::ensure!(
        covered == expected,
        "proposal does not cover the complete snapshot"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::LogEventInput;

    #[test]
    fn keeps_days_snapshots_and_revisions_immutable() {
        let store = Store::open(
            std::env::temp_dir().join(format!("log-inbox-daily-domain-{}.sqlite3", Uuid::new_v4())),
        )
        .expect("store opens");
        let profile = store
            .create_pending_workspace_profile(
                "daily-binding",
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
        let date = NaiveDate::from_ymd_opt(2026, 3, 29).expect("date");
        let day = store
            .ensure_daily_day(date, "Work Log/2026-03-29.md", None)
            .expect("day stores");
        let frozen = store
            .ensure_daily_day(date, "Other/ignored.md", Some("ignored"))
            .expect("existing day resolves");
        assert_eq!(frozen.destination_path, day.destination_path);
        assert_eq!(day.end_utc - day.start_utc, chrono::Duration::hours(23));
        assert!(
            store
                .create_manual_daily_entry(
                    &profile.id,
                    date,
                    "Invalid reference",
                    &["javascript:alert(1)".to_owned()],
                )
                .is_err()
        );
        let manual = store
            .create_manual_daily_entry(
                &profile.id,
                date,
                "Recorded the reviewed trade-off.",
                &["https://example.test/decisions/42".to_owned()],
            )
            .expect("manual entry stores");
        assert_eq!(manual.text, "Recorded the reviewed trade-off.");
        let manual_entries = store
            .manual_daily_entries(&profile.id, date)
            .expect("manual entries read");
        assert_eq!(manual_entries.len(), 1);
        assert_eq!(manual_entries[0], manual);
        let manual_revision = store
            .create_proposal_revision(
                &profile.id,
                date,
                None,
                "manual",
                &serde_json::json!({
                    "schema_version": 1,
                    "manual_entry_ids": [manual.id],
                    "workstreams": []
                }),
            )
            .expect("manual revision stores");
        assert_eq!(manual_revision.origin, "manual");

        let first = store
            .insert_event(LogEventInput {
                source: "codex/test".to_owned(),
                level: None,
                timestamp: Some(day.start_utc + chrono::Duration::hours(1)),
                message: "Decision: keep review before apply.".to_owned(),
                metadata: None,
                fingerprint: None,
            })
            .expect("first event stores");
        let second = store
            .insert_event(LogEventInput {
                source: "codex/test".to_owned(),
                level: None,
                timestamp: Some(day.start_utc + chrono::Duration::hours(2)),
                message: "Validation passed.".to_owned(),
                metadata: None,
                fingerprint: None,
            })
            .expect("second event stores");
        let event_ids = vec![first.id, second.id];
        let outside = store
            .insert_event(LogEventInput {
                source: "codex/test".to_owned(),
                level: None,
                timestamp: Some(day.end_utc),
                message: "This belongs to the next day.".to_owned(),
                metadata: None,
                fingerprint: None,
            })
            .expect("outside event stores");
        assert!(
            store
                .create_evidence_snapshot(&profile.id, date, &[outside.id])
                .unwrap_err()
                .to_string()
                .contains("outside the frozen daily boundary")
        );
        let snapshot = store
            .create_evidence_snapshot(&profile.id, date, &event_ids)
            .expect("snapshot stores");
        assert!(
            store
                .create_proposal_revision(
                    &profile.id,
                    date,
                    Some(&snapshot.id),
                    "generated",
                    &serde_json::json!({
                        "schema_version": 1,
                        "workstreams": []
                    }),
                )
                .unwrap_err()
                .to_string()
                .contains("requires a workstream")
        );
        let repeated = store
            .create_evidence_snapshot(&profile.id, date, &event_ids)
            .expect("snapshot is idempotent");
        assert_eq!(repeated.id, snapshot.id);
        store
            .mark_reviewed(&event_ids, "Daily note", "owner")
            .expect("review state changes");
        let after_review = store
            .create_evidence_snapshot(&profile.id, date, &event_ids)
            .expect("derived review state does not alter snapshot identity");
        assert_eq!(after_review.id, snapshot.id);
        store
            .set_daily_generation_status(&profile.id, date, "failed")
            .expect("generation failure records");
        assert_eq!(
            store
                .daily_day(&profile.id, date)
                .unwrap()
                .unwrap()
                .generation_status,
            "failed"
        );
        store
            .connect()
            .unwrap()
            .execute(
                "UPDATE log_events SET received_at = ?1 WHERE id = ?2",
                params![
                    (Utc::now() - chrono::Duration::days(31)).to_rfc3339(),
                    event_ids[0]
                ],
            )
            .expect("snapshotted evidence ages");
        assert_eq!(store.prune_old_events(30).expect("retention runs"), 0);
        assert_eq!(
            store
                .get_events_by_ids(&event_ids[..1])
                .expect("evidence reads")
                .len(),
            1
        );
        store
            .decide_snapshot_evidence(&snapshot.id, &event_ids[0], "include", None, "owner", None)
            .expect("evidence decision stores");

        let first_revision = store
            .create_proposal_revision(
                &profile.id,
                date,
                Some(&snapshot.id),
                "generated",
                &serde_json::json!({
                    "schema_version": 1,
                    "manual_entry_ids": [manual.id],
                    "workstreams": [{
                        "id": "task:test",
                        "title": "Daily domain",
                        "evidence_event_ids": event_ids,
                        "decision": [{
                            "text": "Kept review before apply.",
                            "evidence_event_ids": [event_ids[0]]
                        }],
                        "validation": [{
                            "text": "Validation passed.",
                            "evidence_event_ids": [event_ids[1]]
                        }]
                    }]
                }),
            )
            .expect("first revision stores");
        let edited = store
            .create_proposal_revision(
                &profile.id,
                date,
                Some(&snapshot.id),
                "structured_edit",
                &serde_json::json!({
                    "schema_version": 1,
                    "manual_entry_ids": [manual.id],
                    "workstreams": [{
                        "id": "task:test",
                        "title": "Daily domain",
                        "evidence_event_ids": event_ids,
                        "outcome": [{
                            "text": "Reviewed the immutable daily domain.",
                            "evidence_event_ids": event_ids
                        }]
                    }]
                }),
            )
            .expect("edited revision stores");
        assert_eq!(first_revision.revision_number, 2);
        assert_eq!(edited.revision_number, 3);
        assert_eq!(
            store
                .daily_day(&profile.id, date)
                .expect("day reads")
                .expect("day exists")
                .current_revision_id,
            Some(edited.id)
        );
    }
}
