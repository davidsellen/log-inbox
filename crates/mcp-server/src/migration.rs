use crate::{proposal_inbox, vault_context::VaultCatalog};
use chrono::Utc;
use log_inbox_core::{
    daily::render_daily_path,
    models::{
        ContextMapping, IgnoredContextIdentity, IgnoredLinkIdentity, LegacyCutoverImport,
        LegacyManualEventImport, LegacyMigrationArtifact, LinkSelector, MigrationItem,
        StoredLogEvent, VaultLinkRule, WorkspaceProfile,
    },
    store::Store,
    workspace::{InspectedWorkspace, MarkdownPathMode},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::Path};

const LEGACY_PREFERENCE_KEYS: &[&str] = &[
    "browser_vault_catalog",
    "consolidation_instructions",
    "daily_consolidation_prompt",
    "ingest_url",
    "agent_name",
    "source_prefix",
    "default_host",
    "extra_instructions",
];
const MAX_PROPOSAL_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CutoverReport {
    pub operation_id: String,
    pub report_digest: String,
    pub workspace_id: String,
    pub root_binding: String,
    pub ready: bool,
    pub cutover_status: String,
    pub completed_operation_id: Option<String>,
    pub items: Vec<CutoverItem>,
    pub blockers: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct CutoverItem {
    pub kind: String,
    pub source_identity: String,
    pub source_digest: String,
    pub action: String,
    pub status: String,
    pub details: Value,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CutoverCommitRequest {
    pub operation_id: String,
    pub report_digest: String,
}

#[derive(Debug, Serialize)]
pub(crate) struct CutoverCommitResult {
    pub operation_id: String,
    pub status: String,
    pub backup_file: String,
    pub imported_items: usize,
    pub cleaned_files: usize,
    pub preserved_items: usize,
}

#[derive(Serialize)]
struct ReportBody<'a> {
    workspace_id: &'a str,
    root_binding: &'a str,
    items: &'a [CutoverItem],
    blockers: &'a [String],
    warnings: &'a [String],
}

pub(crate) fn inventory_cutover(
    store: &Store,
    profile: &WorkspaceProfile,
    workspace: &InspectedWorkspace,
    proposal_dir: Option<&Path>,
) -> anyhow::Result<CutoverReport> {
    anyhow::ensure!(
        profile.root_binding == workspace.root_binding(),
        "active workspace binding changed"
    );
    let preferences = store.get_preferences()?;
    let browser_catalog = preferences
        .get("browser_vault_catalog")
        .and_then(|value| serde_json::from_str::<VaultCatalog>(value).ok());
    let mut items = Vec::new();
    let mut blockers = Vec::new();
    let mut warnings = Vec::new();

    inventory_rules(
        store.list_link_rules()?,
        &store.list_context_mappings(&profile.id)?,
        browser_catalog.as_ref(),
        workspace,
        &mut items,
        &mut blockers,
    )?;
    inventory_ignores(store.list_ignored_link_identities()?, &mut items)?;
    inventory_preferences(&preferences, &mut items, &mut warnings)?;
    inventory_manual_events(store.all_events()?, &mut items)?;
    inventory_proposals(proposal_dir, &mut items, &mut blockers, &mut warnings)?;
    items.sort_by(|left, right| {
        (&left.kind, &left.source_identity).cmp(&(&right.kind, &right.source_identity))
    });
    blockers.sort();
    warnings.sort();

    let body = ReportBody {
        workspace_id: &profile.id,
        root_binding: workspace.root_binding(),
        items: &items,
        blockers: &blockers,
        warnings: &warnings,
    };
    let computed_digest = sha256(&serde_json::to_vec(&body)?);
    let source_identity = format!("legacy-runtime:{}", workspace.root_binding());
    let existing = store.latest_migration_operation("refocus-cutover", &source_identity)?;
    let operation_id = existing
        .as_ref()
        .map(|operation| operation.operation_id.clone())
        .unwrap_or_else(|| format!("refocus_{}", &computed_digest[..24]));
    let report_digest = existing
        .as_ref()
        .and_then(|operation| operation.details["report_digest"].as_str())
        .map(ToOwned::to_owned)
        .unwrap_or(computed_digest);
    let cutover_status = existing
        .as_ref()
        .map(|operation| operation.status.clone())
        .unwrap_or_else(|| "not_started".to_owned());
    Ok(CutoverReport {
        operation_id,
        report_digest,
        workspace_id: profile.id.clone(),
        root_binding: workspace.root_binding().to_owned(),
        ready: blockers.is_empty(),
        cutover_status,
        completed_operation_id: existing
            .as_ref()
            .filter(|operation| operation.status == "completed")
            .map(|operation| operation.operation_id.clone()),
        items,
        blockers,
        warnings,
    })
}

pub(crate) fn commit_cutover(
    store: &Store,
    profile: &WorkspaceProfile,
    workspace: &InspectedWorkspace,
    proposal_dir: Option<&Path>,
    request: &CutoverCommitRequest,
) -> anyhow::Result<CutoverCommitResult> {
    if let Some(operation) = store.migration_operation(&request.operation_id)? {
        anyhow::ensure!(
            operation.migration_name == "refocus-cutover"
                && operation.details["report_digest"] == request.report_digest,
            "migration operation does not match the reviewed report"
        );
        return finish_cutover_cleanup(store, proposal_dir, operation);
    }

    let report = inventory_cutover(store, profile, workspace, proposal_dir)?;
    anyhow::ensure!(
        report.operation_id == request.operation_id
            && report.report_digest == request.report_digest,
        "legacy sources changed after the cutover report was reviewed"
    );
    anyhow::ensure!(report.ready, "cutover report still has blockers");

    let backup_root = store
        .database_path()
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("migration-backups");
    let backup_path = backup_root.join(format!(
        "{}-{}.sqlite3",
        Utc::now().format("%Y%m%dT%H%M%S%.3fZ"),
        request.operation_id
    ));
    anyhow::ensure!(
        !backup_path.starts_with(workspace.canonical_root()),
        "migration backup must remain outside the Markdown workspace"
    );
    let verification = store.create_verified_backup(&backup_path)?;
    anyhow::ensure!(
        verification.integrity_check == "ok",
        "migration backup verification failed"
    );

    let import = prepare_cutover_import(
        store,
        profile,
        proposal_dir,
        &report,
        backup_path.to_string_lossy().as_ref(),
    )?;
    for manual in &import.manual_events {
        let destination = render_daily_path(
            &profile.daily_root,
            &profile.daily_pattern,
            manual.local_date,
        )?;
        store.ensure_daily_day(manual.local_date, &destination, None)?;
    }
    let operation = store.commit_legacy_cutover_import(&import)?;
    finish_cutover_cleanup(store, proposal_dir, operation)
}

fn prepare_cutover_import(
    store: &Store,
    profile: &WorkspaceProfile,
    proposal_dir: Option<&Path>,
    report: &CutoverReport,
    backup_path: &str,
) -> anyhow::Result<LegacyCutoverImport> {
    let preferences = store.get_preferences()?;
    let events = store
        .all_events()?
        .into_iter()
        .map(|event| (event.id.clone(), event))
        .collect::<BTreeMap<_, _>>();
    let timezone = profile.timezone.parse::<chrono_tz::Tz>()?;
    let mut items = Vec::with_capacity(report.items.len());
    let mut mappings = Vec::new();
    let mut ignored = Vec::new();
    let mut artifacts = Vec::new();
    let mut manual_events = Vec::new();
    let mut obsolete_preferences = BTreeMap::new();

    for report_item in &report.items {
        let status = match (report_item.kind.as_str(), report_item.status.as_str()) {
            ("proposal_file", "ready") => "cleanup_pending",
            ("proposal_file", _) => "preserved",
            ("legacy_preference", _) if report_item.action == "preserve" => "preserved",
            _ => "imported",
        };
        items.push(MigrationItem {
            operation_id: report.operation_id.clone(),
            item_kind: report_item.kind.clone(),
            source_identity: report_item.source_identity.clone(),
            source_digest: report_item.source_digest.clone(),
            status: status.to_owned(),
            details: report_item.details.clone(),
            updated_at: Utc::now(),
        });

        match report_item.kind.as_str() {
            "context_mapping" => {
                if report_item.action == "already_present" {
                    continue;
                }
                let selectors = serde_json::from_value::<Vec<LinkSelector>>(
                    report_item.details["selectors"].clone(),
                )?;
                let target_path = report_item.details["target_path"]
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("reviewed context target is missing"))?;
                mappings.push(ContextMapping {
                    id: format!("context_migrated_{}", &report_item.source_digest[..24]),
                    workspace_id: profile.id.clone(),
                    selectors,
                    canonical_note_path: target_path.to_owned(),
                    enabled: report_item.details["enabled"].as_bool().unwrap_or(true),
                    source_identity: Some(report_item.source_identity.clone()),
                    source_digest: Some(report_item.source_digest.clone()),
                    created_at: Utc::now(),
                    updated_at: Utc::now(),
                });
            }
            "ignored_context_identity" => ignored.push(IgnoredContextIdentity {
                id: format!("ignored_migrated_{}", &report_item.source_digest[..24]),
                workspace_id: profile.id.clone(),
                field: required_detail(&report_item.details, "field")?.to_owned(),
                value: required_detail(&report_item.details, "value")?.to_owned(),
                normalized_value: required_detail(&report_item.details, "normalized_value")?
                    .to_owned(),
                source_identity: Some(report_item.source_identity.clone()),
                source_digest: Some(report_item.source_digest.clone()),
                created_at: Utc::now(),
            }),
            "legacy_preference" => {
                let key = required_detail(&report_item.details, "key")?;
                let value = preferences
                    .get(key)
                    .ok_or_else(|| anyhow::anyhow!("legacy preference disappeared: {key}"))?;
                anyhow::ensure!(
                    sha256(value.as_bytes()) == report_item.source_digest,
                    "legacy preference changed: {key}"
                );
                obsolete_preferences.insert(key.to_owned(), value.clone());
                artifacts.push(LegacyMigrationArtifact {
                    operation_id: report.operation_id.clone(),
                    artifact_kind: report_item.kind.clone(),
                    source_identity: report_item.source_identity.clone(),
                    source_digest: report_item.source_digest.clone(),
                    content: value.as_bytes().to_vec(),
                    parse_status: "valid".to_owned(),
                    details: json!({"key": key}),
                    created_at: Utc::now(),
                });
            }
            "proposal_file" => {
                let filename = required_detail(&report_item.details, "filename")?;
                let contents =
                    read_exact_proposal(proposal_dir, filename, &report_item.source_digest)?;
                artifacts.push(LegacyMigrationArtifact {
                    operation_id: report.operation_id.clone(),
                    artifact_kind: report_item.kind.clone(),
                    source_identity: report_item.source_identity.clone(),
                    source_digest: report_item.source_digest.clone(),
                    content: contents,
                    parse_status: if report_item.status == "ready" {
                        "valid"
                    } else {
                        "unparseable"
                    }
                    .to_owned(),
                    details: report_item.details.clone(),
                    created_at: Utc::now(),
                });
            }
            "manual_event" => {
                let event_id = required_detail(&report_item.details, "event_id")?;
                let event = events.get(event_id).ok_or_else(|| {
                    anyhow::anyhow!("legacy manual event disappeared: {event_id}")
                })?;
                anyhow::ensure!(
                    event_source_digest(event)? == report_item.source_digest,
                    "legacy manual event changed: {event_id}"
                );
                manual_events.push(LegacyManualEventImport {
                    event_id: event.id.clone(),
                    local_date: event.timestamp.with_timezone(&timezone).date_naive(),
                    text: event.message.clone(),
                    references: manual_references(event),
                    source_digest: report_item.source_digest.clone(),
                });
            }
            _ => anyhow::bail!("unsupported cutover item kind: {}", report_item.kind),
        }
    }

    Ok(LegacyCutoverImport {
        operation_id: report.operation_id.clone(),
        source_identity: format!("legacy-runtime:{}", report.root_binding),
        report_digest: report.report_digest.clone(),
        workspace_id: profile.id.clone(),
        items,
        mappings,
        ignored,
        artifacts,
        manual_events,
        obsolete_preferences,
        backup_path: backup_path.to_owned(),
    })
}

