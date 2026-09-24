use crate::{redaction::redact_text, store::Store};
use anyhow::Result;
use chrono::{DateTime, NaiveDate, Utc};
use rusqlite::{OptionalExtension, Row, params};
use serde_json::{Value, json};
use uuid::Uuid;

const ATTEMPT_COLUMNS: &str = "id,workspace_id,local_date,operation_type,state,stage,started_at,finished_at,timeout_seconds,error,reference_mode,completed_groups,total_groups,failure_code";

impl Store {
    pub fn create_generation_attempt(
        &self,
        workspace_id: &str,
        local_date: NaiveDate,
        operation_type: &str,
        timeout_seconds: u64,
        now: DateTime<Utc>,
    ) -> Result<Value> {
        self.create_generation_attempt_with_references(
            workspace_id,
            local_date,
            operation_type,
            timeout_seconds,
            "configured",
            now,
        )
    }

    pub fn create_generation_attempt_with_references(
        &self,
        workspace_id: &str,
        local_date: NaiveDate,
        operation_type: &str,
        timeout_seconds: u64,
        reference_mode: &str,
        now: DateTime<Utc>,
    ) -> Result<Value> {
        anyhow::ensure!(
            matches!(reference_mode, "configured" | "none"),
            "invalid reference mode"
        );
        anyhow::ensure!(
            timeout_seconds > 0 && timeout_seconds <= i64::MAX as u64,
            "invalid timeout"
        );
        anyhow::ensure!(
            !operation_type.is_empty() && operation_type.len() <= 64,
            "invalid operation type"
        );
        let id = format!("generation_{}", Uuid::new_v4().simple());
        self.connect()?.execute(
            "INSERT INTO generation_attempts (id,workspace_id,local_date,operation_type,state,stage,started_at,timeout_seconds,reference_mode) VALUES (?1,?2,?3,?4,'running','preparing',?5,?6,?7)",
            params![id,workspace_id,local_date.to_string(),operation_type,now.to_rfc3339(),timeout_seconds as i64,reference_mode],
        )?;
        Ok(self.connect()?.query_row(
            &format!("SELECT {ATTEMPT_COLUMNS} FROM generation_attempts WHERE id=?1"),
            params![id],
            attempt_from_row,
        )?)
    }

    pub fn update_generation_attempt_progress(
        &self,
        id: &str,
        completed: usize,
        total: usize,
        fallback: bool,
    ) -> Result<bool> {
        anyhow::ensure!(
            completed <= total && total <= 500,
            "invalid generation progress"
        );
        Ok(self.connect()?.execute("UPDATE generation_attempts SET completed_groups=?2,total_groups=?3,stage=?4 WHERE id=?1 AND state='running' AND completed_groups<=?2", params![id, completed as i64,total as i64,if fallback {"requesting_model_fallback"} else {"requesting_model"}])? == 1)
    }

    pub fn update_generation_attempt_stage(&self, id: &str, stage: &str) -> Result<bool> {
        anyhow::ensure!(
            !stage.is_empty() && stage.len() <= 64,
            "invalid generation stage"
        );
        Ok(self.connect()?.execute(
            "UPDATE generation_attempts SET stage=?2 WHERE id=?1 AND state='running'",
            params![id, stage],
        )? == 1)
    }

