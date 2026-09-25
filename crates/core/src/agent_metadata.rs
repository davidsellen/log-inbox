use crate::{models::StoredLogEvent, store::Store};
use anyhow::{Context, Result, ensure};
use chrono::Utc;
use rusqlite::{OptionalExtension, params};
use serde_json::Value;
use std::collections::BTreeSet;

pub const METADATA_FIELDS: &[&str] = &[
    "task_id",
    "session_id",
    "sequence",
    "event_type",
    "status",
    "agent",
    "host",
    "repo",
    "project",
    "branch",
    "base_branch",
    "target_branch",
    "product",
    "modules",
    "changed_paths",
    "commit",
    "commit_message",
    "tests",
    "validation",
    "work_item",
    "pull_request",
    "canonical_note_candidates",
];
pub const DEFAULT_METADATA_FIELDS: &[&str] = &[
    "task_id",
    "session_id",
    "sequence",
    "event_type",
    "status",
    "agent",
    "host",
];

fn valid_field(field: &str) -> bool {
    METADATA_FIELDS.contains(&field)
}

fn scope_key(event: &StoredLogEvent) -> String {
    for field in ["task_id", "session_id"] {
        if let Some(value) = event.metadata.get(field).and_then(Value::as_str)
            && !value.trim().is_empty()
        {
            return format!("{field}:{value}");
        }
    }
    format!("event:{}", event.id)
}

