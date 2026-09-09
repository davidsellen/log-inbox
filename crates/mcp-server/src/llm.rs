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
const MAX_PROMPT_BYTES: usize = 512 * 1024;
const MAX_LLM_RESPONSE_BYTES: usize = 256 * 1024;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub structured_draft: Option<StructuredDailyDraft>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StructuredDailyDraft {
    pub workstreams: Vec<StructuredWorkstream>,
    #[serde(default)]
    pub open_questions: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StructuredWorkstream {
    pub id: String,
    pub title: String,
    pub evidence_event_ids: Vec<String>,
    #[serde(default)]
    pub outcome: Vec<StructuredFact>,
    #[serde(default)]
    pub decision: Vec<StructuredFact>,
    #[serde(default)]
    pub trade_off: Vec<StructuredFact>,
    #[serde(default)]
    pub validation: Vec<StructuredFact>,
    #[serde(default)]
    pub blocker: Vec<StructuredFact>,
    #[serde(default)]
    pub follow_up: Vec<StructuredFact>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct StructuredFact {
    pub text: String,
    pub evidence_event_ids: Vec<String>,
}

pub fn parse_strict_daily_draft(
    content: &str,
    expected_event_ids: &[String],
) -> Result<StructuredDailyDraft, String> {
    let draft: StructuredDailyDraft = serde_json::from_str(content)
        .map_err(|error| format!("LLM daily draft did not match the structured schema: {error}"))?;
    if draft.workstreams.is_empty() {
        return Err("LLM daily draft must contain at least one workstream".to_owned());
    }
    let expected = expected_event_ids.iter().cloned().collect::<BTreeSet<_>>();
    if expected.len() != expected_event_ids.len() {
        return Err("evidence snapshot contains duplicate event IDs".to_owned());
    }
    let mut covered = BTreeSet::new();
    let mut workstream_ids = BTreeSet::new();
    for workstream in &draft.workstreams {
        if workstream.id.trim().is_empty() || workstream.title.trim().is_empty() {
            return Err("every workstream requires a stable ID and title".to_owned());
        }
        let title = workstream.title.to_ascii_lowercase();
        if title.contains("concise workstream") || title.contains("workstream name") {
            return Err(format!(
                "workstream {} retained a schema placeholder title",
                workstream.id
            ));
        }
        if !workstream_ids.insert(workstream.id.clone()) {
            return Err(format!("duplicate workstream ID: {}", workstream.id));
        }
        if workstream.evidence_event_ids.is_empty() {
            return Err(format!("workstream {} has no evidence", workstream.id));
        }
        let facts = [
            &workstream.outcome,
            &workstream.decision,
            &workstream.trade_off,
            &workstream.validation,
            &workstream.blocker,
            &workstream.follow_up,
        ]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
        if facts.is_empty() {
            return Err(format!(
                "workstream {} has no factual fields",
                workstream.id
            ));
        }
        let workstream_evidence = workstream
            .evidence_event_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let mut fact_evidence = BTreeSet::new();
        for fact in facts {
            if fact.text.trim().is_empty() || fact.evidence_event_ids.is_empty() {
                return Err(format!(
                    "workstream {} contains a fact without text or evidence",
                    workstream.id
                ));
            }
            for event_id in &fact.evidence_event_ids {
                if !workstream_evidence.contains(event_id.as_str()) {
                    return Err(format!(
                        "fact in workstream {} cites evidence outside the workstream: {event_id}",
                        workstream.id
                    ));
                }
                fact_evidence.insert(event_id.as_str());
            }
        }
        if fact_evidence != workstream_evidence {
            return Err(format!(
                "workstream {} contains evidence that supports no factual field",
                workstream.id
            ));
        }
        for event_id in &workstream.evidence_event_ids {
            if !expected.contains(event_id) {
                return Err(format!(
                    "workstream {} invented evidence {event_id}",
                    workstream.id
                ));
            }
            if !covered.insert(event_id.clone()) {
                return Err(format!(
                    "evidence {event_id} appears in multiple workstreams"
                ));
            }
        }
    }
    let missing = expected.difference(&covered).cloned().collect::<Vec<_>>();
    if !missing.is_empty() {
        return Err(format!(
            "daily draft omitted evidence: {}",
            missing.join(", ")
        ));
    }
    Ok(draft)
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
                    structured_draft: None,
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

pub async fn generate_automated_daily_summary(
    config: Option<&LlmConfig>,
    args: SuggestMarkdownSummaryArgs,
    events: Vec<StoredLogEvent>,
) -> Result<SummaryProposal, String> {
    if args.mode != "daily-consolidation" {
        return Err("automated daily generation requires daily-consolidation mode".to_owned());
    }
    if events.is_empty() {
        return Err("automated daily generation requires evidence".to_owned());
    }
    suggest_automated_summary(config, args, events).await
}

async fn suggest_automated_summary(
    config: Option<&LlmConfig>,
    args: SuggestMarkdownSummaryArgs,
    events: Vec<StoredLogEvent>,
) -> Result<SummaryProposal, String> {
    let Some(config) = config else {
        if args.mode == "daily-consolidation" {
            return Err(
                "Daily consolidation requires a configured LLM; no raw-log fallback was created."
                    .to_owned(),
            );
        }
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

    let mut response = request.send().await.map_err(|error| error.to_string())?;
    let status = response.status();
    if !status.is_success() {
        return Err(format!("LLM provider returned HTTP {status}"));
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_LLM_RESPONSE_BYTES as u64)
    {
        return Err("LLM response exceeded the configured size limit".to_owned());
    }
    let mut response_bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|error| error.to_string())? {
        if response_bytes.len() + chunk.len() > MAX_LLM_RESPONSE_BYTES {
            return Err("LLM response exceeded the configured size limit".to_owned());
        }
        response_bytes.extend_from_slice(&chunk);
    }
    let chat: ChatResponse = serde_json::from_slice(&response_bytes)
        .map_err(|error| format!("LLM response envelope was invalid: {error}"))?;
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
        .and_then(|mut segments| segments.rfind(|segment| !segment.is_empty()))
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
    let (response_shape, format_rules) = if args.mode == "daily-consolidation" {
        (
            r#"{
  "workstreams": [{
    "id": "copy supplied group_id",
    "title": "Concise human-readable workstream name",
    "evidence_event_ids": ["every supplied event ID exactly once"],
    "outcome": [{"text": "Supported fact", "evidence_event_ids": ["evt_..."]}],
    "decision": [],
    "trade_off": [],
    "validation": [],
    "blocker": [],
    "follow_up": []
  }],
  "open_questions": []
}"#,
            "- Include exactly one workstream for every supplied group_id.\n- Copy each group_id into id exactly and assign every supplied event ID to that group.\n- Every event ID must appear exactly once at workstream level and support at least one factual item; never invent an ID.\n- Every factual item has exactly text and evidence_event_ids. Use the adaptive factual fields and omit unsupported facts. Arrays may be empty, but each workstream needs at least one factual item.\n- Merge repetitive lifecycle transport while retaining distinct outcomes, decisions, trade-offs, validation, blockers, and follow-up.\n- Do not choose links, references, or write Markdown; the server renders reviewed Markdown.",
        )
    } else {
        (
            r#"{
  "target_note": "Configured daily note",
  "canonical_links": [],
  "markdown": "",
  "evidence_event_ids": ["evt_..."],
  "confidence": "low|medium|high",
  "open_questions": []
}"#,
            "- Write 2-4 concise factual bullets covering outcome, important changes or diagnosis, validation, and any remaining follow-up. Do not add a heading or raw log dump.",
        )
    };

    let prompt = format!(
        r#"Task: {task}
Mode: {mode}

Vault context:
{vault_context}

Allowed canonical links:
{allowed_links}

Events:
{event_slice}

Return JSON with this exact shape:
{response_shape}

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
        response_shape = response_shape,
        format_rules = format_rules,
    );
    if prompt.len() > MAX_PROMPT_BYTES {
        return Err(format!(
            "daily evidence exceeds the {MAX_PROMPT_BYTES}-byte model input limit"
        ));
    }
    Ok(prompt)
}

fn events_for_prompt<'a>(_mode: &str, events: &'a [StoredLogEvent]) -> Vec<&'a StoredLogEvent> {
    let mut selected = events.iter().collect::<Vec<_>>();
    selected.sort_by_key(|event| (event.timestamp, event.received_at));
    selected
}

