use chrono::Utc;
use log_inbox_core::models::{StoredLogEvent, VaultLinkRule};
use regex::Regex;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    env, fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock},
};

const METADATA_FIELDS: &[(&str, &str)] = &[
    ("repo", "repo"),
    ("project", "project"),
    ("product", "product"),
    ("app", "app"),
    ("service", "service"),
    ("modules", "module"),
    ("module", "module"),
    ("work_item", "work_item"),
    ("branch", "branch"),
];

#[derive(Debug, Clone)]
pub struct VaultContextProvider {
    config_path: Option<PathBuf>,
    product_index_path: Option<PathBuf>,
    vault_dir: Option<PathBuf>,
    excluded_prefixes: Vec<PathBuf>,
    browser_catalog: Arc<Mutex<Option<VaultCatalog>>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultCatalog {
    #[serde(default)]
    pub vault_id: String,
    pub configured: bool,
    pub root: Option<String>,
    pub revision: String,
    pub notes: Vec<VaultNote>,
    #[serde(default, alias = "tree_paths")]
    pub markdown_paths: Vec<String>,
    #[serde(default)]
    pub folder_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VaultNote {
    pub id: String,
    pub title: String,
    pub wikilink: String,
    pub path: String,
    pub group: String,
    pub aliases: Vec<String>,
    pub tags: Vec<String>,
    pub references: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ObservedIdentity {
    pub field: String,
    pub value: String,
    pub event_count: usize,
    pub status: String,
    pub resolved_notes: Vec<String>,
    pub resolution_kind: Option<String>,
    pub suggestions: Vec<LinkSuggestion>,
    pub sample_message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct LinkSuggestion {
    pub note_id: String,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
struct VaultContextConfig {
    #[serde(default = "default_daily_note_format")]
    daily_note_format: String,
    #[serde(default)]
    products: Vec<ProductMapping>,
}

impl Default for VaultContextConfig {
    fn default() -> Self {
        Self {
            daily_note_format: default_daily_note_format(),
            products: Vec::new(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct ProductMapping {
    note: String,
    #[serde(default)]
    aliases: Vec<String>,
}

#[derive(Debug, Deserialize, Default)]
struct Frontmatter {
    #[serde(default, deserialize_with = "one_or_many")]
    aliases: Vec<String>,
    #[serde(default, deserialize_with = "one_or_many")]
    tags: Vec<String>,
    #[serde(flatten)]
    extra: BTreeMap<String, Value>,
}

impl VaultContextProvider {
    pub fn from_env() -> Self {
        let excluded_prefixes = env::var("LOG_INBOX_VAULT_EXCLUDE_PREFIXES")
            .unwrap_or_else(|_| "00 Inbox,01 Work Log,.obsidian".to_owned())
            .split(',')
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .collect();
        Self {
            config_path: env_path("LOG_INBOX_VAULT_CONTEXT_FILE"),
            product_index_path: env_path("LOG_INBOX_PRODUCT_INDEX_FILE"),
            vault_dir: env_path("LOG_INBOX_VAULT_DIR"),
            excluded_prefixes,
            browser_catalog: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_browser_catalog(&self, catalog: Option<VaultCatalog>) -> Result<(), String> {
        *self
            .browser_catalog
            .lock()
            .map_err(|_| "browser vault catalog lock is poisoned".to_owned())? = catalog;
        Ok(())
    }

    pub fn browser_catalog(&self) -> Result<Option<VaultCatalog>, String> {
        self.browser_catalog
            .lock()
            .map(|catalog| catalog.clone())
            .map_err(|_| "browser vault catalog lock is poisoned".to_owned())
    }

    pub fn catalog_from_browser_files(
        &self,
        vault_id: &str,
        name: &str,
        files: &[(String, String)],
        markdown_paths: Vec<String>,
        folder_paths: Vec<String>,
    ) -> Result<VaultCatalog, String> {
        let mut notes = files
            .iter()
            .filter(|(path, _)| {
                let path = Path::new(path);
                path.extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
                    && !path
                        .components()
                        .any(|component| component.as_os_str().to_string_lossy().starts_with('.'))
                    && !self
                        .excluded_prefixes
                        .iter()
                        .any(|prefix| path.starts_with(prefix))
            })
            .map(|(path, contents)| note_from_contents(Path::new(path), contents))
            .collect::<Result<Vec<_>, _>>()?;
        notes.sort_by(|left, right| left.path.cmp(&right.path));
        finish_catalog_links(&mut notes);
        Ok(VaultCatalog {
            vault_id: vault_id.to_owned(),
            configured: true,
            root: Some(name.to_owned()),
            revision: catalog_revision(&notes, &folder_paths),
            notes,
            markdown_paths,
            folder_paths,
        })
    }

    pub fn catalog(&self) -> Result<VaultCatalog, String> {
        if let Some(catalog) = self.browser_catalog()? {
            return Ok(catalog);
        }
        let Some(root) = &self.vault_dir else {
            return self.legacy_catalog();
        };
        let mut files = Vec::new();
        collect_markdown(root, root, &self.excluded_prefixes, &mut files)?;
        files.sort();
        let mut tree_files = Vec::new();
        collect_markdown(root, root, &[], &mut tree_files)?;
        tree_files.sort();
        let markdown_paths = tree_files
            .iter()
            .filter_map(|path| path.strip_prefix(root).ok())
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .collect();
        let mut folder_paths = vec![String::new()];
        collect_folders(root, root, &mut folder_paths)?;
        folder_paths.sort();
        let mut notes = files
            .into_iter()
            .map(|path| read_note(root, &path))
            .collect::<Result<Vec<_>, _>>()?;
        finish_catalog_links(&mut notes);
        Ok(VaultCatalog {
            vault_id: format!("mounted:{}", root.display()),
            configured: true,
            root: Some(root.display().to_string()),
            revision: catalog_revision(&notes, &folder_paths),
            notes,
            markdown_paths,
            folder_paths,
        })
    }

    pub fn for_events(
        &self,
        events: &[StoredLogEvent],
        rules: &[VaultLinkRule],
    ) -> Result<Value, String> {
        let config = self.load_config()?;
        let catalog = self.catalog()?;
        let note_date = events
            .iter()
            .map(|event| event.timestamp)
            .max()
            .unwrap_or_else(Utc::now);
        let mut candidates = BTreeSet::new();

        for event in events {
            let one_event = std::slice::from_ref(event);
            for value in field_values(one_event, "canonical_note")
                .into_iter()
                .chain(field_values(one_event, "canonical_note_candidates"))
            {
                if let Some(note) = find_note(&catalog.notes, &value) {
                    candidates.insert(note.wikilink.clone());
                } else if !catalog.configured {
                    candidates.insert(wikilink(&value));
                }
            }
            let mut matching = rules
                .iter()
                .filter(|rule| rule.enabled && rule_matches(rule, one_event))
                .collect::<Vec<_>>();
            let specificity = matching
                .iter()
                .map(|rule| rule.selectors.len())
                .max()
                .unwrap_or(0);
            matching.retain(|rule| rule.selectors.len() == specificity);
            if !matching.is_empty() {
                for rule in matching {
                    if let Some(note) = catalog
                        .notes
                        .iter()
                        .find(|note| note.id == rule.target_note_id)
                    {
                        candidates.insert(note.wikilink.clone());
                    }
                }
                continue;
            }
            for (_, field) in METADATA_FIELDS {
                for value in field_values(one_event, field) {
                    for note in exact_note_matches(&catalog.notes, &value) {
                        candidates.insert(note.wikilink.clone());
                    }
                }
            }
            for note in exact_note_matches(&catalog.notes, &event.source) {
                candidates.insert(note.wikilink.clone());
            }
        }
        for mapping in config.products {
            if mapping.aliases.iter().any(|alias| {
                all_event_fields().any(|field| event_contains(events, field, "exact", alias))
            }) && let Some(note) = find_note(&catalog.notes, &mapping.note)
            {
                candidates.insert(note.wikilink.clone());
            }
        }
        Ok(json!({
            "daily_note": note_date.format(&config.daily_note_format).to_string(),
            "candidate_notes": candidates,
            "link_context_revision": context_revision(&catalog.revision, rules),
        }))
    }

    pub fn observed(
        &self,
        events: &[StoredLogEvent],
        rules: &[VaultLinkRule],
    ) -> Result<Vec<ObservedIdentity>, String> {
        let catalog = self.catalog()?;
        let mut grouped = BTreeMap::<(String, String), (usize, String)>::new();
        for event in events {
            add_observed(&mut grouped, "source", &event.source, &event.message);
            for (key, field) in METADATA_FIELDS {
                if let Some(value) = event.metadata.get(*key) {
                    for text in scalar_strings(value) {
                        add_observed(&mut grouped, field, &text, &event.message);
                    }
                }
            }
        }
        let mut output = grouped
            .into_iter()
            .map(|((field, value), (event_count, sample_message))| {
                let mut resolved = rules
                    .iter()
                    .filter(|rule| {
                        rule.enabled && selectors_match_identity(&rule.selectors, &field, &value)
                    })
                    .filter_map(|rule| {
                        catalog
                            .notes
                            .iter()
                            .find(|note| note.id == rule.target_note_id)
                    })
                    .map(|note| note.wikilink.clone())
                    .collect::<BTreeSet<_>>();
                let mut resolution_kind = (!resolved.is_empty()).then(|| "rule".to_owned());
                let exact = exact_note_matches(&catalog.notes, &value);
                if resolved.is_empty() && exact.len() == 1 {
                    resolved.insert(exact[0].wikilink.clone());
                    resolution_kind = Some("automatic".to_owned());
                }
                let suggestions = exact
                    .into_iter()
                    .map(|note| LinkSuggestion {
                        note_id: note.id.clone(),
                        reason: "Exact title or alias".to_owned(),
                    })
                    .collect::<Vec<_>>();
                let status = match resolved.len() {
                    0 if suggestions.len() > 1 => "ambiguous",
                    0 => "unresolved",
                    1 => "resolved",
                    _ => "ambiguous",
                };
                ObservedIdentity {
                    field,
                    value,
                    event_count,
                    status: status.to_owned(),
                    resolved_notes: resolved.into_iter().collect(),
                    resolution_kind,
                    suggestions,
                    sample_message: sample_message.chars().take(180).collect(),
                }
            })
            .collect::<Vec<_>>();
        output.sort_by(|a, b| {
            b.event_count
                .cmp(&a.event_count)
                .then_with(|| a.field.cmp(&b.field))
                .then_with(|| a.value.cmp(&b.value))
        });
        Ok(output)
    }

    fn load_config(&self) -> Result<VaultContextConfig, String> {
        let Some(path) = &self.config_path else {
            return Ok(VaultContextConfig::default());
        };
        let contents = fs::read_to_string(path)
            .map_err(|error| format!("reading vault context {}: {error}", path.display()))?;
        serde_json::from_str(&contents)
            .map_err(|error| format!("parsing vault context {}: {error}", path.display()))
    }

    fn legacy_catalog(&self) -> Result<VaultCatalog, String> {
        let Some(path) = &self.product_index_path else {
            return Ok(VaultCatalog {
                vault_id: "unconfigured".to_owned(),
                configured: false,
                root: None,
                revision: "unconfigured".to_owned(),
                notes: Vec::new(),
                markdown_paths: Vec::new(),
                folder_paths: vec![String::new()],
            });
        };
        let contents = fs::read_to_string(path)
            .map_err(|error| format!("reading product navigation {}: {error}", path.display()))?;
        let notes = wiki_link_targets(&contents)
            .into_iter()
            .map(|title| VaultNote {
                id: title.clone(),
                title: title.clone(),
                wikilink: wikilink(&title),
                path: format!("{title}.md"),
                group: String::new(),
                aliases: Vec::new(),
                tags: Vec::new(),
                references: BTreeMap::new(),
            })
            .collect::<Vec<_>>();
        Ok(VaultCatalog {
            vault_id: format!("legacy:{}", path.display()),
            configured: true,
            root: path.parent().map(|value| value.display().to_string()),
            revision: catalog_revision(&notes, &[String::new()]),
            markdown_paths: notes.iter().map(|note| note.path.clone()).collect(),
            folder_paths: vec![String::new()],
            notes,
        })
    }
}

pub fn validate_rule(rule: &VaultLinkRule, catalog: &VaultCatalog) -> Result<(), String> {
    if rule.selectors.is_empty() || rule.selectors.len() > 8 {
        return Err("a mapping requires 1 to 8 selectors".to_owned());
    }
    if !catalog
        .notes
        .iter()
        .any(|note| note.id == rule.target_note_id)
    {
        return Err("mapping target is not present in the vault catalog".to_owned());
    }
    for selector in &rule.selectors {
        if !matches!(
            selector.field.as_str(),
            "source"
                | "repo"
                | "project"
                | "product"
                | "app"
                | "service"
                | "module"
                | "work_item"
                | "branch"
        ) {
            return Err(format!("unsupported selector field {}", selector.field));
        }
        if !matches!(selector.operator.as_str(), "exact" | "prefix") {
            return Err("selector operator must be exact or prefix".to_owned());
        }
        if selector.value.trim().is_empty() || selector.value.len() > 300 {
            return Err("selector value must contain 1 to 300 bytes".to_owned());
        }
    }
    Ok(())
}

fn collect_markdown(
    root: &Path,
    dir: &Path,
    excluded: &[PathBuf],
    output: &mut Vec<PathBuf>,
) -> Result<(), String> {
    for entry in fs::read_dir(dir)
        .map_err(|error| format!("reading vault directory {}: {error}", dir.display()))?
    {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let relative = path.strip_prefix(root).map_err(|error| error.to_string())?;
        if excluded.iter().any(|prefix| relative.starts_with(prefix))
            || entry.file_name().to_string_lossy().starts_with('.')
        {
            continue;
        }
        let kind = entry.file_type().map_err(|error| error.to_string())?;
        if kind.is_dir() {
            collect_markdown(root, &path, excluded, output)?;
        } else if kind.is_file()
            && path
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("md"))
        {
            output.push(path);
        }
    }
    Ok(())
}

fn collect_folders(root: &Path, directory: &Path, folders: &mut Vec<String>) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("reading vault folder {}: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("reading vault folder entry: {error}"))?;
        let path = entry.path();
        let kind = entry
            .file_type()
            .map_err(|error| format!("reading vault folder type: {error}"))?;
        if !kind.is_dir() || entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|error| format!("resolving vault folder {}: {error}", path.display()))?
            .to_string_lossy()
            .replace('\\', "/");
        folders.push(relative);
        collect_folders(root, &path, folders)?;
    }
    Ok(())
}

fn read_note(root: &Path, path: &Path) -> Result<VaultNote, String> {
    let relative = path.strip_prefix(root).map_err(|error| error.to_string())?;
    let contents = fs::read_to_string(path)
        .map_err(|error| format!("reading note {}: {error}", path.display()))?;
    note_from_contents(relative, &contents)
}

fn note_from_contents(relative: &Path, contents: &str) -> Result<VaultNote, String> {
    let id = relative
        .with_extension("")
        .to_string_lossy()
        .replace('\\', "/");
    let title = relative
        .file_stem()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("invalid Markdown filename {}", relative.display()))?
        .to_owned();
    let frontmatter = parse_frontmatter(contents).unwrap_or_default();
    let mut references = BTreeMap::new();
    for key in ["ado", "work_item", "pr", "pull_request"] {
        if let Some(value) = frontmatter.extra.get(key) {
            references.insert(key.to_owned(), scalar_strings(value));
        }
    }
    Ok(VaultNote {
        id,
        title,
        wikilink: String::new(),
        path: relative.to_string_lossy().replace('\\', "/"),
        group: relative
            .components()
            .next()
            .map(|value| value.as_os_str().to_string_lossy().into_owned())
            .unwrap_or_default(),
        aliases: frontmatter.aliases,
        tags: frontmatter.tags,
        references,
    })
}

fn finish_catalog_links(notes: &mut [VaultNote]) {
    let mut counts = HashMap::<String, usize>::new();
    for note in notes.iter() {
        *counts.entry(note.title.clone()).or_default() += 1;
    }
    for note in notes {
        note.wikilink = if counts.get(&note.title).copied().unwrap_or(0) > 1 {
            format!("[[{}]]", note.id)
        } else {
            format!("[[{}]]", note.title)
        };
    }
}

fn parse_frontmatter(contents: &str) -> Option<Frontmatter> {
    let rest = contents.strip_prefix("---\n")?;
    let (yaml, _) = rest.split_once("\n---")?;
    serde_yaml::from_str(yaml).ok()
}
fn one_or_many<'de, D>(deserializer: D) -> Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = Value::deserialize(deserializer)?;
    Ok(scalar_strings(&value))
}
fn scalar_strings(value: &Value) -> Vec<String> {
    match value {
        Value::String(value) => vec![value.clone()],
        Value::Array(values) => values
            .iter()
            .filter_map(|value| value.as_str().map(ToOwned::to_owned))
            .collect(),
        Value::Number(value) => vec![value.to_string()],
        _ => Vec::new(),
    }
}
fn field_values(events: &[StoredLogEvent], field: &str) -> Vec<String> {
    if field == "source" {
        return events.iter().map(|event| event.source.clone()).collect();
    }
    let keys = if field == "module" {
        vec!["module", "modules"]
    } else {
        vec![field]
    };
    events
        .iter()
        .flat_map(|event| {
            keys.iter()
                .filter_map(|key| event.metadata.get(*key))
                .flat_map(scalar_strings)
        })
        .collect()
}
fn all_event_fields() -> impl Iterator<Item = &'static str> {
    std::iter::once("source").chain(METADATA_FIELDS.iter().map(|(_, field)| *field))
}
fn event_contains(events: &[StoredLogEvent], field: &str, operator: &str, expected: &str) -> bool {
    let expected = normalized_identity(expected);
    field_values(events, field).into_iter().any(|value| {
        let value = normalized_identity(&value);
        if operator == "prefix" {
            value.starts_with(&expected)
        } else {
            value == expected
        }
    })
}
fn rule_matches(rule: &VaultLinkRule, events: &[StoredLogEvent]) -> bool {
    rule.selectors.iter().all(|selector| {
        event_contains(events, &selector.field, &selector.operator, &selector.value)
    })
}
fn selectors_match_identity(
    selectors: &[log_inbox_core::models::LinkSelector],
    field: &str,
    value: &str,
) -> bool {
    selectors.len() == 1
        && selectors[0].field == field
        && if selectors[0].operator == "prefix" {
            normalized_identity(value).starts_with(&normalized_identity(&selectors[0].value))
        } else {
            normalized_identity(value) == normalized_identity(&selectors[0].value)
        }
}
fn exact_note_matches<'a>(notes: &'a [VaultNote], value: &str) -> Vec<&'a VaultNote> {
    let raw_value = value;
    let value = normalized_identity(raw_value);
    notes
        .iter()
        .filter(|note| {
            normalized_identity(&note.title) == value
                || normalized_identity(&note.id) == value
                || note
                    .aliases
                    .iter()
                    .any(|alias| normalized_identity(alias) == value)
                || note
                    .references
                    .values()
                    .flatten()
                    .any(|reference| reference_matches(reference, raw_value))
        })
        .collect()
}
fn reference_matches(reference: &str, value: &str) -> bool {
    let reference = normalized_identity(reference);
    reference == normalized_identity(value)
        || value
            .split(|character: char| !character.is_ascii_alphanumeric())
            .any(|token| token.len() >= 3 && normalized_identity(token) == reference)
}
fn find_note<'a>(notes: &'a [VaultNote], value: &str) -> Option<&'a VaultNote> {
    exact_note_matches(notes, value).into_iter().next()
}
fn wikilink(value: &str) -> String {
    if value.starts_with("[[") {
        value.to_owned()
    } else {
        format!("[[{value}]]")
    }
}
pub(crate) fn normalized_identity(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

pub(crate) fn supports_selector_field(field: &str) -> bool {
    all_event_fields().any(|candidate| candidate == field)
}
fn default_daily_note_format() -> String {
    "Daily log %b %-d".to_owned()
}
fn env_path(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}
fn catalog_revision(notes: &[VaultNote], folder_paths: &[String]) -> String {
    let mut bytes = serde_json::to_vec(notes).unwrap_or_default();
    bytes.extend_from_slice(&serde_json::to_vec(folder_paths).unwrap_or_default());
    stable_hash(&bytes)
}
fn context_revision(catalog: &str, rules: &[VaultLinkRule]) -> String {
    let mut bytes = catalog.as_bytes().to_vec();
    for rule in rules.iter().filter(|rule| rule.enabled) {
        bytes.extend_from_slice(rule.id.as_bytes());
        bytes.extend_from_slice(rule.updated_at.to_rfc3339().as_bytes());
    }
    stable_hash(&bytes)
}
fn stable_hash(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}
fn add_observed(
    grouped: &mut BTreeMap<(String, String), (usize, String)>,
    field: &str,
    value: &str,
    message: &str,
) {
    if !value.trim().is_empty() {
        let entry = grouped
            .entry((field.to_owned(), value.to_owned()))
            .or_insert_with(|| (0, message.to_owned()));
        entry.0 += 1;
    }
}

fn wiki_link_targets(markdown: &str) -> BTreeSet<String> {
    static WIKI_LINK: OnceLock<Regex> = OnceLock::new();
    WIKI_LINK
        .get_or_init(|| Regex::new(r"\[\[([^\]]+)\]\]").expect("wiki-link regex is valid"))
        .captures_iter(markdown)
        .filter_map(|capture| capture.get(1))
        .filter_map(|target| {
            let note = target.as_str().split('|').next()?.split('#').next()?.trim();
            (!note.is_empty()).then(|| note.to_owned())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use log_inbox_core::models::LinkSelector;
    use serde_json::{Map, json};
    use std::time::{SystemTime, UNIX_EPOCH};

    fn event(metadata: Map<String, Value>) -> StoredLogEvent {
        StoredLogEvent {
            id: "evt_1".to_owned(),
            received_at: Utc::now(),
            timestamp: Utc::now(),
            source: "agent/host".to_owned(),
            level: "info".to_owned(),
            message: "activity".to_owned(),
            metadata,
            fingerprint: None,
            truncated: false,
            reviewed: false,
        }
    }

    #[test]
    fn reads_aliases_and_array_metadata_from_an_arbitrary_vault() {
        let root = std::env::temp_dir().join(format!(
            "vault-catalog-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("Knowledge/Products")).unwrap();
        fs::write(
            root.join("Knowledge/Products/Customer Portal.md"),
            "---\naliases:\n  - portal-api\ntags:\n  - product/portal\n---\n# Customer Portal\n",
        )
        .unwrap();
        let provider = VaultContextProvider {
            config_path: None,
            product_index_path: None,
            vault_dir: Some(root.clone()),
            excluded_prefixes: Vec::new(),
            browser_catalog: Arc::new(Mutex::new(None)),
        };
        let events = vec![event(Map::from_iter([(
            "product".to_owned(),
            json!(["portal-api", "companion"]),
        )]))];
        assert_eq!(
            provider.for_events(&events, &[]).unwrap()["candidate_notes"],
            json!(["[[Customer Portal]]"])
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn builds_the_same_catalog_shape_from_browser_files() {
        let provider = VaultContextProvider {
            config_path: None,
            product_index_path: None,
            vault_dir: None,
            excluded_prefixes: vec![PathBuf::from("Private")],
            browser_catalog: Arc::new(Mutex::new(None)),
        };
        let catalog = provider
            .catalog_from_browser_files(
                "browser-test",
                "My vault",
                &[
                    (
                        "Projects/Customer Portal.md".to_owned(),
                        "---\naliases: [portal-api]\n---\n# Customer Portal\n".to_owned(),
                    ),
                    ("Private/Secret.md".to_owned(), "# Secret".to_owned()),
                ],
                vec![
                    "Projects/Customer Portal.md".to_owned(),
                    "Private/Secret.md".to_owned(),
                ],
                vec![String::new(), "Projects".to_owned(), "Private".to_owned()],
            )
            .expect("browser catalog builds");

        assert_eq!(catalog.root.as_deref(), Some("My vault"));
        assert_eq!(catalog.vault_id, "browser-test");
        assert_eq!(catalog.notes.len(), 1);
        assert_eq!(catalog.markdown_paths.len(), 2);
        assert_eq!(catalog.folder_paths.len(), 3);
        assert_eq!(catalog.notes[0].aliases, ["portal-api"]);
        assert_eq!(catalog.notes[0].wikilink, "[[Customer Portal]]");
    }

    #[test]
    fn resolves_plural_canonical_note_candidates() {
        let root = std::env::temp_dir().join(format!(
            "vault-candidates-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("Products.md"), "# Products\n").unwrap();
        let provider = VaultContextProvider {
            config_path: None,
            product_index_path: None,
            vault_dir: Some(root.clone()),
            excluded_prefixes: Vec::new(),
            browser_catalog: Arc::new(Mutex::new(None)),
        };
        let events = vec![event(Map::from_iter([(
            "canonical_note_candidates".to_owned(),
            json!(["Products"]),
        )]))];

        assert_eq!(
            provider.for_events(&events, &[]).unwrap()["candidate_notes"],
            json!(["[[Products]]"])
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn applies_generic_conditional_rules() {
        let rule = VaultLinkRule {
            id: "rule_1".to_owned(),
            selectors: vec![
                LinkSelector {
                    field: "repo".to_owned(),
                    operator: "exact".to_owned(),
                    value: "portal-api".to_owned(),
                },
                LinkSelector {
                    field: "branch".to_owned(),
                    operator: "prefix".to_owned(),
                    value: "feature/navigation".to_owned(),
                },
            ],
            target_note_id: "Record Navigation".to_owned(),
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let metadata = Map::from_iter([
            ("repo".to_owned(), json!("portal-api")),
            ("branch".to_owned(), json!("feature/navigation-v2")),
        ]);
        assert!(rule_matches(&rule, &[event(metadata)]));
    }

    #[test]
    fn discovers_wiki_links_without_assuming_folder_names() {
        assert_eq!(
            wiki_link_targets("- [[Customer Portal]]\n- [[Billing#Ops|Billing]]"),
            BTreeSet::from(["Billing".to_owned(), "Customer Portal".to_owned()])
        );
    }

    #[test]
    fn matches_a_reference_inside_composite_metadata() {
        assert!(reference_matches("12345", "12345; follow-up task 67890"));
        assert!(!reference_matches("12345", "follow-up task 67890"));
    }

    #[test]
    fn specific_rules_do_not_hide_other_workstreams() {
        let root = std::env::temp_dir().join(format!(
            "vault-precedence-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("Notes")).unwrap();
        fs::write(
            root.join("Notes/Record Navigation.md"),
            "# Record Navigation\n",
        )
        .unwrap();
        fs::write(
            root.join("Notes/Customer Portal.md"),
            "---\naliases: [portal-api]\n---\n# Customer Portal\n",
        )
        .unwrap();
        let provider = VaultContextProvider {
            config_path: None,
            product_index_path: None,
            vault_dir: Some(root.clone()),
            excluded_prefixes: Vec::new(),
            browser_catalog: Arc::new(Mutex::new(None)),
        };
        let rule = VaultLinkRule {
            id: "rule_navigation".to_owned(),
            selectors: vec![LinkSelector {
                field: "repo".to_owned(),
                operator: "exact".to_owned(),
                value: "navigation-api".to_owned(),
            }],
            target_note_id: "Notes/Record Navigation".to_owned(),
            enabled: true,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };
        let events = vec![
            event(Map::from_iter([(
                "repo".to_owned(),
                json!("navigation-api"),
            )])),
            event(Map::from_iter([("repo".to_owned(), json!("portal-api"))])),
        ];
        assert_eq!(
            provider.for_events(&events, &[rule]).unwrap()["candidate_notes"],
            json!(["[[Customer Portal]]", "[[Record Navigation]]"])
        );
        fs::remove_dir_all(root).unwrap();
    }
}
