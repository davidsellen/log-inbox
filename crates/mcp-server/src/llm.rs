use log_inbox_core::models::StoredLogEvent;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};
use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet, HashSet},
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
    "entry_kind",
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
    "commit_message",
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
    "canonical_note_candidates",
    "workstream",
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
    group_id: String,
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

    if args.mode == "daily-consolidation" {
        let all_event_ids = events
            .iter()
            .map(|event| event.id.clone())
            .collect::<Vec<_>>();
        let (manual, automated): (Vec<_>, Vec<_>) = events.into_iter().partition(is_manual_event);
        if !manual.is_empty() {
            let manual_markdown = render_manual_entries(&args, &manual);
            if automated.is_empty() {
                return Ok(SummaryProposal {
                    target_note: default_target_note(&args),
                    canonical_links: configured_workstream_links(&args),
                    link_candidates: allowed_canonical_links(&args),
                    markdown: format!("### My notes\n\n{manual_markdown}"),
                    evidence_event_ids: all_event_ids,
                    confidence: "high".to_owned(),
                    open_questions: Vec::new(),
                    requires_review: true,
                    provider: "manual".to_owned(),
                    supersedes_proposal_ids: Vec::new(),
                    consolidation_job_id: None,
                    link_context_revision: link_context_revision(&args),
                });
            }
            let mut proposal = suggest_automated_summary(config, args.clone(), automated).await?;
            proposal.markdown = format!(
                "### My notes\n\n{manual_markdown}\n\n### Automated activity\n\n{}",
                demote_workstream_headings(&proposal.markdown)
            );
            proposal.evidence_event_ids = all_event_ids;
            proposal.canonical_links = configured_workstream_links(&args);
            proposal.link_candidates = allowed_canonical_links(&args);
            return Ok(proposal);
        }
        return suggest_automated_summary(config, args, automated).await;
    }

    suggest_automated_summary(config, args, events).await
}

