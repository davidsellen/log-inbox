use crate::daily_writer::strip_managed_daily_blocks;
use crate::llm;
use log_inbox_core::{
    models::{ContextMapping, KnowledgeCollection, StoredLogEvent},
    workspace::{InspectedWorkspace, MarkdownPathMode, WorkspaceMarkdownDocument},
};
use serde::Serialize;
use serde_json::{Value as JsonValue, json};
use serde_yaml::{Mapping, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

const MAX_TITLE_BYTES: usize = 200;
const MAX_ALIAS_COUNT: usize = 32;
const MAX_REFERENCE_COUNT: usize = 64;
const MAX_IDENTITY_BYTES: usize = 300;
const MAX_CATALOG_NOTES: usize = 2_000;
const MAX_NOTE_BYTES: u64 = 1024 * 1024;
const MAX_CATALOG_BYTES: u64 = 32 * 1024 * 1024;
const MAX_CONTEXT_SNAPSHOT_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct KnowledgeResolution {
    pub snapshot_payload: JsonValue,
    pub vault_context: JsonValue,
}

pub fn context_snapshot_digest(payload: &JsonValue) -> Result<String, String> {
    let bytes = serde_json::to_vec(payload).map_err(|error| error.to_string())?;
    if bytes.len() > MAX_CONTEXT_SNAPSHOT_BYTES {
        return Err(format!(
            "Knowledge context snapshot exceeds {MAX_CONTEXT_SNAPSHOT_BYTES} bytes"
        ));
    }
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

pub fn configuration_digest(
    workspace: &InspectedWorkspace,
    collections: &[KnowledgeCollection],
    mappings: &[ContextMapping],
) -> Result<String, String> {
    let mut hasher = Sha256::new();
    digest_part(&mut hasher, workspace.root_binding().as_bytes());
    let mut collections = collections
        .iter()
        .filter(|collection| collection.enabled)
        .collect::<Vec<_>>();
    collections.sort_by(|left, right| left.id.cmp(&right.id));
    for collection in collections {
        let bytes = serde_json::to_vec(&json!({
            "id": collection.id,
            "revision_digest": collection.revision_digest,
        }))
        .map_err(|error| error.to_string())?;
        digest_part(&mut hasher, &bytes);
    }
    let mut mappings = mappings
        .iter()
        .filter(|mapping| mapping.enabled)
        .collect::<Vec<_>>();
    mappings.sort_by(|left, right| left.id.cmp(&right.id));
    for mapping in mappings {
        let bytes = serde_json::to_vec(&json!({
            "id": mapping.id,
            "selectors": mapping.selectors,
            "canonical_note_path": mapping.canonical_note_path,
            "source_identity": mapping.source_identity,
            "source_digest": mapping.source_digest,
        }))
        .map_err(|error| error.to_string())?;
        digest_part(&mut hasher, &bytes);
    }
    Ok(format!("{:x}", hasher.finalize()))
}

fn digest_part(hasher: &mut Sha256, bytes: &[u8]) {
    hasher.update((bytes.len() as u64).to_be_bytes());
    hasher.update(bytes);
}

fn json_digest(payload: &JsonValue) -> Result<String, String> {
    serde_json::to_vec(payload)
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
        .map_err(|error| error.to_string())
}

#[derive(Debug, Clone)]
struct CatalogNote {
    note: ParsedKnowledgeNote,
    collection_ids: BTreeSet<String>,
}

struct CatalogBuild {
    notes: BTreeMap<String, CatalogNote>,
    missing_roots: BTreeSet<String>,
    oversized_note_count: usize,
    unreadable_note_count: usize,
    invalid_note_count: usize,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct KnowledgeNoteOption {
    pub path: String,
    pub title: String,
    pub collection_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UnresolvedIdentity {
    pub field: String,
    pub value: String,
    pub normalized_value: String,
    pub group_count: usize,
    pub event_count: usize,
    pub latest_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct UnresolvedIdentityReview {
    pub identities: Vec<UnresolvedIdentity>,
    pub total_count: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedKnowledgeNote {
    pub path: String,
    pub title: String,
    pub aliases: Vec<String>,
    pub references: BTreeMap<String, Vec<String>>,
    pub source_digest: String,
    pub usable_digest: String,
    pub usable_content: String,
}

pub fn parse_knowledge_note(
    document: &WorkspaceMarkdownDocument,
) -> Result<ParsedKnowledgeNote, String> {
    let normalized = document
        .content
        .strip_prefix('\u{feff}')
        .unwrap_or(&document.content)
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let stripped = strip_managed_daily_blocks(&normalized)?;
    let (frontmatter, body) = split_frontmatter(&stripped)?;
    let mapping = frontmatter.as_ref().and_then(Value::as_mapping);
    let title = mapping
        .map(|value| scalar_string(value, "title"))
        .transpose()?
        .flatten()
        .or_else(|| first_heading(body))
        .unwrap_or_else(|| filename_title(&document.source.path));
    validate_identity("Knowledge note title", &title)?;

    let aliases = mapping
        .map(|value| strings(value, "aliases", false))
        .transpose()?
        .unwrap_or_default();
    if aliases.len() > MAX_ALIAS_COUNT {
        return Err(format!(
            "Knowledge note has more than {MAX_ALIAS_COUNT} aliases"
        ));
    }
    for alias in &aliases {
        validate_identity("Knowledge note alias", alias)?;
    }

    let mut references = BTreeMap::new();
    if let Some(mapping) = mapping {
        for (stored_field, keys) in [
            ("repo", &["repo"][..]),
            ("project", &["project"][..]),
            ("product", &["product"][..]),
            ("app", &["app"][..]),
            ("service", &["service"][..]),
            ("module", &["module", "modules"][..]),
            ("work_item", &["work_item", "ado"][..]),
            ("pull_request", &["pull_request", "pr"][..]),
        ] {
            let mut values = Vec::new();
            for key in keys {
                values.extend(strings(mapping, key, true)?);
            }
            values = deduplicate(values);
            if !values.is_empty() {
                references.insert(stored_field.to_owned(), values);
            }
        }
    }
    let reference_count = references.values().map(Vec::len).sum::<usize>();
    if reference_count > MAX_REFERENCE_COUNT {
        return Err(format!(
            "Knowledge note has more than {MAX_REFERENCE_COUNT} references"
        ));
    }
    for reference in references.values().flatten() {
        validate_identity("Knowledge note reference", reference)?;
    }

    let usable_content = body.trim().to_owned();
    let usable_digest = format!("{:x}", Sha256::digest(usable_content.as_bytes()));
    Ok(ParsedKnowledgeNote {
        path: document.source.path.clone(),
        title: title.trim().to_owned(),
        aliases: deduplicate(aliases),
        references,
        source_digest: document.content_digest.clone(),
        usable_digest,
        usable_content,
    })
}

fn build_catalog(
    workspace: &InspectedWorkspace,
    enabled_collections: &[&KnowledgeCollection],
) -> Result<CatalogBuild, String> {
    let mut notes = BTreeMap::<String, CatalogNote>::new();
    let mut seen_paths = BTreeSet::new();
    let mut missing_roots = BTreeSet::new();
    let mut oversized_note_count = 0_usize;
    let mut unreadable_note_count = 0_usize;
    let mut invalid_note_count = 0_usize;
    let mut catalog_bytes = 0_u64;
    for collection in enabled_collections {
        let selection = workspace
            .preview_markdown_sources(&collection.roots, &collection.exclusions, MAX_CATALOG_NOTES)
            .map_err(|error| error.to_string())?;
        missing_roots.extend(selection.missing_roots);
        for source in selection.sources {
            if let Some(existing) = notes.get_mut(&source.path) {
                existing.collection_ids.insert(collection.id.clone());
                continue;
            }
            if !seen_paths.insert(source.path.clone()) {
                continue;
            }
            if notes.len() >= MAX_CATALOG_NOTES {
                return Err(format!(
                    "Knowledge catalog exceeds {MAX_CATALOG_NOTES} unique notes"
                ));
            }
            if source.byte_len > MAX_NOTE_BYTES {
                oversized_note_count += 1;
                continue;
            }
            catalog_bytes = catalog_bytes
                .checked_add(source.byte_len)
                .ok_or_else(|| "Knowledge catalog byte count overflow".to_owned())?;
            if catalog_bytes > MAX_CATALOG_BYTES {
                return Err(format!(
                    "Knowledge catalog exceeds {MAX_CATALOG_BYTES} source bytes"
                ));
            }
            let document =
                match workspace.read_markdown_source(Path::new(&source.path), MAX_NOTE_BYTES) {
                    Ok(document) => document,
                    Err(_) => {
                        unreadable_note_count += 1;
                        continue;
                    }
                };
            let mut note = match parse_knowledge_note(&document) {
                Ok(note) => note,
                Err(_) => {
                    invalid_note_count += 1;
                    continue;
                }
            };
            note.usable_content.clear();
            notes.insert(
                source.path,
                CatalogNote {
                    note,
                    collection_ids: BTreeSet::from([collection.id.clone()]),
                },
            );
        }
    }
    Ok(CatalogBuild {
        notes,
        missing_roots,
        oversized_note_count,
        unreadable_note_count,
        invalid_note_count,
    })
}

pub fn search_note_options(
    workspace: &InspectedWorkspace,
    collections: &[KnowledgeCollection],
    query: &str,
    limit: usize,
) -> Result<Vec<KnowledgeNoteOption>, String> {
    let mut enabled = collections
        .iter()
        .filter(|collection| collection.enabled)
        .collect::<Vec<_>>();
    enabled.sort_by(|left, right| left.id.cmp(&right.id));
    let catalog = build_catalog(workspace, &enabled)?.notes;
    let query = normalize_identity(query);
    let mut matches = catalog
        .values()
        .filter(|entry| {
            normalize_identity(&entry.note.title).contains(&query)
                || normalize_identity(&entry.note.path).contains(&query)
                || entry
                    .note
                    .aliases
                    .iter()
                    .any(|alias| normalize_identity(alias).contains(&query))
        })
        .map(|entry| KnowledgeNoteOption {
            path: entry.note.path.clone(),
            title: entry.note.title.clone(),
            collection_ids: entry.collection_ids.iter().cloned().collect(),
        })
        .collect::<Vec<_>>();
    matches.sort_by(|left, right| {
        normalize_identity(&left.title)
            .cmp(&normalize_identity(&right.title))
            .then_with(|| left.path.cmp(&right.path))
    });
    matches.truncate(limit);
    Ok(matches)
}

pub fn find_note_option(
    workspace: &InspectedWorkspace,
    collections: &[KnowledgeCollection],
    path: &str,
) -> Result<Option<KnowledgeNoteOption>, String> {
    let mut enabled = collections
        .iter()
        .filter(|collection| collection.enabled)
        .collect::<Vec<_>>();
    enabled.sort_by(|left, right| left.id.cmp(&right.id));
    Ok(build_catalog(workspace, &enabled)?
        .notes
        .get(path)
        .map(|entry| KnowledgeNoteOption {
            path: entry.note.path.clone(),
            title: entry.note.title.clone(),
            collection_ids: entry.collection_ids.iter().cloned().collect(),
        }))
}

pub fn curate_unresolved_identities(
    events: &[StoredLogEvent],
    mappings: &[ContextMapping],
    ignored: &[log_inbox_core::models::IgnoredContextIdentity],
    resolution: Option<&KnowledgeResolution>,
    limit: usize,
) -> UnresolvedIdentityReview {
    let resolved_groups = resolution
        .and_then(|resolution| resolution.snapshot_payload.get("resolved_groups"))
        .and_then(JsonValue::as_array)
        .into_iter()
        .flatten()
        .filter_map(|group| group.get("source_group_id").and_then(JsonValue::as_str))
        .collect::<BTreeSet<_>>();
    let mapped = mappings
        .iter()
        .filter(|mapping| mapping.enabled)
        .flat_map(|mapping| mapping.selectors.iter())
        .filter(|selector| selector.operator == "exact")
        .map(|selector| (selector.field.as_str(), normalize_identity(&selector.value)))
        .collect::<BTreeSet<_>>();
    let ignored = ignored
        .iter()
        .map(|identity| (identity.field.as_str(), identity.normalized_value.as_str()))
        .collect::<BTreeSet<_>>();
    let mut candidates = BTreeMap::<(String, String), UnresolvedIdentity>::new();
    let groups = group_events(events);
    for (group_id, group_events) in groups {
        if resolved_groups.contains(group_id.as_str()) {
            continue;
        }
        for (field, value) in
            group_identities(&group_events)
                .into_iter()
                .filter(|(field, value)| {
                    automatic_identity_field(field)
                        && !value.is_empty()
                        && value.len() <= MAX_IDENTITY_BYTES
                })
        {
            let normalized_value = normalize_identity(&value);
            if mapped.contains(&(field.as_str(), normalized_value.clone()))
                || ignored.contains(&(field.as_str(), normalized_value.as_str()))
            {
                continue;
            }
            let latest_at = group_events
                .iter()
                .map(|event| event.timestamp)
                .max()
                .expect("an evidence group is nonempty");
            let candidate = candidates
                .entry((field.clone(), normalized_value.clone()))
                .or_insert_with(|| UnresolvedIdentity {
                    field,
                    value: value.clone(),
                    normalized_value,
                    group_count: 0,
                    event_count: 0,
                    latest_at,
                });
            candidate.group_count += 1;
            candidate.event_count += group_events.len();
            candidate.latest_at = candidate.latest_at.max(latest_at);
            if value.len() < candidate.value.len() {
                candidate.value = value;
            }
        }
    }
    let total_count = candidates.len();
    let mut identities = candidates.into_values().collect::<Vec<_>>();
    identities.sort_by(|left, right| {
        identity_priority(&left.field)
            .cmp(&identity_priority(&right.field))
            .then_with(|| right.event_count.cmp(&left.event_count))
            .then_with(|| left.normalized_value.cmp(&right.normalized_value))
    });
    identities.truncate(limit);
    UnresolvedIdentityReview {
        truncated: total_count > identities.len(),
        identities,
        total_count,
    }
}

fn identity_priority(field: &str) -> usize {
    match field {
        "product" => 0,
        "project" => 1,
        "repo" => 2,
        "app" => 3,
        "service" => 4,
        "module" => 5,
        "work_item" => 6,
        "pull_request" => 7,
        _ => usize::MAX,
    }
}

pub fn resolve_knowledge(
    workspace: &InspectedWorkspace,
    collections: &[KnowledgeCollection],
    mappings: &[ContextMapping],
    events: &[StoredLogEvent],
) -> Result<Option<KnowledgeResolution>, String> {
    let mut enabled_collections = collections
        .iter()
        .filter(|collection| collection.enabled)
        .collect::<Vec<_>>();
    enabled_collections.sort_by(|left, right| left.id.cmp(&right.id));
    let mut enabled_mappings = mappings
        .iter()
        .filter(|mapping| mapping.enabled)
        .collect::<Vec<_>>();
    enabled_mappings.sort_by(|left, right| left.id.cmp(&right.id));
    if enabled_collections.is_empty() && enabled_mappings.is_empty() {
        return Ok(None);
    }

    let CatalogBuild {
        notes: catalog,
        missing_roots,
        oversized_note_count,
        unreadable_note_count,
        invalid_note_count,
    } = build_catalog(workspace, &enabled_collections)?;

    let raw_groups = group_events(events);
    let mut group_aliases = BTreeMap::new();
    let mut workstream_links = BTreeMap::<String, Vec<String>>::new();
    let mut workstream_evidence = BTreeMap::<String, Vec<String>>::new();
    let mut used_note_paths = BTreeSet::new();
    let mut resolved = Vec::new();
    let mut ambiguous_group_count = 0_usize;
    let mut invalid_mapping_count = 0_usize;

    for (raw_group_id, group_events) in &raw_groups {
        let resolution = resolve_group(
            workspace,
            &catalog,
            &enabled_mappings,
            group_events,
            &mut invalid_mapping_count,
        );
        match resolution {
            GroupResolution::Resolved {
                path,
                reason,
                merge_group,
                matched_fields,
            } => {
                let canonical_group_id = if merge_group {
                    canonical_group_id(&path)
                } else {
                    raw_group_id.clone()
                };
                let Some(link) = path_wikilink(&path) else {
                    invalid_mapping_count += 1;
                    continue;
                };
                if canonical_group_id != *raw_group_id {
                    group_aliases.insert(raw_group_id.clone(), canonical_group_id.clone());
                }
                workstream_links
                    .entry(canonical_group_id.clone())
                    .or_default()
                    .push(link.clone());
                workstream_evidence
                    .entry(canonical_group_id.clone())
                    .or_default()
                    .extend(group_events.iter().map(|event| event.id.clone()));
                used_note_paths.insert(path.clone());
                resolved.push(json!({
                    "source_group_id": raw_group_id,
                    "canonical_group_id": canonical_group_id,
                    "canonical_note_path": path,
                    "canonical_link": link,
                    "reason": reason,
                    "matched_fields": matched_fields,
                }));
            }
            GroupResolution::Ambiguous => ambiguous_group_count += 1,
            GroupResolution::Unresolved => {}
        }
    }
    for links in workstream_links.values_mut() {
        links.sort();
        links.dedup();
    }
    for event_ids in workstream_evidence.values_mut() {
        event_ids.sort();
        event_ids.dedup();
    }
    resolved.sort_by(|left, right| {
        left.get("source_group_id")
            .and_then(JsonValue::as_str)
            .cmp(&right.get("source_group_id").and_then(JsonValue::as_str))
    });
    let candidate_notes = workstream_links
        .values()
        .flatten()
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    let catalog_revision = catalog
        .values()
        .map(|entry| {
            json!({
                "path": entry.note.path,
                "title": entry.note.title,
                "aliases": entry.note.aliases,
                "references": entry.note.references,
                "collection_ids": entry.collection_ids,
            })
        })
        .collect::<Vec<_>>();
    let catalog_digest = json_digest(&JsonValue::Array(catalog_revision))?;
    let used_note_manifest = used_note_paths
        .iter()
        .filter_map(|path| catalog.get(path))
        .map(|entry| {
            let resolution_digest = json_digest(&json!({
                "path": entry.note.path,
                "title": entry.note.title,
                "aliases": entry.note.aliases,
                "references": entry.note.references,
            }))
            .expect("parsed Knowledge metadata serializes");
            json!({
                "path": entry.note.path,
                "title": entry.note.title,
                "resolution_digest": resolution_digest,
                "collection_ids": entry.collection_ids,
            })
        })
        .collect::<Vec<_>>();
    let collection_manifest = enabled_collections
        .iter()
        .map(|collection| {
            json!({
                "id": collection.id,
                "revision_digest": collection.revision_digest,
            })
        })
        .collect::<Vec<_>>();
    let mapping_manifest = enabled_mappings
        .iter()
        .map(|mapping| {
            let revision_digest = json_digest(&json!({
                "selectors": mapping.selectors,
                "canonical_note_path": mapping.canonical_note_path,
                "source_identity": mapping.source_identity,
                "source_digest": mapping.source_digest,
            }))
            .expect("stored Knowledge mapping serializes");
            json!({
                "id": mapping.id,
                "revision_digest": revision_digest,
            })
        })
        .collect::<Vec<_>>();
    let configuration_digest = configuration_digest(workspace, collections, mappings)?;
    let snapshot_payload = json!({
        "schema_version": 1,
        "resolver_version": "exact-v1",
        "root_binding": workspace.root_binding(),
        "configuration_digest": configuration_digest,
        "collections": collection_manifest,
        "mappings": mapping_manifest,
        "catalog_digest": catalog_digest,
        "catalog_note_count": catalog.len(),
        "used_notes": used_note_manifest,
        "resolved_groups": resolved,
        "group_aliases": group_aliases,
        "workstream_links": workstream_links,
        "workstream_evidence": workstream_evidence,
        "diagnostics": {
            "missing_roots": missing_roots,
            "oversized_note_count": oversized_note_count,
            "unreadable_note_count": unreadable_note_count,
            "invalid_note_count": invalid_note_count,
            "invalid_mapping_count": invalid_mapping_count,
            "ambiguous_group_count": ambiguous_group_count,
        }
    });
    context_snapshot_digest(&snapshot_payload)?;
    let vault_context = json!({
        "candidate_notes": candidate_notes,
        "workstream_links": workstream_links,
        "group_aliases": group_aliases,
        "knowledge": {
            "resolver_version": "exact-v1",
            "context_is_background_only": true,
            "excerpts": [],
        }
    });
    Ok(Some(KnowledgeResolution {
        snapshot_payload,
        vault_context,
    }))
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum GroupResolution {
    Resolved {
        path: String,
        reason: &'static str,
        merge_group: bool,
        matched_fields: Vec<String>,
    },
    Ambiguous,
    Unresolved,
}

fn resolve_group(
    workspace: &InspectedWorkspace,
    catalog: &BTreeMap<String, CatalogNote>,
    mappings: &[&ContextMapping],
    events: &[&StoredLogEvent],
    invalid_mapping_count: &mut usize,
) -> GroupResolution {
    let mut matching = mappings
        .iter()
        .filter(|mapping| mapping_matches(mapping, events))
        .copied()
        .collect::<Vec<_>>();
    let specificity = matching
        .iter()
        .map(|mapping| {
            (
                mapping.selectors.len(),
                mapping
                    .selectors
                    .iter()
                    .filter(|selector| selector.operator == "exact")
                    .count(),
            )
        })
        .max()
        .unwrap_or_default();
    matching.retain(|mapping| {
        (
            mapping.selectors.len(),
            mapping
                .selectors
                .iter()
                .filter(|selector| selector.operator == "exact")
                .count(),
        ) == specificity
    });
    let mut mapped_paths = BTreeSet::new();
    let mut mappings_allowing_merge = 0_usize;
    let mut matched_mapping_fields = BTreeSet::new();
    let has_reviewed_match = !matching.is_empty();
    let mut invalid_reviewed_match = false;
    for mapping in matching {
        if workspace
            .resolve_markdown_path(
                Path::new(&mapping.canonical_note_path),
                MarkdownPathMode::ExistingFile,
            )
            .is_ok()
        {
            mapped_paths.insert(mapping.canonical_note_path.clone());
            if mapping
                .selectors
                .iter()
                .any(|selector| matches!(selector.field.as_str(), "work_item" | "pull_request"))
            {
                mappings_allowing_merge += 1;
            }
            matched_mapping_fields.extend(
                mapping
                    .selectors
                    .iter()
                    .map(|selector| selector.field.clone()),
            );
        } else {
            *invalid_mapping_count += 1;
            invalid_reviewed_match = true;
        }
    }
    if invalid_reviewed_match {
        return GroupResolution::Unresolved;
    }
    if mapped_paths.len() == 1 {
        return GroupResolution::Resolved {
            path: mapped_paths.into_iter().next().expect("one mapped path"),
            reason: "saved_mapping",
            merge_group: mappings_allowing_merge > 0,
            matched_fields: matched_mapping_fields.into_iter().collect(),
        };
    }
    if mapped_paths.len() > 1 {
        return GroupResolution::Ambiguous;
    }
    if has_reviewed_match {
        return GroupResolution::Unresolved;
    }

    let identities = group_identities(events)
        .into_iter()
        .filter(|(field, _)| automatic_identity_field(field))
        .collect::<Vec<_>>();
    let mut matches = BTreeMap::<String, (BTreeSet<&'static str>, BTreeSet<String>)>::new();
    for entry in catalog.values() {
        for (field, value) in &identities {
            let normalized = normalize_identity(value);
            if normalized == normalize_identity(&entry.note.title) {
                let matched = matches.entry(entry.note.path.clone()).or_default();
                matched.0.insert("exact_title");
                matched.1.insert(field.clone());
            }
            if entry
                .note
                .aliases
                .iter()
                .any(|alias| normalize_identity(alias) == normalized)
            {
                let matched = matches.entry(entry.note.path.clone()).or_default();
                matched.0.insert("exact_alias");
                matched.1.insert(field.clone());
            }
            if entry.note.references.get(field).is_some_and(|references| {
                references
                    .iter()
                    .any(|reference| normalize_identity(reference) == normalized)
            }) {
                let matched = matches.entry(entry.note.path.clone()).or_default();
                matched.0.insert("exact_reference");
                matched.1.insert(field.clone());
            }
        }
    }
    if matches.len() == 1 {
        let (path, (reasons, fields)) = matches.into_iter().next().expect("one exact path");
        let reason = ["exact_title", "exact_alias", "exact_reference"]
            .into_iter()
            .find(|reason| reasons.contains(reason))
            .expect("exact match has a reason");
        let merge_group = fields
            .iter()
            .any(|field| matches!(field.as_str(), "work_item" | "pull_request"));
        return GroupResolution::Resolved {
            path,
            reason,
            merge_group,
            matched_fields: fields.into_iter().collect(),
        };
    }
    if matches.len() > 1 {
        return GroupResolution::Ambiguous;
    }
    GroupResolution::Unresolved
}

fn group_events(events: &[StoredLogEvent]) -> BTreeMap<String, Vec<&StoredLogEvent>> {
    let mut groups = BTreeMap::<String, Vec<&StoredLogEvent>>::new();
    for event in events {
        groups
            .entry(llm::event_group_key(event))
            .or_default()
            .push(event);
    }
    groups
}

fn mapping_matches(mapping: &ContextMapping, events: &[&StoredLogEvent]) -> bool {
    events.iter().any(|event| {
        mapping
            .selectors
            .iter()
            .all(|selector| selector_matches_event(selector, event))
    })
}

fn selector_matches_event(
    selector: &log_inbox_core::models::LinkSelector,
    event: &StoredLogEvent,
) -> bool {
    let expected = normalize_identity(&selector.value);
    group_identities(&[event])
        .iter()
        .any(|(candidate_field, value)| {
            if candidate_field != &selector.field {
                return false;
            }
            let candidate = normalize_identity(value);
            match selector.operator.as_str() {
                "exact" => candidate == expected,
                "contains" => candidate.contains(&expected),
                _ => false,
            }
        })
}

fn group_identities(events: &[&StoredLogEvent]) -> Vec<(String, String)> {
    let mut identities = BTreeSet::new();
    for event in events {
        identities.insert(("source".to_owned(), event.source.trim().to_owned()));
        for (source_field, field) in [
            ("repo", "repo"),
            ("project", "project"),
            ("product", "product"),
            ("app", "app"),
            ("service", "service"),
            ("module", "module"),
            ("modules", "module"),
            ("work_item", "work_item"),
            ("pull_request", "pull_request"),
            ("branch", "branch"),
        ] {
            if let Some(value) = event.metadata.get(source_field) {
                for value in json_scalar_strings(value) {
                    identities.insert((field.to_owned(), value));
                }
            }
        }
    }
    identities
        .into_iter()
        .filter(|(_, value)| !value.is_empty())
        .collect()
}

fn automatic_identity_field(field: &str) -> bool {
    matches!(
        field,
        "repo"
            | "project"
            | "product"
            | "app"
            | "service"
            | "module"
            | "work_item"
            | "pull_request"
    )
}

fn json_scalar_strings(value: &JsonValue) -> Vec<String> {
    match value {
        JsonValue::String(value) => vec![value.trim().to_owned()],
        JsonValue::Number(value) => vec![value.to_string()],
        JsonValue::Array(values) => values.iter().flat_map(json_scalar_strings).collect(),
        _ => Vec::new(),
    }
}

fn normalize_identity(value: &str) -> String {
    value.trim().to_lowercase()
}

fn canonical_group_id(path: &str) -> String {
    let digest = format!("{:x}", Sha256::digest(path.as_bytes()));
    format!("canonical:{}", &digest[..24])
}

fn path_wikilink(path: &str) -> Option<String> {
    let path = path.strip_suffix(".md")?;
    (!path.is_empty()
        && !path
            .chars()
            .any(|character| matches!(character, '\r' | '\n' | '[' | ']' | '|' | '#' | '^'))
        && path.len() + 4 <= 512)
        .then(|| format!("[[{path}]]"))
}

fn split_frontmatter(text: &str) -> Result<(Option<Value>, &str), String> {
    let Some(rest) = text.strip_prefix("---\n") else {
        return Ok((None, text));
    };
    let (yaml, body) = if let Some(end) = rest.find("\n---\n") {
        (&rest[..end], &rest[end + 5..])
    } else if let Some(yaml) = rest.strip_suffix("\n---") {
        (yaml, "")
    } else {
        return Err("Knowledge note frontmatter is not terminated".to_owned());
    };
    let value = serde_yaml::from_str::<Value>(yaml)
        .map_err(|error| format!("Knowledge note frontmatter is invalid: {error}"))?;
    if !value.is_mapping() && !value.is_null() {
        return Err("Knowledge note frontmatter must be a mapping".to_owned());
    }
    Ok((Some(value), body))
}

fn scalar_string(mapping: &Mapping, key: &str) -> Result<Option<String>, String> {
    let Some(value) = mapping.get(Value::String(key.to_owned())) else {
        return Ok(None);
    };
    match value {
        Value::String(value) => Ok(Some(value.trim().to_owned())),
        _ => Err(format!("Knowledge note {key} must be text")),
    }
}

fn strings(mapping: &Mapping, key: &str, allow_number: bool) -> Result<Vec<String>, String> {
    let Some(value) = mapping.get(Value::String(key.to_owned())) else {
        return Ok(Vec::new());
    };
    let values = match value {
        Value::Sequence(values) => values,
        value => std::slice::from_ref(value),
    };
    let mut output = Vec::new();
    for value in values {
        let value = match value {
            Value::String(value) => Ok(value.trim().to_owned()),
            Value::Number(value) if allow_number => Ok(value.to_string()),
            _ => Err(format!(
                "Knowledge note {key} must contain only {}",
                if allow_number {
                    "text or numbers"
                } else {
                    "text"
                }
            )),
        }?;
        if !value.is_empty() {
            output.push(value);
        }
    }
    Ok(output)
}

fn first_heading(body: &str) -> Option<String> {
    let mut fence = None;
    for line in body.lines() {
        let trimmed = line.trim();
        let marker = trimmed
            .strip_prefix("```")
            .map(|_| '`')
            .or_else(|| trimmed.strip_prefix("~~~").map(|_| '~'));
        if let Some(character) = marker {
            match fence {
                None => fence = Some(character),
                Some(active) if active == character => fence = None,
                Some(_) => {}
            }
        } else if fence.is_none()
            && let Some(title) = trimmed.strip_prefix("# ")
            && !title.trim().is_empty()
        {
            return Some(title.trim().to_owned());
        }
    }
    None
}

fn filename_title(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or(path)
        .to_owned()
}

fn validate_identity(label: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() || value.len() > MAX_IDENTITY_BYTES {
        return Err(format!("{label} must contain 1-{MAX_IDENTITY_BYTES} bytes"));
    }
    if label == "Knowledge note title" && value.len() > MAX_TITLE_BYTES {
        return Err(format!(
            "Knowledge note title must contain 1-{MAX_TITLE_BYTES} bytes"
        ));
    }
    Ok(())
}

fn deduplicate(values: Vec<String>) -> Vec<String> {
    let mut seen = BTreeSet::new();
    values
        .into_iter()
        .filter(|value| seen.insert(value.trim().to_lowercase()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use log_inbox_core::models::LinkSelector;
    use log_inbox_core::workspace::WorkspaceMarkdownSource;
    use serde_json::{Map, json};
    use std::{fs, path::PathBuf, time::SystemTime};

    fn document(path: &str, content: &str) -> WorkspaceMarkdownDocument {
        WorkspaceMarkdownDocument {
            source: WorkspaceMarkdownSource {
                path: path.to_owned(),
                byte_len: content.len() as u64,
            },
            content: content.to_owned(),
            content_digest: format!("{:x}", Sha256::digest(content.as_bytes())),
        }
    }

    fn workspace(notes: &[(&str, &str)]) -> (PathBuf, InspectedWorkspace) {
        let nonce = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("log-inbox-knowledge-resolver-{nonce}"));
        for (path, content) in notes {
            let path = root.join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
        let inspected = InspectedWorkspace::inspect(&root).unwrap();
        (root, inspected)
    }

    fn collection(root_binding: &str) -> KnowledgeCollection {
        KnowledgeCollection {
            id: "collection_products".to_owned(),
            workspace_id: root_binding.to_owned(),
            label: "Products".to_owned(),
            purpose: "Product facts".to_owned(),
            roots: vec!["Products".to_owned()],
            exclusions: Vec::new(),
            enabled: true,
            revision_digest: "a".repeat(64),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn mapping(field: &str, value: &str, path: &str) -> ContextMapping {
        mapping_with_operator(field, "exact", value, path)
    }

    fn mapping_with_operator(
        field: &str,
        operator: &str,
        value: &str,
        path: &str,
    ) -> ContextMapping {
        ContextMapping {
            id: format!("mapping_{field}_{value}"),
            workspace_id: "workspace".to_owned(),
            selectors: vec![LinkSelector {
                field: field.to_owned(),
                operator: operator.to_owned(),
                value: value.to_owned(),
            }],
            canonical_note_path: path.to_owned(),
            enabled: true,
            source_identity: None,
            source_digest: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    fn event(id: &str, repo: &str, work_item: &str) -> StoredLogEvent {
        StoredLogEvent {
            id: id.to_owned(),
            received_at: Utc::now(),
            timestamp: Utc::now(),
            source: "codex/test".to_owned(),
            level: "info".to_owned(),
            message: "Implemented a change".to_owned(),
            metadata: Map::from_iter([
                ("repo".to_owned(), json!(repo)),
                ("work_item".to_owned(), json!(work_item)),
            ]),
            fingerprint: None,
            truncated: false,
            reviewed: false,
        }
    }

    #[test]
    fn parses_bom_crlf_frontmatter_aliases_references_and_heading() {
        let note = parse_knowledge_note(&document(
            "Products/Alpha.md",
            "\u{feff}---\r\ntitle: Alpha Product\r\naliases: [Alpha, alpha, A] \r\nrepo: alpha-api\r\nado: [57950, 'ADO 57951']\r\n---\r\n# Ignored heading\r\nUseful background.\r\n",
        ))
        .expect("note parses");
        assert_eq!(note.title, "Alpha Product");
        assert_eq!(note.aliases, ["Alpha", "A"]);
        assert_eq!(note.references["repo"], ["alpha-api"]);
        assert_eq!(note.references["work_item"], ["57950", "ADO 57951"]);
        assert_eq!(note.usable_content, "# Ignored heading\nUseful background.");
    }

    #[test]
    fn falls_back_from_heading_to_filename_and_ignores_fenced_headings() {
        let heading = parse_knowledge_note(&document(
            "Engineering/System.md",
            "```md\n# Example\n```\n# Actual system\nBody",
        ))
        .expect("heading parses");
        assert_eq!(heading.title, "Actual system");

        let filename = parse_knowledge_note(&document("Engineering/System.md", "Body only"))
            .expect("filename parses");
        assert_eq!(filename.title, "System");
    }

    #[test]
    fn removes_generated_daily_content_from_usable_context() {
        let note = parse_knowledge_note(&document(
            "Product.md",
            "# Product\n\nOwned context.\n\n<!-- log-inbox:daily:day1:begin -->\nGenerated claim.\n<!-- log-inbox:daily:day1:end -->\n",
        ))
        .expect("note parses");
        assert!(note.usable_content.contains("Owned context."));
        assert!(!note.usable_content.contains("Generated claim."));
    }

    #[test]
    fn rejects_malformed_frontmatter_and_marker_structure() {
        assert!(parse_knowledge_note(&document("Bad.md", "---\naliases: [broken\nBody")).is_err());
        assert!(
            parse_knowledge_note(&document(
                "Bad.md",
                "<!-- log-inbox:daily:a:begin -->\nGenerated"
            ))
            .is_err()
        );
    }

    #[test]
    fn saved_exact_mapping_precedes_automatic_title_matching() {
        let (_root, workspace) = workspace(&[
            ("Products/Alpha.md", "# Alpha\n"),
            ("Products/Beta.md", "# Beta\n"),
        ]);
        let resolution = resolve_knowledge(
            &workspace,
            &[collection("workspace")],
            &[mapping("repo", "Alpha", "Products/Beta.md")],
            &[event("evt_1", "Alpha", "10")],
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            resolution.vault_context["candidate_notes"],
            json!(["[[Products/Beta]]"])
        );
        assert_eq!(
            resolution.snapshot_payload["resolved_groups"][0]["reason"],
            "saved_mapping"
        );
    }

    #[test]
    fn aliases_raw_groups_that_resolve_to_one_canonical_note() {
        let (_root, workspace) = workspace(&[("Products/Alpha.md", "# Alpha\n")]);
        let resolution = resolve_knowledge(
            &workspace,
            &[collection("workspace")],
            &[
                mapping("work_item", "10", "Products/Alpha.md"),
                mapping("work_item", "11", "Products/Alpha.md"),
            ],
            &[
                event("evt_1", "repo-one", "10"),
                event("evt_2", "repo-two", "11"),
            ],
        )
        .unwrap()
        .unwrap();
        let aliases = resolution.vault_context["group_aliases"]
            .as_object()
            .unwrap()
            .values()
            .filter_map(JsonValue::as_str)
            .collect::<BTreeSet<_>>();
        assert_eq!(aliases.len(), 1);
        assert_eq!(
            resolution.vault_context["candidate_notes"],
            json!(["[[Products/Alpha]]"])
        );
    }

    #[test]
    fn ambiguous_exact_aliases_never_authorize_a_link() {
        let (_root, workspace) = workspace(&[
            ("Products/One.md", "---\naliases: [shared]\n---\n# One\n"),
            ("Products/Two.md", "---\naliases: [Shared]\n---\n# Two\n"),
        ]);
        let resolution = resolve_knowledge(
            &workspace,
            &[collection("workspace")],
            &[],
            &[event("evt_1", "shared", "10")],
        )
        .unwrap()
        .unwrap();
        assert_eq!(resolution.vault_context["candidate_notes"], json!([]));
        assert_eq!(
            resolution.snapshot_payload["diagnostics"]["ambiguous_group_count"],
            1
        );
    }

    #[test]
    fn legacy_contains_mapping_remains_explicit_and_deterministic() {
        let (_root, workspace) = workspace(&[("Products/Alpha.md", "# Alpha\n")]);
        let resolution = resolve_knowledge(
            &workspace,
            &[collection("workspace")],
            &[mapping_with_operator(
                "repo",
                "contains",
                "alpha",
                "Products/Alpha.md",
            )],
            &[event("evt_1", "team-alpha-api", "10")],
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            resolution.vault_context["candidate_notes"],
            json!(["[[Products/Alpha]]"])
        );
        assert_eq!(
            resolution.snapshot_payload["diagnostics"]["invalid_mapping_count"],
            0
        );
    }

    #[test]
    fn broad_mapping_links_without_merging_distinct_workstreams() {
        let (_root, workspace) = workspace(&[("Products/Alpha.md", "# Alpha\n")]);
        let resolution = resolve_knowledge(
            &workspace,
            &[collection("workspace")],
            &[mapping("repo", "alpha", "Products/Alpha.md")],
            &[event("evt_1", "alpha", "10"), event("evt_2", "alpha", "11")],
        )
        .unwrap()
        .unwrap();
        assert_eq!(resolution.vault_context["group_aliases"], json!({}));
        assert_eq!(
            resolution.snapshot_payload["resolved_groups"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn snapshot_keeps_only_used_note_metadata_and_catalog_digest() {
        let (_root, workspace) = workspace(&[
            ("Products/Alpha.md", "# Alpha\n"),
            (
                "Products/Unused.md",
                "---\naliases: [private-unused-alias]\n---\n# Unused\n",
            ),
        ]);
        let resolution = resolve_knowledge(
            &workspace,
            &[collection("workspace")],
            &[],
            &[event("evt_1", "Alpha", "10")],
        )
        .unwrap()
        .unwrap();
        assert!(resolution.snapshot_payload.get("notes").is_none());
        assert_eq!(resolution.snapshot_payload["catalog_note_count"], 2);
        assert_eq!(
            resolution.snapshot_payload["used_notes"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert!(
            !resolution
                .snapshot_payload
                .to_string()
                .contains("private-unused-alias")
        );
    }

    #[test]
    fn exact_matching_is_ambiguous_across_title_alias_and_reference_kinds() {
        let (_root, workspace) = workspace(&[
            ("Products/Title.md", "# alpha\n"),
            (
                "Products/Alias.md",
                "---\naliases: [alpha]\n---\n# Different\n",
            ),
        ]);
        let resolution = resolve_knowledge(
            &workspace,
            &[collection("workspace")],
            &[],
            &[event("evt_1", "alpha", "10")],
        )
        .unwrap()
        .unwrap();
        assert_eq!(resolution.vault_context["candidate_notes"], json!([]));
        assert_eq!(
            resolution.snapshot_payload["diagnostics"]["ambiguous_group_count"],
            1
        );
    }

    #[test]
    fn invalid_reviewed_mapping_never_falls_through_to_an_automatic_match() {
        let (_root, workspace) = workspace(&[("Products/Alpha.md", "# Alpha\n")]);
        let resolution = resolve_knowledge(
            &workspace,
            &[collection("workspace")],
            &[mapping("repo", "Alpha", "Products/Missing.md")],
            &[event("evt_1", "Alpha", "10")],
        )
        .unwrap()
        .unwrap();
        assert_eq!(resolution.vault_context["candidate_notes"], json!([]));
        assert_eq!(
            resolution.snapshot_payload["diagnostics"]["invalid_mapping_count"],
            1
        );
    }

    #[test]
    fn mapping_selector_conjunction_must_match_one_event() {
        let (_root, workspace) = workspace(&[("Products/Alpha.md", "# Alpha\n")]);
        let mut mapping = mapping("product", "unrelated-product", "Products/Alpha.md");
        mapping.selectors.push(LinkSelector {
            field: "module".to_owned(),
            operator: "exact".to_owned(),
            value: "chat".to_owned(),
        });
        let mut first = event("evt_1", "unused", "10");
        first.metadata = Map::from_iter([
            ("task_id".to_owned(), json!("same-task")),
            ("product".to_owned(), json!("unrelated-product")),
        ]);
        let mut second = event("evt_2", "unused", "11");
        second.metadata = Map::from_iter([
            ("task_id".to_owned(), json!("same-task")),
            ("module".to_owned(), json!("chat")),
        ]);
        let resolution = resolve_knowledge(
            &workspace,
            &[collection("workspace")],
            &[mapping],
            &[first, second],
        )
        .unwrap()
        .unwrap();
        assert_eq!(resolution.vault_context["candidate_notes"], json!([]));
    }

    #[test]
    fn exact_work_item_references_can_merge_explicit_alias_groups() {
        let (_root, workspace) = workspace(&[(
            "Products/Alpha.md",
            "---\nado: ['10', '11']\n---\n# Alpha\n",
        )]);
        let resolution = resolve_knowledge(
            &workspace,
            &[collection("workspace")],
            &[],
            &[
                event("evt_1", "repo-one", "10"),
                event("evt_2", "repo-two", "11"),
            ],
        )
        .unwrap()
        .unwrap();
        assert_eq!(
            resolution.vault_context["group_aliases"]
                .as_object()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            resolution.snapshot_payload["workstream_evidence"]
                .as_object()
                .unwrap()
                .values()
                .next()
                .and_then(JsonValue::as_array)
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn transient_source_and_branch_names_do_not_create_automatic_links() {
        let (_root, workspace) = workspace(&[
            ("Products/Codex.md", "# codex/test\n"),
            ("Products/Branch.md", "# bugfix/123\n"),
        ]);
        let mut event = event("evt_1", "unmatched", "10");
        event
            .metadata
            .insert("branch".to_owned(), json!("bugfix/123"));
        let resolution = resolve_knowledge(&workspace, &[collection("workspace")], &[], &[event])
            .unwrap()
            .unwrap();
        assert_eq!(resolution.vault_context["candidate_notes"], json!([]));
    }
}
