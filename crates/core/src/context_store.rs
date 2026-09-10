use crate::{
    models::{
        ContextMapping, ExpiredMigrationBackup, IgnoredContextIdentity, LegacyCutoverImport,
        LegacyMigrationArtifact, LinkSelector, MigrationItem, MigrationJournalEntry,
    },
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
    pub fn expired_migration_backups(
        &self,
        completed_before: DateTime<Utc>,
    ) -> Result<Vec<ExpiredMigrationBackup>> {
        let conn = self.connect()?;
        let mut statement = conn.prepare(
            r#"SELECT operation_id, details_json
               FROM migration_journal
               WHERE migration_name = 'refocus-cutover'
                 AND status = 'completed'
                 AND completed_at < ?1
               ORDER BY completed_at, operation_id"#,
        )?;
        let rows = statement.query_map(params![completed_before.to_rfc3339()], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut backups = Vec::new();
        for row in rows {
            let (operation_id, details_json) = row?;
            let details: serde_json::Value = serde_json::from_str(&details_json)?;
            if details.get("backup_deleted_at").is_some() {
                continue;
            }
            if let Some(path) = details
                .get("backup_path")
                .and_then(serde_json::Value::as_str)
            {
                backups.push(ExpiredMigrationBackup {
                    operation_id,
                    path: path.to_owned(),
                });
            }
        }
        Ok(backups)
    }

    pub fn mark_migration_backup_deleted(
        &self,
        operation_id: &str,
        expected_path: &str,
        deleted_at: DateTime<Utc>,
    ) -> Result<()> {
        let conn = self.connect()?;
        let current: (String, String, String) = conn
            .query_row(
                "SELECT migration_name, status, details_json FROM migration_journal WHERE operation_id = ?1",
                params![operation_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?
            .context("migration operation does not exist")?;
        anyhow::ensure!(
            current.0 == "refocus-cutover" && current.1 == "completed",
            "only a completed refocus migration backup can be marked deleted"
        );
        let mut details: serde_json::Value = serde_json::from_str(&current.2)?;
        anyhow::ensure!(
            details
                .get("backup_path")
                .and_then(serde_json::Value::as_str)
                == Some(expected_path),
            "migration backup path changed before deletion was recorded"
        );
        if details.get("backup_deleted_at").is_some() {
            return Ok(());
        }
        details["backup_deleted_at"] = serde_json::Value::String(deleted_at.to_rfc3339());
        let changed = conn.execute(
            "UPDATE migration_journal SET details_json = ?1 WHERE operation_id = ?2 AND details_json = ?3",
            params![serde_json::to_string(&details)?, operation_id, current.2],
        )?;
        anyhow::ensure!(
            changed == 1,
            "migration journal changed while recording backup cleanup"
        );
        Ok(())
    }

    pub fn commit_legacy_cutover_import(
        &self,
        import: &LegacyCutoverImport,
    ) -> Result<MigrationJournalEntry> {
        validate_id(&import.operation_id)?;
        validate_digest(&import.report_digest)?;
        anyhow::ensure!(
            !import.source_identity.trim().is_empty() && import.source_identity.len() <= 4096,
            "migration source identity is invalid"
        );
        anyhow::ensure!(
            !import.workspace_id.trim().is_empty(),
            "migration workspace is required"
        );
        anyhow::ensure!(
            !import.backup_path.trim().is_empty() && import.backup_path.len() <= 4096,
            "migration backup path is invalid"
        );
        validate_cutover_import(import)?;

        let now = Utc::now().to_rfc3339();
        let mut conn = self.connect()?;
        let transaction = conn.transaction()?;
        let active_workspace: Option<String> = transaction
            .query_row(
                "SELECT id FROM workspace_profiles WHERE id = ?1 AND status = 'active'",
                params![import.workspace_id],
                |row| row.get(0),
            )
            .optional()?;
        anyhow::ensure!(
            active_workspace.is_some(),
            "migration workspace is not the active profile"
        );
        let operation_details = serde_json::json!({
            "phase": "import_committed",
            "report_digest": import.report_digest,
            "workspace_id": import.workspace_id,
            "backup_path": import.backup_path,
        });
        transaction.execute(
            r#"INSERT INTO migration_journal
                (operation_id, migration_name, source_identity, status, details_json, started_at)
               VALUES (?1, 'refocus-cutover', ?2, 'started', ?3, ?4)
               ON CONFLICT(operation_id) DO NOTHING"#,
            params![
                import.operation_id,
                import.source_identity,
                serde_json::to_string(&operation_details)?,
                now,
            ],
        )?;
        let existing_operation: (String, String, String) = transaction.query_row(
            "SELECT migration_name, source_identity, status FROM migration_journal WHERE operation_id = ?1",
            params![import.operation_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )?;
        anyhow::ensure!(
            existing_operation.0 == "refocus-cutover"
                && existing_operation.1 == import.source_identity
                && existing_operation.2 == "started",
            "migration operation conflicts with the reviewed import"
        );

        for item in &import.items {
            transaction.execute(
                r#"INSERT INTO migration_items
                    (operation_id, item_kind, source_identity, source_digest, status, details_json, updated_at)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
                   ON CONFLICT(operation_id, item_kind, source_identity) DO UPDATE SET
                     status = excluded.status,
                     details_json = excluded.details_json,
                     updated_at = excluded.updated_at
                   WHERE migration_items.source_digest = excluded.source_digest"#,
                params![
                    item.operation_id,
                    item.item_kind,
                    item.source_identity,
                    item.source_digest,
                    item.status,
                    serde_json::to_string(&item.details)?,
                    now,
                ],
            )?;
            let saved_digest: String = transaction.query_row(
                "SELECT source_digest FROM migration_items WHERE operation_id = ?1 AND item_kind = ?2 AND source_identity = ?3",
                params![item.operation_id, item.item_kind, item.source_identity],
                |row| row.get(0),
            )?;
            anyhow::ensure!(
                saved_digest == item.source_digest,
                "migration source changed for {}",
                item.source_identity
            );
        }

        for mapping in &import.mappings {
            transaction.execute(
                r#"INSERT INTO context_mappings
                    (id, workspace_id, selectors_json, canonical_note_path, enabled,
                     source_identity, source_digest, created_at, updated_at)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)
                   ON CONFLICT(id) DO UPDATE SET
                     canonical_note_path = excluded.canonical_note_path,
                     enabled = excluded.enabled,
                     updated_at = excluded.updated_at
                   WHERE context_mappings.workspace_id = excluded.workspace_id
                     AND context_mappings.selectors_json = excluded.selectors_json
                     AND context_mappings.source_identity = excluded.source_identity
                     AND context_mappings.source_digest = excluded.source_digest"#,
                params![
                    mapping.id,
                    mapping.workspace_id,
                    serde_json::to_string(&normalized_selectors(&mapping.selectors)?)?,
                    mapping.canonical_note_path,
                    mapping.enabled,
                    mapping.source_identity,
                    mapping.source_digest,
                    now,
                ],
            )?;
        }

        for ignored in &import.ignored {
            transaction.execute(
                r#"INSERT INTO ignored_context_identities
                    (id, workspace_id, field, value, normalized_value,
                     source_identity, source_digest, created_at)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                   ON CONFLICT(workspace_id, field, normalized_value) DO NOTHING"#,
                params![
                    ignored.id,
                    ignored.workspace_id,
                    ignored.field,
                    ignored.value,
                    ignored.normalized_value,
                    ignored.source_identity,
                    ignored.source_digest,
                    now,
                ],
            )?;
        }

        for artifact in &import.artifacts {
            transaction.execute(
                r#"INSERT INTO legacy_migration_artifacts
                    (operation_id, artifact_kind, source_identity, source_digest, content,
                     parse_status, details_json, created_at)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                   ON CONFLICT(operation_id, artifact_kind, source_identity) DO NOTHING"#,
                params![
                    artifact.operation_id,
                    artifact.artifact_kind,
                    artifact.source_identity,
                    artifact.source_digest,
                    artifact.content,
                    artifact.parse_status,
                    serde_json::to_string(&artifact.details)?,
                    now,
                ],
            )?;
        }

        for manual in &import.manual_events {
            let manual_id = format!("manual_legacy_{}", &manual.source_digest[..24]);
            transaction.execute(
                "INSERT INTO manual_daily_entries (id, workspace_id, local_date, text, references_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6) ON CONFLICT(id) DO NOTHING",
                params![
                    manual_id,
                    import.workspace_id,
                    manual.local_date.to_string(),
                    manual.text,
                    serde_json::to_string(&manual.references)?,
                    now,
                ],
            )?;
            transaction.execute(
                r#"INSERT INTO legacy_manual_event_imports
                    (event_id, live_event_id, operation_id, workspace_id, manual_entry_id, source_digest, imported_at)
                   VALUES (?1, ?1, ?2, ?3, ?4, ?5, ?6)
                   ON CONFLICT(event_id) DO NOTHING"#,
                params![
                    manual.event_id,
                    import.operation_id,
                    import.workspace_id,
                    manual_id,
                    manual.source_digest,
                    now,
                ],
            )?;
            let saved_digest: String = transaction.query_row(
                "SELECT source_digest FROM legacy_manual_event_imports WHERE event_id = ?1",
                params![manual.event_id],
                |row| row.get(0),
            )?;
            anyhow::ensure!(
                saved_digest == manual.source_digest,
                "legacy manual event changed after import"
            );
        }

        for (key, expected_value) in &import.obsolete_preferences {
            let changed = transaction.execute(
                "DELETE FROM app_preferences WHERE key = ?1 AND value = ?2",
                params![key, expected_value],
            )?;
            anyhow::ensure!(
                changed == 1,
                "legacy preference changed before import: {key}"
            );
        }
        transaction.execute(
            "UPDATE migration_journal SET details_json = ?1 WHERE operation_id = ?2",
            params![
                serde_json::to_string(&operation_details)?,
                import.operation_id
            ],
        )?;
        transaction.commit()?;
        self.migration_operation(&import.operation_id)?
            .context("migration operation missing after import")
    }

    pub fn preserve_legacy_migration_artifact(
        &self,
        artifact: &LegacyMigrationArtifact,
    ) -> Result<LegacyMigrationArtifact> {
        validate_id(&artifact.operation_id)?;
        validate_item_kind(&artifact.artifact_kind)?;
        anyhow::ensure!(
            !artifact.source_identity.trim().is_empty() && artifact.source_identity.len() <= 4096,
            "migration source identity is invalid"
        );
        validate_digest(&artifact.source_digest)?;
        anyhow::ensure!(
            artifact.content.len() <= 4 * 1024 * 1024,
            "legacy artifact is too large"
        );
        anyhow::ensure!(
            matches!(artifact.parse_status.as_str(), "valid" | "unparseable"),
            "legacy artifact parse status is invalid"
        );
        let details_json = serde_json::to_string(&artifact.details)?;
        anyhow::ensure!(
            details_json.len() <= 64 * 1024,
            "legacy artifact details are too large"
        );
        let conn = self.connect()?;
        conn.execute(
            r#"INSERT INTO legacy_migration_artifacts
                (operation_id, artifact_kind, source_identity, source_digest, content,
                 parse_status, details_json, created_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
               ON CONFLICT(operation_id, artifact_kind, source_identity) DO NOTHING"#,
            params![
                artifact.operation_id,
                artifact.artifact_kind,
                artifact.source_identity,
                artifact.source_digest,
                artifact.content,
                artifact.parse_status,
                details_json,
                Utc::now().to_rfc3339(),
            ],
        )?;
        let saved = self
            .legacy_migration_artifact(
                &artifact.operation_id,
                &artifact.artifact_kind,
                &artifact.source_identity,
            )?
            .context("legacy migration artifact missing after save")?;
        anyhow::ensure!(
            saved.source_digest == artifact.source_digest && saved.content == artifact.content,
            "legacy migration source changed after it was preserved"
        );
        Ok(saved)
    }

    pub fn legacy_migration_artifact(
        &self,
        operation_id: &str,
        artifact_kind: &str,
        source_identity: &str,
    ) -> Result<Option<LegacyMigrationArtifact>> {
        self.connect()?
            .query_row(
                "SELECT operation_id, artifact_kind, source_identity, source_digest, content, parse_status, details_json, created_at FROM legacy_migration_artifacts WHERE operation_id = ?1 AND artifact_kind = ?2 AND source_identity = ?3",
                params![operation_id, artifact_kind, source_identity],
                legacy_artifact_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_migration_items(&self, operation_id: &str) -> Result<Vec<MigrationItem>> {
        validate_id(operation_id)?;
        let conn = self.connect()?;
        let mut statement = conn.prepare(
            "SELECT operation_id, item_kind, source_identity, source_digest, status, details_json, updated_at FROM migration_items WHERE operation_id = ?1 ORDER BY item_kind, source_identity",
        )?;
        statement
            .query_map(params![operation_id], migration_item_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn save_migration_item(&self, item: &MigrationItem) -> Result<MigrationItem> {
        validate_id(&item.operation_id)?;
        validate_item_kind(&item.item_kind)?;
        anyhow::ensure!(
            !item.source_identity.trim().is_empty() && item.source_identity.len() <= 4096,
            "migration source identity is invalid"
        );
        validate_digest(&item.source_digest)?;
        validate_migration_item_status(&item.status)?;
        let details_json = serde_json::to_string(&item.details)?;
        anyhow::ensure!(
            details_json.len() <= 1024 * 1024,
            "migration item details are too large"
        );
        let now = Utc::now().to_rfc3339();
        let conn = self.connect()?;
        conn.execute(
            r#"INSERT INTO migration_items
                (operation_id, item_kind, source_identity, source_digest, status, details_json, updated_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
               ON CONFLICT(operation_id, item_kind, source_identity) DO UPDATE SET
                 status = excluded.status,
                 details_json = excluded.details_json,
                 updated_at = excluded.updated_at
               WHERE migration_items.source_digest = excluded.source_digest"#,
            params![
                item.operation_id,
                item.item_kind,
                item.source_identity,
                item.source_digest,
                item.status,
                details_json,
                now,
            ],
        )?;
        let saved = conn
            .query_row(
                "SELECT operation_id, item_kind, source_identity, source_digest, status, details_json, updated_at FROM migration_items WHERE operation_id = ?1 AND item_kind = ?2 AND source_identity = ?3",
                params![item.operation_id, item.item_kind, item.source_identity],
                migration_item_from_row,
            )
            .optional()?;
        let saved = saved.context("migration item missing after save")?;
        anyhow::ensure!(
            saved.source_digest == item.source_digest,
            "migration source changed for an existing identity"
        );
        Ok(saved)
    }

    pub fn transition_migration_item(
        &self,
        item: &MigrationItem,
        expected_status: &str,
    ) -> Result<MigrationItem> {
        validate_id(&item.operation_id)?;
        validate_item_kind(&item.item_kind)?;
        validate_digest(&item.source_digest)?;
        validate_migration_item_status(expected_status)?;
        validate_migration_item_status(&item.status)?;
        let details_json = serde_json::to_string(&item.details)?;
        let conn = self.connect()?;
        let changed = conn.execute(
            r#"UPDATE migration_items
               SET status = ?1, details_json = ?2, updated_at = ?3
               WHERE operation_id = ?4 AND item_kind = ?5 AND source_identity = ?6
                 AND source_digest = ?7 AND status = ?8"#,
            params![
                item.status,
                details_json,
                Utc::now().to_rfc3339(),
                item.operation_id,
                item.item_kind,
                item.source_identity,
                item.source_digest,
                expected_status,
            ],
        )?;
        let current = conn
            .query_row(
                "SELECT operation_id, item_kind, source_identity, source_digest, status, details_json, updated_at FROM migration_items WHERE operation_id = ?1 AND item_kind = ?2 AND source_identity = ?3",
                params![item.operation_id, item.item_kind, item.source_identity],
                migration_item_from_row,
            )
            .optional()?
            .context("migration item not found")?;
        anyhow::ensure!(
            current.source_digest == item.source_digest,
            "migration source changed for an existing identity"
        );
        anyhow::ensure!(
            changed == 1 || current.status == item.status,
            "migration item is not in the expected state"
        );
        Ok(current)
    }

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

fn validate_cutover_import(import: &LegacyCutoverImport) -> Result<()> {
    anyhow::ensure!(
        import.items.len() <= 10_000,
        "migration contains too many items"
    );
    for item in &import.items {
        anyhow::ensure!(
            item.operation_id == import.operation_id,
            "migration item belongs to another operation"
        );
        validate_item_kind(&item.item_kind)?;
        validate_digest(&item.source_digest)?;
        validate_migration_item_status(&item.status)?;
        anyhow::ensure!(
            serde_json::to_vec(&item.details)?.len() <= 1024 * 1024,
            "migration item details are too large"
        );
    }
    for mapping in &import.mappings {
        anyhow::ensure!(
            mapping.workspace_id == import.workspace_id,
            "context mapping belongs to another workspace"
        );
        validate_id(&mapping.id)?;
        normalized_selectors(&mapping.selectors)?;
        validate_markdown_path(&mapping.canonical_note_path)?;
        validate_source_provenance(
            mapping.source_identity.as_deref(),
            mapping.source_digest.as_deref(),
        )?;
    }
    for ignored in &import.ignored {
        anyhow::ensure!(
            ignored.workspace_id == import.workspace_id,
            "ignored identity belongs to another workspace"
        );
        validate_id(&ignored.id)?;
        validate_selector_field(&ignored.field)?;
        anyhow::ensure!(
            ignored.normalized_value == ignored.value.trim().to_lowercase(),
            "ignored context normalized value is not canonical"
        );
        validate_source_provenance(
            ignored.source_identity.as_deref(),
            ignored.source_digest.as_deref(),
        )?;
    }
    for artifact in &import.artifacts {
        anyhow::ensure!(
            artifact.operation_id == import.operation_id,
            "legacy artifact belongs to another operation"
        );
        validate_item_kind(&artifact.artifact_kind)?;
        validate_digest(&artifact.source_digest)?;
        anyhow::ensure!(
            artifact.content.len() <= 4 * 1024 * 1024,
            "legacy artifact is too large"
        );
        anyhow::ensure!(
            matches!(artifact.parse_status.as_str(), "valid" | "unparseable"),
            "legacy artifact parse status is invalid"
        );
    }
    for manual in &import.manual_events {
        validate_digest(&manual.source_digest)?;
        anyhow::ensure!(
            !manual.text.trim().is_empty() && manual.text.len() <= 16 * 1024,
            "legacy manual entry text is invalid"
        );
        anyhow::ensure!(manual.references.len() <= 20, "too many manual references");
        anyhow::ensure!(
            manual.references.iter().all(|reference| {
                let authority = reference
                    .strip_prefix("https://")
                    .or_else(|| reference.strip_prefix("http://"));
                reference.len() <= 2048
                    && !reference.chars().any(char::is_whitespace)
                    && authority.is_some_and(|value| !value.is_empty() && !value.starts_with('/'))
            }),
            "legacy manual references must be absolute HTTP(S) URLs"
        );
    }
    for key in import.obsolete_preferences.keys() {
        anyhow::ensure!(
            !key.is_empty() && key.len() <= 1024,
            "legacy preference key is invalid"
        );
    }
    Ok(())
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
        validate_digest(digest)?;
    }
    Ok(())
}

fn validate_digest(digest: &str) -> Result<()> {
    anyhow::ensure!(
        digest.len() == 64
            && digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase()),
        "source digest must be lowercase SHA-256"
    );
    Ok(())
}

fn validate_item_kind(kind: &str) -> Result<()> {
    anyhow::ensure!(
        !kind.is_empty()
            && kind.len() <= 64
            && kind
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_'),
        "migration item kind is invalid"
    );
    Ok(())
}

fn validate_migration_item_status(status: &str) -> Result<()> {
    anyhow::ensure!(
        matches!(
            status,
            "inventoried" | "imported" | "cleanup_pending" | "cleaned" | "preserved" | "error"
        ),
        "migration item status is invalid"
    );
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

fn migration_item_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MigrationItem> {
    let details_json: String = row.get(5)?;
    Ok(MigrationItem {
        operation_id: row.get(0)?,
        item_kind: row.get(1)?,
        source_identity: row.get(2)?,
        source_digest: row.get(3)?,
        status: row.get(4)?,
        details: serde_json::from_str(&details_json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, error.into())
        })?,
        updated_at: parse_time(row.get::<_, String>(6)?)?,
    })
}

fn legacy_artifact_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<LegacyMigrationArtifact> {
    let details_json: String = row.get(6)?;
    Ok(LegacyMigrationArtifact {
        operation_id: row.get(0)?,
        artifact_kind: row.get(1)?,
        source_identity: row.get(2)?,
        source_digest: row.get(3)?,
        content: row.get(4)?,
        parse_status: row.get(5)?,
        details: serde_json::from_str(&details_json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, error.into())
        })?,
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
    use crate::models::{LegacyCutoverImport, WorkspaceProfile};

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

    #[test]
    fn journals_migration_items_with_bounded_validated_provenance() {
        let store = Store::open(std::env::temp_dir().join(format!(
            "log-inbox-migration-item-{}.sqlite3",
            Uuid::new_v4()
        )))
        .unwrap();
        store
            .begin_migration_operation(
                "migration_test",
                "refocus-cutover",
                "legacy-runtime:test",
                &serde_json::json!({"dry_run": true}),
            )
            .unwrap();
        let item = MigrationItem {
            operation_id: "migration_test".to_owned(),
            item_kind: "context_mapping".to_owned(),
            source_identity: "legacy-rule:rule_1".to_owned(),
            source_digest: "c".repeat(64),
            status: "inventoried".to_owned(),
            details: serde_json::json!({"target": "Products/Log Inbox.md"}),
            updated_at: Utc::now(),
        };
        let saved = store.save_migration_item(&item).unwrap();
        assert_eq!(saved.source_digest, item.source_digest);
        assert_eq!(
            store.list_migration_items("migration_test").unwrap(),
            vec![saved]
        );

        let artifact = LegacyMigrationArtifact {
            operation_id: item.operation_id.clone(),
            artifact_kind: item.item_kind.clone(),
            source_identity: item.source_identity.clone(),
            source_digest: item.source_digest.clone(),
            content: b"legacy proposal bytes".to_vec(),
            parse_status: "unparseable".to_owned(),
            details: serde_json::json!({"reason": "missing frontmatter"}),
            created_at: Utc::now(),
        };
        let preserved = store.preserve_legacy_migration_artifact(&artifact).unwrap();
        assert_eq!(preserved.content, artifact.content);
        assert_eq!(
            store
                .legacy_migration_artifact(
                    &item.operation_id,
                    &item.item_kind,
                    &item.source_identity,
                )
                .unwrap(),
            Some(preserved)
        );
        let imported_item = MigrationItem {
            status: "imported".to_owned(),
            details: serde_json::json!({"imported": true}),
            ..item.clone()
        };
        let transitioned = store
            .transition_migration_item(&imported_item, "inventoried")
            .unwrap();
        assert_eq!(transitioned.status, "imported");
        assert_eq!(
            store
                .transition_migration_item(&imported_item, "inventoried")
                .unwrap()
                .status,
            "imported"
        );

        assert!(
            store
                .save_migration_item(&MigrationItem {
                    source_digest: "d".repeat(64),
                    ..item.clone()
                })
                .is_err()
        );

        assert!(
            store
                .save_migration_item(&MigrationItem {
                    status: "completed".to_owned(),
                    ..item.clone()
                })
                .is_err()
        );
        assert!(
            store
                .save_migration_item(&MigrationItem {
                    source_digest: "NOT-A-DIGEST".to_owned(),
                    ..item
                })
                .is_err()
        );
    }

    #[test]
    fn commits_cutover_import_atomically_and_requires_exact_preferences() {
        let store = Store::open(std::env::temp_dir().join(format!(
            "log-inbox-cutover-import-{}.sqlite3",
            Uuid::new_v4()
        )))
        .unwrap();
        let profile = active_profile(&store);
        store
            .set_preferences(&std::collections::BTreeMap::from([(
                "legacy".to_owned(),
                "original".to_owned(),
            )]))
            .unwrap();
        let item = MigrationItem {
            operation_id: "cutover_test".to_owned(),
            item_kind: "context_mapping".to_owned(),
            source_identity: "vault_link_rule:rule_1".to_owned(),
            source_digest: "a".repeat(64),
            status: "imported".to_owned(),
            details: serde_json::json!({}),
            updated_at: Utc::now(),
        };
        let mapping = ContextMapping {
            id: "migrated_rule_1".to_owned(),
            workspace_id: profile.id.clone(),
            selectors: vec![LinkSelector {
                field: "repo".to_owned(),
                operator: "exact".to_owned(),
                value: "log-inbox".to_owned(),
            }],
            canonical_note_path: "Products/Log Inbox.md".to_owned(),
            enabled: true,
            source_identity: Some(item.source_identity.clone()),
            source_digest: Some(item.source_digest.clone()),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let import = LegacyCutoverImport {
            operation_id: item.operation_id.clone(),
            source_identity: "legacy-runtime:test".to_owned(),
            report_digest: "f".repeat(64),
            workspace_id: profile.id.clone(),
            items: vec![item],
            mappings: vec![mapping],
            ignored: Vec::new(),
            artifacts: Vec::new(),
            manual_events: Vec::new(),
            obsolete_preferences: std::collections::BTreeMap::from([(
                "legacy".to_owned(),
                "changed".to_owned(),
            )]),
            backup_path: "/data/migration-backups/test.sqlite3".to_owned(),
        };
        assert!(store.commit_legacy_cutover_import(&import).is_err());
        assert!(store.list_context_mappings(&profile.id).unwrap().is_empty());
        assert!(store.migration_operation("cutover_test").unwrap().is_none());
        assert_eq!(store.get_preferences().unwrap()["legacy"], "original");

        let committed = store
            .commit_legacy_cutover_import(&LegacyCutoverImport {
                obsolete_preferences: std::collections::BTreeMap::from([(
                    "legacy".to_owned(),
                    "original".to_owned(),
                )]),
                ..import
            })
            .unwrap();
        assert_eq!(committed.status, "started");
        assert_eq!(committed.details["phase"], "import_committed");
        assert_eq!(store.list_context_mappings(&profile.id).unwrap().len(), 1);
        assert!(!store.get_preferences().unwrap().contains_key("legacy"));
    }
}