pub(crate) fn event_group_key(event: &StoredLogEvent) -> String {
    let repo = event
        .metadata
        .get("repo")
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .map(stable_identity);
    let namespace = repo.as_ref().map_or_else(
        || format!("source:{}", stable_identity(&event.source)),
        |repo| format!("repo:{repo}"),
    );
    if let Some(work_item) = event.metadata.get("work_item").and_then(Value::as_str) {
        return format!(
            "{namespace}|work-item:{}",
            normalized_reference_value(work_item, "ado")
        );
    }
    if let Some(pull_request) = event.metadata.get("pull_request").and_then(Value::as_str) {
        return format!(
            "{namespace}|pull-request:{}",
            normalized_reference_value(pull_request, "pr")
        );
    }
    technical_event_group_key(event, &namespace)
}

fn technical_event_group_key(event: &StoredLogEvent, namespace: &str) -> String {
    for (field, label) in [("task_id", "task"), ("session_id", "session")] {
        if let Some(value) = event.metadata.get(field).and_then(Value::as_str)
            && !value.trim().is_empty()
        {
            return format!("{namespace}|{label}:{}", stable_identity(value));
        }
    }
    format!("event:{}", stable_identity(&event.id))
}

fn stable_identity(value: &str) -> String {
    value
        .trim()
        .to_lowercase()
        .bytes()
        .map(|byte| {
            if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.') {
                (byte as char).to_string()
            } else {
                format!("%{byte:02X}")
            }
        })
        .collect()
}

