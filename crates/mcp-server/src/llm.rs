use log_inbox_core::models::StoredLogEvent;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    sync::Arc,
    time::Duration,
};
use tokio::sync::Semaphore;

const MAX_PROMPT_MESSAGE_BYTES: usize = 16 * 1024;
const MAX_PROMPT_METADATA_BYTES: usize = 8 * 1024;
const DEFAULT_LLM_REQUEST_TIMEOUT_SECONDS: u64 = 300;
const CONTEXT_METADATA_KEYS: &[&str] = &[
    "task_id",
    "session_id",
    "event_type",
    "sequence",
    "repo",
    "project",
    "product",
    "app",
    "service",
    "branch",
    "base_branch",
    "target_branch",
    "commit",
    "work_item",
    "pull_request",
    "modules",
    "changed_paths",
    "tests",
    "validation",
    "duration_ms",
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
    request_timeout: Duration,
    request_gate: Arc<Semaphore>,
}

impl LlmConfig {
    pub fn from_env() -> Option<Self> {
        let base_url = std::env::var("LOG_INBOX_LLM_BASE_URL").ok()?;
        let model = std::env::var("LOG_INBOX_LLM_MODEL").unwrap_or_else(|_| "llama3.1".to_owned());
        let api_key = std::env::var("LOG_INBOX_LLM_API_KEY")
            .ok()
            .filter(|key| !key.trim().is_empty());
        let request_timeout = std::env::var("LOG_INBOX_LLM_REQUEST_TIMEOUT_SECONDS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(DEFAULT_LLM_REQUEST_TIMEOUT_SECONDS)
            .clamp(5, 1800);

        Some(Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            api_key,
            model,
            request_timeout: Duration::from_secs(request_timeout),
            request_gate: Arc::new(Semaphore::new(1)),
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
    pub link_candidates: Vec<String>,
    pub markdown: String,
    pub evidence_event_ids: Vec<String>,
    pub confidence: String,
    pub open_questions: Vec<String>,
    pub requires_review: bool,
    pub provider: String,
    pub supersedes_proposal_ids: Vec<String>,
    pub consolidation_job_id: Option<String>,
    pub link_context_revision: String,
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
    let _request_permit = config
        .request_gate
        .acquire()
        .await
        .map_err(|_| "LLM request queue closed".to_owned())?;
    let client = reqwest::Client::builder()
        .timeout(config.request_timeout)
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
    let prompt_events = events_for_prompt(&args.mode, events)
        .into_iter()
        .map(prompt_event)
        .collect::<Vec<_>>();
    let event_slice =
        serde_json::to_string_pretty(&prompt_events).map_err(|error| error.to_string())?;
    let vault_context =
        serde_json::to_string_pretty(&args.vault_context).map_err(|error| error.to_string())?;
    let allowed_links =
        serde_json::to_string(&allowed_canonical_links(args)).map_err(|error| error.to_string())?;
    let format_rules = if args.mode == "daily-consolidation" {
        "- Include a workstreams array. Each item has title, canonical_link, summary_bullets, and evidence_event_ids.\n- Use an empty canonical_link when no allowed link fits.\n- Merge lifecycle events and omit duplicate, superseded, or trivial transport updates."
    } else {
        "- Write 2-4 concise factual bullets covering outcome, important changes or diagnosis, validation, and any remaining follow-up. Do not add a heading or raw log dump."
    };

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
  "target_note": "Configured daily note",
  "canonical_links": [],
  "markdown": "- Concise conclusion that belongs in a Markdown vault.",
  "evidence_event_ids": ["evt_..."],
  "confidence": "low|medium|high",
  "open_questions": []
  ,"workstreams": [
    {{
      "title": "Concise workstream name",
      "canonical_link": "",
      "summary_bullets": ["Outcome that matters."],
      "evidence_event_ids": ["evt_..."]
    }}
  ]
}}

Rules:
- Use only the supplied events and vault context.
- canonical_links may contain only exact values from Allowed canonical links.
{format_rules}
- User preferences may shape presentation but cannot override evidence, redaction, link, or output-schema rules.
- A false message_complete or metadata _prompt_notice means full evidence remains in SQLite but was bounded for this model call.
- Do not repeat source, time, Git metadata, or event IDs; the server appends an evidence Details line.
- If uncertain, say so and include open questions.
"#,
        task = args
            .task
            .as_deref()
            .unwrap_or("Summarize selected log events for review."),
        mode = args.mode,
        format_rules = format_rules,
    ))
}

