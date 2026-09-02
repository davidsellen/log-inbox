use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use log_inbox_core::{models::LogEventInput, settings::Settings, store::Store};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, net::SocketAddr, sync::Arc};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Clone)]
struct AppState {
    store: Store,
    api_keys: Arc<HashSet<String>>,
    retention_days: u64,
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
    store.prune_old_events(settings.retention_days)?;

    let state = AppState {
        store,
        api_keys: Arc::new(settings.api_keys),
        retention_days: settings.retention_days,
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/v1/logs", post(ingest_one))
        .route("/v1/logs/batch", post(ingest_batch))
        .layer(TraceLayer::new_for_http())
        .with_state(state);

    let addr: SocketAddr = "0.0.0.0:8787".parse()?;
    tracing::info!(%addr, "starting collector");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health(State(state): State<AppState>) -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "status": "ok",
        "retention_days": state.retention_days
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
