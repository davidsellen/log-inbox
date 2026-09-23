use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{HeaderMap, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use log_inbox_core::{models::LogEventInput, settings::Settings, store::Store};
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, net::SocketAddr, sync::Arc};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod mcp;

use mcp::LogInboxMcp;

#[derive(Clone)]
struct AppState {
    store: Store,
    api_keys: Arc<HashSet<String>>,
}

#[derive(Debug, Serialize)]
struct IngestResponse {
    id: String,
    status: &'static str,
    truncated: bool,
}

#[derive(Debug, Deserialize)]
struct BatchRequest {
    events: Vec<LogEventInput>,
}

#[derive(Debug, Serialize)]
struct BatchResponse {
    results: Vec<BatchItemResult>,
}

#[derive(Debug, Serialize)]
struct BatchItemResult {
    index: usize,
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(default)]
    truncated: bool,
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

    let state = AppState {
        store,
        api_keys: Arc::new(settings.api_keys),
    };

    let app = build_router(state);

    let addr: SocketAddr = "0.0.0.0:8787".parse()?;
    tracing::info!(%addr, "starting collector");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn build_router(state: AppState) -> Router {
    let mcp_store = state.store.clone();
    let mcp_service: StreamableHttpService<LogInboxMcp, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(LogInboxMcp::new(mcp_store.clone())),
            Default::default(),
            StreamableHttpServerConfig::default()
                .with_legacy_session_mode(false)
                .with_json_response(true)
                .with_sse_keep_alive(None),
        );
    let mcp_router = Router::new()
        .nest_service("/mcp", mcp_service)
        .route_layer(middleware::from_fn_with_state(state.clone(), authorize_mcp));

    Router::new()
        .route("/health", get(health))
        .route("/v1/logs", post(ingest_one))
        .route("/v1/logs/batch", post(ingest_batch))
        .merge(mcp_router)
        .layer(middleware::from_fn(log_request_response))
        .with_state(state)
}

async fn authorize_mcp(
    State(state): State<AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    match authorize(request.headers(), &state.api_keys) {
        Ok(()) => next.run(request).await,
        Err(error) => error.into_response(),
    }
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

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok"
    }))
}

async fn ingest_one(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<LogEventInput>,
) -> Result<Json<IngestResponse>, ApiError> {
    authorize(&headers, &state.api_keys)?;
    let event = state.store.insert_event(input)?;
    Ok(Json(IngestResponse {
        id: event.id,
        status: "stored",
        truncated: event.truncated,
    }))
}

async fn ingest_batch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<BatchRequest>,
) -> Result<Json<BatchResponse>, ApiError> {
    authorize(&headers, &state.api_keys)?;
    let results = request
        .events
        .into_iter()
        .enumerate()
        .map(|(index, input)| match state.store.insert_event(input) {
            Ok(event) => BatchItemResult {
                index,
                status: "stored",
                id: Some(event.id),
                error: None,
                truncated: event.truncated,
            },
            Err(error) => BatchItemResult {
                index,
                status: "rejected",
                id: None,
                error: Some(error.to_string()),
                truncated: false,
            },
        })
        .collect();
    Ok(Json(BatchResponse { results }))
}

fn authorize(headers: &HeaderMap, api_keys: &HashSet<String>) -> Result<(), ApiError> {
    if api_keys.is_empty() {
        return Err(ApiError::unauthorized(
            "LOG_INBOX_API_KEYS must configure at least one key",
        ));
    }

    let Some(value) = headers
        .get("authorization")
        .and_then(|value| value.to_str().ok())
    else {
        return Err(ApiError::unauthorized("missing bearer token"));
    };
    let Some(token) = value.strip_prefix("Bearer ") else {
        return Err(ApiError::unauthorized(
            "authorization must use bearer token",
        ));
    };
    if api_keys.contains(token.trim()) {
        Ok(())
    } else {
        Err(ApiError::unauthorized("invalid bearer token"))
    }
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
}

impl ApiError {
    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }
}

