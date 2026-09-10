use crate::daily_writer::strip_managed_daily_blocks;
use log_inbox_core::workspace::WorkspaceMarkdownDocument;
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
    use log_inbox_core::workspace::WorkspaceMarkdownSource;

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
}