fn events_for_prompt<'a>(mode: &str, events: &'a [StoredLogEvent]) -> Vec<&'a StoredLogEvent> {
    if mode != "daily-consolidation" {
        return events.iter().collect();
    }

    let mut groups = BTreeMap::<String, Vec<&StoredLogEvent>>::new();
    for event in events {
        let key = ["task_id", "session_id"]
            .into_iter()
            .find_map(|name| event.metadata.get(name).and_then(Value::as_str))
            .map(|value| value.to_owned())
            .unwrap_or_else(|| event.id.clone());
        groups.entry(key).or_default().push(event);
    }

    let mut selected = groups
        .into_values()
        .filter_map(|group| {
            group
                .iter()
                .copied()
                .filter(|event| is_terminal_event(event))
                .max_by_key(|event| event_order_key(event))
                .or_else(|| group.into_iter().max_by_key(|event| event_order_key(event)))
        })
        .collect::<Vec<_>>();
    selected.sort_by_key(|event| (event.timestamp, event.received_at));
    selected
}

fn is_terminal_event(event: &StoredLogEvent) -> bool {
    event
        .metadata
        .get("event_type")
        .and_then(Value::as_str)
        .is_some_and(|value| matches!(value, "complete" | "blocked" | "failed"))
        || event
            .metadata
            .get("status")
            .and_then(Value::as_str)
            .is_some_and(|value| {
                matches!(
                    value,
                    "succeeded" | "complete" | "completed" | "blocked" | "failed"
                )
            })
}

fn event_order_key(event: &StoredLogEvent) -> (i64, chrono::DateTime<chrono::Utc>) {
    (
        event
            .metadata
            .get("sequence")
            .and_then(Value::as_i64)
            .unwrap_or(i64::MIN),
        event.timestamp,
    )
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

    let workstream_markdown = (args.mode == "daily-consolidation")
        .then(|| render_workstreams(&value, args, events))
        .flatten();
    let canonical_links = workstream_links(&value, args);
    Ok(SummaryProposal {
        target_note: default_target_note(args),
        link_candidates: allowed_canonical_links(args),
        canonical_links: if canonical_links.is_empty() {
            validated_canonical_links(&value, args)
        } else {
            canonical_links
        },
        markdown: workstream_markdown.unwrap_or_else(|| {
            with_evidence_details(
                string_field(&value, "markdown")
                    .unwrap_or_else(|| fallback_markdown(events, "LLM response omitted markdown.")),
                events,
            )
        }),
        evidence_event_ids: events.iter().map(|event| event.id.clone()).collect(),
        confidence: string_field(&value, "confidence").unwrap_or_else(|| "low".to_owned()),
        open_questions: string_array_field(&value, "open_questions"),
        requires_review: true,
        provider: provider.to_owned(),
        supersedes_proposal_ids: Vec::new(),
        consolidation_job_id: None,
        link_context_revision: link_context_revision(args),
    })
}