impl From<anyhow::Error> for ApiError {
    fn from(error: anyhow::Error) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: error.to_string(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let body = Json(serde_json::json!({ "error": self.message }));
        (self.status, body).into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{Request, header};
    use serde_json::Value;
    use tower::ServiceExt;
    use uuid::Uuid;

    fn test_app() -> (Router, Store, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "log-inbox-collector-http-{}.sqlite3",
            Uuid::new_v4().simple()
        ));
        let store = Store::open(path.clone()).expect("test store opens");
        let state = AppState {
            store: store.clone(),
            api_keys: Arc::new(HashSet::from(["test-ingest-key".to_owned()])),
        };
        (build_router(state), store, path)
    }

    fn mcp_request(body: &'static str, token: Option<&str>) -> Request<Body> {
        let mut request = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header(header::HOST, "127.0.0.1:8787")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream");
        if let Some(token) = token {
            request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
        }
        request.body(Body::from(body)).expect("request builds")
    }

    #[tokio::test]
    async fn mcp_requires_the_existing_ingest_bearer_token() {
        let (app, store, path) = test_app();
        let body = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26","capabilities":{},"clientInfo":{"name":"test","version":"1.0"}}}"#;

        let missing = app
            .clone()
            .oneshot(mcp_request(body, None))
            .await
            .expect("router responds");
        assert_eq!(missing.status(), StatusCode::UNAUTHORIZED);

        let invalid = app
            .clone()
            .oneshot(mcp_request(body, Some("wrong-key")))
            .await
            .expect("router responds");
        assert_eq!(invalid.status(), StatusCode::UNAUTHORIZED);

        let valid = app
            .oneshot(mcp_request(body, Some("test-ingest-key")))
            .await
            .expect("router responds");
        assert_eq!(valid.status(), StatusCode::OK);
        assert_eq!(
            valid
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok()),
            Some("application/json")
        );
        let bytes = axum::body::to_bytes(valid.into_body(), 1024 * 1024)
            .await
            .expect("response body reads");
        let response: Value = serde_json::from_slice(&bytes).expect("response is JSON");
        assert_eq!(response["result"]["serverInfo"]["name"], "log-inbox");

        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn mcp_tool_call_returns_structured_content_and_persists() {
        let (app, store, path) = test_app();
        let body = r#"{
            "jsonrpc":"2.0",
            "id":2,
            "method":"tools/call",
            "params":{
                "name":"log_activity",
                "arguments":{
                    "source":"codex/protocol-test",
                    "level":"info",
                    "message":"Stored through MCP protocol",
                    "metadata":{"task_id":"mcp-protocol-test","event_type":"complete"}
                },
                "_meta":{
                    "io.modelcontextprotocol/protocolVersion":"2026-07-28",
                    "io.modelcontextprotocol/clientInfo":{"name":"test","version":"1.0"},
                    "io.modelcontextprotocol/clientCapabilities":{}
                }
            }
        }"#;
        let request = Request::builder()
            .method("POST")
            .uri("/mcp")
            .header(header::HOST, "127.0.0.1:8787")
            .header(header::AUTHORIZATION, "Bearer test-ingest-key")
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::ACCEPT, "application/json, text/event-stream")
            .header("MCP-Protocol-Version", "2026-07-28")
            .header("Mcp-Method", "tools/call")
            .header("Mcp-Name", "log_activity")
            .body(Body::from(body))
            .expect("request builds");

        let response = app.oneshot(request).await.expect("router responds");
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("response body reads");
        let response: Value = serde_json::from_slice(&bytes).expect("response is JSON");
        assert_eq!(response["result"]["structuredContent"]["status"], "stored");
        assert_eq!(response["result"]["isError"], false);

        let events = store
            .query_logs(log_inbox_core::models::LogQuery {
                source: Some("codex/protocol-test".to_owned()),
                since: None,
                level: None,
                query: None,
                limit: Some(10),
            })
            .expect("stored event reads");
        assert_eq!(events.events.len(), 1);
        assert_eq!(events.events[0].message, "Stored through MCP protocol");

        drop(store);
        let _ = std::fs::remove_file(path);
    }
}
