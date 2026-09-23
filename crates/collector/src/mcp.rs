use chrono::{DateTime, Utc};
use log_inbox_core::{models::LogEventInput, store::Store};
use rmcp::{
    ServerHandler,
    handler::server::{
        router::tool::ToolRouter,
        wrapper::{Json, Parameters},
    },
    model::{Implementation, ServerCapabilities, ServerConfig},
    schemars, tool, tool_handler, tool_router,
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct LogActivityRequest {
    #[schemars(description = "Stable producer identity, normally codex/<host-id>.")]
    pub source: String,
    #[serde(default)]
    #[schemars(description = "Optional severity such as info, warning, or error.")]
    pub level: Option<String>,
    #[serde(default)]
    #[schemars(
        description = "Optional ISO 8601 event timestamp. Collector receive time is used when omitted."
    )]
    pub timestamp: Option<String>,
    #[schemars(description = "Concise human-readable activity or outcome.")]
    pub message: String,
    #[serde(default)]
    #[schemars(
        description = "Structured activity facts such as task_id, session_id, sequence, event_type, status, repository, branch, modules, and validation."
    )]
    pub metadata: Option<Map<String, Value>>,
    #[serde(default)]
    #[schemars(description = "Optional producer fingerprint.")]
    pub fingerprint: Option<String>,
}

#[derive(Debug, Serialize, schemars::JsonSchema)]
pub struct LogActivityResponse {
    pub id: String,
    pub status: &'static str,
    pub truncated: bool,
}

#[derive(Debug, Clone)]
pub struct LogInboxMcp {
    store: Store,
    tool_router: ToolRouter<Self>,
}

impl LogInboxMcp {
    pub fn new(store: Store) -> Self {
        Self {
            store,
            tool_router: Self::tool_router(),
        }
    }
}

#[tool_router(router = tool_router)]
impl LogInboxMcp {
    #[tool(
        name = "log_activity",
        description = "Store one bounded Log Inbox activity event. Use one call for task start and one for the terminal outcome; reuse task_id and session_id in metadata and increment sequence.",
        annotations(
            title = "Log activity",
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    pub async fn log_activity(
        &self,
        Parameters(request): Parameters<LogActivityRequest>,
    ) -> Result<Json<LogActivityResponse>, String> {
        let timestamp = request
            .timestamp
            .map(|value| {
                value
                    .parse::<DateTime<Utc>>()
                    .map_err(|_| "timestamp must be an ISO 8601 date-time".to_owned())
            })
            .transpose()?;
        let event = self
            .store
            .insert_event(LogEventInput {
                source: request.source,
                level: request.level,
                timestamp,
                message: request.message,
                metadata: request.metadata,
                fingerprint: request.fingerprint,
            })
            .map_err(|error| error.to_string())?;

        Ok(Json(LogActivityResponse {
            id: event.id,
            status: "stored",
            truncated: event.truncated,
        }))
    }
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for LogInboxMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("log-inbox", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Use log_activity only for concise agent lifecycle reporting. Never send secrets, source contents, full diffs, personal data, or large command output.",
            )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use log_inbox_core::models::LogQuery;
    use std::path::PathBuf;
    use uuid::Uuid;

    fn test_store() -> (Store, PathBuf) {
        let path = std::env::temp_dir().join(format!(
            "log-inbox-collector-mcp-{}.sqlite3",
            Uuid::new_v4().simple()
        ));
        (Store::open(path.clone()).expect("test store opens"), path)
    }

    #[test]
    fn exposes_only_the_ingestion_tool_with_safe_annotations() {
        let (store, path) = test_store();
        let server = LogInboxMcp::new(store.clone());
        let tools = server.tool_router.list_all();

        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0].name, "log_activity");
        let annotations = tools[0]
            .annotations
            .as_ref()
            .expect("tool annotations are declared");
        assert_eq!(annotations.read_only_hint, Some(false));
        assert_eq!(annotations.destructive_hint, Some(false));
        assert_eq!(annotations.idempotent_hint, Some(false));
        assert_eq!(annotations.open_world_hint, Some(false));

        drop(server);
        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn log_activity_persists_one_event() {
        let (store, path) = test_store();
        let server = LogInboxMcp::new(store.clone());
        let result = server
            .log_activity(Parameters(LogActivityRequest {
                source: "codex/test-host".to_owned(),
                level: Some("info".to_owned()),
                timestamp: None,
                message: "Completed MCP reporting test".to_owned(),
                metadata: Some(Map::from_iter([(
                    "event_type".to_owned(),
                    Value::String("complete".to_owned()),
                )])),
                fingerprint: None,
            }))
            .await
            .expect("tool succeeds");

        assert_eq!(result.0.status, "stored");
        let events = store
            .query_logs(LogQuery {
                source: Some("codex/test-host".to_owned()),
                since: None,
                level: None,
                query: None,
                limit: Some(10),
            })
            .expect("stored event reads");
        assert_eq!(events.events.len(), 1);
        assert_eq!(events.events[0].id, result.0.id);

        drop(server);
        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn log_activity_rejects_an_invalid_timestamp_without_writing() {
        let (store, path) = test_store();
        let server = LogInboxMcp::new(store.clone());
        let result = server
            .log_activity(Parameters(LogActivityRequest {
                source: "codex/test-host".to_owned(),
                level: None,
                timestamp: Some("not-a-date".to_owned()),
                message: "Must not be stored".to_owned(),
                metadata: None,
                fingerprint: None,
            }))
            .await;
        let error = match result {
            Ok(_) => panic!("invalid timestamp must fail"),
            Err(error) => error,
        };

        assert!(error.contains("ISO 8601"));
        assert!(
            store
                .query_logs(LogQuery {
                    source: None,
                    since: None,
                    level: None,
                    query: None,
                    limit: Some(10),
                })
                .expect("events read")
                .events
                .is_empty()
        );

        drop(server);
        drop(store);
        let _ = std::fs::remove_file(path);
    }
}
