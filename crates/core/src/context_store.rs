use crate::{
    models::{
        ContextMapping, ContextSnapshot, ExpiredMigrationBackup, IgnoredContextIdentity,
        KnowledgeCollection, LegacyCutoverImport, LegacyMigrationArtifact, LinkSelector,
        MigrationItem, MigrationJournalEntry,
    },
    store::Store,
    workspace::normalize_knowledge_collection_paths,
};
use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, params};
use sha2::{Digest, Sha256};
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
    "pull_request",
    "branch",
];

impl Store {
    pub fn create_context_snapshot(
        &self,
        workspace_id: &str,
        local_date: chrono::NaiveDate,
        payload: &serde_json::Value,
    ) -> Result<ContextSnapshot> {
        self.daily_day(workspace_id, local_date)?
            .context("daily day is required before its context snapshot")?;
        let payload_json = serde_json::to_string(payload)?;
        validate_context_snapshot_payload(payload)?;
        anyhow::ensure!(
            payload_json.len() <= 1024 * 1024,
            "Knowledge context snapshot exceeds 1048576 bytes"
        );
        let snapshot_digest = format!("{:x}", Sha256::digest(payload_json.as_bytes()));
        let mut conn = self.connect()?;
        let transaction = conn.transaction()?;
        let candidate_id = format!("context_snapshot_{}", Uuid::new_v4().simple());
        transaction.execute(
            "INSERT OR IGNORE INTO context_snapshots (id, workspace_id, local_date, snapshot_digest, payload_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![candidate_id, workspace_id, local_date.to_string(), snapshot_digest, payload_json, Utc::now().to_rfc3339()],
        )?;
        let id = transaction.query_row(
            "SELECT id FROM context_snapshots WHERE workspace_id = ?1 AND local_date = ?2 AND snapshot_digest = ?3",
            params![workspace_id, local_date.to_string(), snapshot_digest],
            |row| row.get::<_, String>(0),
        )?;
        transaction.commit()?;
        self.context_snapshot(&id)?
            .context("context snapshot missing after creation")
    }

