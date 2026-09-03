use log_inbox_core::models::StoredLogEvent;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{borrow::Cow, collections::HashSet, time::Duration};

const MAX_PROMPT_MESSAGE_BYTES: usize = 16 * 1024;
const MAX_PROMPT_METADATA_BYTES: usize = 8 * 1024;
const LLM_REQUEST_TIMEOUT: Duration = Duration::from_secs(120);
const CONTEXT_METADATA_KEYS: &[&str] = &[
    "task_id",
    "session_id",
    "event_type",
    "sequence",
    "repo",
    "product",
    "app",
    "service",
    "branch",
    "activity",
    "sender",
    "status",
    "artifact_path",
    "artifact_sha256",
    "canonical_note",
];

#[derive(Debug, Clone)]
pub struct LlmConfig {
    pub base_url: String,
    pub api_key: Option<String>,
    pub model: String,
}

impl LlmConfig {
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("LOG_INBOX_LLM_BASE_URL").ok()?;
        let model = std::env::var("LOG_INBOX_LLM_MODEL").unwrap_or_else(|_| "llama3.1".to_owned());
        let api_key = std::env::var("LOG_INBOX_LLM_API_KEY")
            .ok()
            .filter(|key| !key.trim().is_empty());

        Some(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            api_key,
            model,
        })
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct SuggestMarkdownSummaryArgs {
    pub event_ids: Vec<String>,
    #[serde(default)]
    pub vault_context: Value,
    #[serde(default = "default_mode")]
    pub mode: String,
    #[serde(default)]
    pub task: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct SummaryProposal {
    pub target_note: String,
    pub canonical_links: Vec<String>,
    pub markdown: String,
    pub evidence_event_ids: Vec<String>,
    pub confidence: String,
    pub open_questions: Vec<String>,
    pub requires_review: bool,
    pub provider: String,
}

#[derive(Debug, Deserialize)]
struct ChatResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatMessage,
}

#[derive(Debug, Deserialize)]
struct ChatMessage {
    content: String,
}

#[derive(Serialize)]
struct PromptEvent<'a> {
    id: &'a str,
    timestamp: &'a chrono::DateTime<chrono::Utc>,
    source: &'a str,
    level: &'a str,
    message: &'a str,
    message_complete: bool,
    metadata: Cow<'a, Map<String, Value>>,
    fingerprint: Option<&'a str>,
}

pub async fn suggest_markdown_summary(
    config: Option<&LlmConfig>,
    args: SuggestMarkdownSummaryArgs,
    events: Vec<StoredLogEvent>,
) -> Result<SummaryProposal, String> {
    if events.is_empty() {
        return Err("suggest_markdown_summary requires at least one event".to_owned());
    }

    let Some(config) = config else {
        return Ok(fallback_proposal(
            args,
            events,
            "not_configured",
            "LLM is not configured. Set LOG_INBOX_LLM_BASE_URL and LOG_INBOX_LLM_MODEL.",
        ));
    };

    let prompt = build_prompt(&args, &events)?;
    let client = reqwest::Client::builder()
        .timeout(LLM_REQUEST_TIMEOUT)
        .build()
        .map_err(|error| error.to_string())?;
    let mut request = client
        .post(format!("{}/chat/completions", config.base_url))
        .json(&json!({
            "model": config.model,
            "temperature": 0.2,
            "response_format": { "type": "json_object" },
            "messages": [
                {
                    "role": "system",
                    "content": "You summarize bounded redacted log events into concise Markdown proposals. Return only JSON matching the requested schema. Never include raw stack traces, secrets, or long log dumps."
                },
                {
                    "role": "user",
                    "content": prompt
                }
            ]
        }));

    if let Some(api_key) = &config.api_key {
        request = request.bearer_auth(api_key);
    }

    let response = request.send().await.map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        let body = response.text().await.unwrap_or_default();
        return Err(format!("LLM request failed with {status}: {body}"));
    }

    let chat: ChatResponse = response.json().await.map_err(|error| error.to_string())?;
    let content = chat
        .choices
        .first()
        .map(|choice| choice.message.content.as_str())
        .ok_or_else(|| "LLM response did not include a choice".to_owned())?;

    parse_proposal(content, &args, &events, &config.base_url)
}