    /// First terminal transition wins: a late completion cannot overwrite cancellation.
    pub fn finish_generation_attempt(
        &self,
        id: &str,
        state: &str,
        error: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<bool> {
        self.finish_generation_attempt_with_failure(id, state, error, None, now)
    }

    pub fn finish_generation_attempt_with_failure(
        &self,
        id: &str,
        state: &str,
        error: Option<&str>,
        failure_code: Option<&str>,
        now: DateTime<Utc>,
    ) -> Result<bool> {
        anyhow::ensure!(
            matches!(
                state,
                "succeeded" | "failed" | "canceled" | "interrupted" | "timed_out"
            ),
            "invalid terminal generation state"
        );
        let error =
            error.map(|message| redact_text(message).chars().take(2048).collect::<String>());
        Ok(self.connect()?.execute("UPDATE generation_attempts SET state=?2,stage='complete',finished_at=?3,error=?4,failure_code=?5 WHERE id=?1 AND state='running'",params![id,state,now.to_rfc3339(),error,failure_code])? == 1)
    }

    pub fn latest_generation_attempt(
        &self,
        workspace_id: &str,
        local_date: NaiveDate,
    ) -> Result<Option<Value>> {
        Ok(self.connect()?.query_row(&format!("SELECT {ATTEMPT_COLUMNS} FROM generation_attempts WHERE workspace_id=?1 AND local_date=?2 ORDER BY started_at DESC,rowid DESC LIMIT 1"),params![workspace_id,local_date.to_string()],attempt_from_row).optional()?)
    }

    pub fn active_generation_attempt(&self) -> Result<Option<Value>> {
        Ok(self.connect()?.query_row(&format!("SELECT {ATTEMPT_COLUMNS} FROM generation_attempts WHERE state='running' LIMIT 1"),[],attempt_from_row).optional()?)
    }

    /// Invoke only at service startup, before accepting work. Also repairs legacy
    /// manual generation states which have no durable attempt record.
    pub fn interrupt_generation_attempts(&self, now: DateTime<Utc>) -> Result<u64> {
        let mut conn = self.connect()?;
        let tx = conn.transaction()?;
        let changed = tx.execute("UPDATE generation_attempts SET state='interrupted',stage='complete',finished_at=?1,error='Service restarted before generation completed',failure_code='interrupted' WHERE state='running'",params![now.to_rfc3339()])?;
        tx.execute("UPDATE daily_days SET generation_status='failed',updated_at=?1 WHERE generation_status IN ('queued','running')",params![now.to_rfc3339()])?;
        tx.commit()?;
        Ok(changed as u64)
    }

    pub fn cancel_daily_schedule_run(
        &self,
        workspace_id: &str,
        local_date: NaiveDate,
        now: DateTime<Utc>,
    ) -> Result<bool> {
        Ok(self.connect()?.execute("UPDATE daily_schedule_runs SET state='dismissed',claim_token=NULL,lease_expires_at=NULL,last_error='Canceled by owner',updated_at=?3 WHERE workspace_id=?1 AND local_date=?2 AND state IN ('pending','claimed','failed')",params![workspace_id,local_date.to_string(),now.to_rfc3339()])? == 1)
    }

    /// Collector receipt-time facts: historical event timestamps do not hide
    /// freshly received activity. Sources and latest receipt cover retained logs.
    pub fn intake_summary(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Value> {
        anyhow::ensure!(start < end, "invalid intake interval");
        let conn = self.connect()?;
        let count: i64 = conn.query_row(
            "SELECT COUNT(*) FROM log_events WHERE received_at >= ?1 AND received_at < ?2",
            params![start.to_rfc3339(), end.to_rfc3339()],
            |row| row.get(0),
        )?;
        let latest: Option<String> =
            conn.query_row("SELECT MAX(received_at) FROM log_events", [], |row| {
                row.get(0)
            })?;
        let mut statement = conn.prepare("SELECT source,MAX(received_at) AS latest FROM log_events GROUP BY source ORDER BY latest DESC,source LIMIT 10")?;
        let sources = statement.query_map([],|row|Ok(json!({"source":row.get::<_,String>(0)?,"latest_received_at":row.get::<_,String>(1)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(json!({"today_count":count,"latest_received_at":latest,"sources":sources}))
    }
}

fn attempt_from_row(row: &Row<'_>) -> rusqlite::Result<Value> {
    let state: String = row.get(4)?;
    let error: Option<String> = row.get(9)?;
    let failure_code: Option<String> = row.get::<_, Option<String>>(13)?.or_else(|| {
        // Preserve useful recovery for attempts recorded before typed failures.
        match error.as_deref() {
            Some("resolved Knowledge group source is not a used note") => {
                Some("reference_context_invalid".to_owned())
            }
            Some("database is locked" | "database is busy") => Some("database_busy".to_owned()),
            _ => None,
        }
    });
    let recovery_actions = match failure_code.as_deref() {
        Some("reference_context_invalid") => vec!["retry_without_references"],
        _ if matches!(
            state.as_str(),
            "failed" | "canceled" | "interrupted" | "timed_out"
        ) =>
        {
            vec!["retry"]
        }
        _ => vec![],
    };
    Ok(json!({
        "id":row.get::<_,String>(0)?,"workspace_id":row.get::<_,String>(1)?,
        "local_date":row.get::<_,String>(2)?,"operation_type":row.get::<_,String>(3)?,
        "state":state,"stage":row.get::<_,String>(5)?,
        "started_at":row.get::<_,String>(6)?,"finished_at":row.get::<_,Option<String>>(7)?,
        "timeout_seconds":row.get::<_,i64>(8)?,"error":error,
        "reference_mode":row.get::<_,String>(10)?,"completed_groups":row.get::<_,i64>(11)?,"total_groups":row.get::<_,i64>(12)?,
        "failure_code":failure_code,"recovery_actions":recovery_actions
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_failures_keep_actionable_recovery_after_upgrade() {
        let (store, workspace, date) = fixture();
        for (error, expected) in [
            (
                "resolved Knowledge group source is not a used note",
                "retry_without_references",
            ),
            ("database is locked", "retry"),
        ] {
            let attempt = store
                .create_generation_attempt(&workspace, date, "manual", 1200, Utc::now())
                .unwrap();
            store
                .finish_generation_attempt(
                    attempt["id"].as_str().unwrap(),
                    "failed",
                    Some(error),
                    Utc::now(),
                )
                .unwrap();
            let saved = store
                .latest_generation_attempt(&workspace, date)
                .unwrap()
                .unwrap();
            assert_eq!(saved["recovery_actions"], json!([expected]));
            assert!(saved["failure_code"].is_string());
        }
    }

    fn fixture() -> (Store, String, NaiveDate) {
        let store = Store::open(
            std::env::temp_dir().join(format!("generation-test-{}.sqlite", Uuid::new_v4())),
        )
        .unwrap();
        let profile = store
            .create_pending_workspace_profile(
                "test-binding",
                "Europe/Stockholm",
                "Work Log",
                "{date}.md",
                None,
                "markdown",
            )
            .unwrap();
        store.activate_workspace_profile(&profile.id).unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 9, 23).unwrap();
        store
            .ensure_daily_day(date, "Work Log/2026-09-23.md", None)
            .unwrap();
        (store, profile.id, date)
    }

    #[test]
    fn progress_mode_and_recovery_survive_reload_without_regressing_counts() {
        let (store, workspace, date) = fixture();
        let attempt = store
            .create_generation_attempt_with_references(
                &workspace,
                date,
                "manual",
                120,
                "none",
                Utc::now(),
            )
            .unwrap();
        let id = attempt["id"].as_str().unwrap();
        assert!(
            store
                .update_generation_attempt_progress(id, 0, 4, false)
                .unwrap()
        );
        assert!(
            store
                .update_generation_attempt_progress(id, 2, 4, true)
                .unwrap()
        );
        assert!(
            !store
                .update_generation_attempt_progress(id, 1, 4, false)
                .unwrap()
        );
        store
            .finish_generation_attempt_with_failure(
                id,
                "failed",
                Some("Invalid references"),
                Some("reference_context_invalid"),
                Utc::now(),
            )
            .unwrap();
        let reopened = Store::open(store.database_path().to_path_buf()).unwrap();
        let saved = reopened
            .latest_generation_attempt(&workspace, date)
            .unwrap()
            .unwrap();
        assert_eq!(saved["reference_mode"], "none");
        assert_eq!(saved["completed_groups"], 2);
        assert_eq!(saved["total_groups"], 4);
        assert_eq!(saved["failure_code"], "reference_context_invalid");
        assert_eq!(
            saved["recovery_actions"],
            json!(["retry_without_references"])
        );
        assert!(
            !reopened
                .update_generation_attempt_progress(id, 4, 4, false)
                .unwrap()
        );
    }

    #[test]
    fn generation_terminal_race_preserves_cancel_and_allows_next_attempt() {
        let (store, workspace, date) = fixture();
        let now = Utc::now();
        let attempt = store
            .create_generation_attempt(&workspace, date, "manual", 120, now)
            .unwrap();
        let id = attempt["id"].as_str().unwrap();
        assert!(
            store
                .create_generation_attempt(&workspace, date, "scheduled", 120, now)
                .is_err()
        );
        assert!(
            store
                .finish_generation_attempt(id, "canceled", None, now)
                .unwrap()
        );
        assert!(
            !store
                .finish_generation_attempt(id, "succeeded", None, now)
                .unwrap()
        );
        assert!(!store.update_generation_attempt_stage(id, "saving").unwrap());
        assert_eq!(
            store
                .latest_generation_attempt(&workspace, date)
                .unwrap()
                .unwrap()["state"],
            "canceled"
        );
        assert!(store.active_generation_attempt().unwrap().is_none());
        store
            .create_generation_attempt(&workspace, date, "manual", 120, now)
            .unwrap();
    }

    #[test]
    fn restart_interrupts_attempts_and_legacy_manual_days() {
        let (store, workspace, date) = fixture();
        let now = Utc::now();
        store
            .create_generation_attempt(&workspace, date, "manual", 120, now)
            .unwrap();
        store
            .set_daily_generation_status(&workspace, date, "running")
            .unwrap();
        let orphan = date.succ_opt().unwrap();
        store
            .ensure_daily_day(orphan, "Work Log/2026-09-24.md", None)
            .unwrap();
        store
            .set_daily_generation_status(&workspace, orphan, "queued")
            .unwrap();
        let reopened = Store::open(store.database_path().to_path_buf()).unwrap();
        assert_eq!(reopened.interrupt_generation_attempts(now).unwrap(), 1);
        assert_eq!(reopened.interrupt_generation_attempts(now).unwrap(), 0);
        assert_eq!(
            reopened
                .latest_generation_attempt(&workspace, date)
                .unwrap()
                .unwrap()["state"],
            "interrupted"
        );
        assert_eq!(
            reopened
                .daily_day(&workspace, date)
                .unwrap()
                .unwrap()
                .generation_status,
            "failed"
        );
        assert_eq!(
            reopened
                .daily_day(&workspace, orphan)
                .unwrap()
                .unwrap()
                .generation_status,
            "failed"
        );
    }

    #[test]
    fn generated_candidate_cannot_replace_a_newer_edit_or_an_initial_candidate() {
        let (store, workspace, date) = fixture();
        let note = store
            .create_manual_daily_entry(&workspace, date, "A useful outcome", &[])
            .unwrap();
        let content = json!({"schema_version":1,"workstreams":[],"manual_entry_ids":[note.id],"open_questions":[]});
        let initial = store
            .create_generated_revision_if_current(
                &workspace, date, None, None, "manual", &content, None,
            )
            .unwrap();
        assert!(
            store
                .create_generated_revision_if_current(
                    &workspace, date, None, None, "manual", &content, None
                )
                .unwrap_err()
                .to_string()
                .contains("current proposal revision changed")
        );
        let mut edited_content = content.clone();
        edited_content["open_questions"] = json!(["Preserve the owner's correction"]);
        let edited = store
            .create_proposal_revision_if_current(
                &workspace,
                date,
                None,
                "structured_edit",
                &edited_content,
                &initial.id,
            )
            .unwrap();
        assert!(
            store
                .create_generated_revision_if_current(
                    &workspace,
                    date,
                    None,
                    None,
                    "manual",
                    &content,
                    Some(&initial.id)
                )
                .unwrap_err()
                .to_string()
                .contains("current proposal revision changed")
        );
        let current = store
            .current_proposal_revision(&workspace, date)
            .unwrap()
            .unwrap();
        assert_eq!(current.id, edited.id);
        assert_eq!(current.content, edited_content);
    }

    #[test]
    fn canceled_schedule_is_not_reclaimed_after_restart_or_retry() {
        let (store, workspace, date) = fixture();
        let now = Utc::now();
        store
            .enqueue_daily_schedule_run(&workspace, date, now, "Europe/Stockholm", "test")
            .unwrap();
        assert!(
            store
                .claim_daily_schedule_run(&workspace, date, now)
                .unwrap()
                .is_some()
        );
        assert!(
            store
                .cancel_daily_schedule_run(&workspace, date, now)
                .unwrap()
        );
        assert_eq!(
            store.recover_interrupted_daily_schedule_runs(now).unwrap(),
            0
        );
        assert_eq!(
            store
                .retry_all_failed_daily_schedule_runs(&workspace, now)
                .unwrap(),
            0
        );
        assert!(
            store
                .claim_daily_schedule_run(&workspace, date, now)
                .unwrap()
                .is_none()
        );
        assert_eq!(
            store.daily_schedule_runs(&workspace, 10).unwrap()[0].state,
            "dismissed"
        );
    }

    #[test]
    fn intake_uses_receipt_time_and_half_open_local_day_bounds() {
        let (store, _, date) = fixture();
        let resolved = crate::daily::resolve_day(date, "Europe/Stockholm").unwrap();
        let conn = store.connect().unwrap();
        for (id, received) in [
            ("before", resolved.start_utc - chrono::Duration::seconds(1)),
            ("start", resolved.start_utc),
            ("end", resolved.end_utc),
        ] {
            conn.execute("INSERT INTO log_events (id,received_at,timestamp,source,level,message,metadata_json,truncated) VALUES (?1,?2,'2020-01-01T00:00:00+00:00','codex/test','info','test','{}',0)",params![id,received.to_rfc3339()]).unwrap();
        }
        let summary = store
            .intake_summary(resolved.start_utc, resolved.end_utc)
            .unwrap();
        assert_eq!(summary["today_count"], 1);
        assert_eq!(summary["latest_received_at"], resolved.end_utc.to_rfc3339());
        assert_eq!(summary["sources"].as_array().unwrap().len(), 1);
    }
}
