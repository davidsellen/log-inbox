use crate::{
    daily::resolve_day,
    models::{
        ApplyOperation, DailyDay, DailyRevisionContent, DailyWorkstream, EvidenceSnapshot,
        ManualDailyEntry, PrepareApplyOperation, ProposalRevision, SnapshotEvidence,
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
                "INSERT OR IGNORE INTO evidence_snapshot_events (snapshot_id, event_id, live_event_id, position, event_digest) VALUES (?1, ?2, ?2, ?3, ?4)",
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
        self.create_proposal_revision_internal(
            workspace_id,
            local_date,
            snapshot_id,
            origin,
            content,
            None,
        )
    }

    pub fn create_proposal_revision_if_current(
        &self,
        workspace_id: &str,
        local_date: NaiveDate,
        snapshot_id: Option<&str>,
        origin: &str,
        content: &Value,
        expected_current_revision_id: &str,
    ) -> Result<ProposalRevision> {
        anyhow::ensure!(
            !expected_current_revision_id.trim().is_empty(),
            "expected current revision ID is required"
        );
        self.create_proposal_revision_internal(
            workspace_id,
            local_date,
            snapshot_id,
            origin,
            content,
            Some(expected_current_revision_id),
        )
    }

    fn create_proposal_revision_internal(
        &self,
        workspace_id: &str,
        local_date: NaiveDate,
        snapshot_id: Option<&str>,
        origin: &str,
        content: &Value,
        expected_current_revision_id: Option<&str>,
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
        if let Some(expected) = expected_current_revision_id {
            let current: Option<String> = transaction
                .query_row(
                    "SELECT current_revision_id FROM daily_days WHERE workspace_id = ?1 AND local_date = ?2",
                    params![workspace_id, local_date.to_string()],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();
            anyhow::ensure!(
                current.as_deref() == Some(expected),
                "current proposal revision changed"
            );
        }
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

    pub fn prepare_apply_operation(&self, input: &PrepareApplyOperation) -> Result<ApplyOperation> {
        validate_apply_operation_input(input)?;
        let day = self
            .daily_day(&input.workspace_id, input.local_date)?
            .context("apply operation day does not exist")?;
        anyhow::ensure!(
            day.destination_path == input.destination_path,
            "apply destination differs from the frozen daily destination"
        );
        anyhow::ensure!(
            day.current_revision_id.as_deref() == Some(input.revision_id.as_str()),
            "apply revision is not the current daily revision"
        );
        let revision = self
            .proposal_revision(&input.revision_id)?
            .context("apply proposal revision does not exist")?;
        anyhow::ensure!(
            revision.workspace_id == input.workspace_id
                && revision.local_date == input.local_date
                && revision.content_hash == input.revision_content_hash,
            "apply revision identity or content hash does not match"
        );

        let now = Utc::now().to_rfc3339();
        let conn = self.connect()?;
        conn.execute(
            r#"INSERT OR IGNORE INTO apply_operations
                (id, workspace_id, local_date, revision_id, revision_content_hash,
                 destination_path, expected_old_block_hash, intended_new_block_hash,
                 recovery_payload, recovery_path, state, failure_reason, created_at, updated_at,
                 expected_target_exists, expected_original_content_hash,
                 intended_updated_content_hash, temporary_name)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
                       'prepared', NULL, ?11, ?11, ?12, ?13, ?14, ?15)"#,
            params![
                input.id,
                input.workspace_id,
                input.local_date.to_string(),
                input.revision_id,
                input.revision_content_hash,
                input.destination_path,
                input.expected_old_block_hash,
                input.intended_new_block_hash,
                input.recovery_payload,
                input.recovery_path,
                now,
                input.expected_target_exists,
                input.expected_original_content_hash,
                input.intended_updated_content_hash,
                input.temporary_name,
            ],
        )?;
        let operation = self
            .apply_operation(&input.id)?
            .context("apply operation missing after preparation")?;
        anyhow::ensure!(
            operation_matches_input(&operation, input),
            "apply operation ID already belongs to different immutable inputs"
        );
        Ok(operation)
    }

    pub fn apply_operation(&self, id: &str) -> Result<Option<ApplyOperation>> {
        self.connect()?
            .query_row(
                r#"SELECT id, workspace_id, local_date, revision_id,
                          revision_content_hash, destination_path,
                          expected_old_block_hash, intended_new_block_hash,
                          recovery_payload, recovery_path, state, failure_reason,
                          created_at, updated_at, expected_target_exists,
                          expected_original_content_hash, intended_updated_content_hash,
                          temporary_name
                   FROM apply_operations WHERE id = ?1"#,
                params![id],
                apply_operation_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_unfinished_apply_operations(&self, limit: usize) -> Result<Vec<ApplyOperation>> {
        self.list_apply_operations_by_states(
            &[
                "prepared",
                "writing",
                "written",
                "failed",
                "reconciliation_required",
            ],
            limit,
        )
    }

    pub fn list_recoverable_apply_operations(&self, limit: usize) -> Result<Vec<ApplyOperation>> {
        self.list_apply_operations_by_states(&["prepared", "writing", "written"], limit)
    }

    fn list_apply_operations_by_states(
        &self,
        states: &[&str],
        limit: usize,
    ) -> Result<Vec<ApplyOperation>> {
        anyhow::ensure!(!states.is_empty(), "at least one Apply state is required");
        anyhow::ensure!(
            states.iter().all(|state| valid_apply_state(state)),
            "invalid Apply state filter"
        );
        let conn = self.connect()?;
        let placeholders = (1..=states.len())
            .map(|index| format!("?{index}"))
            .collect::<Vec<_>>()
            .join(", ");
        let limit_parameter = states.len() + 1;
        let sql = format!(
            r#"SELECT id, workspace_id, local_date, revision_id,
                      revision_content_hash, destination_path,
                      expected_old_block_hash, intended_new_block_hash,
                      recovery_payload, recovery_path, state, failure_reason,
                      created_at, updated_at, expected_target_exists,
                      expected_original_content_hash, intended_updated_content_hash,
                      temporary_name
               FROM apply_operations
               WHERE state IN ({placeholders})
               ORDER BY updated_at, created_at, id
               LIMIT ?{limit_parameter}"#
        );
        let mut statement = conn.prepare(&sql)?;
        let mut parameters = states
            .iter()
            .map(|state| state as &dyn rusqlite::ToSql)
            .collect::<Vec<_>>();
        let bounded_limit = limit.clamp(1, 500) as i64;
        parameters.push(&bounded_limit);
        statement
            .query_map(parameters.as_slice(), apply_operation_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn transition_apply_operation(
        &self,
        id: &str,
        expected_state: &str,
        next_state: &str,
        reason: Option<&str>,
    ) -> Result<ApplyOperation> {
        anyhow::ensure!(
            valid_apply_state(expected_state) && valid_apply_state(next_state),
            "invalid apply operation state"
        );
        let reason = reason.map(str::trim).filter(|value| !value.is_empty());
        anyhow::ensure!(
            reason.is_none_or(|value| value.len() <= 4096),
            "apply transition reason is too large"
        );
        let needs_reason = matches!(next_state, "failed" | "reconciliation_required");
        anyhow::ensure!(
            needs_reason == reason.is_some(),
            "failed or reconciliation transitions require a reason; other transitions forbid one"
        );

        let current = self
            .apply_operation(id)?
            .context("apply operation does not exist")?;
        if current.state == next_state {
            anyhow::ensure!(
                current.failure_reason.as_deref() == reason,
                "idempotent transition reason differs from the stored reason"
            );
            return Ok(current);
        }
        anyhow::ensure!(
            current.state == expected_state,
            "apply operation state changed: expected {expected_state}, found {}",
            current.state
        );
        anyhow::ensure!(
            allowed_apply_transition(expected_state, next_state),
            "apply operation transition {expected_state} -> {next_state} is not allowed"
        );
        let now = Utc::now().to_rfc3339();
        let changed = self.connect()?.execute(
            "UPDATE apply_operations SET state = ?1, failure_reason = ?2, updated_at = ?3 WHERE id = ?4 AND state = ?5",
            params![next_state, reason, now, id, expected_state],
        )?;
        anyhow::ensure!(changed == 1, "apply operation state changed concurrently");
        self.apply_operation(id)?
            .context("apply operation missing after transition")
    }

    pub fn finalize_apply_operation(
        &self,
        id: &str,
        expected_revision_id: &str,
        expected_revision_content_hash: &str,
    ) -> Result<ApplyOperation> {
        let mut conn = self.connect()?;
        let transaction = conn.transaction()?;
        let operation = transaction
            .query_row(
                r#"SELECT id, workspace_id, local_date, revision_id,
                          revision_content_hash, destination_path,
                          expected_old_block_hash, intended_new_block_hash,
                          recovery_payload, recovery_path, state, failure_reason,
                          created_at, updated_at, expected_target_exists,
                          expected_original_content_hash, intended_updated_content_hash,
                          temporary_name
                   FROM apply_operations WHERE id = ?1"#,
                params![id],
                apply_operation_from_row,
            )
            .optional()?
            .context("apply operation does not exist")?;
        anyhow::ensure!(
            operation.revision_id == expected_revision_id
                && operation.revision_content_hash == expected_revision_content_hash,
            "apply finalization revision identity does not match"
        );
        if operation.state == "finalized" {
            return Ok(operation);
        }
        anyhow::ensure!(
            operation.state == "written",
            "only a written apply operation can be finalized"
        );

        let current_revision_id: Option<String> = transaction.query_row(
            "SELECT current_revision_id FROM daily_days WHERE workspace_id = ?1 AND local_date = ?2",
            params![operation.workspace_id, operation.local_date.to_string()],
            |row| row.get(0),
        )?;
        anyhow::ensure!(
            current_revision_id.as_deref() == Some(operation.revision_id.as_str()),
            "daily revision changed before apply finalization"
        );
        let (revision_hash, snapshot_id): (String, Option<String>) = transaction.query_row(
            "SELECT content_hash, snapshot_id FROM proposal_revisions WHERE id = ?1 AND workspace_id = ?2 AND local_date = ?3",
            params![
                operation.revision_id,
                operation.workspace_id,
                operation.local_date.to_string()
            ],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        anyhow::ensure!(
            revision_hash == operation.revision_content_hash,
            "stored proposal revision content hash changed"
        );

        if let Some(snapshot_id) = snapshot_id {
            let unresolved: u64 = transaction.query_row(
                "SELECT COUNT(*) FROM evidence_snapshot_events WHERE snapshot_id = ?1 AND disposition IS NULL",
                params![snapshot_id],
                |row| row.get(0),
            )?;
            anyhow::ensure!(
                unresolved == 0,
                "all snapshot evidence requires a review decision before finalization"
            );
            let now = Utc::now().to_rfc3339();
            transaction.execute(
                r#"INSERT INTO review_state (event_id, reviewed_at, reviewed_by, note)
                   SELECT live_event_id, ?1, ?2, ?3
                   FROM evidence_snapshot_events
                   WHERE snapshot_id = ?4
                     AND disposition = 'include'
                     AND live_event_id IS NOT NULL
                   ON CONFLICT(event_id) DO UPDATE SET
                       reviewed_at = excluded.reviewed_at,
                       reviewed_by = excluded.reviewed_by,
                       note = excluded.note"#,
                params![
                    now,
                    format!("apply:{}", operation.id),
                    operation.destination_path,
                    snapshot_id
                ],
            )?;
        }

        let now = Utc::now().to_rfc3339();
        let day_changed = transaction.execute(
            "UPDATE daily_days SET review_status = 'applied', freshness = 'current', updated_at = ?1 WHERE workspace_id = ?2 AND local_date = ?3 AND current_revision_id = ?4",
            params![
                now,
                operation.workspace_id,
                operation.local_date.to_string(),
                operation.revision_id
            ],
        )?;
        anyhow::ensure!(
            day_changed == 1,
            "daily revision changed during finalization"
        );
        let operation_changed = transaction.execute(
            "UPDATE apply_operations SET state = 'finalized', failure_reason = NULL, updated_at = ?1 WHERE id = ?2 AND state = 'written'",
            params![now, operation.id],
        )?;
        anyhow::ensure!(
            operation_changed == 1,
            "apply operation state changed during finalization"
        );
        transaction.commit()?;
        let finalized = self
            .apply_operation(id)?
            .context("apply operation missing after finalization")?;
        debug_assert_eq!(finalized.state, "finalized");
        Ok(finalized)
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
            reason.is_none_or(|value| value.len() <= 2048),
            "decision reason is too large"
        );
        anyhow::ensure!(
            matches!(disposition, "include" | "omit") || related_event_id.is_some(),
            "related evidence is required for duplicate or superseded decisions"
        );
        anyhow::ensure!(
            !matches!(disposition, "include" | "omit") || related_event_id.is_none(),
            "include or omit decisions cannot name related evidence"
        );
        anyhow::ensure!(
            related_event_id != Some(event_id),
            "evidence cannot refer to itself"
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

    pub fn reopen_snapshot_evidence(&self, snapshot_id: &str, event_id: &str) -> Result<()> {
        let changed = self.connect()?.execute(
            "UPDATE evidence_snapshot_events SET disposition = NULL, related_event_id = NULL, decision_actor = NULL, decision_reason = NULL, decided_at = NULL WHERE snapshot_id = ?1 AND event_id = ?2",
            params![snapshot_id, event_id],
        )?;
        anyhow::ensure!(changed == 1, "snapshot evidence was not found");
        Ok(())
    }

    pub fn snapshot_evidence(&self, snapshot_id: &str) -> Result<Vec<SnapshotEvidence>> {
        let conn = self.connect()?;
        let mut statement = conn.prepare(
            "SELECT event_id, position, event_digest, disposition, related_event_id, decision_actor, decision_reason, decided_at FROM evidence_snapshot_events WHERE snapshot_id = ?1 ORDER BY position",
        )?;
        statement
            .query_map(params![snapshot_id], snapshot_evidence_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
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

fn validate_apply_operation_input(input: &PrepareApplyOperation) -> Result<()> {
    anyhow::ensure!(
        !input.id.is_empty()
            && input.id.len() <= 200
            && input
                .id
                .bytes()
                .all(|byte| { byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') }),
        "apply operation ID is invalid"
    );
    anyhow::ensure!(
        !input.workspace_id.trim().is_empty() && !input.revision_id.trim().is_empty(),
        "apply workspace and revision IDs are required"
    );
    validate_sha256(&input.revision_content_hash, "revision content")?;
    if let Some(hash) = &input.expected_old_block_hash {
        validate_sha256(hash, "expected old block")?;
    }
    validate_sha256(&input.intended_new_block_hash, "intended new block")?;
    validate_sha256(
        &input.expected_original_content_hash,
        "expected original content",
    )?;
    validate_sha256(
        &input.intended_updated_content_hash,
        "intended updated content",
    )?;
    validate_relative_path(&input.destination_path)?;
    anyhow::ensure!(
        !input.temporary_name.is_empty()
            && input.temporary_name.len() <= 255
            && !input.temporary_name.contains(['/', '\\', '\0'])
            && input.temporary_name.starts_with('.')
            && input.temporary_name.ends_with(".tmp"),
        "apply temporary filename is invalid"
    );
    anyhow::ensure!(
        input.recovery_payload.is_some() ^ input.recovery_path.is_some(),
        "provide exactly one recovery payload or recovery path"
    );
    anyhow::ensure!(
        input
            .recovery_payload
            .as_ref()
            .is_none_or(|payload| payload.len() <= 16 * 1024 * 1024),
        "apply recovery payload is too large"
    );
    anyhow::ensure!(
        input.recovery_path.as_ref().is_none_or(|path| {
            let path = path.trim();
            !path.is_empty() && path.len() <= 4096 && !path.contains('\0')
        }),
        "apply recovery path is invalid"
    );
    if let Some(payload) = &input.recovery_payload {
        anyhow::ensure!(
            digest(payload) == input.expected_original_content_hash,
            "apply recovery payload does not match the expected original content hash"
        );
        if !input.expected_target_exists {
            anyhow::ensure!(
                payload.is_empty(),
                "a missing apply target must have an empty recovery payload"
            );
        }
    }
    let target_name = std::path::Path::new(&input.destination_path)
        .file_name()
        .and_then(|value| value.to_str())
        .context("apply destination filename is invalid")?;
    anyhow::ensure!(
        input.temporary_name == format!(".{target_name}.log-inbox-{}.tmp", input.id),
        "apply temporary filename does not match the operation identity"
    );
    Ok(())
}

fn validate_sha256(value: &str, label: &str) -> Result<()> {
    anyhow::ensure!(
        value.len() == 64
            && value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "{label} hash must be a lowercase SHA-256 digest"
    );
    Ok(())
}

fn operation_matches_input(operation: &ApplyOperation, input: &PrepareApplyOperation) -> bool {
    operation.id == input.id
        && operation.workspace_id == input.workspace_id
        && operation.local_date == input.local_date
        && operation.revision_id == input.revision_id
        && operation.revision_content_hash == input.revision_content_hash
        && operation.destination_path == input.destination_path
        && operation.expected_old_block_hash == input.expected_old_block_hash
        && operation.intended_new_block_hash == input.intended_new_block_hash
        && operation.expected_target_exists == Some(input.expected_target_exists)
        && operation.expected_original_content_hash.as_deref()
            == Some(input.expected_original_content_hash.as_str())
        && operation.intended_updated_content_hash.as_deref()
            == Some(input.intended_updated_content_hash.as_str())
        && operation.temporary_name.as_deref() == Some(input.temporary_name.as_str())
        && operation.recovery_payload == input.recovery_payload
        && operation.recovery_path == input.recovery_path
}

fn valid_apply_state(state: &str) -> bool {
    matches!(
        state,
        "prepared" | "writing" | "written" | "finalized" | "failed" | "reconciliation_required"
    )
}

fn allowed_apply_transition(current: &str, next: &str) -> bool {
    matches!(
        (current, next),
        ("prepared", "writing" | "failed" | "reconciliation_required")
            | ("writing", "written" | "failed" | "reconciliation_required")
            | ("written", "reconciliation_required")
            | ("failed", "prepared" | "reconciliation_required")
            | ("reconciliation_required", "prepared" | "written")
    )
}

fn apply_operation_from_row(row: &Row<'_>) -> rusqlite::Result<ApplyOperation> {
    let local_date: String = row.get(2)?;
    let created_at: String = row.get(12)?;
    let updated_at: String = row.get(13)?;
    Ok(ApplyOperation {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        local_date: parse_date(&local_date).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, error.into())
        })?,
        revision_id: row.get(3)?,
        revision_content_hash: row.get(4)?,
        destination_path: row.get(5)?,
        expected_old_block_hash: row.get(6)?,
        intended_new_block_hash: row.get(7)?,
        recovery_payload: row.get(8)?,
        recovery_path: row.get(9)?,
        state: row.get(10)?,
        failure_reason: row.get(11)?,
        expected_target_exists: row.get(14)?,
        expected_original_content_hash: row.get(15)?,
        intended_updated_content_hash: row.get(16)?,
        temporary_name: row.get(17)?,
        created_at: parse_time(&created_at).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(12, rusqlite::types::Type::Text, error.into())
        })?,
        updated_at: parse_time(&updated_at).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(13, rusqlite::types::Type::Text, error.into())
        })?,
    })
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

fn snapshot_evidence_from_row(row: &Row<'_>) -> rusqlite::Result<SnapshotEvidence> {
    let decided_at: Option<String> = row.get(7)?;
    Ok(SnapshotEvidence {
        event_id: row.get(0)?,
        position: row.get(1)?,
        event_digest: row.get(2)?,
        disposition: row.get(3)?,
        related_event_id: row.get(4)?,
        decision_actor: row.get(5)?,
        decision_reason: row.get(6)?,
        decided_at: decided_at
            .map(|value| {
                parse_time(&value).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        7,
                        rusqlite::types::Type::Text,
                        error.into(),
                    )
                })
            })
            .transpose()?,
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
            workstream.canonical_links.iter().all(|link| {
                link.strip_prefix("[[")
                    .and_then(|value| value.strip_suffix("]]"))
                    .is_some_and(|name| {
                        !name.trim().is_empty()
                            && name.len() <= 512
                            && !name
                                .chars()
                                .any(|character| matches!(character, '\r' | '\n' | '[' | ']'))
                    })
            }),
            "canonical links must be bounded wikilinks"
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
        assert_eq!(store.prune_old_events(30).expect("retention runs"), 1);
        assert!(
            store.get_events_by_ids(&event_ids[..1]).is_err(),
            "raw evidence expires independently of its immutable snapshot identity"
        );
        store
            .decide_snapshot_evidence(&snapshot.id, &event_ids[0], "include", None, "owner", None)
            .expect("evidence decision stores");
        assert!(
            store
                .decide_snapshot_evidence(
                    &snapshot.id,
                    &event_ids[1],
                    "duplicate_of",
                    Some(&event_ids[1]),
                    "owner",
                    None,
                )
                .unwrap_err()
                .to_string()
                .contains("cannot refer to itself")
        );
        store
            .decide_snapshot_evidence(
                &snapshot.id,
                &event_ids[1],
                "duplicate_of",
                Some(&event_ids[0]),
                "owner",
                Some("Same validation evidence"),
            )
            .expect("related decision stores");
        let decisions = store
            .snapshot_evidence(&snapshot.id)
            .expect("snapshot decisions read");
        assert_eq!(decisions[0].disposition.as_deref(), Some("include"));
        assert_eq!(decisions[1].disposition.as_deref(), Some("duplicate_of"));
        store
            .reopen_snapshot_evidence(&snapshot.id, &event_ids[1])
            .expect("evidence reopens");
        assert!(
            store.snapshot_evidence(&snapshot.id).unwrap()[1]
                .disposition
                .is_none()
        );

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
            .create_proposal_revision_if_current(
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
                &first_revision.id,
            )
            .expect("edited revision stores");
        assert!(
            store
                .create_proposal_revision_if_current(
                    &profile.id,
                    date,
                    Some(&snapshot.id),
                    "structured_edit",
                    &edited.content,
                    &first_revision.id,
                )
                .unwrap_err()
                .to_string()
                .contains("current proposal revision changed")
        );
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

    #[test]
    fn raw_retention_preserves_snapshot_identity_and_clears_its_live_reference() {
        let store = Store::open(std::env::temp_dir().join(format!(
            "log-inbox-snapshot-retention-{}.sqlite3",
            Uuid::new_v4()
        )))
        .expect("store opens");
        let profile = store
            .create_pending_workspace_profile(
                "retention-binding",
                "UTC",
                "Work Log",
                "{date}.md",
                None,
                "markdown",
            )
            .expect("profile stores");
        let profile = store
            .activate_workspace_profile(&profile.id)
            .expect("profile activates");
        let date = NaiveDate::from_ymd_opt(2026, 9, 9).unwrap();
        store
            .ensure_daily_day(date, "Work Log/2026-09-09.md", None)
            .expect("day freezes");
        let event = store
            .insert_event(LogEventInput {
                source: "codex/test".to_owned(),
                level: Some("info".to_owned()),
                timestamp: Some("2026-09-09T12:00:00Z".parse().unwrap()),
                message: "retained snapshot evidence".to_owned(),
                metadata: None,
                fingerprint: None,
            })
            .expect("event stores");
        let snapshot = store
            .create_evidence_snapshot(&profile.id, date, std::slice::from_ref(&event.id))
            .expect("snapshot stores");
        store
            .connect()
            .unwrap()
            .execute(
                "UPDATE log_events SET received_at = ?1 WHERE id = ?2",
                params![
                    (Utc::now() - chrono::Duration::days(31)).to_rfc3339(),
                    event.id
                ],
            )
            .expect("event ages");

        assert_eq!(store.prune_old_events(30).expect("retention runs"), 1);
        let evidence = store
            .snapshot_evidence(&snapshot.id)
            .expect("snapshot evidence remains");
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].event_id, event.id);
        let live_event_id: Option<String> = store
            .connect()
            .unwrap()
            .query_row(
                "SELECT live_event_id FROM evidence_snapshot_events WHERE snapshot_id = ?1 AND event_id = ?2",
                params![snapshot.id, event.id],
                |row| row.get(0),
            )
            .expect("live reference reads");
        assert_eq!(live_event_id, None);
    }

    fn apply_operation_fixture() -> (Store, PrepareApplyOperation) {
        let store = Store::open(std::env::temp_dir().join(format!(
            "log-inbox-apply-operation-{}.sqlite3",
            Uuid::new_v4()
        )))
        .expect("store opens");
        let profile = store
            .create_pending_workspace_profile(
                "apply-binding",
                "UTC",
                "Work Log",
                "{date}.md",
                None,
                "markdown",
            )
            .expect("profile stores");
        let profile = store
            .activate_workspace_profile(&profile.id)
            .expect("profile activates");
        let date = NaiveDate::from_ymd_opt(2026, 9, 9).unwrap();
        let day = store
            .ensure_daily_day(date, "Work Log/2026-09-09.md", None)
            .expect("day freezes");
        let revision = store
            .create_proposal_revision(
                &profile.id,
                date,
                None,
                "advanced_markdown",
                &serde_json::json!("Reviewed daily content"),
            )
            .expect("revision stores");
        let input = PrepareApplyOperation {
            id: "apply_2026-09-09_primary".to_owned(),
            workspace_id: profile.id,
            local_date: date,
            revision_id: revision.id,
            revision_content_hash: revision.content_hash,
            destination_path: day.destination_path,
            expected_old_block_hash: Some("a".repeat(64)),
            intended_new_block_hash: "b".repeat(64),
            expected_target_exists: true,
            expected_original_content_hash: digest(b"original daily note"),
            intended_updated_content_hash: "c".repeat(64),
            temporary_name: ".2026-09-09.md.log-inbox-apply_2026-09-09_primary.tmp".to_owned(),
            recovery_payload: Some(b"original daily note".to_vec()),
            recovery_path: None,
        };
        (store, input)
    }

    #[test]
    fn prepares_apply_operations_idempotently_with_immutable_exact_inputs() {
        let (store, input) = apply_operation_fixture();
        let prepared = store
            .prepare_apply_operation(&input)
            .expect("operation prepares");
        assert_eq!(prepared.state, "prepared");
        assert_eq!(prepared.revision_content_hash, input.revision_content_hash);
        assert_eq!(
            prepared.expected_old_block_hash,
            input.expected_old_block_hash
        );
        assert_eq!(prepared.recovery_payload, input.recovery_payload);
        assert_eq!(prepared.expected_target_exists, Some(true));
        assert_eq!(
            prepared.expected_original_content_hash.as_deref(),
            Some(input.expected_original_content_hash.as_str())
        );
        assert_eq!(
            prepared.intended_updated_content_hash.as_deref(),
            Some(input.intended_updated_content_hash.as_str())
        );
        assert_eq!(
            prepared.temporary_name.as_deref(),
            Some(input.temporary_name.as_str())
        );
        assert_eq!(
            store
                .prepare_apply_operation(&input)
                .expect("identical retry resolves"),
            prepared
        );
        assert_eq!(
            store
                .apply_operation(&input.id)
                .expect("operation reads")
                .expect("operation exists"),
            prepared
        );

        let mut conflicting = input.clone();
        conflicting.intended_new_block_hash = "c".repeat(64);
        assert!(
            store
                .prepare_apply_operation(&conflicting)
                .unwrap_err()
                .to_string()
                .contains("different immutable inputs")
        );
        let mut changed_original = input.clone();
        changed_original.recovery_payload = Some(b"different original".to_vec());
        changed_original.expected_original_content_hash = digest(b"different original");
        assert!(
            store
                .prepare_apply_operation(&changed_original)
                .unwrap_err()
                .to_string()
                .contains("different immutable inputs")
        );
        let mut wrong_revision_hash = input.clone();
        wrong_revision_hash.id = "apply_wrong_revision_hash".to_owned();
        wrong_revision_hash.temporary_name =
            ".2026-09-09.md.log-inbox-apply_wrong_revision_hash.tmp".to_owned();
        wrong_revision_hash.revision_content_hash = "c".repeat(64);
        assert!(
            store
                .prepare_apply_operation(&wrong_revision_hash)
                .unwrap_err()
                .to_string()
                .contains("content hash does not match")
        );
        let mut wrong_destination = input.clone();
        wrong_destination.id = "apply_wrong_destination".to_owned();
        wrong_destination.destination_path = "Work Log/other.md".to_owned();
        wrong_destination.temporary_name =
            ".other.md.log-inbox-apply_wrong_destination.tmp".to_owned();
        assert!(
            store
                .prepare_apply_operation(&wrong_destination)
                .unwrap_err()
                .to_string()
                .contains("frozen daily destination")
        );
        assert!(
            store
                .connect()
                .unwrap()
                .execute(
                    "UPDATE apply_operations SET destination_path = 'other.md' WHERE id = ?1",
                    params![input.id],
                )
                .is_err()
        );
        assert!(
            store
                .connect()
                .unwrap()
                .execute(
                    r#"INSERT INTO apply_operations
                        (id, workspace_id, local_date, revision_id, revision_content_hash,
                         destination_path, expected_old_block_hash, intended_new_block_hash,
                         recovery_payload, recovery_path, state, failure_reason, created_at, updated_at)
                       SELECT 'apply_missing_identity', workspace_id, local_date, revision_id,
                         revision_content_hash, destination_path, expected_old_block_hash,
                         intended_new_block_hash, recovery_payload, recovery_path, state,
                         failure_reason, created_at, updated_at
                       FROM apply_operations WHERE id = ?1"#,
                    params![input.id],
                )
                .is_err()
        );
    }

    #[test]
    fn guards_apply_operation_transitions_and_makes_retries_idempotent() {
        let (store, input) = apply_operation_fixture();
        store
            .prepare_apply_operation(&input)
            .expect("operation prepares");
        assert!(
            store
                .transition_apply_operation(&input.id, "prepared", "written", None)
                .unwrap_err()
                .to_string()
                .contains("not allowed")
        );
        let writing = store
            .transition_apply_operation(&input.id, "prepared", "writing", None)
            .expect("writing records");
        assert_eq!(writing.state, "writing");
        assert_eq!(
            store
                .transition_apply_operation(&input.id, "prepared", "writing", None)
                .expect("transition retry resolves"),
            writing
        );
        assert!(
            store
                .transition_apply_operation(&input.id, "prepared", "failed", Some("write failed"))
                .unwrap_err()
                .to_string()
                .contains("state changed")
        );
        let written = store
            .transition_apply_operation(&input.id, "writing", "written", None)
            .expect("written records");
        assert_eq!(written.state, "written");
        let finalized = store
            .finalize_apply_operation(&input.id, &input.revision_id, &input.revision_content_hash)
            .expect("finalized records");
        assert_eq!(finalized.state, "finalized");
        assert!(
            store
                .transition_apply_operation(&input.id, "finalized", "writing", None)
                .unwrap_err()
                .to_string()
                .contains("not allowed")
        );

        let mut recovery = input.clone();
        recovery.id = "apply_recovery_path".to_owned();
        recovery.temporary_name = ".2026-09-09.md.log-inbox-apply_recovery_path.tmp".to_owned();
        recovery.recovery_payload = None;
        recovery.recovery_path = Some("recovery/apply_recovery_path.md".to_owned());
        store
            .prepare_apply_operation(&recovery)
            .expect("path-backed recovery prepares");
        let failed = store
            .transition_apply_operation(
                &recovery.id,
                "prepared",
                "failed",
                Some("preflight failed"),
            )
            .expect("failure records");
        assert_eq!(failed.failure_reason.as_deref(), Some("preflight failed"));
        let retried = store
            .transition_apply_operation(&recovery.id, "failed", "prepared", None)
            .expect("failed operation can retry with the same identity");
        assert_eq!(retried.state, "prepared");
        assert_eq!(retried.failure_reason, None);
        let reconciliation = store
            .transition_apply_operation(
                &recovery.id,
                "prepared",
                "reconciliation_required",
                Some("destination changed after review"),
            )
            .expect("reconciliation records");
        assert_eq!(reconciliation.state, "reconciliation_required");
        let unfinished = store
            .list_unfinished_apply_operations(10)
            .expect("unfinished operations list");
        assert_eq!(unfinished.len(), 1);
        assert_eq!(unfinished[0].id, recovery.id);
    }

    #[test]
    fn finalization_is_atomic_for_the_exact_revision_and_included_evidence() {
        let store = Store::open(std::env::temp_dir().join(format!(
            "log-inbox-apply-finalization-{}.sqlite3",
            Uuid::new_v4()
        )))
        .expect("store opens");
        let profile = store
            .create_pending_workspace_profile(
                "binding",
                "UTC",
                "Work Log",
                "{date}.md",
                None,
                "markdown",
            )
            .and_then(|profile| store.activate_workspace_profile(&profile.id))
            .expect("profile activates");
        let date = NaiveDate::from_ymd_opt(2026, 9, 9).unwrap();
        let day = store
            .ensure_daily_day(date, "Work Log/2026-09-09.md", None)
            .expect("day freezes");
        let mut event_ids = Vec::new();
        for hour in [1, 2] {
            event_ids.push(
                store
                    .insert_event(LogEventInput {
                        source: "codex/test".to_owned(),
                        level: None,
                        timestamp: Some(day.start_utc + chrono::Duration::hours(hour)),
                        message: format!("evidence {hour}"),
                        metadata: None,
                        fingerprint: None,
                    })
                    .expect("event stores")
                    .id,
            );
        }
        let snapshot = store
            .create_evidence_snapshot(&profile.id, date, &event_ids)
            .expect("snapshot stores");
        store
            .decide_snapshot_evidence(&snapshot.id, &event_ids[0], "include", None, "owner", None)
            .expect("include records");
        store
            .decide_snapshot_evidence(
                &snapshot.id,
                &event_ids[1],
                "omit",
                None,
                "owner",
                Some("noise"),
            )
            .expect("omit records");
        let revision = store
            .create_proposal_revision(
                &profile.id,
                date,
                Some(&snapshot.id),
                "generated",
                &serde_json::json!({
                    "schema_version": 1,
                    "workstreams": [{
                        "id": "work",
                        "title": "Work",
                        "evidence_event_ids": event_ids,
                        "outcome": [{ "text": "Completed work.", "evidence_event_ids": event_ids }]
                    }]
                }),
            )
            .expect("revision stores");
        let input = PrepareApplyOperation {
            id: "apply_evidence".to_owned(),
            workspace_id: profile.id.clone(),
            local_date: date,
            revision_id: revision.id,
            revision_content_hash: revision.content_hash,
            destination_path: day.destination_path,
            expected_old_block_hash: None,
            intended_new_block_hash: "b".repeat(64),
            expected_target_exists: false,
            expected_original_content_hash: digest(b""),
            intended_updated_content_hash: "c".repeat(64),
            temporary_name: ".2026-09-09.md.log-inbox-apply_evidence.tmp".to_owned(),
            recovery_payload: Some(Vec::new()),
            recovery_path: None,
        };
        store
            .prepare_apply_operation(&input)
            .expect("apply prepares");
        store
            .transition_apply_operation(&input.id, "prepared", "writing", None)
            .and_then(|_| store.transition_apply_operation(&input.id, "writing", "written", None))
            .expect("write records");
        let finalized = store
            .finalize_apply_operation(&input.id, &input.revision_id, &input.revision_content_hash)
            .expect("finalization succeeds");
        assert_eq!(finalized.state, "finalized");
        assert_eq!(
            store
                .daily_day(&profile.id, date)
                .unwrap()
                .unwrap()
                .review_status,
            "applied"
        );
        let events = store.get_events_by_ids(&event_ids).expect("events read");
        assert!(events[0].reviewed);
        assert!(!events[1].reviewed);
        assert_eq!(
            store.snapshot_evidence(&snapshot.id).unwrap()[1]
                .disposition
                .as_deref(),
            Some("omit")
        );
        assert_eq!(
            store
                .finalize_apply_operation(
                    &input.id,
                    &input.revision_id,
                    &input.revision_content_hash
                )
                .expect("finalization retry resolves"),
            finalized
        );
    }

    #[test]
    fn stale_revisions_cannot_finalize_and_unfinished_operations_are_recoverable() {
        let (store, input) = apply_operation_fixture();
        store
            .prepare_apply_operation(&input)
            .expect("apply prepares");
        store
            .transition_apply_operation(&input.id, "prepared", "writing", None)
            .and_then(|_| store.transition_apply_operation(&input.id, "writing", "written", None))
            .expect("write records");
        store
            .create_proposal_revision(
                &input.workspace_id,
                input.local_date,
                None,
                "advanced_markdown",
                &serde_json::json!("Newer reviewed content"),
            )
            .expect("newer revision stores");
        assert!(
            store
                .finalize_apply_operation(
                    &input.id,
                    &input.revision_id,
                    &input.revision_content_hash
                )
                .unwrap_err()
                .to_string()
                .contains("revision changed")
        );
        assert_eq!(
            store.apply_operation(&input.id).unwrap().unwrap().state,
            "written"
        );
        assert_ne!(
            store
                .daily_day(&input.workspace_id, input.local_date)
                .unwrap()
                .unwrap()
                .review_status,
            "applied"
        );
        let unfinished = store.list_unfinished_apply_operations(10).unwrap();
        assert_eq!(unfinished.len(), 1);
        assert_eq!(unfinished[0].id, input.id);
        assert!(store.list_unfinished_apply_operations(0).unwrap().len() <= 1);
    }
}