fn build_prompt(
    args: &SuggestMarkdownSummaryArgs,
    events: &[StoredLogEvent],
) -> Result<String, String> {
    let prompt_events = events.iter().map(prompt_event).collect::<Vec<_>>();
    let event_slice =
        serde_json::to_string_pretty(&prompt_events).map_err(|error| error.to_string())?;
    let vault_context =
        serde_json::to_string_pretty(&args.vault_context).map_err(|error| error.to_string())?;
    let allowed_links =
        serde_json::to_string(&allowed_canonical_links(args)).map_err(|error| error.to_string())?;

    Ok(format!(
        r#"Task: {task}
Mode: {mode}

Vault context:
{vault_context}

Allowed canonical links:
{allowed_links}

Events:
{event_slice}

Return JSON with this exact shape:
{{
  "target_note": "Daily log Sep 3",
  "canonical_links": [],
  "markdown": "- Concise conclusion that belongs in a Markdown vault.",
  "evidence_event_ids": ["evt_..."],
  "confidence": "low|medium|high",
  "open_questions": []
}}

Rules:
- Use only the supplied events and vault context.
- canonical_links may contain only exact values from Allowed canonical links.
- Keep markdown concise; no raw log dumps.
- A false message_complete or metadata _prompt_notice means full evidence remains in SQLite but was bounded for this model call.
- Include source, time window, and event IDs in a Details line when useful.
- If uncertain, say so and include open questions.
"#,
        task = args
            .task
            .as_deref()
            .unwrap_or("Summarize selected log events for review."),
        mode = args.mode,
    ))
}

fn prompt_event(event: &StoredLogEvent) -> PromptEvent<'_> {
    let (message, message_complete) = bounded_prefix(&event.message, MAX_PROMPT_MESSAGE_BYTES);
    PromptEvent {
        id: &event.id,
        timestamp: &event.timestamp,
        source: &event.source,
        level: &event.level,
        message,
        message_complete,
        metadata: bounded_metadata(&event.metadata),
        fingerprint: event.fingerprint.as_deref(),
    }
}

fn bounded_prefix(value: &str, max_bytes: usize) -> (&str, bool) {
    if value.len() <= max_bytes {
        return (value, true);
    }

    let mut end = max_bytes;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    (&value[..end], false)
}

fn bounded_metadata(metadata: &Map<String, Value>) -> Cow<'_, Map<String, Value>> {
    if serde_json::to_vec(metadata).is_ok_and(|encoded| encoded.len() <= MAX_PROMPT_METADATA_BYTES)
    {
        return Cow::Borrowed(metadata);
    }

    let mut context = Map::new();
    for key in CONTEXT_METADATA_KEYS {
        if let Some(value) = metadata.get(*key) {
            context.insert((*key).to_owned(), value.clone());
        }
    }
    context.insert(
        "_prompt_notice".to_owned(),
        Value::String(
            "metadata bounded for LLM; full redacted metadata remains in SQLite".to_owned(),
        ),
    );
    Cow::Owned(context)
}

fn parse_proposal(
    content: &str,
    args: &SuggestMarkdownSummaryArgs,
    events: &[StoredLogEvent],
    provider: &str,
) -> Result<SummaryProposal, String> {
    let value: Value = serde_json::from_str(content).map_err(|error| {
        format!("LLM did not return valid JSON: {error}; response content was: {content}")
    })?;

    Ok(SummaryProposal {
        target_note: default_target_note(args),
        canonical_links: validated_canonical_links(&value, args),
        markdown: string_field(&value, "markdown")
            .unwrap_or_else(|| fallback_markdown(events, "LLM response omitted markdown.")),
        evidence_event_ids: events.iter().map(|event| event.id.clone()).collect(),
        confidence: string_field(&value, "confidence").unwrap_or_else(|| "low".to_owned()),
        open_questions: string_array_field(&value, "open_questions"),
        requires_review: true,
        provider: provider.to_owned(),
    })
}

fn fallback_proposal(
    args: SuggestMarkdownSummaryArgs,
    events: Vec<StoredLogEvent>,
    provider: &str,
    reason: &str,
) -> SummaryProposal {
    SummaryProposal {
        target_note: default_target_note(&args),
        canonical_links: Vec::new(),
        markdown: fallback_markdown(&events, reason),
        evidence_event_ids: events.into_iter().map(|event| event.id).collect(),
        confidence: "low".to_owned(),
        open_questions: vec![reason.to_owned()],
        requires_review: true,
        provider: provider.to_owned(),
    }
}