fn finish_cutover_cleanup(
    store: &Store,
    proposal_dir: Option<&Path>,
    operation: log_inbox_core::models::MigrationJournalEntry,
) -> anyhow::Result<CutoverCommitResult> {
    if operation.status == "completed" {
        return Ok(CutoverCommitResult {
            operation_id: operation.operation_id,
            status: operation.status,
            backup_file: backup_filename(&operation.details),
            imported_items: operation.details["imported_items"].as_u64().unwrap_or(0) as usize,
            cleaned_files: operation.details["cleaned_files"].as_u64().unwrap_or(0) as usize,
            preserved_items: operation.details["preserved_items"].as_u64().unwrap_or(0) as usize,
        });
    }
    let mut cleaned_files = 0;
    for item in store.list_migration_items(&operation.operation_id)? {
        if item.kind_status() != ("proposal_file", "cleanup_pending") {
            continue;
        }
        let filename = required_detail(&item.details, "filename")?;
        let cleanup = read_exact_proposal(proposal_dir, filename, &item.source_digest)
            .and_then(|_| remove_exact_proposal(proposal_dir, filename));
        match cleanup {
            Ok(()) => {
                store.transition_migration_item(
                    &MigrationItem {
                        status: "cleaned".to_owned(),
                        details: json!({"filename": filename}),
                        ..item.clone()
                    },
                    "cleanup_pending",
                )?;
                cleaned_files += 1;
            }
            Err(error) => {
                store.transition_migration_item(
                    &MigrationItem {
                        status: "preserved".to_owned(),
                        details: json!({"filename": filename, "reason": error.to_string()}),
                        ..item.clone()
                    },
                    "cleanup_pending",
                )?;
            }
        }
    }
    let items = store.list_migration_items(&operation.operation_id)?;
    let preserved_items = items
        .iter()
        .filter(|item| item.status == "preserved")
        .count();
    let imported_items = items
        .iter()
        .filter(|item| matches!(item.status.as_str(), "imported" | "cleaned" | "preserved"))
        .count();
    let details = json!({
        "phase": "complete",
        "report_digest": operation.details["report_digest"],
        "workspace_id": operation.details["workspace_id"],
        "backup_path": operation.details["backup_path"],
        "imported_items": imported_items,
        "cleaned_files": cleaned_files,
        "preserved_items": preserved_items,
    });
    let completed =
        store.finish_migration_operation(&operation.operation_id, "completed", &details)?;
    Ok(CutoverCommitResult {
        operation_id: completed.operation_id,
        status: completed.status,
        backup_file: backup_filename(&completed.details),
        imported_items,
        cleaned_files,
        preserved_items,
    })
}