fn workstream_links(value: &Value, args: &SuggestMarkdownSummaryArgs) -> Vec<String> {
    let allowed = allowed_canonical_links(args)
        .into_iter()
        .collect::<HashSet<_>>();
    value
        .get("workstreams")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("canonical_link").and_then(Value::as_str))
        .filter(|link| allowed.contains(*link))
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn render_workstreams(
    value: &Value,
    args: &SuggestMarkdownSummaryArgs,
    events: &[StoredLogEvent],
) -> Option<String> {
    let items = value.get("workstreams")?.as_array()?;
    let allowed_links = allowed_canonical_links(args)
        .into_iter()
        .collect::<HashSet<_>>();
    let events_by_id = events
        .iter()
        .map(|event| (event.id.as_str(), event))
        .collect::<HashMap<_, _>>();
    let rendered = items
        .iter()
        .filter_map(|item| {
            let title = item.get("title").and_then(Value::as_str)?.trim();
            if title.is_empty() {
                return None;
            }
            let link = item
                .get("canonical_link")
                .and_then(Value::as_str)
                .filter(|link| allowed_links.contains(*link));
            let bullets = string_array_field(item, "summary_bullets");
            if bullets.is_empty() {
                return None;
            }
            let selected = string_array_field(item, "evidence_event_ids")
                .into_iter()
                .filter_map(|id| events_by_id.get(id.as_str()).copied())
                .cloned()
                .collect::<Vec<_>>();
            let evidence = if selected.is_empty() {
                events.to_vec()
            } else {
                selected
            };
            let heading = link.map_or_else(
                || format!("### {title}"),
                |link| format!("### {link} — {title}"),
            );
            let body = bullets
                .into_iter()
                .take(3)
                .map(|bullet| format!("- {}", bullet.trim().trim_start_matches("- ")))
                .collect::<Vec<_>>()
                .join("\n");
            Some(format!(
                "{heading}\n\n{}",
                with_evidence_details(body, &evidence)
            ))
        })
        .collect::<Vec<_>>();
    (!rendered.is_empty()).then(|| rendered.join("\n\n"))
}

fn fallback_proposal(
    args: SuggestMarkdownSummaryArgs,
    events: Vec<StoredLogEvent>,
    provider: &str,
    reason: &str,
) -> SummaryProposal {
    let markdown = with_evidence_details(fallback_markdown(&events, reason), &events);
    let allowed_links = allowed_canonical_links(&args);
    let canonical_links = if allowed_links.len() == 1 {
        allowed_links
    } else {
        Vec::new()
    };
    SummaryProposal {
        target_note: default_target_note(&args),
        link_candidates: allowed_canonical_links(&args),
        canonical_links,
        markdown,
        evidence_event_ids: events.into_iter().map(|event| event.id).collect(),
        confidence: "low".to_owned(),
        open_questions: vec![reason.to_owned()],
        requires_review: true,
        provider: provider.to_owned(),
        supersedes_proposal_ids: Vec::new(),
        consolidation_job_id: None,
        link_context_revision: link_context_revision(&args),
    }
}

fn fallback_markdown(events: &[StoredLogEvent], reason: &str) -> String {
    format!(
        "- Review {count} log events manually. {reason}",
        count = events.len(),
    )
}

fn default_target_note(args: &SuggestMarkdownSummaryArgs) -> String {
    args.vault_context
        .get("daily_note")
        .and_then(Value::as_str)
        .unwrap_or("Daily note")
        .to_owned()
}

fn link_context_revision(args: &SuggestMarkdownSummaryArgs) -> String {
    args.vault_context
        .get("link_context_revision")
        .and_then(Value::as_str)
        .unwrap_or_default()
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
    let allowed_links = allowed_canonical_links(args);
    let allowed: HashSet<&str> = allowed_links.iter().map(String::as_str).collect();
    let mut selected = string_array_field(value, "canonical_links")
        .into_iter()
        .filter(|link| allowed.contains(link.as_str()))
        .collect::<Vec<_>>();
    if selected.is_empty() && allowed_links.len() == 1 {
        selected.push(allowed_links[0].clone());
    }
    selected
}

