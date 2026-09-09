use crate::{
    models::{ContextMapping, IgnoredContextIdentity, LinkSelector},
    store::Store,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, params};
use std::path::{Component, Path};
use uuid::Uuid;

const SELECTOR_FIELDS: &[&str] = &[
    "source",
    "repo",
    "project",
    "product",
    "app",
    "service",
    "module",
    "work_item",
    "branch",
];

impl Store {
    pub fn list_context_mappings(&self, workspace_id: &str) -> Result<Vec<ContextMapping>> {
        let conn = self.connect()?;
        let mut statement = conn.prepare(
            "SELECT id, workspace_id, selectors_json, canonical_note_path, enabled, source_identity, source_digest, created_at, updated_at FROM context_mappings WHERE workspace_id = ?1 ORDER BY canonical_note_path, id",
        )?;
        statement
            .query_map(params![workspace_id], context_mapping_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn save_context_mapping(&self, mapping: &ContextMapping) -> Result<ContextMapping> {
        let selectors = normalized_selectors(&mapping.selectors)?;
        validate_markdown_path(&mapping.canonical_note_path)?;
        validate_source_provenance(
            mapping.source_identity.as_deref(),
            mapping.source_digest.as_deref(),
        )?;
        let selectors_json = serde_json::to_string(&selectors)?;
        let now = Utc::now().to_rfc3339();
        let id = if mapping.id.trim().is_empty() {
            format!("context_{}", Uuid::new_v4().simple())
        } else {
            validate_id(&mapping.id)?;
            mapping.id.clone()
        };
        self.connect()?.execute(
            r#"INSERT INTO context_mappings
                (id, workspace_id, selectors_json, canonical_note_path, enabled,
                 source_identity, source_digest, created_at, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
               ON CONFLICT(id) DO UPDATE SET
                 selectors_json = excluded.selectors_json,
                 canonical_note_path = excluded.canonical_note_path,
                 enabled = excluded.enabled,
                 source_identity = excluded.source_identity,
                 source_digest = excluded.source_digest,
                 updated_at = excluded.updated_at
               WHERE context_mappings.workspace_id = excluded.workspace_id"#,
            params![
                id,
                mapping.workspace_id,
                selectors_json,
                mapping.canonical_note_path,
                mapping.enabled,
                mapping.source_identity,
                mapping.source_digest,
                now,
            ],
        )?;
        let saved = self
            .context_mapping(&id)?
            .context("context mapping missing after save")?;
        anyhow::ensure!(
            saved.workspace_id == mapping.workspace_id,
            "context mapping ID belongs to a different workspace"
        );
        Ok(saved)
    }

    pub fn context_mapping(&self, id: &str) -> Result<Option<ContextMapping>> {
        self.connect()?
            .query_row(
                "SELECT id, workspace_id, selectors_json, canonical_note_path, enabled, source_identity, source_digest, created_at, updated_at FROM context_mappings WHERE id = ?1",
                params![id],
                context_mapping_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn save_ignored_context_identity(
        &self,
        identity: &IgnoredContextIdentity,
    ) -> Result<IgnoredContextIdentity> {
        validate_selector_field(&identity.field)?;
        anyhow::ensure!(
            !identity.value.trim().is_empty() && identity.value.len() <= 1024,
            "ignored context value is invalid"
        );
        let normalized = identity.value.trim().to_lowercase();
        anyhow::ensure!(
            normalized == identity.normalized_value,
            "ignored context normalized value is not canonical"
        );
        validate_source_provenance(
            identity.source_identity.as_deref(),
            identity.source_digest.as_deref(),
        )?;
        let id = if identity.id.trim().is_empty() {
            format!("ignored_context_{}", Uuid::new_v4().simple())
        } else {
            validate_id(&identity.id)?;
            identity.id.clone()
        };
        let now = Utc::now().to_rfc3339();
        self.connect()?.execute(
            r#"INSERT INTO ignored_context_identities
                (id, workspace_id, field, value, normalized_value,
                 source_identity, source_digest, created_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
               ON CONFLICT(workspace_id, field, normalized_value) DO NOTHING"#,
            params![
                id,
                identity.workspace_id,
                identity.field,
                identity.value.trim(),
                normalized,
                identity.source_identity,
                identity.source_digest,
                now,
            ],
        )?;
        self.connect()?
            .query_row(
                "SELECT id, workspace_id, field, value, normalized_value, source_identity, source_digest, created_at FROM ignored_context_identities WHERE workspace_id = ?1 AND field = ?2 AND normalized_value = ?3",
                params![identity.workspace_id, identity.field, normalized],
                ignored_context_from_row,
            )
            .map_err(Into::into)
    }
}

fn normalized_selectors(selectors: &[LinkSelector]) -> Result<Vec<LinkSelector>> {
    anyhow::ensure!(
        !selectors.is_empty() && selectors.len() <= 16,
        "context mapping requires 1-16 selectors"
    );
    let mut result = selectors.to_vec();
    for selector in &mut result {
        validate_selector_field(&selector.field)?;
        anyhow::ensure!(
            matches!(selector.operator.as_str(), "exact" | "contains"),
            "unsupported context selector operator"
        );
        selector.value = selector.value.trim().to_owned();
        anyhow::ensure!(
            !selector.value.is_empty() && selector.value.len() <= 1024,
            "context selector value is invalid"
        );
    }
    result.sort_by(|left, right| {
        (&left.field, &left.operator, &left.value).cmp(&(
            &right.field,
            &right.operator,
            &right.value,
        ))
    });
    result.dedup();
    anyhow::ensure!(
        result.len() == selectors.len(),
        "context selectors must be unique"
    );
    Ok(result)
}

fn validate_selector_field(field: &str) -> Result<()> {
    anyhow::ensure!(
        SELECTOR_FIELDS.contains(&field),
        "unsupported context selector field"
    );
    Ok(())
}

fn validate_markdown_path(value: &str) -> Result<()> {
    let path = Path::new(value);
    anyhow::ensure!(
        !path.is_absolute() && path.extension().and_then(|value| value.to_str()) == Some("md"),
        "canonical note must be a relative Markdown path"
    );
    anyhow::ensure!(
        path.components()
            .all(|component| matches!(component, Component::Normal(_))),
        "canonical note path cannot contain traversal"
    );
    Ok(())
}

fn validate_source_provenance(identity: Option<&str>, digest: Option<&str>) -> Result<()> {
    anyhow::ensure!(
        identity.is_some() == digest.is_some(),
        "context migration provenance must be complete"
    );
    if let Some(identity) = identity {
        anyhow::ensure!(
            !identity.trim().is_empty() && identity.len() <= 4096,
            "context source identity is invalid"
        );
    }
    if let Some(digest) = digest {
        anyhow::ensure!(
            digest.len() == 64
                && digest
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
            "context source digest must be lowercase SHA-256"
        );
    }
    Ok(())
}

fn validate_id(id: &str) -> Result<()> {
    anyhow::ensure!(
        id.len() <= 200
            && id
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.')),
        "context record ID is invalid"
    );
    Ok(())
}

fn context_mapping_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ContextMapping> {
    let selectors_json: String = row.get(2)?;
    Ok(ContextMapping {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        selectors: serde_json::from_str(&selectors_json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(2, rusqlite::types::Type::Text, error.into())
        })?,
        canonical_note_path: row.get(3)?,
        enabled: row.get(4)?,
        source_identity: row.get(5)?,
        source_digest: row.get(6)?,
        created_at: parse_time(row.get::<_, String>(7)?)?,
        updated_at: parse_time(row.get::<_, String>(8)?)?,
    })
}

fn ignored_context_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<IgnoredContextIdentity> {
    Ok(IgnoredContextIdentity {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        field: row.get(2)?,
        value: row.get(3)?,
        normalized_value: row.get(4)?,
        source_identity: row.get(5)?,
        source_digest: row.get(6)?,
        created_at: parse_time(row.get::<_, String>(7)?)?,
    })
}

fn parse_time(value: String) -> rusqlite::Result<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(&value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, error.into())
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::WorkspaceProfile;

    fn active_profile(store: &Store) -> WorkspaceProfile {
        let profile = store
            .create_pending_workspace_profile(
                "context-binding",
                "UTC",
                "Work Log",
                "{date}.md",
                None,
                "markdown",
            )
            .unwrap();
        store.activate_workspace_profile(&profile.id).unwrap()
    }

    #[test]
    fn scopes_context_records_to_the_workspace_and_preserves_provenance() {
        let store = Store::open(
            std::env::temp_dir().join(format!("log-inbox-context-{}.sqlite3", Uuid::new_v4())),
        )
        .unwrap();
        let profile = active_profile(&store);
        let mapping = ContextMapping {
            id: "mapping_test".to_owned(),
            workspace_id: profile.id.clone(),
            selectors: vec![LinkSelector {
                field: "repo".to_owned(),
                operator: "exact".to_owned(),
                value: "log-inbox".to_owned(),
            }],
            canonical_note_path: "Products/Log Inbox.md".to_owned(),
            enabled: true,
            source_identity: Some("legacy-rule:rule_1".to_owned()),
            source_digest: Some("a".repeat(64)),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let saved = store.save_context_mapping(&mapping).unwrap();
        assert_eq!(saved.workspace_id, profile.id);
        assert_eq!(
            store.list_context_mappings(&profile.id).unwrap(),
            vec![saved.clone()]
        );
        assert!(
            store
                .list_context_mappings("other-workspace")
                .unwrap()
                .is_empty()
        );
        assert!(
            store
                .save_context_mapping(&ContextMapping {
                    canonical_note_path: "../escape.md".to_owned(),
                    ..mapping.clone()
                })
                .is_err()
        );

        let ignored = store
            .save_ignored_context_identity(&IgnoredContextIdentity {
                id: "ignored_test".to_owned(),
                workspace_id: profile.id,
                field: "module".to_owned(),
                value: "Noise".to_owned(),
                normalized_value: "noise".to_owned(),
                source_identity: Some("legacy-ignore:ignore_1".to_owned()),
                source_digest: Some("b".repeat(64)),
                created_at: Utc::now(),
            })
            .unwrap();
        assert_eq!(ignored.normalized_value, "noise");
    }
}