fn fallback_markdown(events: &[StoredLogEvent], reason: &str) -> String {
    let first = events.first().expect("fallback requires events");
    let last = events.last().unwrap_or(first);
    let event_ids = events
        .iter()
        .map(|event| event.id.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    format!(
        "- Review {count} `{source}` log events manually. {reason}\n\nDetails: source `{source}` · window `{start}/{end}` · events `{event_ids}`",
        count = events.len(),
        source = first.source,
        start = first.timestamp.to_rfc3339(),
        end = last.timestamp.to_rfc3339(),
    )
}

fn default_target_note(args: &SuggestMarkdownSummaryArgs) -> String {
    args.vault_context
        .get("daily_note")
        .and_then(Value::as_str)
        .unwrap_or("Daily note")
        .to_owned()
}

fn string_field(value: &Value, name: &str) -> Option<String> {
    value
        .get(name)
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn string_array_field(value: &Value, name: &str) -> Vec<String> {
    value
        .get(name)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(ToOwned::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

fn allowed_canonical_links(args: &SuggestMarkdownSummaryArgs) -> Vec<String> {
    args.vault_context
        .get("candidate_notes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(|note| {
            if note.starts_with("[[") && note.ends_with("]]") {
                note.to_owned()
            } else {
                format!("[[{note}]]")
            }
        })
        .collect()
}

fn validated_canonical_links(value: &Value, args: &SuggestMarkdownSummaryArgs) -> Vec<String> {
    let allowed: HashSet<String> = allowed_canonical_links(args).into_iter().collect();
    string_array_field(value, "canonical_links")
        .into_iter()
        .filter(|link| allowed.contains(link))
        .collect()
}

fn default_mode() -> String {
    "daily-note".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::Map;

    #[tokio::test]
    async fn returns_reviewable_fallback_when_llm_is_not_configured() {
        let event = StoredLogEvent {
            id: "evt_test".to_owned(),
            received_at: Utc::now(),
            timestamp: Utc::now(),
            source: "codex/test".to_owned(),
            level: "info".to_owned(),
            message: "Completed a useful task".to_owned(),
            metadata: Map::new(),
            fingerprint: None,
            truncated: false,
            reviewed: false,
        };
        let args = SuggestMarkdownSummaryArgs {
            event_ids: vec![event.id.clone()],
            vault_context: json!({ "daily_note": "Daily log Sep 3" }),
            mode: "daily-note".to_owned(),
            task: None,
        };

        let proposal = suggest_markdown_summary(None, args, vec![event])
            .await
            .expect("fallback proposal succeeds");

        assert_eq!(proposal.target_note, "Daily log Sep 3");
        assert_eq!(proposal.evidence_event_ids, ["evt_test"]);
        assert!(proposal.requires_review);
        assert_eq!(proposal.provider, "not_configured");
    }

    #[test]
    fn keeps_database_event_ids_instead_of_model_supplied_ids() {
        let event = StoredLogEvent {
            id: "evt_real".to_owned(),
            received_at: Utc::now(),
            timestamp: Utc::now(),
            source: "codex/test".to_owned(),
            level: "info".to_owned(),
            message: "Completed a useful task".to_owned(),
            metadata: Map::new(),
            fingerprint: None,
            truncated: false,
            reviewed: false,
        };
        let args = SuggestMarkdownSummaryArgs {
            event_ids: vec![event.id.clone()],
            vault_context: json!({ "daily_note": "Approved daily note" }),
            mode: "daily-note".to_owned(),
            task: None,
        };
        let model_output = json!({
            "target_note": "Model-selected note",
            "markdown": "- A result.",
            "evidence_event_ids": ["evt_invented"],
            "canonical_links": ["[[Invented Note]]"]
        })
        .to_string();

        let proposal =
            parse_proposal(&model_output, &args, &[event], "test").expect("valid proposal parses");

        assert_eq!(proposal.evidence_event_ids, ["evt_real"]);
        assert_eq!(proposal.target_note, "Approved daily note");
        assert!(proposal.canonical_links.is_empty());
    }

    #[test]
    fn bounds_only_the_prompt_projection() {
        let full_message = format!("{}END", "x".repeat(MAX_PROMPT_MESSAGE_BYTES));
        let event = StoredLogEvent {
            id: "evt_large".to_owned(),
            received_at: Utc::now(),
            timestamp: Utc::now(),
            source: "codex/test".to_owned(),
            level: "info".to_owned(),
            message: full_message.clone(),
            metadata: Map::from_iter([
                ("task_id".to_owned(), Value::from("task_123")),
                (
                    "large".to_owned(),
                    Value::from("x".repeat(MAX_PROMPT_METADATA_BYTES)),
                ),
            ]),
            fingerprint: None,
            truncated: false,
            reviewed: false,
        };

        let projected = prompt_event(&event);
        assert!(!projected.message_complete);
        assert!(!projected.message.ends_with("END"));
        assert_eq!(
            projected.metadata.get("task_id"),
            Some(&Value::from("task_123"))
        );
        assert!(projected.metadata.contains_key("_prompt_notice"));
        assert_eq!(event.message, full_message);
    }
}