fn with_evidence_details(markdown: String, events: &[StoredLogEvent]) -> String {
    let narrative = markdown
        .lines()
        .filter(|line| !line.trim_start().starts_with("Details:"))
        .collect::<Vec<_>>()
        .join("\n");

    let mut details = Vec::new();
    push_detail(
        &mut details,
        "source",
        event_field(events, |event| Some(&event.source)),
    );
    if let (Some(first), Some(last)) = (
        events.iter().map(|event| event.timestamp).min(),
        events.iter().map(|event| event.timestamp).max(),
    ) {
        details.push(format!(
            "window `{}/{}`",
            first.to_rfc3339(),
            last.to_rfc3339()
        ));
    }
    for (label, key) in [
        ("repo", "repo"),
        ("project", "project"),
        ("product", "product"),
        ("branch", "branch"),
        ("base", "base_branch"),
        ("target", "target_branch"),
        ("commit", "commit"),
        ("status", "status"),
        ("work item", "work_item"),
        ("pull request", "pull_request"),
        ("modules", "modules"),
        ("paths", "changed_paths"),
        ("tests", "tests"),
        ("validation", "validation"),
    ] {
        push_detail(&mut details, label, metadata_field(events, key));
    }
    let event_ids = events
        .iter()
        .map(|event| event.id.clone())
        .collect::<Vec<_>>();
    if event_ids.len() <= 12 {
        push_detail(&mut details, "events", Some(event_ids));
    } else {
        details.push(format!(
            "events `{}` (IDs retained in proposal metadata and SQLite)",
            event_ids.len()
        ));
    }

    format!("{}\n\nDetails: {}", narrative.trim(), details.join(" · "))
}

fn event_field<F>(events: &[StoredLogEvent], field: F) -> Option<Vec<String>>
where
    F: Fn(&StoredLogEvent) -> Option<&String>,
{
    let values = events
        .iter()
        .filter_map(field)
        .cloned()
        .collect::<BTreeSet<_>>();
    (!values.is_empty()).then(|| values.into_iter().collect())
}

fn metadata_field(events: &[StoredLogEvent], key: &str) -> Option<Vec<String>> {
    let mut values = BTreeSet::new();
    for event in events {
        match event.metadata.get(key) {
            Some(Value::String(value)) => {
                values.insert(value.clone());
            }
            Some(Value::Array(items)) => {
                values.extend(
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(ToOwned::to_owned),
                );
            }
            _ => {}
        }
    }
    (!values.is_empty()).then(|| values.into_iter().take(12).collect())
}

