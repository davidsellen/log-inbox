//! Workspace-scoped provider configuration. Credential values are encrypted by the service.
use crate::store::Store;
use anyhow::Result;
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};

impl Store {
    pub fn generation_settings(&self, workspace_id: &str) -> Result<Value> {
        let text: Option<String> = self
            .connect()?
            .query_row(
                "SELECT settings_json FROM generation_settings WHERE workspace_id=?1",
                [workspace_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(match text {
            Some(text) => serde_json::from_str(&text)?,
            None => json!({"connections":[],"default":null,"environment_consent":null}),
        })
    }
    pub fn save_generation_settings(&self, workspace_id: &str, settings: &Value) -> Result<()> {
        self.connect()?.execute("INSERT INTO generation_settings(workspace_id,settings_json) VALUES(?1,?2) ON CONFLICT(workspace_id) DO UPDATE SET settings_json=excluded.settings_json",params![workspace_id,serde_json::to_string(settings)?])?;
        Ok(())
    }
    pub fn set_generation_attempt_provenance(&self, id: &str, provenance: &Value) -> Result<()> {
        self.set_generation_provenance("attempt", id, provenance)
    }
    pub fn set_generation_revision_provenance(&self, id: &str, provenance: &Value) -> Result<()> {
        self.set_generation_provenance("revision", id, provenance)
    }
    pub fn generation_attempt_provenance(&self, id: &str) -> Result<Option<Value>> {
        self.generation_provenance("attempt", id)
    }
    pub fn generation_comparison_provenance(&self, id: &str) -> Result<Option<Value>> {
        self.generation_provenance("comparison", id)
    }
    pub fn generation_revision_provenance(&self, id: &str) -> Result<Option<Value>> {
        self.generation_provenance("revision", id)
    }
    fn set_generation_provenance(&self, kind: &str, id: &str, value: &Value) -> Result<()> {
        // Once captured, an attempt's identity cannot change underneath running work.
        self.connect()?.execute("INSERT INTO generation_provenance(kind,record_id,provenance_json) VALUES(?1,?2,?3) ON CONFLICT(kind,record_id) DO NOTHING", params![kind,id,serde_json::to_string(value)?])?;
        Ok(())
    }
    fn generation_provenance(&self, kind: &str, id: &str) -> Result<Option<Value>> {
        let text: Option<String> = self
            .connect()?
            .query_row(
                "SELECT provenance_json FROM generation_provenance WHERE kind=?1 AND record_id=?2",
                params![kind, id],
                |row| row.get(0),
            )
            .optional()?;
        text.map(|text| serde_json::from_str(&text).map_err(Into::into))
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn populated_v26_database_upgrades_without_losing_content_and_reopens_idempotently() {
        use crate::models::LogEventInput;
        let directory =
            std::env::temp_dir().join(format!("generation-v26-upgrade-{}", uuid::Uuid::new_v4()));
        let path = directory.join("store.sqlite");
        let store = Store::open(path.clone()).unwrap();
        let profile = store
            .save_active_workspace_profile(
                "legacy-binding",
                "Europe/Stockholm",
                "Daily",
                "{date}.md",
                None,
                "markdown",
                None,
            )
            .unwrap();
        let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 14).unwrap();
        store
            .ensure_daily_day(date, "Daily/2026-10-14.md", None)
            .unwrap();
        let event = store
            .insert_event(LogEventInput {
                source: "codex/legacy".to_owned(),
                level: None,
                timestamp: None,
                message: "Preserved legacy event".to_owned(),
                metadata: None,
                fingerprint: None,
            })
            .unwrap();
        let manual = store
            .create_manual_daily_entry(&profile.id, date, "Preserved manual note", &[])
            .unwrap();
        let content = json!({"schema_version":1,"workstreams":[],"manual_entry_ids":[manual.id],"open_questions":[]});
        let revision = store
            .create_proposal_revision(&profile.id, date, None, "manual", &content)
            .unwrap();
        let preferences = std::collections::BTreeMap::from([(
            "legacy-preference".to_owned(),
            "preserved-value".to_owned(),
        )]);
        store.set_preferences(&preferences).unwrap();
        // Remove the additive v27-v29 objects to reproduce the populated v26 schema.
        store
            .connect()
            .unwrap()
            .execute_batch(
                "DROP TRIGGER knowledge_promotion_immutable;
            DROP TABLE knowledge_promotion_operations;
            DROP TRIGGER knowledge_candidates_immutable_update;
            DROP TRIGGER knowledge_candidates_immutable_delete;
            DROP TRIGGER knowledge_dispositions_immutable_update;
            DROP TRIGGER knowledge_dispositions_immutable_delete;
            DROP TABLE knowledge_dispositions;
            DROP TABLE knowledge_candidates;
            DROP TRIGGER generation_revision_provenance_delete;
            DROP TRIGGER generation_attempt_provenance_delete;
            DROP TRIGGER generation_comparison_provenance_delete;
            DROP TABLE generation_provenance;
            DROP TABLE generation_settings;
            DELETE FROM schema_migrations WHERE version>=27;",
            )
            .unwrap();
        assert_eq!(store.schema_version().unwrap(), 26);
        drop(store);
        for _ in 0..2 {
            let upgraded = Store::open(path.clone()).unwrap();
            assert_eq!(upgraded.schema_version().unwrap(), 29);
            assert_eq!(
                serde_json::to_value(upgraded.active_workspace_profile().unwrap().unwrap())
                    .unwrap(),
                serde_json::to_value(&profile).unwrap()
            );
            assert_eq!(
                upgraded
                    .daily_day(&profile.id, date)
                    .unwrap()
                    .unwrap()
                    .current_revision_id,
                Some(revision.id.clone())
            );
            assert_eq!(
                upgraded.manual_daily_entries(&profile.id, date).unwrap(),
                vec![manual.clone()]
            );
            assert_eq!(
                serde_json::to_value(
                    upgraded
                        .get_events_by_ids(std::slice::from_ref(&event.id))
                        .unwrap()
                )
                .unwrap(),
                serde_json::to_value(vec![event.clone()]).unwrap()
            );
            let (saved_content, saved_hash): (String, String) = upgraded
                .connect()
                .unwrap()
                .query_row(
                    "SELECT content_json,content_hash FROM proposal_revisions WHERE id=?1",
                    [&revision.id],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .unwrap();
            assert_eq!(
                serde_json::from_str::<Value>(&saved_content).unwrap(),
                revision.content
            );
            assert_eq!(saved_hash, revision.content_hash);
            assert_eq!(upgraded.get_preferences().unwrap(), preferences);
            assert_eq!(
                upgraded
                    .generation_revision_provenance(&revision.id)
                    .unwrap(),
                None
            );
            assert!(
                upgraded.generation_settings(&profile.id).unwrap()["connections"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
            let migration_count: i64 = upgraded
                .connect()
                .unwrap()
                .query_row(
                    "SELECT COUNT(*) FROM schema_migrations WHERE version=27",
                    [],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(migration_count, 1);
        }
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn revision_provenance_is_atomic_and_survives_edits() {
        let directory =
            std::env::temp_dir().join(format!("revision-provenance-{}", uuid::Uuid::new_v4()));
        let store = Store::open(directory.join("store.sqlite")).unwrap();
        let workspace = store
            .create_pending_workspace_profile(
                "binding",
                "UTC",
                "Daily",
                "{date}.md",
                None,
                "markdown",
            )
            .unwrap();
        store.activate_workspace_profile(&workspace.id).unwrap();
        let date = chrono::NaiveDate::from_ymd_opt(2026, 10, 15).unwrap();
        store
            .ensure_daily_day(date, "Daily/2026-10-15.md", None)
            .unwrap();
        let note = store
            .create_manual_daily_entry(&workspace.id, date, "Manual note", &[])
            .unwrap();
        let content = json!({"schema_version":1,"workstreams":[],"manual_entry_ids":[note.id],"open_questions":[]});
        let provenance = json!({"provider":"local","model":"first"});
        let first = store
            .create_generated_revision_if_current_with_provenance(
                &workspace.id,
                date,
                None,
                None,
                "manual",
                &content,
                None,
                &provenance,
            )
            .unwrap();
        assert_eq!(
            store.generation_revision_provenance(&first.id).unwrap(),
            Some(provenance.clone())
        );
        let edited = store
            .create_proposal_revision_if_current(
                &workspace.id,
                date,
                None,
                "structured_edit",
                &content,
                &first.id,
            )
            .unwrap();
        assert_eq!(
            store.generation_revision_provenance(&edited.id).unwrap(),
            Some(provenance)
        );
        assert!(
            store
                .create_generated_revision_if_current_with_provenance(
                    &workspace.id,
                    date,
                    None,
                    None,
                    "manual",
                    &content,
                    Some(&first.id),
                    &json!({"model":"wrong"})
                )
                .is_err()
        );
        assert_eq!(
            store
                .daily_day(&workspace.id, date)
                .unwrap()
                .unwrap()
                .current_revision_id,
            Some(edited.id)
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
    #[test]
    fn settings_and_frozen_provenance_survive_restart() {
        let directory =
            std::env::temp_dir().join(format!("generation-settings-{}", uuid::Uuid::new_v4()));
        let path = directory.join("store.sqlite");
        let store = Store::open(path.clone()).unwrap();
        let workspace = store
            .create_pending_workspace_profile(
                "binding",
                "Europe/Stockholm",
                "Daily",
                "{date}.md",
                None,
                "wikilink",
            )
            .unwrap();
        let settings = json!({"connections":[{"id":"local","credential":{"ciphertext":"opaque"}}],"default":{"connection_id":"local","model":"first"}});
        store
            .save_generation_settings(&workspace.id, &settings)
            .unwrap();
        let provenance = json!({"provider":"local","model":"first"});
        store
            .set_generation_attempt_provenance("attempt-one", &provenance)
            .unwrap();
        store
            .set_generation_attempt_provenance("attempt-one", &json!({"model":"changed"}))
            .unwrap();
        let reopened = Store::open(path).unwrap();
        assert_eq!(
            reopened.generation_settings(&workspace.id).unwrap(),
            settings
        );
        assert_eq!(
            reopened
                .generation_attempt_provenance("attempt-one")
                .unwrap(),
            Some(provenance)
        );
        assert_eq!(
            reopened
                .generation_revision_provenance("legacy-revision")
                .unwrap(),
            None
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
