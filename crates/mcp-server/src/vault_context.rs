use chrono::Utc;
use log_inbox_core::models::StoredLogEvent;
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeSet, env, fs, path::PathBuf, sync::OnceLock};

const MATCH_METADATA_KEYS: &[&str] = &["product", "repo", "app", "service"];

#[derive(Debug, Clone)]
pub struct VaultContextProvider {
    path: Option<PathBuf>,
    product_index_path: Option<PathBuf>,
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

impl VaultContextProvider {
    pub fn from_env() -> Self {
        Self {
            path: env::var_os("LOG_INBOX_VAULT_CONTEXT_FILE")
                .filter(|path| !path.is_empty())
                .map(PathBuf::from),
            product_index_path: env::var_os("LOG_INBOX_PRODUCT_INDEX_FILE")
                .filter(|path| !path.is_empty())
                .map(PathBuf::from),
        }
    }

    pub fn for_events(&self, events: &[StoredLogEvent]) -> Result<Value, String> {
        let config = self.load()?;
        let note_date = events
            .iter()
            .map(|event| event.timestamp)
            .max()
            .unwrap_or_else(Utc::now);
        let mut candidate_notes = explicit_notes(events);
        let event_values = match_values(events);
        let navigation_notes = self.navigation_notes()?;

        for note in &navigation_notes {
            if event_values.contains(&note.trim().to_lowercase()) {
                candidate_notes.insert(note.clone());
            }
        }

        for product in config.products {
            if !product.note.trim().is_empty()
                && (navigation_notes.is_empty() || navigation_notes.contains(&product.note))
                && product
                    .aliases
                    .iter()
                    .any(|alias| event_values.contains(&alias.trim().to_lowercase()))
            {
                candidate_notes.insert(product.note);
            }
        }

        Ok(json!({
            "daily_note": note_date.format(&config.daily_note_format).to_string(),
            "candidate_notes": candidate_notes,
        }))
    }

    fn navigation_notes(&self) -> Result<BTreeSet<String>, String> {
        let Some(path) = &self.product_index_path else {
            return Ok(BTreeSet::new());
        };
        let contents = fs::read_to_string(path)
            .map_err(|error| format!("reading product navigation {}: {error}", path.display()))?;
        Ok(wiki_link_targets(&contents))
    }

    fn load(&self) -> Result<VaultContextConfig, String> {
        let Some(path) = &self.path else {
            return Ok(VaultContextConfig::default());
        };
        let contents = fs::read_to_string(path)
            .map_err(|error| format!("reading vault context {}: {error}", path.display()))?;
        serde_json::from_str(&contents)
            .map_err(|error| format!("parsing vault context {}: {error}", path.display()))
    }
}

fn explicit_notes(events: &[StoredLogEvent]) -> BTreeSet<String> {
    events
        .iter()
        .filter_map(|event| event.metadata.get("canonical_note"))
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

fn match_values(events: &[StoredLogEvent]) -> BTreeSet<String> {
    let mut values = BTreeSet::new();
    for event in events {
        values.insert(event.source.trim().to_lowercase());
        for key in MATCH_METADATA_KEYS {
            if let Some(value) = event.metadata.get(*key).and_then(Value::as_str) {
                values.insert(value.trim().to_lowercase());
            }
        }
    }
    values
}

fn default_daily_note_format() -> String {
    "Daily log %b %-d".to_owned()
}

impl Default for VaultContextProvider {
    fn default() -> Self {
        Self {
            path: None,
            product_index_path: None,
        }
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
    use chrono::Utc;
    use serde_json::Map;

    fn event(metadata: Map<String, Value>) -> StoredLogEvent {
        StoredLogEvent {
            id: "evt_1".to_owned(),
            received_at: Utc::now(),
            timestamp: Utc::now(),
            source: "windows/iis".to_owned(),
            level: "info".to_owned(),
            message: "activity".to_owned(),
            metadata,
            fingerprint: None,
            truncated: false,
            reviewed: false,
        }
    }

    #[test]
    fn matches_user_configured_product_aliases() {
        let events = vec![event(Map::from_iter([(
            "repo".to_owned(),
            Value::from("customer-portal"),
        )]))];
        let config = VaultContextConfig {
            daily_note_format: "Work %Y-%m-%d".to_owned(),
            products: vec![ProductMapping {
                note: "Customer Portal".to_owned(),
                aliases: vec!["customer-portal".to_owned()],
            }],
        };
        let values = match_values(&events);

        assert!(
            config.products[0]
                .aliases
                .iter()
                .any(|alias| values.contains(&alias.to_lowercase()))
        );
    }

    #[test]
    fn discovers_existing_notes_from_markdown_navigation() {
        let notes = wiki_link_targets(
            "# Products\n- [[Customer Portal]]\n- [[Billing#Operations|Billing ops]]\n",
        );

        assert_eq!(
            notes,
            BTreeSet::from(["Billing".to_owned(), "Customer Portal".to_owned()])
        );
    }

    #[test]
    fn keeps_explicit_notes_without_inventing_defaults() {
        let events = vec![event(Map::from_iter([(
            "canonical_note".to_owned(),
            Value::from("Operations"),
        )]))];
        let context = VaultContextProvider::default()
            .for_events(&events)
            .expect("default context builds");

        assert_eq!(context["candidate_notes"], json!(["Operations"]));
        assert!(
            context["daily_note"]
                .as_str()
                .is_some_and(|note| note.starts_with("Daily log "))
        );
    }
}