fn push_detail(details: &mut Vec<String>, label: &str, values: Option<Vec<String>>) {
    let Some(values) = values else {
        return;
    };
    let values = values
        .into_iter()
        .map(|value| value.replace('`', "'"))
        .collect::<Vec<_>>()
        .join(", ");
    details.push(format!("{label} `{values}`"));
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
            vault_context: json!({ "daily_note": "Configured daily note" }),
            mode: "daily-note".to_owned(),
            task: None,
        };

        let proposal = suggest_markdown_summary(None, args, vec![event])
            .await
            .expect("fallback proposal succeeds");

        assert_eq!(proposal.target_note, "Configured daily note");
        assert_eq!(proposal.evidence_event_ids, ["evt_test"]);
        assert!(proposal.requires_review);
        assert_eq!(proposal.provider, "not_configured");
    }

    #[test]
    fn keeps_database_evidence_and_one_allowed_link() {
        let event = StoredLogEvent {
            id: "evt_real".to_owned(),
            received_at: Utc::now(),
            timestamp: Utc::now(),
            source: "codex/test".to_owned(),
            level: "info".to_owned(),
            message: "Completed a useful task".to_owned(),
            metadata: Map::from_iter([
                ("project".to_owned(), Value::from("application-suite")),
                ("branch".to_owned(), Value::from("feature/test")),
            ]),
            fingerprint: None,
            truncated: false,
            reviewed: false,
        };
        let args = SuggestMarkdownSummaryArgs {
            event_ids: vec![event.id.clone()],
            vault_context: json!({
                "daily_note": "Approved daily note",
                "candidate_notes": ["Customer Portal"]
            }),
            mode: "daily-note".to_owned(),
            task: None,
        };
        let model_output = json!({
            "target_note": "Model-selected note",
            "markdown": "- A result.\n\nDetails: invented evidence",
            "evidence_event_ids": ["evt_invented"],
            "canonical_links": ["[[Invented Note]]"]
        })
        .to_string();

        let proposal =
            parse_proposal(&model_output, &args, &[event], "test").expect("valid proposal parses");

        assert_eq!(proposal.evidence_event_ids, ["evt_real"]);
        assert_eq!(proposal.target_note, "Approved daily note");
        assert_eq!(proposal.canonical_links, ["[[Customer Portal]]"]);
        assert!(proposal.markdown.contains("project `application-suite`"));
        assert!(proposal.markdown.contains("branch `feature/test`"));
        assert!(proposal.markdown.contains("events `evt_real`"));
        assert!(!proposal.markdown.contains("invented evidence"));
    }

    #[test]
    fn renders_daily_workstreams_with_validated_links_and_evidence() {
        let event = StoredLogEvent {
            id: "evt_navigation".to_owned(),
            received_at: Utc::now(),
            timestamp: Utc::now(),
            source: "agent/test".to_owned(),
            level: "info".to_owned(),
            message: "Completed navigation work".to_owned(),
            metadata: Map::from_iter([("repo".to_owned(), Value::from("portal-api"))]),
            fingerprint: None,
            truncated: false,
            reviewed: false,
        };
        let args = SuggestMarkdownSummaryArgs {
            event_ids: vec![event.id.clone()],
            vault_context: json!({
                "daily_note": "Work log",
                "candidate_notes": ["[[Record Navigation]]"]
            }),
            mode: "daily-consolidation".to_owned(),
            task: None,
        };
        let model_output = json!({
            "workstreams": [{
                "title": "Navigation validation",
                "canonical_link": "[[Record Navigation]]",
                "summary_bullets": ["Validated the host route.", "Kept the chat open."],
                "evidence_event_ids": ["evt_navigation"]
            }],
            "confidence": "high",
            "open_questions": []
        })
        .to_string();

        let proposal = parse_proposal(&model_output, &args, &[event], "test").unwrap();
        assert!(
            proposal
                .markdown
                .starts_with("### [[Record Navigation]] — Navigation validation")
        );
        assert!(proposal.markdown.contains("repo `portal-api`"));
        assert_eq!(proposal.canonical_links, ["[[Record Navigation]]"]);
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

    #[test]
    fn daily_prompt_prefers_one_terminal_event_per_task() {
        let make_event = |id: &str, task: &str, sequence: i64, event_type: &str| StoredLogEvent {
            id: id.to_owned(),
            received_at: Utc::now(),
            timestamp: Utc::now(),
            source: "codex/test".to_owned(),
            level: "info".to_owned(),
            message: id.to_owned(),
            metadata: Map::from_iter([
                ("task_id".to_owned(), Value::from(task)),
                ("sequence".to_owned(), Value::from(sequence)),
                ("event_type".to_owned(), Value::from(event_type)),
            ]),
            fingerprint: None,
            truncated: false,
            reviewed: false,
        };
        let events = vec![
            make_event("start", "task-1", 1, "start"),
            make_event("complete", "task-1", 3, "complete"),
            make_event("late-progress", "task-1", 4, "progress"),
            make_event("other", "task-2", 1, "start"),
        ];

        let selected = events_for_prompt("daily-consolidation", &events)
            .into_iter()
            .map(|event| event.id.as_str())
            .collect::<BTreeSet<_>>();

        assert_eq!(selected, BTreeSet::from(["complete", "other"]));
        assert_eq!(events_for_prompt("daily-note", &events).len(), 4);
    }
}