trait MigrationItemStatus {
    fn kind_status(&self) -> (&str, &str);
}

impl MigrationItemStatus for MigrationItem {
    fn kind_status(&self) -> (&str, &str) {
        (&self.item_kind, &self.status)
    }
}

fn required_detail<'a>(details: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    details[key]
        .as_str()
        .ok_or_else(|| anyhow::anyhow!("cutover item is missing {key}"))
}

fn read_exact_proposal(
    proposal_dir: Option<&Path>,
    filename: &str,
    expected_digest: &str,
) -> anyhow::Result<Vec<u8>> {
    let dir =
        proposal_dir.ok_or_else(|| anyhow::anyhow!("legacy proposal directory is missing"))?;
    anyhow::ensure!(
        Path::new(filename)
            .file_name()
            .and_then(|value| value.to_str())
            == Some(filename),
        "legacy proposal filename is unsafe"
    );
    let metadata = fs::symlink_metadata(dir)?;
    anyhow::ensure!(
        metadata.is_dir() && !metadata.file_type().is_symlink(),
        "legacy proposal directory is unsafe"
    );
    let path = dir.join(filename);
    let metadata = fs::symlink_metadata(&path)?;
    anyhow::ensure!(
        metadata.is_file() && !metadata.file_type().is_symlink(),
        "legacy proposal file is unsafe"
    );
    anyhow::ensure!(
        metadata.len() <= MAX_PROPOSAL_BYTES,
        "legacy proposal exceeds 4 MiB"
    );
    let contents = fs::read(path)?;
    anyhow::ensure!(
        sha256(&contents) == expected_digest,
        "legacy proposal changed after review"
    );
    Ok(contents)
}