async fn suggest_automated_summary(
    config: Option<&LlmConfig>,
    args: SuggestMarkdownSummaryArgs,
    events: Vec<StoredLogEvent>,
) -> Result<SummaryProposal, String> {
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

fn is_manual_event(event: &StoredLogEvent) -> bool {
    event.metadata.get("entry_kind").and_then(Value::as_str) == Some("manual")
}

fn render_manual_entries(args: &SuggestMarkdownSummaryArgs, events: &[StoredLogEvent]) -> String {
    let mut events = events.iter().collect::<Vec<_>>();
    events.sort_by_key(|event| (event.timestamp, event.received_at));
    events
        .into_iter()
        .map(|event| {
            let links = links_for_group(args, &event_group_key(event));
            let prefix = if links.is_empty() {
                String::new()
            } else {
                format!("{} — ", links.join(" · "))
            };
            let message = event.message.trim().replace('\n', "\n    ");
            let references = ["work_item", "pull_request"]
                .into_iter()
                .filter_map(|key| {
                    event
                        .metadata
                        .get(key)
                        .and_then(Value::as_str)
                        .map(|value| (key, value))
                })
                .map(|(kind, value)| render_manual_reference(kind, value))
                .collect::<Vec<_>>();
            if references.is_empty() {
                format!("- {prefix}{message}")
            } else {
                format!("- {prefix}{message} · {}", references.join(" · "))
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_manual_reference(kind: &str, value: &str) -> String {
    let Ok(url) = reqwest::Url::parse(value) else {
        return format!("`{}`", value.replace('`', "'"));
    };
    if !matches!(url.scheme(), "http" | "https") {
        return format!("`{}`", value.replace('`', "'"));
    }
    let label = url
        .path_segments()
        .and_then(|segments| segments.filter(|segment| !segment.is_empty()).next_back())
        .filter(|segment| segment.chars().all(|character| character.is_ascii_digit()))
        .map(|id| {
            if kind == "pull_request" {
                format!("PR {id}")
            } else {
                format!("Work item {id}")
            }
        })
        .unwrap_or_else(|| url.host_str().unwrap_or("Reference").to_owned());
    format!("[{label}](<{}>)", url.as_str())
}

fn demote_workstream_headings(markdown: &str) -> String {
    markdown
        .lines()
        .map(|line| {
            line.strip_prefix("### ")
                .map_or_else(|| line.to_owned(), |line| format!("#### {line}"))
        })
        .collect::<Vec<_>>()
        .join("\n")
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
        "- Include exactly one workstreams item for every supplied group_id. Each item has group_id, title, and summary_bullets.\n- Copy group_id exactly. Do not choose links or write Markdown.\n- Merge lifecycle updates represented by each group and omit trivial transport details."
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
  "markdown": "",
  "evidence_event_ids": ["evt_..."],
  "confidence": "low|medium|high",
  "open_questions": []
  ,"workstreams": [
    {{
      "group_id": "task-or-session-id",
      "title": "Concise workstream name",
      "summary_bullets": ["Outcome that matters."]
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
        let key = technical_event_group_key(event);
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

pub(crate) fn event_group_key(event: &StoredLogEvent) -> String {
    let repo = event
        .metadata
        .get("repo")
        .and_then(Value::as_str)
        .map(normalized_group_value)
        .unwrap_or_default();
    if let Some(work_item) = event.metadata.get("work_item").and_then(Value::as_str) {
        return format!(
            "repo:{repo}|work-item:{}",
            normalized_reference_value(work_item)
        );
    }
    if let Some(pull_request) = event.metadata.get("pull_request").and_then(Value::as_str) {
        return format!(
            "repo:{repo}|pull-request:{}",
            normalized_reference_value(pull_request)
        );
    }
    if let Some(project) = event.metadata.get("project").and_then(Value::as_str) {
        return format!("project:{}", normalized_group_value(project));
    }
    if !repo.is_empty() {
        return format!("repo:{repo}");
    }
    technical_event_group_key(event)
}

fn technical_event_group_key(event: &StoredLogEvent) -> String {
    ["task_id", "session_id"]
        .into_iter()
        .find_map(|name| event.metadata.get(name).and_then(Value::as_str))
        .unwrap_or(&event.id)
        .to_owned()
}

fn normalized_group_value(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|character| character.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn normalized_reference_value(value: &str) -> String {
    value
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .next_back()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| normalized_group_value(value))
}

fn event_groups(events: &[StoredLogEvent]) -> BTreeMap<String, Vec<StoredLogEvent>> {
    let mut groups = BTreeMap::new();
    for event in events {
        groups
            .entry(event_group_key(event))
            .or_insert_with(Vec::new)
            .push(event.clone());
    }
    groups
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
        group_id: event_group_key(event),
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
    let canonical_links = if args.mode == "daily-consolidation" {
        configured_workstream_links(args)
    } else {
        workstream_links(&value, args)
    };
    Ok(SummaryProposal {
        target_note: default_target_note(args),
        link_candidates: allowed_canonical_links(args),
        canonical_links: if canonical_links.is_empty() {
            validated_canonical_links(&value, args)
        } else {
            canonical_links
        },
        markdown: if args.mode == "daily-consolidation" {
            workstream_markdown.unwrap_or_else(|| render_deterministic_workstreams(args, events))
        } else {
            with_evidence_details(
                string_field(&value, "markdown")
                    .unwrap_or_else(|| fallback_markdown(events, "LLM response omitted markdown.")),
                events,
            )
        },
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
    let groups = event_groups(events);
    if items.len() != groups.len() {
        return None;
    }
    let mut seen = HashSet::new();
    let rendered = items
        .iter()
        .filter_map(|item| {
            let group_id = item.get("group_id").and_then(Value::as_str)?;
            let evidence = groups.get(group_id)?;
            if !seen.insert(group_id) {
                return None;
            }
            let title = item.get("title").and_then(Value::as_str)?.trim();
            if title.is_empty() || title.to_ascii_lowercase().contains("concise workstream") {
                return None;
            }
            let bullets = string_array_field(item, "summary_bullets");
            if bullets.is_empty()
                || bullets.iter().any(|bullet| {
                    bullet.to_ascii_lowercase().contains("outcome that matters")
                        || bullet.to_ascii_lowercase().contains("concise conclusion")
                })
            {
                return None;
            }
            let heading = workstream_heading(title, &links_for_group(args, group_id));
            let body = bullets
                .into_iter()
                .take(3)
                .map(|bullet| format!("- {}", bullet.trim().trim_start_matches("- ")))
                .collect::<Vec<_>>()
                .join("\n");
            Some(format!(
                "{heading}\n\n{}",
                with_daily_details(body, evidence)
            ))
        })
        .collect::<Vec<_>>();
    (rendered.len() == groups.len() && seen.len() == groups.len()).then(|| rendered.join("\n\n"))
}

fn configured_workstream_links(args: &SuggestMarkdownSummaryArgs) -> Vec<String> {
    args.vault_context
        .get("workstream_links")
        .and_then(Value::as_object)
        .into_iter()
        .flat_map(|groups| groups.values())
        .flat_map(|links| links.as_array().into_iter().flatten())
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn links_for_group(args: &SuggestMarkdownSummaryArgs, group_id: &str) -> Vec<String> {
    args.vault_context
        .get("workstream_links")
        .and_then(Value::as_object)
        .and_then(|groups| groups.get(group_id))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect()
}

fn workstream_heading(title: &str, links: &[String]) -> String {
    if links.is_empty() {
        format!("### {title}")
    } else {
        format!("### {} — {title}", links.join(" · "))
    }
}

fn render_deterministic_workstreams(
    args: &SuggestMarkdownSummaryArgs,
    events: &[StoredLogEvent],
) -> String {
    event_groups(events)
        .into_iter()
        .map(|(group_id, evidence)| {
            let authoritative = evidence
                .iter()
                .filter(|event| is_terminal_event(event))
                .max_by_key(|event| event_order_key(event))
                .or_else(|| evidence.iter().max_by_key(|event| event_order_key(event)))
                .expect("event group is non-empty");
            let title = deterministic_title(authoritative);
            let mut seen_messages = HashSet::new();
            let bullets = events_for_prompt("daily-consolidation", &evidence)
                .into_iter()
                .map(|event| event.message.trim().to_owned())
                .filter(|message| !message.is_empty() && seen_messages.insert(message.clone()))
                .take(5)
                .map(|message| format!("- {message}"))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "{}\n\n{}",
                workstream_heading(&title, &links_for_group(args, &group_id)),
                with_daily_details(bullets, &evidence)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn deterministic_title(event: &StoredLogEvent) -> String {
    for key in ["work_item", "project"] {
        if let Some(value) = event.metadata.get(key).and_then(Value::as_str) {
            return value.to_owned();
        }
    }
    for key in ["modules", "module"] {
        if let Some(value) = event.metadata.get(key) {
            if let Some(first) = value
                .as_str()
                .or_else(|| value.as_array()?.first()?.as_str())
            {
                return first.to_owned();
            }
        }
    }
    event
        .metadata
        .get("repo")
        .and_then(Value::as_str)
        .unwrap_or(&event.source)
        .to_owned()
}

fn fallback_proposal(
    args: SuggestMarkdownSummaryArgs,
    events: Vec<StoredLogEvent>,
    provider: &str,
    reason: &str,
) -> SummaryProposal {
    let markdown = if args.mode == "daily-consolidation" {
        render_deterministic_workstreams(&args, &events)
    } else {
        with_evidence_details(fallback_markdown(&events, reason), &events)
    };
    let canonical_links = if args.mode == "daily-consolidation" {
        configured_workstream_links(&args)
    } else {
        let allowed_links = allowed_canonical_links(&args);
        if allowed_links.len() == 1 {
            allowed_links
        } else {
            Vec::new()
        }
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

fn with_daily_details(markdown: String, events: &[StoredLogEvent]) -> String {
    let mut details = Vec::new();
    for (label, key) in [
        ("work item", "work_item"),
        ("pull request", "pull_request"),
        ("commit summary", "commit_message"),
    ] {
        push_detail(&mut details, label, metadata_field(events, key));
    }
    let commits = metadata_field(events, "commit").map(|values| {
        values
            .into_iter()
            .map(|value| value.chars().take(8).collect())
            .collect()
    });
    push_detail(&mut details, "commits", commits);
    push_detail(
        &mut details,
        "tests",
        metadata_field_limited(events, "tests", 4),
    );
    push_detail(
        &mut details,
        "validation",
        metadata_field_limited(events, "validation", 4),
    );
    if details.is_empty() {
        markdown.trim().to_owned()
    } else {
        format!("{}\n\nReferences: {}", markdown.trim(), details.join(" · "))
    }
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
    metadata_field_limited(events, key, 12)
}

fn metadata_field_limited(
    events: &[StoredLogEvent],
    key: &str,
    limit: usize,
) -> Option<Vec<String>> {
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
    (!values.is_empty()).then(|| values.into_iter().take(limit).collect())
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

    #[tokio::test]
    async fn renders_manual_daily_entries_verbatim_without_the_llm() {
        let now = Utc::now();
        let event = StoredLogEvent {
            id: "evt_manual".to_owned(),
            received_at: now,
            timestamp: now,
            source: "manual/dashboard".to_owned(),
            level: "info".to_owned(),
            message: "Reviewed the design decision.".to_owned(),
            metadata: Map::from_iter([
                ("entry_kind".to_owned(), Value::from("manual")),
                ("task_id".to_owned(), Value::from("manual_1")),
                (
                    "pull_request".to_owned(),
                    Value::from("https://dev.azure.com/org/project/pullrequest/9374"),
                ),
            ]),
            fingerprint: None,
            truncated: false,
            reviewed: false,
        };
        let args = SuggestMarkdownSummaryArgs {
            event_ids: vec![event.id.clone()],
            vault_context: json!({
                "daily_note": "Daily log Sep 8",
                "candidate_notes": ["[[Sweet CRM]]"],
                "workstream_links": { "repo:|pull-request:9374": ["[[Sweet CRM]]"] },
            }),
            mode: "daily-consolidation".to_owned(),
            task: None,
        };

        let proposal = suggest_markdown_summary(None, args, vec![event])
            .await
            .expect("manual proposal renders");

        assert_eq!(proposal.provider, "manual");
        assert!(proposal.markdown.starts_with("### My notes"));
        assert!(
            proposal
                .markdown
                .contains("[[Sweet CRM]] — Reviewed the design decision.")
        );
        assert!(proposal.markdown.contains("[PR 9374]"));
        assert!(!proposal.markdown.contains("LLM is not configured"));
    }

    #[tokio::test]
    async fn separates_manual_notes_from_automated_daily_activity() {
        let now = Utc::now();
        let manual = StoredLogEvent {
            id: "evt_manual".to_owned(),
            received_at: now,
            timestamp: now,
            source: "manual/dashboard".to_owned(),
            level: "info".to_owned(),
            message: "Recorded the customer decision.".to_owned(),
            metadata: Map::from_iter([
                ("entry_kind".to_owned(), Value::from("manual")),
                ("task_id".to_owned(), Value::from("manual_1")),
            ]),
            fingerprint: None,
            truncated: false,
            reviewed: false,
        };
        let automated = StoredLogEvent {
            id: "evt_auto".to_owned(),
            received_at: now,
            timestamp: now,
            source: "codex/fedora".to_owned(),
            level: "info".to_owned(),
            message: "Validated the implementation.".to_owned(),
            metadata: Map::from_iter([("task_id".to_owned(), Value::from("task_1"))]),
            fingerprint: None,
            truncated: false,
            reviewed: false,
        };
        let args = SuggestMarkdownSummaryArgs {
            event_ids: vec![manual.id.clone(), automated.id.clone()],
            vault_context: json!({
                "daily_note": "Daily log Sep 8",
                "workstream_links": { "manual_1": [], "task_1": [] },
            }),
            mode: "daily-consolidation".to_owned(),
            task: None,
        };

        let proposal = suggest_markdown_summary(None, args, vec![manual, automated])
            .await
            .expect("mixed proposal renders");

        assert!(proposal.markdown.contains("### My notes"));
        assert!(proposal.markdown.contains("### Automated activity"));
        assert!(proposal.markdown.contains("#### codex/fedora"));
        assert_eq!(proposal.evidence_event_ids.len(), 2);
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
                "candidate_notes": ["[[Record Navigation]]"],
                "workstream_links": {
                    "repo:portalapi": ["[[Record Navigation]]"]
                }
            }),
            mode: "daily-consolidation".to_owned(),
            task: None,
        };
        let model_output = json!({
            "workstreams": [{
                "group_id": "repo:portalapi",
                "title": "Navigation validation",
                "summary_bullets": ["Validated the host route.", "Kept the chat open."]
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
        assert!(!proposal.markdown.contains("source `agent/test`"));
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

    #[test]
    fn malformed_daily_output_falls_back_to_one_entry_per_task() {
        let make_event = |id: &str, task: &str, message: &str, repo: &str| StoredLogEvent {
            id: id.to_owned(),
            received_at: Utc::now(),
            timestamp: Utc::now(),
            source: "codex/test".to_owned(),
            level: "info".to_owned(),
            message: message.to_owned(),
            metadata: Map::from_iter([
                ("task_id".to_owned(), Value::from(task)),
                ("event_type".to_owned(), Value::from("complete")),
                ("repo".to_owned(), Value::from(repo)),
            ]),
            fingerprint: None,
            truncated: false,
            reviewed: false,
        };
        let events = vec![
            make_event("one", "task-one", "Completed Forms work.", "SweetOne"),
            make_event("two", "task-two", "Validated SCIM locally.", "SweetNext"),
        ];
        let args = SuggestMarkdownSummaryArgs {
            event_ids: events.iter().map(|event| event.id.clone()).collect(),
            vault_context: json!({
                "daily_note": "Daily log Sep 7",
                "candidate_notes": ["[[Sweet CRM]]", "[[Sweet Next]]"],
                "workstream_links": {
                    "repo:sweetone": ["[[Sweet CRM]]"],
                    "repo:sweetnext": ["[[Sweet Next]]"]
                }
            }),
            mode: "daily-consolidation".to_owned(),
            task: None,
        };
        let malformed = json!({
            "markdown": "- Concise conclusion that belongs in a Markdown vault.",
            "confidence": "high"
        })
        .to_string();

        let proposal = parse_proposal(&malformed, &args, &events, "test").unwrap();

        assert!(proposal.markdown.contains("### [[Sweet CRM]] — SweetOne"));
        assert!(proposal.markdown.contains("- Completed Forms work."));
        assert!(proposal.markdown.contains("### [[Sweet Next]] — SweetNext"));
        assert!(proposal.markdown.contains("- Validated SCIM locally."));
        assert!(!proposal.markdown.contains("Concise conclusion"));
    }

    #[test]
    fn daily_groups_prefer_repository_and_work_item_or_pull_request() {
        let event_with = |repo: &str, field: &str, value: &str| StoredLogEvent {
            id: format!("evt-{repo}-{field}"),
            received_at: Utc::now(),
            timestamp: Utc::now(),
            source: "codex/test".to_owned(),
            level: "info".to_owned(),
            message: "done".to_owned(),
            metadata: Map::from_iter([
                ("repo".to_owned(), Value::from(repo)),
                (field.to_owned(), Value::from(value)),
            ]),
            fingerprint: None,
            truncated: false,
            reviewed: false,
        };

        assert_eq!(
            event_group_key(&event_with("SweetOne", "work_item", "ADO 57950")),
            "repo:sweetone|work-item:57950"
        );
        assert_eq!(
            event_group_key(&event_with(
                "SweetOne",
                "pull_request",
                "https://dev.azure.com/org/project/pullrequest/9374"
            )),
            "repo:sweetone|pull-request:9374"
        );
        assert_ne!(
            event_group_key(&event_with("SweetOne", "work_item", "ADO 57950")),
            event_group_key(&event_with("SweetNext", "work_item", "ADO 57950"))
        );
    }
}
