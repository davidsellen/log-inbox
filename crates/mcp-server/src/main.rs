use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::Request,
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
};
use chrono::{DateTime, Duration, Utc};
use log_inbox_core::{models::LogQuery, settings::Settings, store::Store};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    env,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod auto_stage;
mod llm;
mod proposal_inbox;
mod vault_context;

#[derive(Clone)]
struct AppState {
    store: Store,
    llm_config: Option<llm::LlmConfig>,
    proposal_inbox: Option<proposal_inbox::ProposalInbox>,
    daily_notes_dir: Option<PathBuf>,
    vault_context: vault_context::VaultContextProvider,
    apply_lock: Arc<Mutex<()>>,
}

#[derive(Debug, Deserialize)]
struct JsonRpcRequest {
    #[serde(default, rename = "jsonrpc")]
    _jsonrpc: Option<String>,
    id: Option<Value>,
    method: String,
    #[serde(default)]
    params: Value,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<JsonRpcError>,
}

#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i64,
    message: String,
}

#[derive(Debug, Deserialize)]
struct ListSourcesArgs {
    since: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
struct ReadRecentLogsArgs {
    source: Option<String>,
    since: Option<DateTime<Utc>>,
    level: Option<String>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct SearchLogsArgs {
    query: String,
    since: Option<DateTime<Utc>>,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct GetLogWindowArgs {
    event_id: String,
    #[serde(default = "default_before")]
    before: String,
    #[serde(default = "default_after")]
    after: String,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct MarkReviewedArgs {
    event_ids: Vec<String>,
    note: String,
}

#[derive(Debug, Deserialize)]
struct ApplyMarkdownProposalArgs {
    proposal_id: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    let settings = Settings::from_env();
    let store = Store::open(settings.database_path())?;
    let vault_context = vault_context::VaultContextProvider::from_env();
    let state = AppState {
        store,
        llm_config: llm::LlmConfig::from_env(),
        proposal_inbox: proposal_inbox::ProposalInbox::from_env(),
        daily_notes_dir: env::var_os("LOG_INBOX_DAILY_NOTES_DIR")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from),
        vault_context,
        apply_lock: Arc::new(Mutex::new(())),
    };

    if let (Some(config), Some(inbox)) = (
        auto_stage::AutoStageConfig::from_env(),
        state.proposal_inbox.clone(),
    ) {
        tracing::info!("automatic Markdown proposal staging enabled");
        tokio::spawn(auto_stage::run(
            config,
            state.store.clone(),
            state.llm_config.clone(),
            inbox,
            state.vault_context.clone(),
        ));
    }

    let app = Router::new()
        .route("/health", get(health))
        .route("/mcp", post(mcp))
        .layer(middleware::from_fn(log_request_response))
        .with_state(state);

    let addr: SocketAddr = "0.0.0.0:8788".parse()?;
    tracing::info!(%addr, "starting mcp server");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn log_request_response(request: Request<Body>, next: Next) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    tracing::info!(%method, %uri, "incoming request");

    let started = std::time::Instant::now();
    let response = next.run(request).await;
    tracing::info!(
        %method,
        %uri,
        status = response.status().as_u16(),
        latency_ms = started.elapsed().as_millis(),
        "outgoing response"
    );
    response
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn mcp(
    State(state): State<AppState>,
    Json(request): Json<JsonRpcRequest>,
) -> Json<JsonRpcResponse> {
    let id = request.id.clone();
    let response = match request.method.as_str() {
        "initialize" => Ok(json!({
            "protocolVersion": "2025-06-18",
            "capabilities": {
                "tools": { "listChanged": false }
            },
            "serverInfo": {
                "name": "log-inbox",
                "version": env!("CARGO_PKG_VERSION")
            }
        })),
        "tools/list" => Ok(json!({ "tools": tool_definitions() })),
        "tools/call" => call_tool(&state, request.params).await,
        _ => Err(format!("unknown method {}", request.method)),
    };

    Json(match response {
        Ok(result) => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        },
        Err(message) => JsonRpcResponse {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(JsonRpcError {
                code: -32000,
                message,
            }),
        },
    })
}

async fn call_tool(state: &AppState, params: Value) -> Result<Value, String> {
    let name = params
        .get("name")
        .and_then(Value::as_str)
        .ok_or_else(|| "tools/call requires params.name".to_owned())?;
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    match name {
        "list_sources" => {
            let args: ListSourcesArgs = parse_args(arguments)?;
            let sources = state
                .store
                .list_sources(args.since)
                .map_err(|error| error.to_string())?;
            Ok(tool_text(json!({ "sources": sources })))
        }
        "read_recent_logs" => {
            let args: ReadRecentLogsArgs = parse_args(arguments)?;
            let result = state
                .store
                .query_logs(LogQuery {
                    source: args.source,
                    since: args.since,
                    level: args.level,
                    query: None,
                    limit: args.limit,
                })
                .map_err(|error| error.to_string())?;
            Ok(tool_text(json!(result)))
        }
        "search_logs" => {
            let args: SearchLogsArgs = parse_args(arguments)?;
            let result = state
                .store
                .query_logs(LogQuery {
                    source: None,
                    since: args.since,
                    level: None,
                    query: Some(args.query),
                    limit: args.limit,
                })
                .map_err(|error| error.to_string())?;
            Ok(tool_text(json!(result)))
        }
        "get_log_window" => {
            let args: GetLogWindowArgs = parse_args(arguments)?;
            let result = state
                .store
                .get_log_window(
                    &args.event_id,
                    parse_duration(&args.before)?,
                    parse_duration(&args.after)?,
                    args.limit,
                )
                .map_err(|error| error.to_string())?;
            Ok(tool_text(json!(result)))
        }
        "mark_reviewed" => {
            let args: MarkReviewedArgs = parse_args(arguments)?;
            let result = state
                .store
                .mark_reviewed(&args.event_ids, &args.note, "mcp")
                .map_err(|error| error.to_string())?;
            Ok(tool_text(json!(result)))
        }
        "suggest_markdown_summary" => {
            let mut args: llm::SuggestMarkdownSummaryArgs = parse_args(arguments)?;
            let events = state
                .store
                .get_events_by_ids(&args.event_ids)
                .map_err(|error| error.to_string())?;
            enrich_vault_context(&mut args, state.vault_context.for_events(&events)?);
            let proposal =
                llm::suggest_markdown_summary(state.llm_config.as_ref(), args, events).await?;
            Ok(tool_text(json!(proposal)))
        }
        "stage_markdown_summary" => {
            let mut args: llm::SuggestMarkdownSummaryArgs = parse_args(arguments)?;
            let event_ids = args.event_ids.clone();
            let events = state
                .store
                .get_events_by_ids(&event_ids)
                .map_err(|error| error.to_string())?;
            enrich_vault_context(&mut args, state.vault_context.for_events(&events)?);
            let proposal =
                llm::suggest_markdown_summary(state.llm_config.as_ref(), args, events).await?;
            let staged = state
                .proposal_inbox
                .as_ref()
                .ok_or_else(|| {
                    "proposal inbox is not configured; set LOG_INBOX_PROPOSAL_DIR".to_owned()
                })?
                .stage(&proposal)
                .map_err(|error| error.to_string())?;
            state
                .store
                .mark_staged(&event_ids, &staged.proposal_id)
                .map_err(|error| error.to_string())?;
            Ok(tool_text(json!(staged)))
        }
        "apply_markdown_proposal" => {
            let args: ApplyMarkdownProposalArgs = parse_args(arguments)?;
            let _guard = state
                .apply_lock
                .lock()
                .map_err(|_| "daily-note apply lock is poisoned".to_owned())?;
            let inbox = state.proposal_inbox.as_ref().ok_or_else(|| {
                "proposal inbox is not configured; set LOG_INBOX_PROPOSAL_DIR".to_owned()
            })?;
            let daily_notes_dir = state.daily_notes_dir.as_deref().ok_or_else(|| {
                "daily notes directory is not configured; set LOG_INBOX_DAILY_NOTES_DIR".to_owned()
            })?;
            let mut applied = inbox.apply(&args.proposal_id, daily_notes_dir)?;
            state
                .store
                .mark_reviewed(
                    &applied.evidence_event_ids,
                    &applied.daily_path.display().to_string(),
                    "mcp-apply",
                )
                .map_err(|error| error.to_string())?;
            inbox.discard(&args.proposal_id)?;
            applied.proposal_removed = true;
            Ok(tool_text(json!(applied)))
        }
        _ => Err(format!("unknown tool {name}")),
    }
}

fn enrich_vault_context(args: &mut llm::SuggestMarkdownSummaryArgs, discovered: Value) {
    let Some(discovered) = discovered.as_object() else {
        return;
    };
    if !args.vault_context.is_object() {
        args.vault_context = json!({});
    }
    let context = args
        .vault_context
        .as_object_mut()
        .expect("vault context was initialized as an object");
    for (key, value) in discovered {
        context.entry(key.clone()).or_insert_with(|| value.clone());
    }
}

fn parse_args<T: for<'de> Deserialize<'de>>(arguments: Value) -> Result<T, String> {
    serde_json::from_value(arguments).map_err(|error| error.to_string())
}

fn tool_text(value: Value) -> Value {
    json!({
        "content": [
            {
                "type": "text",
                "text": serde_json::to_string_pretty(&value).unwrap_or_else(|_| "{}".to_owned())
            }
        ],
        "isError": false
    })
}

fn parse_duration(input: &str) -> Result<Duration, String> {
    let input = input.trim();
    let (number, unit) = input.split_at(input.len().saturating_sub(1));
    let amount: i64 = number
        .parse()
        .map_err(|_| format!("invalid duration {input}"))?;
    match unit {
        "s" => Ok(Duration::seconds(amount)),
        "m" => Ok(Duration::minutes(amount)),
        "h" => Ok(Duration::hours(amount)),
        _ => Err(format!("duration must end in s, m, or h: {input}")),
    }
}

fn default_before() -> String {
    "5m".to_owned()
}

fn default_after() -> String {
    "2m".to_owned()
}

fn tool_definitions() -> Vec<Value> {
    vec![
        json!({
            "name": "list_sources",
            "description": "Return known log sources and recent event counts.",
            "inputSchema": {
                "type": "object",
                "properties": { "since": { "type": "string", "format": "date-time" } }
            }
        }),
        json!({
            "name": "read_recent_logs",
            "description": "Return a bounded recent log window.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "source": { "type": "string" },
                    "since": { "type": "string", "format": "date-time" },
                    "level": { "type": "string" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 500 }
                }
            }
        }),
        json!({
            "name": "search_logs",
            "description": "Search messages and metadata.",
            "inputSchema": {
                "type": "object",
                "required": ["query"],
                "properties": {
                    "query": { "type": "string" },
                    "since": { "type": "string", "format": "date-time" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 500 }
                }
            }
        }),
        json!({
            "name": "get_log_window",
            "description": "Return logs around a specific event ID.",
            "inputSchema": {
                "type": "object",
                "required": ["event_id"],
                "properties": {
                    "event_id": { "type": "string" },
                    "before": { "type": "string", "default": "5m" },
                    "after": { "type": "string", "default": "2m" },
                    "limit": { "type": "integer", "minimum": 1, "maximum": 500 }
                }
            }
        }),
        json!({
            "name": "mark_reviewed",
            "description": "Mark events reviewed after handling.",
            "inputSchema": {
                "type": "object",
                "required": ["event_ids", "note"],
                "properties": {
                    "event_ids": { "type": "array", "items": { "type": "string" } },
                    "note": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "suggest_markdown_summary",
            "description": "Use configured local or remote LLM to propose Markdown for selected event IDs.",
            "inputSchema": {
                "type": "object",
                "required": ["event_ids"],
                "properties": {
                    "event_ids": { "type": "array", "items": { "type": "string" } },
                    "vault_context": {
                        "type": "object",
                        "properties": {
                            "candidate_notes": { "type": "array", "items": { "type": "string" } },
                            "daily_note": { "type": "string" }
                        }
                    },
                    "mode": { "type": "string", "default": "daily-note" },
                    "task": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "stage_markdown_summary",
            "description": "Generate a reviewable summary and atomically write it as a new Markdown file in the configured proposal inbox.",
            "inputSchema": {
                "type": "object",
                "required": ["event_ids"],
                "properties": {
                    "event_ids": { "type": "array", "items": { "type": "string" } },
                    "vault_context": {
                        "type": "object",
                        "properties": {
                            "candidate_notes": { "type": "array", "items": { "type": "string" } },
                            "daily_note": { "type": "string" }
                        }
                    },
                    "mode": { "type": "string", "default": "daily-note" },
                    "task": { "type": "string" }
                }
            }
        }),
        json!({
            "name": "apply_markdown_proposal",
            "description": "Apply one reviewed pending proposal to its daily-note filename, mark its evidence reviewed, and remove the consumed proposal.",
            "inputSchema": {
                "type": "object",
                "required": ["proposal_id"],
                "properties": {
                    "proposal_id": { "type": "string", "pattern": "^proposal_[A-Za-z0-9_]+$" }
                }
            }
        }),
    ]
}
