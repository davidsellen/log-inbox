use crate::{proposal_inbox, vault_context::VaultCatalog};
use log_inbox_core::{
    models::{IgnoredLinkIdentity, StoredLogEvent, VaultLinkRule, WorkspaceProfile},
    store::Store,
    workspace::{InspectedWorkspace, MarkdownPathMode},
};
use serde::Serialize;
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
    let report_digest = sha256(&serde_json::to_vec(&body)?);
    Ok(CutoverReport {
        operation_id: format!("refocus_{}", &report_digest[..24]),
        report_digest,
        workspace_id: profile.id.clone(),
        root_binding: workspace.root_binding().to_owned(),
        ready: blockers.is_empty(),
        items,
        blockers,
        warnings,
    })
}

fn inventory_rules(
    rules: Vec<VaultLinkRule>,
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
        let status = if unsupported {
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
        items.push(CutoverItem {
            kind: "context_mapping".to_owned(),
            source_identity,
            source_digest,
            action: if status == "ready" {
                "import"
            } else {
                "preserve"
            }
            .to_owned(),
            status: status.to_owned(),
            details: json!({
                "selectors": rule.selectors,
                "target_note_id": rule.target_note_id,
                "target_path": target_path,
                "enabled": rule.enabled,
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
        let source = json!({
            "id": event.id,
            "timestamp": event.timestamp,
            "source": event.source,
            "message": event.message,
            "metadata": event.metadata,
        });
        items.push(CutoverItem {
            kind: "manual_event".to_owned(),
            source_identity: format!("log_event:{}", event.id),
            source_digest: sha256(&serde_json::to_vec(&source)?),
            action: "import".to_owned(),
            status: "ready".to_owned(),
            details: json!({"event_id": event.id, "timestamp": event.timestamp}),
        });
    }
    Ok(())
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
    use log_inbox_core::models::{LinkSelector, VaultLinkRule};
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
}