fn normalized_reference_value(value: &str, label: &str) -> String {
    let value = value.trim();
    if value.chars().all(|character| character.is_ascii_digit()) {
        return value.to_owned();
    }
    if let Some((prefix, identifier)) = value.split_once(char::is_whitespace)
        && prefix.eq_ignore_ascii_case(label)
        && identifier
            .chars()
            .all(|character| character.is_ascii_digit())
    {
        return identifier.to_owned();
    }
    if let Ok(url) = reqwest::Url::parse(value)
        && let Some(identifier) = url
            .path_segments()
            .and_then(|mut segments| segments.rfind(|part| !part.is_empty()))
            .filter(|part| part.chars().all(|character| character.is_ascii_digit()))
    {
        return identifier.to_owned();
    }
    stable_identity(value)
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
    if args.mode == "daily-consolidation" {
        let expected_event_ids = events
            .iter()
            .map(|event| event.id.clone())
            .collect::<Vec<_>>();
        let draft = parse_strict_daily_draft(content, &expected_event_ids)?;
        validate_daily_workstream_groups(&draft, events)?;
        return Ok(SummaryProposal {
            target_note: default_target_note(args),
            link_candidates: allowed_canonical_links(args),
            canonical_links: configured_workstream_links(args),
            markdown: render_strict_daily_markdown(&draft, args, events),
            evidence_event_ids: expected_event_ids,
            confidence: "medium".to_owned(),
            open_questions: draft.open_questions.clone(),
            requires_review: true,
            provider: provider.to_owned(),
            supersedes_proposal_ids: Vec::new(),
            consolidation_job_id: None,
            link_context_revision: link_context_revision(args),
            structured_draft: Some(draft),
        });
    }

    let value: Value = serde_json::from_str(content).map_err(|error| {
        format!("LLM did not return valid JSON: {error}; response content was: {content}")
    })?;

    let canonical_links = workstream_links(&value, args);
    Ok(SummaryProposal {
        target_note: default_target_note(args),
        link_candidates: allowed_canonical_links(args),
        canonical_links: if canonical_links.is_empty() {
            validated_canonical_links(&value, args)
        } else {
            canonical_links
        },
        markdown: with_evidence_details(
            string_field(&value, "markdown")
                .unwrap_or_else(|| fallback_markdown(events, "LLM response omitted markdown.")),
            events,
        ),
        evidence_event_ids: events.iter().map(|event| event.id.clone()).collect(),
        confidence: string_field(&value, "confidence").unwrap_or_else(|| "low".to_owned()),
        open_questions: string_array_field(&value, "open_questions"),
        requires_review: true,
        provider: provider.to_owned(),
        supersedes_proposal_ids: Vec::new(),
        consolidation_job_id: None,
        link_context_revision: link_context_revision(args),
        structured_draft: None,
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

fn validate_daily_workstream_groups(
    draft: &StructuredDailyDraft,
    events: &[StoredLogEvent],
) -> Result<(), String> {
    let expected = event_groups(events);
    if draft.workstreams.len() != expected.len() {
        return Err(format!(
            "daily draft returned {} workstreams for {} evidence groups",
            draft.workstreams.len(),
            expected.len()
        ));
    }
    for workstream in &draft.workstreams {
        let Some(group_events) = expected.get(&workstream.id) else {
            return Err(format!(
                "daily draft invented workstream ID: {}",
                workstream.id
            ));
        };
        let expected_ids = group_events
            .iter()
            .map(|event| event.id.as_str())
            .collect::<BTreeSet<_>>();
        let actual_ids = workstream
            .evidence_event_ids
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        if actual_ids != expected_ids {
            return Err(format!(
                "workstream {} contains evidence from a different group",
                workstream.id
            ));
        }
    }
    Ok(())
}

fn render_strict_daily_markdown(
    draft: &StructuredDailyDraft,
    args: &SuggestMarkdownSummaryArgs,
    events: &[StoredLogEvent],
) -> String {
    let groups = event_groups(events);
    draft
        .workstreams
        .iter()
        .map(|workstream| {
            let evidence = groups
                .get(&workstream.id)
                .expect("validated workstream group exists");
            let fields = [
                ("Outcome", &workstream.outcome),
                ("Decision", &workstream.decision),
                ("Trade-off", &workstream.trade_off),
                ("Validation", &workstream.validation),
                ("Blocker", &workstream.blocker),
                ("Follow-up", &workstream.follow_up),
            ];
            let body = fields
                .into_iter()
                .flat_map(|(label, values)| {
                    values.iter().filter_map(move |fact| {
                        let value = fact.text.trim().trim_start_matches("- ").trim();
                        (!value.is_empty()).then(|| format!("- **{label}:** {value}"))
                    })
                })
                .map(|line| safe_model_markdown_line(&line))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "{}\n\n{}",
                workstream_heading(
                    &safe_model_markdown_text(workstream.title.trim()),
                    &links_for_group(args, &workstream.id)
                ),
                with_daily_details(body, evidence)
            )
        })
        .collect::<Vec<_>>()
        .join("\n\n")
}

fn safe_model_markdown_line(line: &str) -> String {
    let Some((prefix, value)) = line.split_once(":** ") else {
        return safe_model_markdown_text(line);
    };
    format!("{prefix}:** {}", safe_model_markdown_text(value))
}

fn safe_model_markdown_text(value: &str) -> String {
    let single_line = value
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let mut escaped = String::with_capacity(single_line.len());
    for character in single_line.chars() {
        if matches!(
            character,
            '\\' | '`'
                | '*'
                | '_'
                | '{'
                | '}'
                | '['
                | ']'
                | '('
                | ')'
                | '<'
                | '>'
                | '#'
                | '!'
                | '|'
        ) {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped.replace("://", ":\u{200b}//")
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
        structured_draft: None,
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
    use chrono::{DateTime, Utc};
    use serde::Deserialize;
    use serde_json::Map;

    #[derive(Debug, Deserialize)]
    struct GroupingFixtures {
        schema_version: u64,
        cases: Vec<GroupingCase>,
    }

    #[derive(Debug, Deserialize)]
    struct GroupingCase {
        name: String,
        events: Vec<FixtureEvent>,
        expected_daily_prompt_event_ids: Vec<String>,
        target_contract_note: String,
    }

    #[derive(Debug, Deserialize)]
    struct FixtureEvent {
        id: String,
        timestamp: DateTime<Utc>,
        received_at: DateTime<Utc>,
        source: String,
        message: String,
        metadata: Map<String, Value>,
        expected_group_key: String,
    }

    impl FixtureEvent {
        fn stored(&self) -> StoredLogEvent {
            StoredLogEvent {
                id: self.id.clone(),
                received_at: self.received_at,
                timestamp: self.timestamp,
                source: self.source.clone(),
                level: "info".to_owned(),
                message: self.message.clone(),
                metadata: self.metadata.clone(),
                fingerprint: None,
                truncated: false,
                reviewed: false,
            }
        }
    }

    #[test]
    fn golden_daily_grouping_fixtures_characterize_the_baseline() {
        let fixtures: GroupingFixtures =
            serde_json::from_str(include_str!("../tests/fixtures/daily-grouping.json"))
                .expect("daily grouping fixture is valid JSON");
        assert_eq!(fixtures.schema_version, 1);

        for case in fixtures.cases {
            assert!(
                !case.target_contract_note.trim().is_empty(),
                "{} must explain the target contract",
                case.name
            );
            let events = case
                .events
                .iter()
                .map(FixtureEvent::stored)
                .collect::<Vec<_>>();

            for (fixture, event) in case.events.iter().zip(&events) {
                assert_eq!(
                    event_group_key(event),
                    fixture.expected_group_key,
                    "{}: unexpected durable group key for {}",
                    case.name,
                    fixture.id
                );
            }

            let selected = events_for_prompt("daily-consolidation", &events)
                .into_iter()
                .map(|event| event.id.clone())
                .collect::<Vec<_>>();
            assert_eq!(
                selected, case.expected_daily_prompt_event_ids,
                "{}: prompt projection changed",
                case.name
            );
        }
    }

    #[test]
    fn strict_daily_drafts_require_exact_evidence_coverage() {
        let expected = vec!["evt_decision".to_owned(), "evt_validation".to_owned()];
        let valid = json!({
            "workstreams": [{
                "id": "repo:portal|work-item:42",
                "title": "Portal authentication",
                "evidence_event_ids": expected,
                "decision": [{
                    "text": "Kept the callback local to the development profile.",
                    "evidence_event_ids": ["evt_decision"]
                }],
                "validation": [{
                    "text": "The authentication tests passed.",
                    "evidence_event_ids": ["evt_validation"]
                }]
            }],
            "open_questions": []
        })
        .to_string();
        let parsed = parse_strict_daily_draft(
            &valid,
            &["evt_decision".to_owned(), "evt_validation".to_owned()],
        )
        .expect("strict draft validates");
        assert!(parsed.workstreams[0].outcome.is_empty());

        let missing = json!({
            "workstreams": [{
                "id": "repo:portal|work-item:42",
                "title": "Portal authentication",
                "evidence_event_ids": ["evt_decision"],
                "decision": [{
                    "text": "Kept the callback local.",
                    "evidence_event_ids": ["evt_decision"]
                }]
            }]
        })
        .to_string();
        assert!(
            parse_strict_daily_draft(
                &missing,
                &["evt_decision".to_owned(), "evt_validation".to_owned()]
            )
            .unwrap_err()
            .contains("omitted evidence")
        );
        let invented = valid.replace("evt_validation", "evt_invented");
        assert!(
            parse_strict_daily_draft(
                &invented,
                &["evt_decision".to_owned(), "evt_validation".to_owned()]
            )
            .unwrap_err()
            .contains("invented evidence")
        );
    }

    #[test]
    fn strict_daily_drafts_reject_raw_or_boilerplate_shapes() {
        assert!(parse_strict_daily_draft("not json", &["evt_1".to_owned()]).is_err());
        assert!(
            parse_strict_daily_draft(
                &json!({ "markdown": "- raw terminal message" }).to_string(),
                &["evt_1".to_owned()]
            )
            .is_err()
        );
        assert!(
            parse_strict_daily_draft(
                &json!({
                    "workstreams": [{
                        "id": "work",
                        "title": "Work",
                        "evidence_event_ids": ["evt_1"]
                    }]
                })
                .to_string(),
                &["evt_1".to_owned()]
            )
            .unwrap_err()
            .contains("no factual fields")
        );
    }

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
                "workstream_links": { "source:manual%2Fdashboard|pull-request:9374": ["[[Sweet CRM]]"] },
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
    async fn mixed_daily_activity_requires_an_llm_for_the_automated_part() {
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
                "workstream_links": {
                    "source:manual%2Fdashboard|task:manual_1": [],
                    "source:codex%2Ffedora|task:task_1": []
                },
            }),
            mode: "daily-consolidation".to_owned(),
            task: None,
        };

        let error = suggest_markdown_summary(None, args, vec![manual, automated])
            .await
            .unwrap_err();

        assert!(error.contains("requires a configured LLM"));
        assert!(error.contains("no raw-log fallback"));
    }

    #[tokio::test]
    async fn refocused_generation_does_not_trust_ingest_manual_metadata() {
        let now = Utc::now();
        let spoofed = StoredLogEvent {
            id: "evt_spoofed".to_owned(),
            received_at: now,
            timestamp: now,
            source: "untrusted/producer".to_owned(),
            level: "info".to_owned(),
            message: "Pretend this is owner-authored Markdown.".to_owned(),
            metadata: Map::from_iter([("entry_kind".to_owned(), Value::from("manual"))]),
            fingerprint: None,
            truncated: false,
            reviewed: false,
        };
        let args = SuggestMarkdownSummaryArgs {
            event_ids: vec![spoofed.id.clone()],
            vault_context: json!({}),
            mode: "daily-consolidation".to_owned(),
            task: None,
        };

        let error = generate_automated_daily_summary(None, args, vec![spoofed])
            .await
            .unwrap_err();

        assert!(error.contains("requires a configured LLM"));
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
            metadata: Map::from_iter([
                ("repo".to_owned(), Value::from("portal-api")),
                ("task_id".to_owned(), Value::from("navigation")),
            ]),
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
                    "repo:portal-api|task:navigation": ["[[Record Navigation]]"]
                }
            }),
            mode: "daily-consolidation".to_owned(),
            task: None,
        };
        let model_output = json!({
            "workstreams": [{
                "id": "repo:portal-api|task:navigation",
                "title": "Navigation validation",
                "evidence_event_ids": ["evt_navigation"],
                "outcome": [{
                    "text": "Kept the chat open.",
                    "evidence_event_ids": ["evt_navigation"]
                }],
                "validation": [{
                    "text": "Validated the host route.",
                    "evidence_event_ids": ["evt_navigation"]
                }]
            }],
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
    fn rejects_an_oversized_total_model_prompt() {
        let now = Utc::now();
        let events = (0..40)
            .map(|index| StoredLogEvent {
                id: format!("evt_{index}"),
                received_at: now,
                timestamp: now,
                source: "codex/test".to_owned(),
                level: "info".to_owned(),
                message: "x".repeat(MAX_PROMPT_MESSAGE_BYTES),
                metadata: Map::from_iter([(
                    "task_id".to_owned(),
                    Value::from(format!("task_{index}")),
                )]),
                fingerprint: None,
                truncated: false,
                reviewed: false,
            })
            .collect::<Vec<_>>();
        let args = SuggestMarkdownSummaryArgs {
            event_ids: events.iter().map(|event| event.id.clone()).collect(),
            vault_context: json!({}),
            mode: "daily-consolidation".to_owned(),
            task: None,
        };

        assert!(
            build_prompt(&args, &events)
                .unwrap_err()
                .contains("model input limit")
        );
    }

    #[test]
    fn escapes_model_controlled_markdown_structure_and_links() {
        let escaped = safe_model_markdown_text(
            "Injected\n## heading [[Unauthorized]] ![pixel](https://evil.example/pixel)",
        );

        assert!(!escaped.contains('\n'));
        assert!(!escaped.contains("[["));
        assert!(!escaped.contains("]("));
        assert!(!escaped.contains("://"));
        assert!(escaped.contains("\\#\\# heading"));
    }

    #[test]
    fn daily_prompt_preserves_all_bounded_lifecycle_evidence() {
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

        assert_eq!(
            selected,
            BTreeSet::from(["start", "complete", "late-progress", "other"])
        );
        assert_eq!(events_for_prompt("daily-note", &events).len(), 4);
    }

    #[test]
    fn malformed_daily_output_fails_instead_of_dumping_raw_events() {
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

        let error = parse_proposal(&malformed, &args, &events, "test").unwrap_err();

        assert!(error.contains("did not match the structured schema"));
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
        assert_ne!(
            event_group_key(&event_with("portal-api", "work_item", "42")),
            event_group_key(&event_with("portal_api", "work_item", "42"))
        );
        assert_eq!(
            event_group_key(&event_with(
                "SweetOne",
                "pull_request",
                "https://dev.azure.com/org/project/pullrequest/9374?api-version=7.1"
            )),
            "repo:sweetone|pull-request:9374"
        );
    }

    #[test]
    fn daily_fallback_groups_never_merge_unrelated_sources_or_repo_events() {
        let make_event = |id: &str, source: &str, repo: Option<&str>, task: Option<&str>| {
            let mut metadata = Map::new();
            if let Some(repo) = repo {
                metadata.insert("repo".to_owned(), Value::from(repo));
            }
            if let Some(task) = task {
                metadata.insert("task_id".to_owned(), Value::from(task));
            }
            StoredLogEvent {
                id: id.to_owned(),
                received_at: Utc::now(),
                timestamp: Utc::now(),
                source: source.to_owned(),
                level: "info".to_owned(),
                message: "done".to_owned(),
                metadata,
                fingerprint: None,
                truncated: false,
                reviewed: false,
            }
        };

        assert_ne!(
            event_group_key(&make_event("one", "agent/one", None, Some("shared"))),
            event_group_key(&make_event("two", "agent/two", None, Some("shared")))
        );
        assert_ne!(
            event_group_key(&make_event("one", "agent/one", Some("repo"), None)),
            event_group_key(&make_event("two", "agent/one", Some("repo"), None))
        );
    }
}