    pub fn context_snapshot(&self, id: &str) -> Result<Option<ContextSnapshot>> {
        self.connect()?
            .query_row(
                "SELECT id, workspace_id, local_date, snapshot_digest, payload_json, created_at FROM context_snapshots WHERE id = ?1",
                params![id],
                context_snapshot_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn proposal_context_snapshot(&self, revision_id: &str) -> Result<Option<ContextSnapshot>> {
        self.connect()?
            .query_row(
                r#"SELECT snapshot.id, snapshot.workspace_id, snapshot.local_date,
                          snapshot.snapshot_digest, snapshot.payload_json, snapshot.created_at
                   FROM proposal_context_snapshots AS link
                   JOIN context_snapshots AS snapshot ON snapshot.id = link.context_snapshot_id
                   WHERE link.revision_id = ?1"#,
                params![revision_id],
                context_snapshot_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

    pub fn list_knowledge_collections(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<KnowledgeCollection>> {
        let conn = self.connect()?;
        let mut statement = conn.prepare(
            "SELECT id, workspace_id, label, purpose, roots_json, exclusions_json, enabled, revision_digest, created_at, updated_at FROM knowledge_collections WHERE workspace_id = ?1 ORDER BY label, id",
        )?;
        statement
            .query_map(params![workspace_id], knowledge_collection_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn save_knowledge_collection(
        &self,
        id: Option<&str>,
        workspace_id: &str,
        label: &str,
        purpose: &str,
        roots: &[String],
        exclusions: &[String],
        enabled: bool,
        expected_updated_at: Option<DateTime<Utc>>,
    ) -> Result<KnowledgeCollection> {
        let label = label.trim();
        let purpose = purpose.trim();
        anyhow::ensure!(
            !label.is_empty() && label.len() <= 100,
            "Knowledge collection label must contain 1-100 bytes"
        );
        anyhow::ensure!(
            !purpose.is_empty() && purpose.len() <= 1000,
            "Knowledge collection purpose must contain 1-1000 bytes"
        );
        let (roots, exclusions) = normalize_knowledge_collection_paths(roots, exclusions)?;
        let revision_digest =
            collection_revision_digest(label, purpose, &roots, &exclusions, enabled)?;
        let mut conn = self.connect()?;
        let transaction = conn.transaction()?;
        let workspace_is_active: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM workspace_profiles WHERE id = ?1 AND status = 'active')",
            params![workspace_id],
            |row| row.get(0),
        )?;
        anyhow::ensure!(
            workspace_is_active,
            "Knowledge collection must belong to the active workspace"
        );
        let now = Utc::now();
        let saved_id = if let Some(id) = id {
            validate_id(id)?;
            let current: Option<String> = transaction
                .query_row(
                    "SELECT updated_at FROM knowledge_collections WHERE id = ?1 AND workspace_id = ?2",
                    params![id, workspace_id],
                    |row| row.get(0),
                )
                .optional()?;
            let current = current.context("Knowledge collection does not exist")?;
            let expected = expected_updated_at
                .context("Knowledge collection update requires its expected timestamp")?;
            anyhow::ensure!(
                parse_time(current)? == expected,
                "Knowledge collection changed; reload and try again"
            );
            let changed = transaction.execute(
                r#"UPDATE knowledge_collections
                   SET label = ?1, purpose = ?2, roots_json = ?3, exclusions_json = ?4,
                       enabled = ?5, revision_digest = ?6, updated_at = ?7
                   WHERE id = ?8 AND workspace_id = ?9 AND updated_at = ?10"#,
                params![
                    label,
                    purpose,
                    serde_json::to_string(&roots)?,
                    serde_json::to_string(&exclusions)?,
                    enabled,
                    revision_digest,
                    now.to_rfc3339(),
                    id,
                    workspace_id,
                    expected.to_rfc3339(),
                ],
            )?;
            anyhow::ensure!(changed == 1, "Knowledge collection changed while saving");
            id.to_owned()
        } else {
            anyhow::ensure!(
                expected_updated_at.is_none(),
                "new Knowledge collection cannot have an expected timestamp"
            );
            let count: u32 = transaction.query_row(
                "SELECT COUNT(*) FROM knowledge_collections WHERE workspace_id = ?1",
                params![workspace_id],
                |row| row.get(0),
            )?;
            anyhow::ensure!(
                count < 8,
                "a workspace can have at most 8 Knowledge collections"
            );
            let id = format!("knowledge_{}", Uuid::new_v4().simple());
            transaction.execute(
                r#"INSERT INTO knowledge_collections
                   (id, workspace_id, label, purpose, roots_json, exclusions_json, enabled,
                    revision_digest, created_at, updated_at)
                   VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)"#,
                params![
                    id,
                    workspace_id,
                    label,
                    purpose,
                    serde_json::to_string(&roots)?,
                    serde_json::to_string(&exclusions)?,
                    enabled,
                    revision_digest,
                    now.to_rfc3339(),
                ],
            )?;
            id
        };
        transaction.commit()?;
        self.knowledge_collection(&saved_id)?
            .context("Knowledge collection missing after save")
    }

    pub fn delete_knowledge_collection(
        &self,
        id: &str,
        workspace_id: &str,
        expected_updated_at: DateTime<Utc>,
    ) -> Result<()> {
        validate_id(id)?;
        let mut conn = self.connect()?;
        let transaction = conn.transaction()?;
        let workspace_is_active: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM workspace_profiles WHERE id = ?1 AND status = 'active')",
            params![workspace_id],
            |row| row.get(0),
        )?;
        anyhow::ensure!(
            workspace_is_active,
            "Knowledge collection must belong to the active workspace"
        );
        let changed = transaction.execute(
            "DELETE FROM knowledge_collections WHERE id = ?1 AND workspace_id = ?2 AND updated_at = ?3",
            params![id, workspace_id, expected_updated_at.to_rfc3339()],
        )?;
        anyhow::ensure!(
            changed == 1,
            "Knowledge collection changed or does not exist; reload and try again"
        );
        transaction.commit()?;
        Ok(())
    }

    pub fn knowledge_collection(&self, id: &str) -> Result<Option<KnowledgeCollection>> {
        validate_id(id)?;
        self.connect()?
            .query_row(
                "SELECT id, workspace_id, label, purpose, roots_json, exclusions_json, enabled, revision_digest, created_at, updated_at FROM knowledge_collections WHERE id = ?1",
                params![id],
                knowledge_collection_from_row,
            )
            .optional()
            .map_err(Into::into)
    }

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

    pub fn update_context_mapping(
        &self,
        id: &str,
        workspace_id: &str,
        selectors: &[LinkSelector],
        canonical_note_path: &str,
        enabled: bool,
        expected_updated_at: DateTime<Utc>,
    ) -> Result<ContextMapping> {
        validate_id(id)?;
        let selectors_json = serde_json::to_string(&normalized_selectors(selectors)?)?;
        validate_markdown_path(canonical_note_path)?;
        let now = Utc::now().to_rfc3339();
        let changed = self.connect()?.execute(
            r#"UPDATE context_mappings
               SET selectors_json = ?1, canonical_note_path = ?2, enabled = ?3,
                   source_identity = NULL, source_digest = NULL, updated_at = ?4
               WHERE id = ?5 AND workspace_id = ?6 AND updated_at = ?7"#,
            params![
                selectors_json,
                canonical_note_path,
                enabled,
                now,
                id,
                workspace_id,
                expected_updated_at.to_rfc3339(),
            ],
        )?;
        anyhow::ensure!(
            changed == 1,
            "context mapping changed; reload and try again"
        );
        self.context_mapping(id)?
            .context("context mapping missing after update")
    }

    pub fn delete_context_mapping(
        &self,
        id: &str,
        workspace_id: &str,
        expected_updated_at: DateTime<Utc>,
    ) -> Result<()> {
        validate_id(id)?;
        let changed = self.connect()?.execute(
            "DELETE FROM context_mappings WHERE id = ?1 AND workspace_id = ?2 AND updated_at = ?3",
            params![id, workspace_id, expected_updated_at.to_rfc3339()],
        )?;
        anyhow::ensure!(
            changed == 1,
            "context mapping changed; reload and try again"
        );
        Ok(())
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

    pub fn list_ignored_context_identities(
        &self,
        workspace_id: &str,
    ) -> Result<Vec<IgnoredContextIdentity>> {
        let conn = self.connect()?;
        let mut statement = conn.prepare(
            "SELECT id, workspace_id, field, value, normalized_value, source_identity, source_digest, created_at FROM ignored_context_identities WHERE workspace_id = ?1 ORDER BY field, normalized_value",
        )?;
        statement
            .query_map(params![workspace_id], ignored_context_from_row)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(Into::into)
    }

    pub fn delete_ignored_context_identity(&self, id: &str, workspace_id: &str) -> Result<()> {
        validate_id(id)?;
        let changed = self.connect()?.execute(
            "DELETE FROM ignored_context_identities WHERE id = ?1 AND workspace_id = ?2",
            params![id, workspace_id],
        )?;
        anyhow::ensure!(changed == 1, "ignored context identity was not found");
        Ok(())
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

fn collection_revision_digest(
    label: &str,
    purpose: &str,
    roots: &[String],
    exclusions: &[String],
    enabled: bool,
) -> Result<String> {
    let canonical = serde_json::to_vec(&serde_json::json!({
        "label": label,
        "purpose": purpose,
        "roots": roots,
        "exclusions": exclusions,
        "enabled": enabled,
    }))?;
    Ok(format!("{:x}", Sha256::digest(canonical)))
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
        value == value.trim()
            && value.len() <= 511
            && !value.chars().any(|character| matches!(
                character,
                '\\' | '\r' | '\n' | '[' | ']' | '|' | '#' | '^'
            ))
            && !path.is_absolute()
            && path.extension().and_then(|value| value.to_str()) == Some("md"),
        "canonical note must be a relative Markdown path"
    );
    let components = path.components().collect::<Vec<_>>();
    anyhow::ensure!(
        components
            .iter()
            .all(|component| matches!(component, Component::Normal(_))),
        "canonical note path cannot contain traversal"
    );
    let normalized = components
        .iter()
        .map(|component| component.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/");
    anyhow::ensure!(
        normalized == value,
        "canonical note path must be normalized"
    );
    anyhow::ensure!(
        components.iter().all(|component| {
            let value = component.as_os_str().to_string_lossy();
            ![".git", ".obsidian", ".trash", ".log-inbox"]
                .iter()
                .any(|protected| value.eq_ignore_ascii_case(protected))
        }),
        "canonical note path enters protected workspace metadata"
    );
    Ok(())
}

fn validate_context_snapshot_payload(payload: &serde_json::Value) -> Result<()> {
    let schema_version = payload
        .get("schema_version")
        .and_then(serde_json::Value::as_u64);
    anyhow::ensure!(
        matches!(schema_version, Some(1 | 2)),
        "unsupported Knowledge context snapshot schema"
    );
    let links = payload
        .get("workstream_links")
        .and_then(serde_json::Value::as_object)
        .context("Knowledge context snapshot requires workstream_links")?;
    anyhow::ensure!(links.len() <= 500, "too many Knowledge workstream links");
    for (workstream_id, values) in links {
        anyhow::ensure!(
            !workstream_id.trim().is_empty() && workstream_id.len() <= 512,
            "invalid Knowledge workstream ID"
        );
        let values = values
            .as_array()
            .context("Knowledge workstream links must be arrays")?;
        anyhow::ensure!(
            values.len() <= 16,
            "too many links for a Knowledge workstream"
        );
        for link in values {
            let link = link
                .as_str()
                .context("Knowledge workstream link must be a string")?;
            anyhow::ensure!(valid_canonical_wikilink(link), "invalid Knowledge wikilink");
        }
    }
    if let Some(evidence) = payload.get("workstream_evidence") {
        let evidence = evidence
            .as_object()
            .context("Knowledge workstream evidence must be an object")?;
        anyhow::ensure!(
            evidence.len() <= 500,
            "too many Knowledge workstream evidence groups"
        );
        for (workstream_id, values) in evidence {
            anyhow::ensure!(
                links.contains_key(workstream_id),
                "Knowledge evidence group has no authorized links"
            );
            let values = values
                .as_array()
                .context("Knowledge workstream evidence must be arrays")?;
            anyhow::ensure!(
                !values.is_empty() && values.len() <= 500,
                "invalid Knowledge workstream evidence count"
            );
            for event_id in values {
                let event_id = event_id
                    .as_str()
                    .context("Knowledge evidence ID must be a string")?;
                anyhow::ensure!(
                    !event_id.trim().is_empty() && event_id.len() <= 512,
                    "invalid Knowledge evidence ID"
                );
            }
        }
    }
    if schema_version == Some(2) {
        anyhow::ensure!(
            payload
                .get("resolver_version")
                .and_then(serde_json::Value::as_str)
                == Some("exact-v2"),
            "Knowledge context snapshot v2 requires exact-v2"
        );
        let used_notes = payload
            .get("used_notes")
            .and_then(serde_json::Value::as_array)
            .context("Knowledge context snapshot v2 requires used_notes")?;
        let mut used_paths = std::collections::BTreeSet::new();
        for note in used_notes {
            let path = note
                .get("path")
                .and_then(serde_json::Value::as_str)
                .context("Knowledge used note requires a path")?;
            validate_markdown_path(path)?;
            anyhow::ensure!(used_paths.insert(path), "duplicate Knowledge used note");
            let digest = note
                .get("usable_digest")
                .and_then(serde_json::Value::as_str)
                .context("Knowledge used note requires a usable digest")?;
            anyhow::ensure!(
                digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "invalid Knowledge used-note digest"
            );
        }
        let excerpts = payload
            .get("excerpts")
            .and_then(serde_json::Value::as_array)
            .context("Knowledge context snapshot v2 requires excerpts")?;
        anyhow::ensure!(excerpts.len() <= 32, "too many Knowledge excerpts");
        let mut excerpt_paths = std::collections::BTreeMap::new();
        let mut total_text_bytes = 0_usize;
        for excerpt in excerpts {
            let excerpt = excerpt
                .as_object()
                .context("Knowledge excerpt must be an object")?;
            let id = excerpt
                .get("id")
                .and_then(serde_json::Value::as_str)
                .context("Knowledge excerpt requires an ID")?;
            anyhow::ensure!(
                id.starts_with("excerpt_") && id.len() <= 80,
                "invalid Knowledge excerpt ID"
            );
            let note_path = excerpt
                .get("note_path")
                .and_then(serde_json::Value::as_str)
                .context("Knowledge excerpt requires a note path")?;
            validate_markdown_path(note_path)?;
            anyhow::ensure!(
                used_paths.contains(note_path),
                "Knowledge excerpt source is not a used note"
            );
            anyhow::ensure!(
                excerpt_paths.insert(id, note_path).is_none(),
                "duplicate Knowledge excerpt ID"
            );
            let title = excerpt
                .get("title")
                .and_then(serde_json::Value::as_str)
                .context("Knowledge excerpt requires a title")?;
            anyhow::ensure!(
                !title.trim().is_empty() && title.len() <= 200,
                "invalid Knowledge excerpt title"
            );
            let text = excerpt
                .get("text")
                .and_then(serde_json::Value::as_str)
                .context("Knowledge excerpt requires text")?;
            anyhow::ensure!(
                !text.trim().is_empty() && text.len() <= 4096,
                "invalid Knowledge excerpt text"
            );
            total_text_bytes += text.len();
            let digest = excerpt
                .get("text_digest")
                .and_then(serde_json::Value::as_str)
                .context("Knowledge excerpt requires a digest")?;
            anyhow::ensure!(
                digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()),
                "invalid Knowledge excerpt digest"
            );
            anyhow::ensure!(
                format!("{:x}", Sha256::digest(text.as_bytes())) == digest,
                "Knowledge excerpt digest does not match its text"
            );
            anyhow::ensure!(
                excerpt.get("reason").and_then(serde_json::Value::as_str)
                    == Some("canonical_note_opening"),
                "invalid Knowledge excerpt reason"
            );
        }
        anyhow::ensure!(
            total_text_bytes <= 64 * 1024,
            "Knowledge excerpt text exceeds 65536 bytes"
        );
        let associations = payload
            .get("workstream_excerpts")
            .and_then(serde_json::Value::as_object)
            .context("Knowledge context snapshot v2 requires workstream_excerpts")?;
        anyhow::ensure!(
            associations.len() <= 500,
            "too many Knowledge excerpt workstreams"
        );
        let mut referenced_excerpt_ids = std::collections::BTreeSet::new();
        for (workstream_id, ids) in associations {
            anyhow::ensure!(
                links.contains_key(workstream_id),
                "Knowledge excerpt workstream has no authorized links"
            );
            let ids = ids
                .as_array()
                .context("Knowledge workstream excerpts must be arrays")?;
            anyhow::ensure!(
                !ids.is_empty() && ids.len() <= 4,
                "invalid Knowledge workstream excerpt count"
            );
            let mut workstream_ids = std::collections::BTreeSet::new();
            for id in ids {
                let id = id
                    .as_str()
                    .context("Knowledge excerpt reference must be a string")?;
                anyhow::ensure!(
                    excerpt_paths.contains_key(id),
                    "Knowledge workstream references an unknown excerpt"
                );
                anyhow::ensure!(
                    workstream_ids.insert(id),
                    "duplicate Knowledge excerpt in one workstream"
                );
                referenced_excerpt_ids.insert(id);
            }
        }
        anyhow::ensure!(
            referenced_excerpt_ids.len() == excerpt_paths.len(),
            "Knowledge snapshot contains an unassociated excerpt"
        );
    }
    Ok(())
}

pub(crate) fn valid_canonical_wikilink(link: &str) -> bool {
    link.strip_prefix("[[")
        .and_then(|value| value.strip_suffix("]]"))
        .is_some_and(|target| {
            !target.trim().is_empty()
                && link.len() <= 512
                && !target
                    .chars()
                    .any(|character| matches!(character, '\r' | '\n' | '[' | ']' | '|' | '#' | '^'))
        })
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

fn context_snapshot_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<ContextSnapshot> {
    let local_date: String = row.get(2)?;
    let payload_json: String = row.get(4)?;
    let created_at: String = row.get(5)?;
    Ok(ContextSnapshot {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        local_date: chrono::NaiveDate::parse_from_str(&local_date, "%Y-%m-%d").map_err(
            |error| {
                rusqlite::Error::FromSqlConversionFailure(
                    2,
                    rusqlite::types::Type::Text,
                    error.into(),
                )
            },
        )?,
        snapshot_digest: row.get(3)?,
        payload: serde_json::from_str(&payload_json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, error.into())
        })?,
        created_at: DateTime::parse_from_rfc3339(&created_at)
            .map(|value| value.with_timezone(&Utc))
            .map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    5,
                    rusqlite::types::Type::Text,
                    error.into(),
                )
            })?,
    })
}