impl Store {
    pub fn agent_metadata_fields(&self, workspace_id: &str) -> Result<Vec<String>> {
        let saved = self
            .connect()?
            .query_row(
                "SELECT fields_json FROM agent_metadata_preferences WHERE workspace_id = ?1",
                [workspace_id],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        match saved {
            Some(json) => {
                let fields: Vec<String> = serde_json::from_str(&json)?;
                Ok(fields
                    .into_iter()
                    .filter(|field| valid_field(field))
                    .collect())
            }
            None => Ok(DEFAULT_METADATA_FIELDS
                .iter()
                .map(|field| (*field).to_owned())
                .collect()),
        }
    }

    pub fn save_agent_metadata_fields(
        &self,
        workspace_id: &str,
        fields: &[String],
    ) -> Result<Vec<String>> {
        ensure!(
            fields.len() <= METADATA_FIELDS.len(),
            "too many metadata fields"
        );
        let unique = fields.iter().collect::<BTreeSet<_>>();
        ensure!(
            unique.len() == fields.len(),
            "metadata fields must be unique"
        );
        ensure!(
            fields.iter().all(|field| valid_field(field)),
            "unsupported metadata field"
        );
        let serialized = serde_json::to_string(fields)?;
        self.connect()?.execute(
            "INSERT INTO agent_metadata_preferences(workspace_id, fields_json, updated_at) VALUES (?1, ?2, ?3) ON CONFLICT(workspace_id) DO UPDATE SET fields_json = excluded.fields_json, updated_at = excluded.updated_at",
            params![workspace_id, serialized, Utc::now().to_rfc3339()],
        )?;
        Ok(fields.to_vec())
    }

    pub fn save_event_metadata_override(
        &self,
        workspace_id: &str,
        event_id: &str,
        field: &str,
        value: Value,
    ) -> Result<String> {
        ensure!(valid_field(field), "unsupported metadata field");
        let valid_value = value
            .as_str()
            .is_some_and(|text| !text.trim().is_empty() && text.len() <= 2000)
            || value.as_array().is_some_and(|items| {
                !items.is_empty()
                    && items.iter().all(|item| {
                        item.as_str()
                            .is_some_and(|text| !text.trim().is_empty() && text.len() <= 2000)
                    })
            });
        ensure!(
            valid_value,
            "metadata correction must contain non-empty text up to 2,000 bytes"
        );
        let event = self
            .get_events_by_ids(&[event_id.to_owned()])?
            .into_iter()
            .next()
            .context("event not found")?;
        let scope = scope_key(&event);
        let value_json = value.to_string();
        self.connect()?.execute(
            "INSERT INTO event_metadata_overrides(workspace_id, scope_key, field, value_json, updated_at, anchor_event_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6) ON CONFLICT(workspace_id, scope_key, field) DO UPDATE SET value_json = excluded.value_json, updated_at = excluded.updated_at, anchor_event_id = excluded.anchor_event_id",
            params![workspace_id, scope, field, value_json, Utc::now().to_rfc3339(), event_id],
        )?;
        Ok(scope)
    }

    pub fn remove_event_metadata_override(
        &self,
        workspace_id: &str,
        event_id: &str,
        field: &str,
    ) -> Result<bool> {
        ensure!(valid_field(field), "unsupported metadata field");
        let event = self
            .get_events_by_ids(&[event_id.to_owned()])?
            .into_iter()
            .next()
            .context("event not found")?;
        let changed = self.connect()?.execute(
            "DELETE FROM event_metadata_overrides WHERE workspace_id = ?1 AND scope_key = ?2 AND field = ?3",
            params![workspace_id, scope_key(&event), field],
        )?;
        Ok(changed > 0)
    }

    pub fn apply_event_metadata_overrides(
        &self,
        workspace_id: &str,
        events: &mut [StoredLogEvent],
    ) -> Result<()> {
        let conn = self.connect()?;
        let mut statement = conn.prepare(
            "SELECT scope_key, field, value_json FROM event_metadata_overrides WHERE workspace_id = ?1",
        )?;
        let overrides = statement
            .query_map([workspace_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        for event in events {
            let scope = scope_key(event);
            for (override_scope, field, value) in &overrides {
                if *override_scope == scope {
                    event
                        .metadata
                        .insert(field.clone(), serde_json::from_str(value)?);
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use crate::{models::LogEventInput, store::Store};
    use chrono::Utc;
    use serde_json::{Map, json};
    use std::path::PathBuf;
    use uuid::Uuid;

    fn store() -> Store {
        let path: PathBuf =
            std::env::temp_dir().join(format!("agent-metadata-{}.sqlite3", Uuid::new_v4()));
        Store::open(path).expect("test store opens")
    }

    fn activate_workspace(store: &Store) -> String {
        let pending = store
            .create_pending_workspace_profile(
                "metadata-test",
                "UTC",
                "Daily",
                "{date}.md",
                None,
                "markdown",
            )
            .expect("workspace created");
        store
            .activate_workspace_profile(&pending.id)
            .expect("workspace activated")
            .id
    }

    #[test]
    fn metadata_corrections_are_workspace_scoped_overlays_and_reversible() {
        let store = store();
        let workspace = activate_workspace(&store);
        let event = |message: &str, task: &str| LogEventInput {
            source: "codex/test".to_owned(),
            level: None,
            timestamp: Some(Utc::now()),
            message: message.to_owned(),
            metadata: Some(Map::from_iter([("task_id".to_owned(), json!(task))])),
            fingerprint: None,
        };
        let first = store.insert_event(event("start", "task-a")).unwrap();
        let second = store.insert_event(event("complete", "task-a")).unwrap();
        let unrelated = store.insert_event(event("other", "task-b")).unwrap();
        store
            .save_event_metadata_override(&workspace, &first.id, "product", json!("Log Inbox"))
            .unwrap();

        let mut events = store
            .get_events_by_ids(&[first.id.clone(), second.id.clone(), unrelated.id.clone()])
            .unwrap();
        store
            .apply_event_metadata_overrides(&workspace, &mut events)
            .unwrap();
        assert_eq!(events[0].metadata["product"], "Log Inbox");
        assert_eq!(events[1].metadata["product"], "Log Inbox");
        assert!(events[2].metadata.get("product").is_none());
        assert!(
            store
                .get_events_by_ids(std::slice::from_ref(&first.id))
                .unwrap()[0]
                .metadata
                .get("product")
                .is_none(),
            "stored ingest remains unchanged"
        );

        assert!(
            store
                .remove_event_metadata_override(&workspace, &second.id, "product")
                .unwrap()
        );
        let mut events = store.get_events_by_ids(&[first.id, second.id]).unwrap();
        store
            .apply_event_metadata_overrides(&workspace, &mut events)
            .unwrap();
        assert!(
            events
                .iter()
                .all(|event| event.metadata.get("product").is_none())
        );
    }

    #[test]
    fn guidance_fields_are_workspace_scoped_and_validated() {
        let store = store();
        let workspace = activate_workspace(&store);
        let defaults = store.agent_metadata_fields(&workspace).unwrap();
        assert!(defaults.contains(&"task_id".to_owned()));
        let selected = vec!["task_id".to_owned(), "product".to_owned()];
        assert_eq!(
            store
                .save_agent_metadata_fields(&workspace, &selected)
                .unwrap(),
            selected
        );
        assert!(
            store
                .save_agent_metadata_fields(&workspace, &["secret".to_owned()])
                .is_err()
        );
        assert!(
            store
                .agent_metadata_fields("other-workspace")
                .unwrap()
                .contains(&"session_id".to_owned())
        );
    }
}