fn remove_exact_proposal(proposal_dir: Option<&Path>, filename: &str) -> anyhow::Result<()> {
    let dir =
        proposal_dir.ok_or_else(|| anyhow::anyhow!("legacy proposal directory is missing"))?;
    fs::remove_file(dir.join(filename))?;
    Ok(())
}

fn backup_filename(details: &Value) -> String {
    details["backup_path"]
        .as_str()
        .and_then(|path| Path::new(path).file_name())
        .and_then(|value| value.to_str())
        .unwrap_or("migration backup")
        .to_owned()
}

fn inventory_rules(
    rules: Vec<VaultLinkRule>,
    existing_mappings: &[ContextMapping],
    catalog: Option<&VaultCatalog>,
    workspace: &InspectedWorkspace,
    items: &mut Vec<CutoverItem>,
    blockers: &mut Vec<String>,
) -> anyhow::Result<()> {
    for rule in rules {
        let source_identity = format!("vault_link_rule:{}", rule.id);
        let source = json!({
            "id": rule.id,
            "selectors": rule.selectors,
            "target_note_id": rule.target_note_id,
            "enabled": rule.enabled,
        });
        let source_digest = sha256(&serde_json::to_vec(&source)?);
        let unsupported = rule
            .selectors
            .iter()
            .any(|selector| !matches!(selector.operator.as_str(), "exact" | "contains"));
        let matches = catalog
            .map(|catalog| {
                catalog
                    .notes
                    .iter()
                    .filter(|note| note.id == rule.target_note_id)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let target_path = (matches.len() == 1)
            .then(|| matches[0].path.clone())
            .filter(|path| {
                workspace
                    .resolve_markdown_path(Path::new(path), MarkdownPathMode::ExistingFile)
                    .is_ok()
            });
        let existing_mapping = existing_mappings.iter().find(|mapping| {
            let mut existing = mapping.selectors.clone();
            let mut legacy = rule.selectors.clone();
            existing.sort_by(|left, right| {
                (&left.field, &left.operator, &left.value).cmp(&(
                    &right.field,
                    &right.operator,
                    &right.value,
                ))
            });
            legacy.sort_by(|left, right| {
                (&left.field, &left.operator, &left.value).cmp(&(
                    &right.field,
                    &right.operator,
                    &right.value,
                ))
            });
            existing == legacy
        });
        let existing_conflict = existing_mapping.is_some_and(|mapping| {
            target_path
                .as_deref()
                .is_none_or(|path| mapping.canonical_note_path != path)
        });
        let status = if unsupported || existing_conflict {
            "conflict"
        } else if target_path.is_none() {
            "unresolved"
        } else {
            "ready"
        };
        if status != "ready" {
            blockers.push(format!(
                "Legacy link rule {} needs a reviewed canonical note",
                rule.id
            ));
        }
        let action = if status != "ready" {
            "preserve"
        } else if existing_mapping.is_some() {
            "already_present"
        } else {
            "import"
        };
        items.push(CutoverItem {
            kind: "context_mapping".to_owned(),
            source_identity,
            source_digest,
            action: action.to_owned(),
            status: status.to_owned(),
            details: json!({
                "selectors": rule.selectors,
                "target_note_id": rule.target_note_id,
                "target_path": target_path,
                "enabled": rule.enabled,
                "existing_mapping_id": existing_mapping.map(|mapping| mapping.id.clone()),
            }),
        });
    }
    Ok(())
}

fn inventory_ignores(
    ignored: Vec<IgnoredLinkIdentity>,
    items: &mut Vec<CutoverItem>,
) -> anyhow::Result<()> {
    for ignored in ignored {
        let source_identity = format!("ignored_link_identity:{}", ignored.id);
        let source = json!({"id": ignored.id, "field": ignored.field, "value": ignored.value});
        items.push(CutoverItem {
            kind: "ignored_context_identity".to_owned(),
            source_identity,
            source_digest: sha256(&serde_json::to_vec(&source)?),
            action: "import".to_owned(),
            status: "ready".to_owned(),
            details: json!({
                "field": ignored.field,
                "value": ignored.value,
                "normalized_value": ignored.value.trim().to_lowercase(),
            }),
        });
    }
    Ok(())
}

fn inventory_preferences(
    preferences: &BTreeMap<String, String>,
    items: &mut Vec<CutoverItem>,
    warnings: &mut Vec<String>,
) -> anyhow::Result<()> {
    for (key, value) in preferences {
        if !LEGACY_PREFERENCE_KEYS.contains(&key.as_str())
            && !key.starts_with("knowledge_destinations_v1:")
        {
            continue;
        }
        let action = if key.starts_with("knowledge_destinations_v1:") {
            warnings.push(format!(
                "Legacy Structure preference {key} is preserved as a suggestion and will not change the reviewed Daily convention"
            ));
            "preserve"
        } else {
            "remove_after_import"
        };
        items.push(CutoverItem {
            kind: "legacy_preference".to_owned(),
            source_identity: format!("app_preference:{key}"),
            source_digest: sha256(value.as_bytes()),
            action: action.to_owned(),
            status: "ready".to_owned(),
            details: json!({"key": key, "byte_size": value.len()}),
        });
    }
    Ok(())
}

fn inventory_manual_events(
    events: Vec<StoredLogEvent>,
    items: &mut Vec<CutoverItem>,
) -> anyhow::Result<()> {
    for event in events.into_iter().filter(|event| {
        event.source == "manual/dashboard"
            || event.metadata.get("entry_kind").and_then(Value::as_str) == Some("manual")
    }) {
        items.push(CutoverItem {
            kind: "manual_event".to_owned(),
            source_identity: format!("log_event:{}", event.id),
            source_digest: event_source_digest(&event)?,
            action: "import".to_owned(),
            status: "ready".to_owned(),
            details: json!({"event_id": event.id, "timestamp": event.timestamp}),
        });
    }
    Ok(())
}

fn event_source_digest(event: &StoredLogEvent) -> anyhow::Result<String> {
    Ok(sha256(&serde_json::to_vec(&json!({
        "id": event.id,
        "timestamp": event.timestamp,
        "source": event.source,
        "message": event.message,
        "metadata": event.metadata,
    }))?))
}

fn manual_references(event: &StoredLogEvent) -> Vec<String> {
    let mut references = event
        .metadata
        .values()
        .filter_map(Value::as_str)
        .filter(|value| value.starts_with("https://") || value.starts_with("http://"))
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    references.sort();
    references.dedup();
    references.truncate(20);
    references
}

fn inventory_proposals(
    proposal_dir: Option<&Path>,
    items: &mut Vec<CutoverItem>,
    blockers: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> anyhow::Result<()> {
    let Some(proposal_dir) = proposal_dir else {
        return Ok(());
    };
    let metadata = match fs::symlink_metadata(proposal_dir) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        blockers.push("Legacy proposal location is not a safe directory".to_owned());
        return Ok(());
    }
    let mut entries = fs::read_dir(proposal_dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            warnings.push(format!(
                "Unrecognized proposal-directory entry is preserved: {name}"
            ));
            continue;
        }
        if entry.path().extension().and_then(|value| value.to_str()) != Some("md") {
            warnings.push(format!(
                "Non-Markdown proposal-directory file is preserved: {name}"
            ));
            continue;
        }
        if metadata.len() > MAX_PROPOSAL_BYTES {
            blockers.push(format!(
                "Legacy proposal exceeds 4 MiB and is preserved: {name}"
            ));
            continue;
        }
        let contents = fs::read(entry.path())?;
        let digest = sha256(&contents);
        let inspection = proposal_inbox::inspect_legacy_proposal_bytes(&contents);
        let (status, details) = match inspection {
            Ok(proposal) => ("ready", serde_json::to_value(proposal)?),
            Err(error) => {
                warnings.push(format!("Unparseable legacy proposal is preserved: {name}"));
                ("unparseable", json!({"error": error}))
            }
        };
        items.push(CutoverItem {
            kind: "proposal_file".to_owned(),
            source_identity: format!("proposal_file:{name}"),
            source_digest: digest,
            action: "preserve".to_owned(),
            status: status.to_owned(),
            details: json!({
                "filename": name,
                "byte_size": contents.len(),
                "inspection": details,
            }),
        });
    }
    Ok(())
}

fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use log_inbox_core::models::{LinkSelector, LogEventInput, VaultLinkRule};
    use std::path::PathBuf;
    use uuid::Uuid;

    fn temp_dir(label: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("log-inbox-{label}-{}", Uuid::new_v4()));
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn dry_run_inventories_each_legacy_source_without_mutating_it() {
        let root = temp_dir("migration-workspace");
        fs::create_dir_all(root.join("Products")).unwrap();
        fs::write(root.join("Products/Product.md"), "# Product\n").unwrap();
        let workspace = InspectedWorkspace::inspect(&root).unwrap();
        let store = Store::open(root.join("app.sqlite3")).unwrap();
        let profile = store
            .save_active_workspace_profile(
                workspace.root_binding(),
                "UTC",
                "Work Log",
                "{date}.md",
                None,
                "markdown",
                None,
            )
            .unwrap();
        let catalog = VaultCatalog {
            vault_id: "legacy-vault".to_owned(),
            configured: true,
            root: Some("Legacy".to_owned()),
            revision: "legacy-revision".to_owned(),
            notes: vec![crate::vault_context::VaultNote {
                id: "product".to_owned(),
                title: "Product".to_owned(),
                wikilink: "[[Product]]".to_owned(),
                path: "Products/Product.md".to_owned(),
                group: "Products".to_owned(),
                aliases: Vec::new(),
                tags: Vec::new(),
                references: BTreeMap::new(),
            }],
            markdown_paths: vec!["Products/Product.md".to_owned()],
            folder_paths: vec!["Products".to_owned()],
        };
        store
            .set_preferences(&BTreeMap::from([
                (
                    "browser_vault_catalog".to_owned(),
                    serde_json::to_string(&catalog).unwrap(),
                ),
                ("agent_name".to_owned(), "legacy-agent".to_owned()),
            ]))
            .unwrap();
        store
            .save_link_rule(&VaultLinkRule {
                id: "rule_1".to_owned(),
                selectors: vec![LinkSelector {
                    field: "repo".to_owned(),
                    operator: "exact".to_owned(),
                    value: "product".to_owned(),
                }],
                target_note_id: "product".to_owned(),
                enabled: true,
                created_at: Utc::now(),
                updated_at: Utc::now(),
            })
            .unwrap();
        store
            .ignore_link_identity("module", "Noise", "noise")
            .unwrap();

        let proposals = root.join("legacy-proposals");
        fs::create_dir(&proposals).unwrap();
        fs::write(
            proposals.join("valid.md"),
            "---\nproposal_id: proposal_test\ntarget_note: Daily log Sep 9\nevidence_event_ids: []\n---\n# Log summary proposal\n\nUseful text.\n",
        )
        .unwrap();
        fs::write(proposals.join("broken.md"), [0xff, 0xfe]).unwrap();

        let first = inventory_cutover(&store, &profile, &workspace, Some(&proposals)).unwrap();
        let second = inventory_cutover(&store, &profile, &workspace, Some(&proposals)).unwrap();
        assert!(first.ready);
        assert_eq!(first.report_digest, second.report_digest);
        assert_eq!(
            first
                .items
                .iter()
                .filter(|item| item.kind == "proposal_file")
                .count(),
            2
        );
        assert!(first.items.iter().any(|item| {
            item.kind == "context_mapping"
                && item.status == "ready"
                && item.details["target_path"] == "Products/Product.md"
        }));
        assert!(
            first
                .warnings
                .iter()
                .any(|warning| warning.contains("Unparseable"))
        );
        assert_eq!(store.list_link_rules().unwrap().len(), 1);
        assert!(proposals.join("valid.md").exists());
        assert!(
            store
                .list_migration_items(&first.operation_id)
                .unwrap()
                .is_empty()
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reviewed_cutover_backs_up_imports_and_only_cleans_exact_valid_proposals() {
        let app_root = temp_dir("migration-app-data");
        let root = temp_dir("migration-target");
        let workspace = InspectedWorkspace::inspect(&root).unwrap();
        let store = Store::open(app_root.join("log-inbox.sqlite3")).unwrap();
        let profile = store
            .save_active_workspace_profile(
                workspace.root_binding(),
                "UTC",
                "Work Log",
                "{date}.md",
                None,
                "markdown",
                None,
            )
            .unwrap();
        store
            .set_preferences(&BTreeMap::from([(
                "agent_name".to_owned(),
                "legacy-agent".to_owned(),
            )]))
            .unwrap();
        let event = store
            .insert_event(LogEventInput {
                source: "manual/dashboard".to_owned(),
                level: None,
                timestamp: Some("2026-09-09T12:00:00Z".parse().unwrap()),
                message: "Reviewed the release plan.".to_owned(),
                metadata: Some(serde_json::Map::from_iter([(
                    "entry_kind".to_owned(),
                    Value::String("manual".to_owned()),
                )])),
                fingerprint: None,
            })
            .unwrap();
        let proposals = app_root.join("legacy-proposals");
        fs::create_dir(&proposals).unwrap();
        fs::write(
            proposals.join("valid.md"),
            "---\nproposal_id: proposal_test\ntarget_note: Daily log Sep 9\nevidence_event_ids: []\n---\n# Log summary proposal\n\nUseful text.\n",
        )
        .unwrap();
        fs::write(proposals.join("broken.md"), [0xff, 0xfe]).unwrap();

        let report = inventory_cutover(&store, &profile, &workspace, Some(&proposals)).unwrap();
        let valid_item = report
            .items
            .iter()
            .find(|item| item.source_identity == "proposal_file:valid.md")
            .unwrap()
            .clone();
        let broken_item = report
            .items
            .iter()
            .find(|item| item.source_identity == "proposal_file:broken.md")
            .unwrap()
            .clone();
        let request = CutoverCommitRequest {
            operation_id: report.operation_id,
            report_digest: report.report_digest,
        };
        let result = commit_cutover(&store, &profile, &workspace, Some(&proposals), &request)
            .expect("reviewed cutover commits");
        assert_eq!(result.status, "completed");
        assert_eq!(result.cleaned_files, 1);
        assert!(!proposals.join("valid.md").exists());
        assert!(proposals.join("broken.md").exists());
        assert!(!store.get_preferences().unwrap().contains_key("agent_name"));
        assert!(
            store
                .legacy_migration_artifact(
                    &request.operation_id,
                    "proposal_file",
                    &valid_item.source_identity,
                )
                .unwrap()
                .is_some()
        );
        assert!(
            store
                .legacy_migration_artifact(
                    &request.operation_id,
                    "proposal_file",
                    &broken_item.source_identity,
                )
                .unwrap()
                .is_some()
        );
        assert_eq!(
            store
                .manual_daily_entries(&profile.id, "2026-09-09".parse().unwrap())
                .unwrap()[0]
                .text,
            event.message
        );
        assert!(
            app_root
                .join("migration-backups")
                .join(&result.backup_file)
                .exists()
        );

        let retried = commit_cutover(&store, &profile, &workspace, Some(&proposals), &request)
            .expect("completed cutover is idempotent");
        assert_eq!(retried.cleaned_files, 1);
        fs::remove_dir_all(app_root).unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}