fn knowledge_collection_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<KnowledgeCollection> {
    let roots_json: String = row.get(4)?;
    let exclusions_json: String = row.get(5)?;
    Ok(KnowledgeCollection {
        id: row.get(0)?,
        workspace_id: row.get(1)?,
        label: row.get(2)?,
        purpose: row.get(3)?,
        roots: serde_json::from_str(&roots_json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, error.into())
        })?,
        exclusions: serde_json::from_str(&exclusions_json).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, error.into())
        })?,
        enabled: row.get(6)?,
        revision_digest: row.get(7)?,
        created_at: parse_time(row.get::<_, String>(8)?)?,
        updated_at: parse_time(row.get::<_, String>(9)?)?,
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
    use crate::models::{LegacyCutoverImport, LogEventInput, WorkspaceProfile};

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
    fn validates_bounded_v2_knowledge_excerpts_and_keeps_v1_readable() {
        let text = "# Alpha\nStable product context.";
        let digest = format!("{:x}", Sha256::digest(text.as_bytes()));
        let mut payload = serde_json::json!({
            "schema_version": 2,
            "resolver_version": "exact-v2",
            "workstream_links": {"repo:alpha": ["[[Products/Alpha]]"]},
            "used_notes": [{"path": "Products/Alpha.md", "usable_digest": "a".repeat(64)}],
            "excerpts": [{
                "id": "excerpt_alpha",
                "note_path": "Products/Alpha.md",
                "title": "Alpha",
                "text": text,
                "text_digest": digest,
                "reason": "canonical_note_opening"
            }],
            "workstream_excerpts": {"repo:alpha": ["excerpt_alpha"]}
        });
        validate_context_snapshot_payload(&payload).expect("bounded v2 snapshot is valid");

        payload["excerpts"][0]["text"] = serde_json::json!("tampered");
        assert!(validate_context_snapshot_payload(&payload).is_err());

        let v1 = serde_json::json!({
            "schema_version": 1,
            "workstream_links": {}
        });
        validate_context_snapshot_payload(&v1).expect("existing v1 snapshots remain readable");
    }

    #[test]
    fn freezes_context_snapshots_and_binds_them_to_exact_revisions() {
        let store = Store::open(std::env::temp_dir().join(format!(
            "log-inbox-context-snapshot-{}.sqlite3",
            Uuid::new_v4()
        )))
        .expect("store opens");
        let profile = active_profile(&store);
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 9).unwrap();
        store
            .ensure_daily_day(date, "Work Log/2026-09-09.md", None)
            .expect("day exists");
        let event = store
            .insert_event(LogEventInput {
                source: "codex/test".to_owned(),
                level: None,
                timestamp: Some(
                    chrono::DateTime::parse_from_rfc3339("2026-09-09T12:00:00Z")
                        .unwrap()
                        .with_timezone(&Utc),
                ),
                message: "Implemented Alpha".to_owned(),
                metadata: None,
                fingerprint: None,
            })
            .expect("event exists");
        let payload = serde_json::json!({
            "schema_version": 1,
            "resolver_version": "exact-v1",
            "workstream_links": {"repo:alpha": ["[[Products/Alpha]]"]},
            "workstream_evidence": {"repo:alpha": [event.id.clone()]}
        });
        let snapshot = store
            .create_context_snapshot(&profile.id, date, &payload)
            .expect("context freezes");
        let same = store
            .create_context_snapshot(&profile.id, date, &payload)
            .expect("same context is idempotent");
        assert_eq!(same.id, snapshot.id);
        assert_eq!(same.payload, payload);

        let evidence = store
            .create_evidence_snapshot(&profile.id, date, std::slice::from_ref(&event.id))
            .expect("evidence freezes");
        let content = serde_json::json!({
            "schema_version": 1,
            "workstreams": [{
                "id": "repo:alpha",
                "title": "Alpha",
                "evidence_event_ids": [event.id.clone()],
                "canonical_links": ["[[Products/Alpha]]"],
                "outcome": [{"text": "Implemented Alpha", "evidence_event_ids": [event.id.clone()]}],
                "decision": [],
                "trade_off": [],
                "validation": [],
                "blocker": [],
                "follow_up": []
            }],
            "manual_entry_ids": [],
            "open_questions": []
        });
        let revision = store
            .create_proposal_revision_with_context(
                &profile.id,
                date,
                Some(&evidence.id),
                &snapshot.id,
                "generated",
                &content,
            )
            .expect("revision binds context");
        assert_eq!(
            store
                .proposal_context_snapshot(&revision.id)
                .expect("binding reads")
                .expect("binding exists")
                .id,
            snapshot.id
        );
        assert!(
            store
                .connect()
                .unwrap()
                .execute(
                    "UPDATE proposal_context_snapshots SET context_snapshot_id = ?1 WHERE revision_id = ?2",
                    params![snapshot.id, revision.id],
                )
                .is_err()
        );
        let mut unauthorized = content.clone();
        unauthorized["workstreams"][0]["canonical_links"] =
            serde_json::json!(["[[Products/Invented]]"]);
        assert!(
            store
                .create_proposal_revision_if_current_with_context(
                    &profile.id,
                    date,
                    Some(&evidence.id),
                    &snapshot.id,
                    "structured_edit",
                    &unauthorized,
                    &revision.id,
                )
                .unwrap_err()
                .to_string()
                .contains("not authorized")
        );
        assert!(
            store
                .connect()
                .unwrap()
                .execute(
                    "UPDATE context_snapshots SET payload_json = '{}' WHERE id = ?1",
                    params![snapshot.id],
                )
                .is_err()
        );

        let other_date = chrono::NaiveDate::from_ymd_opt(2026, 9, 10).unwrap();
        store
            .ensure_daily_day(other_date, "Work Log/2026-09-10.md", None)
            .unwrap();
        assert!(
            store
                .create_proposal_revision_with_context(
                    &profile.id,
                    other_date,
                    None,
                    &snapshot.id,
                    "manual",
                    &content,
                )
                .unwrap_err()
                .to_string()
                .contains("different day")
        );
    }

    #[test]
    fn rejects_malformed_context_and_cross_workstream_link_reassignment() {
        let store = Store::open(std::env::temp_dir().join(format!(
            "log-inbox-context-authorization-{}.sqlite3",
            Uuid::new_v4()
        )))
        .expect("store opens");
        let profile = active_profile(&store);
        let date = chrono::NaiveDate::from_ymd_opt(2026, 9, 9).unwrap();
        store
            .ensure_daily_day(date, "Work Log/2026-09-09.md", None)
            .unwrap();
        assert!(
            store
                .create_context_snapshot(&profile.id, date, &serde_json::json!({}))
                .unwrap_err()
                .to_string()
                .contains("schema")
        );
        assert!(
            store
                .create_context_snapshot(
                    &profile.id,
                    date,
                    &serde_json::json!({
                        "schema_version": 1,
                        "workstream_links": {"alpha": ["[[Products/Alpha#section]]"]}
                    }),
                )
                .unwrap_err()
                .to_string()
                .contains("wikilink")
        );

        let first = store
            .insert_event(LogEventInput {
                source: "codex/test".to_owned(),
                level: None,
                timestamp: Some("2026-09-09T10:00:00Z".parse().unwrap()),
                message: "Alpha work".to_owned(),
                metadata: None,
                fingerprint: None,
            })
            .unwrap();
        let second = store
            .insert_event(LogEventInput {
                source: "codex/test".to_owned(),
                level: None,
                timestamp: Some("2026-09-09T11:00:00Z".parse().unwrap()),
                message: "Beta work".to_owned(),
                metadata: None,
                fingerprint: None,
            })
            .unwrap();
        let evidence = store
            .create_evidence_snapshot(&profile.id, date, &[first.id.clone(), second.id.clone()])
            .unwrap();
        let context = store
            .create_context_snapshot(
                &profile.id,
                date,
                &serde_json::json!({
                    "schema_version": 1,
                    "workstream_links": {"alpha": ["[[Products/Alpha]]"]},
                    "workstream_evidence": {"alpha": [first.id.clone()]}
                }),
            )
            .unwrap();
        let fact = |text: &str, event_id: &str| serde_json::json!({"text": text, "evidence_event_ids": [event_id]});
        let content = serde_json::json!({
            "schema_version": 1,
            "workstreams": [
                {
                    "id": "alpha", "title": "Alpha",
                    "evidence_event_ids": [second.id.clone()],
                    "canonical_links": ["[[Products/Alpha]]"],
                    "outcome": [fact("Beta relabeled as Alpha", &second.id)],
                    "decision": [], "trade_off": [], "validation": [], "blocker": [], "follow_up": []
                },
                {
                    "id": "beta", "title": "Beta",
                    "evidence_event_ids": [first.id.clone()],
                    "canonical_links": [],
                    "outcome": [fact("Alpha relabeled as Beta", &first.id)],
                    "decision": [], "trade_off": [], "validation": [], "blocker": [], "follow_up": []
                }
            ],
            "manual_entry_ids": [],
            "open_questions": []
        });
        assert!(
            store
                .create_proposal_revision_with_context(
                    &profile.id,
                    date,
                    Some(&evidence.id),
                    &context.id,
                    "structured_edit",
                    &content,
                )
                .unwrap_err()
                .to_string()
                .contains("workstream evidence")
        );
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
        let updated = store
            .update_context_mapping(
                &saved.id,
                &profile.id,
                &[LinkSelector {
                    field: "pull_request".to_owned(),
                    operator: "exact".to_owned(),
                    value: "9374".to_owned(),
                }],
                "Products/Log Inbox.md",
                false,
                saved.updated_at,
            )
            .unwrap();
        assert!(!updated.enabled);
        assert!(
            store
                .update_context_mapping(
                    &updated.id,
                    &profile.id,
                    &updated.selectors,
                    &updated.canonical_note_path,
                    true,
                    saved.updated_at,
                )
                .is_err()
        );

        let ignored = store
            .save_ignored_context_identity(&IgnoredContextIdentity {
                id: "ignored_test".to_owned(),
                workspace_id: profile.id.clone(),
                field: "module".to_owned(),
                value: "Noise".to_owned(),
                normalized_value: "noise".to_owned(),
                source_identity: Some("legacy-ignore:ignore_1".to_owned()),
                source_digest: Some("b".repeat(64)),
                created_at: Utc::now(),
            })
            .unwrap();
        assert_eq!(ignored.normalized_value, "noise");
        assert_eq!(
            store.list_ignored_context_identities(&profile.id).unwrap(),
            vec![ignored.clone()]
        );
        store
            .delete_ignored_context_identity(&ignored.id, &profile.id)
            .unwrap();
        assert!(
            store
                .list_ignored_context_identities(&profile.id)
                .unwrap()
                .is_empty()
        );
        store
            .delete_context_mapping(&updated.id, &profile.id, updated.updated_at)
            .unwrap();
        assert!(store.list_context_mappings(&profile.id).unwrap().is_empty());
    }

    #[test]
    fn manages_bounded_workspace_knowledge_collections_optimistically() {
        let store = Store::open(std::env::temp_dir().join(format!(
            "log-inbox-knowledge-collection-{}.sqlite3",
            Uuid::new_v4()
        )))
        .unwrap();
        let profile = active_profile(&store);
        let saved = store
            .save_knowledge_collection(
                None,
                &profile.id,
                " Product context ",
                " Product behavior and decisions ",
                &["Products/Zeta".to_owned(), "Products/Alpha".to_owned()],
                &["Products/Zeta/Archive".to_owned()],
                true,
                None,
            )
            .unwrap();
        assert_eq!(saved.label, "Product context");
        assert_eq!(saved.purpose, "Product behavior and decisions");
        assert_eq!(
            saved.roots,
            vec!["Products/Alpha".to_owned(), "Products/Zeta".to_owned()]
        );
        assert_eq!(saved.revision_digest.len(), 64);
        assert_eq!(
            store.list_knowledge_collections(&profile.id).unwrap(),
            vec![saved.clone()]
        );
        assert!(
            store
                .save_knowledge_collection(
                    None,
                    &profile.id,
                    "product CONTEXT",
                    "Duplicate label",
                    &["Products".to_owned()],
                    &[],
                    true,
                    None,
                )
                .is_err()
        );
        assert!(
            store
                .save_knowledge_collection(
                    None,
                    &profile.id,
                    "Duplicate roots",
                    "Duplicate normalized paths",
                    &["Products/Alpha".to_owned(), "Products//Alpha".to_owned()],
                    &[],
                    true,
                    None,
                )
                .is_err()
        );

        assert!(
            store
                .save_knowledge_collection(
                    Some(&saved.id),
                    &profile.id,
                    "Product context",
                    "Changed",
                    &["Products".to_owned()],
                    &[],
                    true,
                    None,
                )
                .is_err()
        );
        assert!(
            store
                .save_knowledge_collection(
                    Some(&saved.id),
                    &profile.id,
                    "Product context",
                    "Changed",
                    &["Products".to_owned()],
                    &[],
                    true,
                    Some(DateTime::<Utc>::UNIX_EPOCH),
                )
                .is_err()
        );
        let updated = store
            .save_knowledge_collection(
                Some(&saved.id),
                &profile.id,
                "Product context",
                "Changed",
                &["Products".to_owned()],
                &[],
                false,
                Some(saved.updated_at),
            )
            .unwrap();
        assert!(!updated.enabled);
        assert_ne!(updated.revision_digest, saved.revision_digest);

        for index in 1..8 {
            store
                .save_knowledge_collection(
                    None,
                    &profile.id,
                    &format!("Collection {index}"),
                    "Bounded context",
                    &[".".to_owned()],
                    &[],
                    true,
                    None,
                )
                .unwrap();
        }
        assert!(
            store
                .save_knowledge_collection(
                    None,
                    &profile.id,
                    "Ninth collection",
                    "Too many",
                    &[".".to_owned()],
                    &[],
                    true,
                    None,
                )
                .is_err()
        );
        for invalid in [
            "../escape",
            "/absolute",
            "folder\\windows",
            ".obsidian/private",
        ] {
            assert!(
                store
                    .save_knowledge_collection(
                        Some(&updated.id),
                        &profile.id,
                        "Product context",
                        "Changed",
                        &[invalid.to_owned()],
                        &[],
                        false,
                        Some(updated.updated_at),
                    )
                    .is_err()
            );
        }
        assert!(
            store
                .save_knowledge_collection(
                    Some(&updated.id),
                    &profile.id,
                    "Product context",
                    "Changed",
                    &["Products".to_owned()],
                    &["Engineering".to_owned()],
                    false,
                    Some(updated.updated_at),
                )
                .is_err()
        );
        assert!(
            store
                .delete_knowledge_collection(&updated.id, &profile.id, DateTime::<Utc>::UNIX_EPOCH,)
                .is_err()
        );
        store
            .delete_knowledge_collection(&updated.id, &profile.id, updated.updated_at)
            .unwrap();
        assert!(store.knowledge_collection(&updated.id).unwrap().is_none());
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
