use axum::{
    Json, Router,
    body::Body,
    extract::{Path as AxumPath, Query, State},
    http::{HeaderMap, Request, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{delete, get, post, put},
};
use chrono::{DateTime, Duration, NaiveDate, NaiveTime, Utc};
use log_inbox_core::{
    auth::{
        DASHBOARD_SCOPES, generate_session_credentials, hash_owner_secret, verify_owner_secret,
    },
    daily::{render_daily_path, resolve_day, resolve_local_time},
    models::{
        ApplyOperation, ContextComparison, ContextMapping, DailyDay, DailyRevisionContent,
        IgnoredContextIdentity, LinkSelector, LogQuery, PrepareApplyOperation, ProposalRevision,
        WorkspaceProfile,
    },
    settings::Settings,
    store::Store,
    workspace::{InspectedWorkspace, MarkdownPathMode, normalize_knowledge_collection_paths},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    env,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod daily_writer;
mod generation;
mod history;
pub mod knowledge;
mod llm;
mod migration;

#[derive(Clone)]
struct AppState {
    store: Store,
    llm_config: Option<llm::LlmConfig>,
    legacy_proposal_dir: Option<PathBuf>,
    legacy_support_files: Vec<(String, PathBuf)>,
    apply_lock: Arc<Mutex<()>>,
    knowledge_write_lock: Arc<Mutex<()>>,
    daily_generation_lock: Arc<tokio::sync::Mutex<()>>,
    generation_cancel: generation::CancelState,
    refocus: RefocusConfig,
    workspace: InspectedWorkspace,
}

#[derive(Clone)]
struct RefocusConfig {
    allowed_hosts: HashSet<String>,
    allowed_origins: HashSet<String>,
}

struct EffectiveDailyWindow {
    timezone: String,
    start_utc: DateTime<Utc>,
    end_utc: DateTime<Utc>,
    destination_path: String,
}

impl RefocusConfig {
    fn from_env(store: &Store) -> anyhow::Result<Self> {
        let owner_secret = env::var("LOG_INBOX_OWNER_SECRET").map_err(|_| {
            anyhow::anyhow!("LOG_INBOX_OWNER_SECRET is required and must contain at least 20 bytes")
        })?;
        match store.owner_secret_hash()? {
            None => store.set_owner_secret_hash(&hash_owner_secret(&owner_secret)?)?,
            Some(hash) if verify_owner_secret(&owner_secret, &hash) => {}
            Some(_) if env::var("LOG_INBOX_ROTATE_OWNER_SECRET").as_deref() == Ok("1") => {
                store.set_owner_secret_hash(&hash_owner_secret(&owner_secret)?)?;
            }
            Some(_) => anyhow::bail!(
                "configured owner secret does not match; set LOG_INBOX_ROTATE_OWNER_SECRET=1 for an explicit rotation"
            ),
        }
        let allowed_hosts = env_list("LOG_INBOX_ALLOWED_HOSTS", "127.0.0.1:8788,localhost:8788");
        let allowed_origins = env_list(
            "LOG_INBOX_ALLOWED_ORIGINS",
            "http://127.0.0.1:8788,http://localhost:8788",
        );
        Ok(Self {
            allowed_hosts,
            allowed_origins,
        })
    }
}

fn env_list(name: &str, fallback: &str) -> HashSet<String> {
    env::var(name)
        .unwrap_or_else(|_| fallback.to_owned())
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

#[derive(Debug, Deserialize)]
struct LoginRequest {
    owner_secret: String,
    #[serde(default)]
    remember_me: bool,
}

#[derive(Debug, Deserialize)]
struct ManualDailyEntryRequest {
    text: String,
    #[serde(default)]
    references: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct EvidenceDecisionRequest {
    expected_revision_id: String,
    disposition: String,
    related_event_id: Option<String>,
    reason: Option<String>,
}

#[derive(Debug, Deserialize)]
struct EvidenceDecisionBatchRequest {
    expected_revision_id: String,
    decisions: Vec<EvidenceDecisionBatchItem>,
}

#[derive(Debug, Deserialize)]
struct EvidenceDecisionBatchItem {
    event_id: String,
    disposition: String,
}

#[derive(Debug, Deserialize)]
struct ExpectedRevisionRequest {
    expected_revision_id: String,
}

#[derive(Debug, Deserialize)]
struct EditDailyCandidateRequest {
    expected_revision_id: String,
    content: DailyRevisionContent,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct KnowledgeExcerptExclusionRequest {
    workstream_id: String,
    note_path: String,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct GenerateDailyRequest {
    #[serde(default)]
    replace_edited: bool,
    #[serde(default)]
    reference_mode: generation::ReferenceMode,
    expected_revision_id: Option<String>,
    context_exclusions: Option<Vec<KnowledgeExcerptExclusionRequest>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct StartContextComparisonRequest {
    expected_revision_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DecideContextComparisonRequest {
    expected_revision_id: String,
    usefulness: String,
    less_editing: String,
    continue_with: String,
    note: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DailyOverviewQuery {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyDailyRequest {
    expected_revision_id: String,
    expected_revision_content_hash: String,
    destination_path: String,
    expected_old_block_hash: Option<String>,
    intended_new_block_hash: String,
    expected_target_exists: bool,
    expected_original_content_hash: String,
    expected_updated_content_hash: String,
}

struct DailyApplyMaterial {
    profile: WorkspaceProfile,
    day: DailyDay,
    revision: ProposalRevision,
    target: PathBuf,
    target_exists: bool,
    template_used: Option<String>,
    original_content: Vec<u8>,
    plan: daily_writer::ManagedBlockPlan,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct WorkspaceSettingsDraft {
    timezone: String,
    daily_root: String,
    daily_pattern: String,
    template_path: Option<String>,
    link_style: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveWorkspaceSettingsRequest {
    settings: WorkspaceSettingsDraft,
    preview_digest: String,
    expected_profile_id: Option<String>,
    expected_updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveAutomationSettingsRequest {
    enabled: bool,
    generation_time: String,
    catch_up_days: u16,
    raw_retention_days: u16,
    audit_retention_days: u16,
    recovery_retention_days: u16,
    expected_updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct KnowledgeCollectionDraft {
    label: String,
    purpose: String,
    roots: Vec<String>,
    #[serde(default)]
    exclusions: Vec<String>,
    enabled: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveKnowledgeCollectionRequest {
    collection: KnowledgeCollectionDraft,
    preview_digest: String,
    expected_updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteKnowledgeCollectionRequest {
    expected_updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct KnowledgeFolderQuery {
    query: String,
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct KnowledgeReviewQuery {
    limit: Option<usize>,
}

#[derive(Debug, Deserialize)]
struct KnowledgeNoteQuery {
    query: String,
    limit: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ContextMappingDraft {
    field: String,
    value: String,
    canonical_note_path: String,
    #[serde(default = "default_true")]
    enabled: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SaveContextMappingRequest {
    mapping: ContextMappingDraft,
    expected_updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DeleteContextMappingRequest {
    expected_updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct IgnoreContextIdentityRequest {
    field: String,
    value: String,
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
    let refocus = RefocusConfig::from_env(&store)?;
    let configured_root = env::var_os("LOG_INBOX_WORKSPACE_DIR")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/workspace"));
    let workspace = InspectedWorkspace::inspect(&configured_root)?;
    let state = AppState {
        store,
        llm_config: llm::LlmConfig::from_env(),
        legacy_proposal_dir: env::var_os("LOG_INBOX_MIGRATION_PROPOSAL_DIR")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from),
        legacy_support_files: [
            ("context_file", "LOG_INBOX_MIGRATION_CONTEXT_FILE"),
            (
                "product_index_file",
                "LOG_INBOX_MIGRATION_PRODUCT_INDEX_FILE",
            ),
        ]
        .into_iter()
        .filter_map(|(kind, name)| {
            env::var_os(name)
                .filter(|path| !path.is_empty())
                .map(|path| (kind.to_owned(), PathBuf::from(path)))
        })
        .collect(),
        apply_lock: Arc::new(Mutex::new(())),
        knowledge_write_lock: Arc::new(Mutex::new(())),
        daily_generation_lock: Arc::new(tokio::sync::Mutex::new(())),
        generation_cancel: Arc::new(Mutex::new(None)),
        refocus,
        workspace,
    };
    state.store.interrupt_generation_attempts(Utc::now())?;
    recover_daily_applies(&state);
    match state
        .store
        .recover_interrupted_daily_schedule_runs(Utc::now())
    {
        Ok(count) if count > 0 => {
            tracing::warn!(count, "Recovered interrupted Daily preparation runs")
        }
        Ok(_) => {}
        Err(error) => {
            tracing::warn!(error = %error, "Could not recover interrupted Daily preparation runs")
        }
    }
    let scheduler_state = state.clone();
    tokio::spawn(async move {
        run_daily_scheduler(scheduler_state).await;
    });

    let app = build_router(state);

    let addr: SocketAddr = "0.0.0.0:8788".parse()?;
    tracing::info!(%addr, "starting mcp server");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/", get(dashboard_page))
        .route("/assets/{name}", get(dashboard_asset))
        .route("/favicon.ico", get(favicon))
        .route("/health", get(health))
        .route("/api/v2/auth/login", post(refocus_login))
        .route("/api/v2/auth/session", get(refocus_session))
        .route("/api/v2/auth/logout", post(refocus_logout))
        .route(
            "/api/v2/settings/workspace",
            get(refocus_workspace_settings).put(refocus_save_workspace_settings),
        )
        .route(
            "/api/v2/settings/workspace/preview",
            post(refocus_preview_workspace_settings),
        )
        .route(
            "/api/v2/settings/automation",
            get(refocus_automation_settings).put(refocus_save_automation_settings),
        )
        .route(
            "/api/v2/knowledge/collections",
            get(refocus_knowledge_collections).post(refocus_create_knowledge_collection),
        )
        .route(
            "/api/v2/knowledge/collections/preview",
            post(refocus_preview_knowledge_collection),
        )
        .route(
            "/api/v2/knowledge/collections/{id}",
            put(refocus_update_knowledge_collection).delete(refocus_delete_knowledge_collection),
        )
        .route("/api/v2/knowledge/folders", get(refocus_knowledge_folders))
        .route("/api/v2/knowledge/review", get(refocus_knowledge_review))
        .route("/api/v2/knowledge/notes", get(refocus_knowledge_notes))
        .route(
            "/api/v2/knowledge/mappings",
            post(refocus_create_context_mapping),
        )
        .route(
            "/api/v2/knowledge/mappings/{id}",
            put(refocus_update_context_mapping).delete(refocus_delete_context_mapping),
        )
        .route(
            "/api/v2/knowledge/ignored",
            post(refocus_ignore_context_identity),
        )
        .route(
            "/api/v2/knowledge/ignored/{id}",
            axum::routing::delete(refocus_reopen_context_identity),
        )
        .route(
            "/api/v2/migration/cutover",
            get(refocus_cutover_report).post(refocus_commit_cutover),
        )
        .route("/api/v2/daily/overview", get(refocus_daily_overview))
        .route("/api/v2/history/search", get(history::search))
        .route("/api/v2/daily/{date}/activity/{id}", get(history::activity))
        .route(
            "/api/v2/settings/dashboard",
            get(history::preferences).put(history::save_preferences),
        )
        .route("/api/v2/daily/{date}", get(refocus_daily_day))
        .route("/api/v2/daily/{date}/context", get(refocus_daily_context))
        .route(
            "/api/v2/daily/{date}/apply-preview",
            get(refocus_daily_apply_preview),
        )
        .route("/api/v2/daily/{date}/apply", post(refocus_daily_apply))
        .route(
            "/api/v2/daily/{date}/apply/{operation_id}/retry",
            post(refocus_retry_daily_apply),
        )
        .route(
            "/api/v2/daily/{date}/generate",
            post(refocus_generate_daily),
        )
        .route(
            "/api/v2/daily/{date}/activity-record",
            post(refocus_create_activity_record),
        )
        .route(
            "/api/v2/daily/{date}/generation/{attempt_id}/cancel",
            post(generation::cancel),
        )
        .route(
            "/api/v2/daily/{date}/context-comparisons",
            post(refocus_start_context_comparison),
        )
        .route(
            "/api/v2/daily/{date}/context-comparisons/{comparison_id}/decision",
            post(refocus_decide_context_comparison),
        )
        .route(
            "/api/v2/daily/{date}/manual",
            post(refocus_create_manual_entry),
        )
        .route(
            "/api/v2/daily/{date}/manual/{entry_id}",
            delete(refocus_delete_manual_entry),
        )
        .route(
            "/api/v2/daily/{date}/dismiss",
            post(refocus_dismiss_daily).delete(refocus_reopen_daily),
        )
        .route(
            "/api/v2/daily/{date}/evidence",
            put(refocus_decide_daily_evidence_batch),
        )
        .route(
            "/api/v2/daily/{date}/evidence/{event_id}",
            put(refocus_decide_daily_evidence).delete(refocus_reopen_daily_evidence),
        )
        .route(
            "/api/v2/daily/{date}/late-evidence/{event_id}",
            post(refocus_defer_daily_evidence).delete(refocus_reopen_deferred_daily_evidence),
        )
        .route(
            "/api/v2/daily/{date}/candidate",
            put(refocus_edit_daily_candidate),
        )
        .layer(middleware::from_fn(log_request_response))
        .with_state(state)
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

async fn refocus_login(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<LoginRequest>,
) -> Result<Response, ApiError> {
    validate_request_boundary(&state.refocus, &headers, true)?;
    let owner_hash = state
        .store
        .owner_secret_hash()
        .map_err(|error| ApiError::internal(error.to_string()))?
        .ok_or_else(|| ApiError::internal("owner authentication is not initialized"))?;
    if !verify_owner_secret(&input.owner_secret, &owner_hash) {
        return Err(ApiError::unauthorized("owner secret is not valid"));
    }
    let credentials = generate_session_credentials();
    let (idle_ttl, absolute_ttl, cookie_max_age) = if input.remember_me {
        (Duration::days(30), Duration::days(30), 30 * 24 * 60 * 60)
    } else {
        (Duration::minutes(30), Duration::hours(8), 8 * 60 * 60)
    };
    state
        .store
        .create_dashboard_session(
            &credentials,
            &DASHBOARD_SCOPES
                .iter()
                .map(|scope| (*scope).to_owned())
                .collect::<Vec<_>>(),
            Utc::now(),
            idle_ttl,
            absolute_ttl,
        )
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let secure = if request_uses_https(&headers) {
        "; Secure"
    } else {
        ""
    };
    let cookie = format!(
        "log_inbox_session={}; Path=/; HttpOnly; SameSite=Strict; Max-Age={cookie_max_age}{secure}",
        credentials.session_token,
    );
    Ok((
        [(header::SET_COOKIE, cookie)],
        Json(json!({ "csrf_token": credentials.csrf_token, "expires_in_seconds": cookie_max_age })),
    )
        .into_response())
}

async fn refocus_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    validate_request_boundary(&state.refocus, &headers, false)?;
    let token = session_cookie(&headers)
        .ok_or_else(|| ApiError::unauthorized("dashboard session cookie is missing"))?;
    let (session, csrf_token) = state
        .store
        .refresh_dashboard_session_csrf(token, "logs:read", Utc::now(), Duration::days(30))
        .map_err(|_| ApiError::unauthorized("dashboard session is not authorized"))?;
    Ok(Json(json!({
        "authenticated": true,
        "scopes": session.scopes,
        "absolute_expires_at": session.absolute_expires_at,
        "csrf_token": csrf_token
    })))
}

async fn refocus_logout(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Response, ApiError> {
    authorize_refocus(&state, &headers, "logs:read", true)?;
    let token = session_cookie(&headers)
        .ok_or_else(|| ApiError::unauthorized("dashboard session cookie is missing"))?;
    state
        .store
        .revoke_dashboard_session(token)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let secure = if request_uses_https(&headers) {
        "; Secure"
    } else {
        ""
    };
    let cookie =
        format!("log_inbox_session=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{secure}");
    Ok(([(header::SET_COOKIE, cookie)], StatusCode::NO_CONTENT).into_response())
}

async fn refocus_workspace_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "logs:read", false)?;
    let workspace = inspect_refocus_workspace(&state)?;
    let active = state
        .store
        .active_workspace_profile()
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let binding_matches = active
        .as_ref()
        .is_some_and(|profile| profile.root_binding == workspace.root_binding());
    Ok(Json(json!({
        "workspace_path": workspace.canonical_root().display().to_string(),
        "active_profile": active,
        "binding_matches": binding_matches
    })))
}

async fn refocus_automation_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "logs:read", false)?;
    let profile = active_refocus_workspace(&state)?;
    let settings = state
        .store
        .daily_automation_settings(&profile.id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let saved = settings.updated_at != DateTime::<Utc>::UNIX_EPOCH;
    let recent_runs = state
        .store
        .daily_schedule_runs(&profile.id, 14)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(Json(json!({
        "settings": settings,
        "saved": saved,
        "recent_runs": recent_runs,
        "writes_markdown_automatically": false
    })))
}

async fn refocus_save_automation_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<SaveAutomationSettingsRequest>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "settings:write", true)?;
    let profile = active_refocus_workspace(&state)?;
    let settings = state
        .store
        .save_daily_automation_settings(
            &profile.id,
            input.enabled,
            input.generation_time.trim(),
            input.catch_up_days,
            input.raw_retention_days,
            input.audit_retention_days,
            input.recovery_retention_days,
            input.expected_updated_at,
        )
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    let requeued_failed_runs = state
        .store
        .retry_all_failed_daily_schedule_runs(&profile.id, Utc::now())
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(Json(json!({
        "settings": settings,
        "saved": true,
        "requeued_failed_runs": requeued_failed_runs,
        "writes_markdown_automatically": false
    })))
}

async fn refocus_knowledge_collections(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "knowledge:read", false)?;
    let profile = active_refocus_workspace(&state)?;
    let collections = state
        .store
        .list_knowledge_collections(&profile.id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(Json(json!({
        "workspace_id": profile.id,
        "collections": collections,
        "count": collections.len(),
        "limit": 8
    })))
}

async fn refocus_preview_knowledge_collection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<KnowledgeCollectionDraft>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "knowledge:read", true)?;
    let preview = preview_knowledge_collection(&state, input)?;
    Ok(Json(preview.as_json(false)))
}

async fn refocus_create_knowledge_collection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<SaveKnowledgeCollectionRequest>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    authorize_refocus(&state, &headers, "settings:write", true)?;
    if input.expected_updated_at.is_some() {
        return Err(ApiError::bad_request(
            "a new Knowledge collection cannot have an expected timestamp",
        ));
    }
    let preview = preview_knowledge_collection(&state, input.collection)?;
    if preview.preview_digest != input.preview_digest {
        return Err(ApiError::conflict(
            "Knowledge collection differs from the reviewed preview",
        ));
    }
    let collection = state
        .store
        .save_knowledge_collection(
            None,
            &preview.workspace_id,
            &preview.collection.label,
            &preview.collection.purpose,
            &preview.collection.roots,
            &preview.collection.exclusions,
            preview.collection.enabled,
            None,
        )
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "collection": collection, "changes_saved": true })),
    ))
}

async fn refocus_update_knowledge_collection(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Json(input): Json<SaveKnowledgeCollectionRequest>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "settings:write", true)?;
    let expected_updated_at = input.expected_updated_at.ok_or_else(|| {
        ApiError::bad_request("Knowledge collection update requires its expected timestamp")
    })?;
    let preview = preview_knowledge_collection(&state, input.collection)?;
    if preview.preview_digest != input.preview_digest {
        return Err(ApiError::conflict(
            "Knowledge collection differs from the reviewed preview",
        ));
    }
    let collection = state
        .store
        .save_knowledge_collection(
            Some(&id),
            &preview.workspace_id,
            &preview.collection.label,
            &preview.collection.purpose,
            &preview.collection.roots,
            &preview.collection.exclusions,
            preview.collection.enabled,
            Some(expected_updated_at),
        )
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    Ok(Json(
        json!({ "collection": collection, "changes_saved": true }),
    ))
}

async fn refocus_delete_knowledge_collection(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Json(input): Json<DeleteKnowledgeCollectionRequest>,
) -> Result<StatusCode, ApiError> {
    authorize_refocus(&state, &headers, "settings:write", true)?;
    let profile = active_refocus_workspace(&state)?;
    state
        .store
        .delete_knowledge_collection(&id, &profile.id, input.expected_updated_at)
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn refocus_knowledge_folders(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(input): Query<KnowledgeFolderQuery>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "knowledge:read", false)?;
    let (_, workspace) = active_refocus_context(&state)?;
    let query = input.query.trim();
    if !(2..=100).contains(&query.len()) {
        return Err(ApiError::bad_request(
            "folder search requires 2-100 characters",
        ));
    }
    let limit = input.limit.unwrap_or(20);
    if !(1..=20).contains(&limit) {
        return Err(ApiError::bad_request(
            "folder search limit must be between 1 and 20",
        ));
    }
    let query = query.to_lowercase();
    let folders = workspace
        .list_markdown_folders(10_000)
        .map_err(|error| ApiError::unprocessable(error.to_string()))?
        .into_iter()
        .filter(|folder| {
            folder.path.to_lowercase().contains(&query)
                || (folder.path == "." && "workspace root".contains(&query))
        })
        .take(limit)
        .map(|folder| folder.path)
        .collect::<Vec<_>>();
    Ok(Json(json!({ "folders": folders, "limit": limit })))
}

async fn refocus_knowledge_review(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(input): Query<KnowledgeReviewQuery>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "knowledge:read", false)?;
    authorize_refocus(&state, &headers, "logs:read", false)?;
    let (profile, workspace) = active_refocus_context(&state)?;
    let limit = input.limit.unwrap_or(50);
    if !(1..=100).contains(&limit) {
        return Err(ApiError::bad_request(
            "Knowledge review limit must be between 1 and 100",
        ));
    }
    let collections = state
        .store
        .list_knowledge_collections(&profile.id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let mappings = state
        .store
        .list_context_mappings(&profile.id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let ignored = state
        .store
        .list_ignored_context_identities(&profile.id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let evidence = state
        .store
        .query_logs(LogQuery {
            source: None,
            since: None,
            level: None,
            query: None,
            limit: Some(500),
        })
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let resolution =
        knowledge::resolve_knowledge(&workspace, &collections, &mappings, &evidence.events);
    let (review_status, unresolved, diagnostics) = match resolution.as_ref() {
        Ok(resolution) => (
            "ready",
            knowledge::curate_unresolved_identities(
                &evidence.events,
                &mappings,
                &ignored,
                resolution.as_ref(),
                limit,
            ),
            resolution
                .as_ref()
                .and_then(|resolution| resolution.snapshot_payload.get("diagnostics"))
                .map(public_knowledge_diagnostics)
                .unwrap_or_else(|| json!({})),
        ),
        Err(error) => (
            "unavailable",
            knowledge::UnresolvedIdentityReview {
                identities: Vec::new(),
                total_count: 0,
                truncated: false,
            },
            json!({
                "resolution_failed": true,
                "resolution_error_code": public_knowledge_resolution_error_code(&error),
            }),
        ),
    };
    let mappings = mappings
        .into_iter()
        .map(|mapping| {
            let target_status = if !mapping.enabled {
                "paused"
            } else if workspace
                .resolve_markdown_path(
                    Path::new(&mapping.canonical_note_path),
                    MarkdownPathMode::ExistingFile,
                )
                .is_ok()
            {
                "ready"
            } else {
                "target_missing"
            };
            json!({"mapping": public_context_mapping(&mapping), "target_status": target_status})
        })
        .collect::<Vec<_>>();
    let ignored = ignored
        .iter()
        .map(public_ignored_context_identity)
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "workspace_id": profile.id,
        "review_status": review_status,
        "unresolved": unresolved,
        "mappings": mappings,
        "ignored": ignored,
        "diagnostics": diagnostics,
        "evidence": {
            "considered_count": evidence.events.len(),
            "limit": evidence.limit,
            "truncated": evidence.truncated,
        }
    })))
}

fn public_knowledge_resolution_error_code(error: &str) -> &'static str {
    if error.contains("not available in the selected reference collections")
        || error.contains("matching reference mapping points to an unavailable note")
    {
        "mapping_outside_collections"
    } else if error.contains("catalog exceeds") {
        "catalog_limit"
    } else {
        "resolver_error"
    }
}

fn public_context_mapping(mapping: &ContextMapping) -> Value {
    json!({
        "id": mapping.id,
        "selectors": mapping.selectors,
        "canonical_note_path": mapping.canonical_note_path,
        "enabled": mapping.enabled,
        "created_at": mapping.created_at,
        "updated_at": mapping.updated_at,
        "imported": mapping.source_identity.is_some(),
    })
}

fn public_ignored_context_identity(identity: &IgnoredContextIdentity) -> Value {
    json!({
        "id": identity.id,
        "field": identity.field,
        "value": identity.value,
        "normalized_value": identity.normalized_value,
        "created_at": identity.created_at,
        "imported": identity.source_identity.is_some(),
    })
}

fn public_knowledge_diagnostics(diagnostics: &Value) -> Value {
    let count = |field: &str| {
        diagnostics
            .get(field)
            .and_then(Value::as_u64)
            .unwrap_or_default()
    };
    json!({
        "missing_root_count": diagnostics.get("missing_roots").and_then(Value::as_array).map(Vec::len).unwrap_or_default(),
        "oversized_note_count": count("oversized_note_count"),
        "unreadable_note_count": count("unreadable_note_count"),
        "invalid_note_count": count("invalid_note_count"),
        "invalid_mapping_count": count("invalid_mapping_count"),
        "ambiguous_group_count": count("ambiguous_group_count"),
    })
}

async fn refocus_knowledge_notes(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(input): Query<KnowledgeNoteQuery>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "knowledge:read", false)?;
    let (profile, workspace) = active_refocus_context(&state)?;
    let query = input.query.trim();
    if !(2..=100).contains(&query.len()) {
        return Err(ApiError::bad_request(
            "note search requires 2-100 characters",
        ));
    }
    let limit = input.limit.unwrap_or(20);
    if !(1..=20).contains(&limit) {
        return Err(ApiError::bad_request(
            "note search limit must be between 1 and 20",
        ));
    }
    let collections = state
        .store
        .list_knowledge_collections(&profile.id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let notes = knowledge::search_note_options(&workspace, &collections, query, limit)
        .map_err(|_| ApiError::unprocessable("Knowledge notes could not be searched"))?;
    Ok(Json(json!({"notes": notes, "limit": limit})))
}

fn reviewed_mapping_target(
    state: &AppState,
    draft: &ContextMappingDraft,
) -> Result<(WorkspaceProfile, String, String), ApiError> {
    let (profile, workspace) = active_refocus_context(state)?;
    let collections = state
        .store
        .list_knowledge_collections(&profile.id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let target = draft.canonical_note_path.trim();
    let note = knowledge::find_note_option(&workspace, &collections, target)
        .map_err(|_| ApiError::unprocessable("Knowledge note could not be validated"))?
        .ok_or_else(|| {
            ApiError::conflict("Choose an existing note from an enabled Knowledge collection")
        })?;
    Ok((profile, draft.field.trim().to_lowercase(), note.path))
}

async fn refocus_create_context_mapping(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<SaveContextMappingRequest>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    authorize_refocus(&state, &headers, "settings:write", true)?;
    if input.expected_updated_at.is_some() {
        return Err(ApiError::bad_request(
            "a new Knowledge link cannot have an expected timestamp",
        ));
    }
    let (profile, field, canonical_note_path) = reviewed_mapping_target(&state, &input.mapping)?;
    let _guard = state
        .knowledge_write_lock
        .lock()
        .map_err(|_| ApiError::internal("Knowledge write lock is unavailable"))?;
    ensure_unique_context_mapping(&state, &profile.id, &field, &input.mapping.value, None)?;
    let mapping = state
        .store
        .save_context_mapping(&ContextMapping {
            id: String::new(),
            workspace_id: profile.id,
            selectors: vec![LinkSelector {
                field,
                operator: "exact".to_owned(),
                value: input.mapping.value,
            }],
            canonical_note_path,
            enabled: input.mapping.enabled,
            source_identity: None,
            source_digest: None,
            created_at: DateTime::<Utc>::UNIX_EPOCH,
            updated_at: DateTime::<Utc>::UNIX_EPOCH,
        })
        .map_err(context_mapping_create_error)?;
    Ok((
        StatusCode::CREATED,
        Json(json!({"mapping": mapping, "affects_new_candidates_only": true})),
    ))
}

async fn refocus_update_context_mapping(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Json(input): Json<SaveContextMappingRequest>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "settings:write", true)?;
    let expected = input.expected_updated_at.ok_or_else(|| {
        ApiError::bad_request("Knowledge link update requires its expected timestamp")
    })?;
    let (profile, field, canonical_note_path) = reviewed_mapping_target(&state, &input.mapping)?;
    let _guard = state
        .knowledge_write_lock
        .lock()
        .map_err(|_| ApiError::internal("Knowledge write lock is unavailable"))?;
    ensure_unique_context_mapping(&state, &profile.id, &field, &input.mapping.value, Some(&id))?;
    let mapping = state
        .store
        .update_context_mapping(
            &id,
            &profile.id,
            &[LinkSelector {
                field,
                operator: "exact".to_owned(),
                value: input.mapping.value,
            }],
            &canonical_note_path,
            input.mapping.enabled,
            expected,
        )
        .map_err(context_mapping_update_error)?;
    Ok(Json(
        json!({"mapping": mapping, "affects_new_candidates_only": true}),
    ))
}

fn context_mapping_create_error(error: anyhow::Error) -> ApiError {
    let message = error.to_string();
    if message.contains("UNIQUE constraint failed") {
        ApiError::conflict("This name already has a saved canonical link")
    } else {
        ApiError::bad_request(message)
    }
}

fn ensure_unique_context_mapping(
    state: &AppState,
    workspace_id: &str,
    field: &str,
    value: &str,
    except_id: Option<&str>,
) -> Result<(), ApiError> {
    let normalized = value.trim().to_lowercase();
    let duplicate = state
        .store
        .list_context_mappings(workspace_id)
        .map_err(|error| ApiError::internal(error.to_string()))?
        .into_iter()
        .filter(|mapping| except_id != Some(mapping.id.as_str()))
        .any(|mapping| {
            mapping.selectors.len() == 1
                && mapping.selectors[0].field == field
                && mapping.selectors[0].operator == "exact"
                && mapping.selectors[0].value.trim().to_lowercase() == normalized
        });
    if duplicate {
        Err(ApiError::conflict(
            "This name already has a saved canonical link",
        ))
    } else {
        Ok(())
    }
}

fn context_mapping_update_error(error: anyhow::Error) -> ApiError {
    let message = error.to_string();
    if message.contains("changed") || message.contains("UNIQUE constraint failed") {
        ApiError::conflict("This Knowledge link changed or conflicts with another saved link")
    } else {
        ApiError::bad_request(message)
    }
}

async fn refocus_delete_context_mapping(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Json(input): Json<DeleteContextMappingRequest>,
) -> Result<StatusCode, ApiError> {
    authorize_refocus(&state, &headers, "settings:write", true)?;
    let profile = active_refocus_workspace(&state)?;
    state
        .store
        .delete_context_mapping(&id, &profile.id, input.expected_updated_at)
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn refocus_ignore_context_identity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<IgnoreContextIdentityRequest>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    authorize_refocus(&state, &headers, "settings:write", true)?;
    let profile = active_refocus_workspace(&state)?;
    let field = input.field.trim().to_lowercase();
    let value = input.value.trim().to_owned();
    let identity = state
        .store
        .save_ignored_context_identity(&IgnoredContextIdentity {
            id: String::new(),
            workspace_id: profile.id,
            field,
            normalized_value: value.to_lowercase(),
            value,
            source_identity: None,
            source_digest: None,
            created_at: DateTime::<Utc>::UNIX_EPOCH,
        })
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok((StatusCode::CREATED, Json(json!({"ignored": identity}))))
}

async fn refocus_reopen_context_identity(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Result<StatusCode, ApiError> {
    authorize_refocus(&state, &headers, "settings:write", true)?;
    let profile = active_refocus_workspace(&state)?;
    state
        .store
        .delete_ignored_context_identity(&id, &profile.id)
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

struct KnowledgeCollectionPreviewMaterial {
    workspace_id: String,
    collection: KnowledgeCollectionDraft,
    matched_note_count: usize,
    eligible_note_count: usize,
    oversized_note_count: usize,
    total_bytes: u64,
    missing_roots: Vec<String>,
    preview_digest: String,
}

impl KnowledgeCollectionPreviewMaterial {
    fn as_json(&self, changes_saved: bool) -> Value {
        json!({
            "collection": self.collection,
            "matched_note_count": self.matched_note_count,
            "eligible_note_count": self.eligible_note_count,
            "oversized_note_count": self.oversized_note_count,
            "total_bytes": self.total_bytes,
            "missing_roots": self.missing_roots,
            "preview_digest": self.preview_digest,
            "changes_saved": changes_saved
        })
    }
}

fn preview_knowledge_collection(
    state: &AppState,
    input: KnowledgeCollectionDraft,
) -> Result<KnowledgeCollectionPreviewMaterial, ApiError> {
    let (profile, workspace) = active_refocus_context(state)?;
    let label = input.label.trim().to_owned();
    let purpose = input.purpose.trim().to_owned();
    if label.is_empty() || label.len() > 100 {
        return Err(ApiError::bad_request(
            "Knowledge collection label must contain 1-100 bytes",
        ));
    }
    if purpose.is_empty() || purpose.len() > 1000 {
        return Err(ApiError::bad_request(
            "Knowledge collection purpose must contain 1-1000 bytes",
        ));
    }
    let (roots, exclusions) = normalize_knowledge_collection_paths(&input.roots, &input.exclusions)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let collection = KnowledgeCollectionDraft {
        label,
        purpose,
        roots,
        exclusions,
        enabled: input.enabled,
    };
    let selection = workspace
        .preview_markdown_sources(&collection.roots, &collection.exclusions, 2_000)
        .map_err(|error| ApiError::unprocessable(error.to_string()))?;
    let total_bytes = selection.sources.iter().try_fold(0_u64, |total, source| {
        total
            .checked_add(source.byte_len)
            .ok_or_else(|| ApiError::unprocessable("Knowledge collection size overflow"))
    })?;
    let oversized_note_count = selection
        .sources
        .iter()
        .filter(|source| source.byte_len > 1024 * 1024)
        .count();
    let eligible_note_count = selection.sources.len() - oversized_note_count;
    let digest_input = json!({
        "root_binding": workspace.root_binding(),
        "collection": collection,
    });
    let preview_digest = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&digest_input)
                .map_err(|error| ApiError::internal(error.to_string()))?
        )
    );
    Ok(KnowledgeCollectionPreviewMaterial {
        workspace_id: profile.id,
        collection,
        matched_note_count: selection.sources.len(),
        eligible_note_count,
        oversized_note_count,
        total_bytes,
        missing_roots: selection.missing_roots,
        preview_digest,
    })
}

async fn refocus_cutover_report(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<migration::CutoverReport>, ApiError> {
    authorize_refocus(&state, &headers, "logs:read", false)?;
    let (profile, workspace) = active_refocus_context(&state)?;
    migration::inventory_cutover(
        &state.store,
        &profile,
        &workspace,
        state.legacy_proposal_dir.as_deref(),
        &state.legacy_support_files,
    )
    .map(Json)
    .map_err(|error| ApiError::internal(error.to_string()))
}

async fn refocus_commit_cutover(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(request): Json<migration::CutoverCommitRequest>,
) -> Result<Json<migration::CutoverCommitResult>, ApiError> {
    authorize_refocus(&state, &headers, "settings:write", true)?;
    let (profile, workspace) = active_refocus_context(&state)?;
    let mut result = migration::commit_cutover(
        &state.store,
        &profile,
        &workspace,
        state.legacy_proposal_dir.as_deref(),
        &state.legacy_support_files,
        &request,
    )
    .map_err(|error| ApiError::conflict(error.to_string()))?;
    result.retried_preparation_runs = state
        .store
        .retry_failed_daily_schedule_runs(
            &profile.id,
            "review and complete the legacy-data migration in Settings before changing a Daily candidate or applying Markdown",
            Utc::now(),
        )
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(Json(result))
}

async fn refocus_preview_workspace_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<WorkspaceSettingsDraft>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "settings:write", true)?;
    let (settings, destination_example, preview_digest, _) =
        preview_workspace_settings(&state, input)?;
    Ok(Json(json!({
        "settings": settings,
        "destination_example": destination_example,
        "preview_digest": preview_digest,
        "changes_saved": false
    })))
}

async fn refocus_save_workspace_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<SaveWorkspaceSettingsRequest>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "settings:write", true)?;
    let (settings, destination_example, preview_digest, workspace) =
        preview_workspace_settings(&state, input.settings)?;
    if preview_digest != input.preview_digest {
        return Err(ApiError::conflict(
            "workspace settings differ from the reviewed preview",
        ));
    }
    let expected = match (
        input.expected_profile_id.as_deref(),
        input.expected_updated_at,
    ) {
        (Some(id), Some(updated_at)) => Some((id, updated_at)),
        (None, None) => None,
        _ => {
            return Err(ApiError::bad_request(
                "expected profile ID and timestamp must be supplied together",
            ));
        }
    };
    let profile = state
        .store
        .save_active_workspace_profile(
            workspace.root_binding(),
            &settings.timezone,
            &settings.daily_root,
            &settings.daily_pattern,
            settings.template_path.as_deref(),
            &settings.link_style,
            expected,
        )
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    Ok(Json(json!({
        "active_profile": profile,
        "destination_example": destination_example,
        "binding_matches": true,
        "changes_saved": true
    })))
}

fn preview_workspace_settings(
    state: &AppState,
    input: WorkspaceSettingsDraft,
) -> Result<(WorkspaceSettingsDraft, String, String, InspectedWorkspace), ApiError> {
    let workspace = inspect_refocus_workspace(state)?;
    let settings = WorkspaceSettingsDraft {
        timezone: input.timezone.trim().to_owned(),
        daily_root: input.daily_root.trim().trim_matches('/').to_owned(),
        daily_pattern: input.daily_pattern.trim().to_owned(),
        template_path: input
            .template_path
            .map(|path| path.trim().to_owned())
            .filter(|path| !path.is_empty()),
        link_style: input.link_style.trim().to_owned(),
    };
    let timezone = settings
        .timezone
        .parse::<chrono_tz::Tz>()
        .map_err(|_| ApiError::bad_request("timezone must be a valid IANA name"))?;
    let example_date = Utc::now().with_timezone(&timezone).date_naive();
    resolve_day(example_date, &settings.timezone)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    if !matches!(settings.link_style.as_str(), "markdown" | "wikilink") {
        return Err(ApiError::bad_request(
            "link style must be markdown or wikilink",
        ));
    }
    let destination_example =
        render_daily_path(&settings.daily_root, &settings.daily_pattern, example_date)
            .map_err(|error| ApiError::bad_request(error.to_string()))?;
    workspace
        .resolve_markdown_path(
            std::path::Path::new(&destination_example),
            MarkdownPathMode::MayCreate,
        )
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    if let Some(template_path) = settings.template_path.as_deref() {
        workspace
            .resolve_markdown_path(
                std::path::Path::new(template_path),
                MarkdownPathMode::ExistingFile,
            )
            .map_err(|error| ApiError::bad_request(error.to_string()))?;
    }
    let digest_input = json!({
        "root_binding": workspace.root_binding(),
        "settings": settings,
        "destination_example": destination_example
    });
    let preview_digest = format!(
        "{:x}",
        Sha256::digest(
            serde_json::to_vec(&digest_input)
                .map_err(|error| ApiError::internal(error.to_string()))?
        )
    );
    Ok((settings, destination_example, preview_digest, workspace))
}

fn active_refocus_workspace(state: &AppState) -> Result<WorkspaceProfile, ApiError> {
    active_refocus_context(state).map(|(profile, _)| profile)
}

fn active_refocus_context(
    state: &AppState,
) -> Result<(WorkspaceProfile, InspectedWorkspace), ApiError> {
    let workspace = inspect_refocus_workspace(state)?;
    let profile = state
        .store
        .active_workspace_profile()
        .map_err(|error| ApiError::internal(error.to_string()))?
        .ok_or_else(|| ApiError::conflict("review and save workspace settings first"))?;
    if profile.root_binding != workspace.root_binding() {
        return Err(ApiError::conflict(
            "the mounted workspace has changed; review and save workspace settings again",
        ));
    }
    Ok((profile, workspace))
}

fn require_cutover_for_daily_mutation(state: &AppState) -> Result<(), ApiError> {
    let (profile, workspace) = active_refocus_context(state)?;
    let report = migration::inventory_cutover(
        &state.store,
        &profile,
        &workspace,
        state.legacy_proposal_dir.as_deref(),
        &state.legacy_support_files,
    )
    .map_err(|error| ApiError::internal(error.to_string()))?;
    if report.cutover_status == "completed" || report.items.is_empty() {
        return Ok(());
    }
    Err(ApiError::conflict(
        "review and complete the legacy-data migration in Settings before changing a Daily candidate or applying Markdown",
    ))
}

fn inspect_refocus_workspace(state: &AppState) -> Result<InspectedWorkspace, ApiError> {
    InspectedWorkspace::inspect(state.workspace.canonical_root()).map_err(|error| {
        ApiError::conflict(format!("the mounted workspace is unavailable: {error}"))
    })
}

async fn refocus_daily_day(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(date): AxumPath<String>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "logs:read", false)?;
    let local_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("date must use YYYY-MM-DD"))?;
    let profile = active_refocus_workspace(&state)?;
    let frozen_day = state
        .store
        .daily_day(&profile.id, local_date)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let window = effective_daily_window(local_date, &profile, frozen_day.as_ref())
        .map_err(ApiError::bad_request)?;
    let evidence = state
        .store
        .get_events_between(window.start_utc, window.end_utc, 500)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let event_count = evidence.events.len();
    let manual_entries = state
        .store
        .manual_daily_entries(&profile.id, local_date)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let current_revision = state
        .store
        .current_proposal_revision(&profile.id, local_date)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let current_snapshot = match current_revision
        .as_ref()
        .and_then(|revision| revision.snapshot_id.as_deref())
    {
        Some(snapshot_id) => state
            .store
            .evidence_snapshot(snapshot_id)
            .map_err(|error| ApiError::internal(error.to_string()))?,
        None => None,
    };
    let current_snapshot_evidence = match current_snapshot.as_ref() {
        Some(snapshot) => state
            .store
            .snapshot_evidence(&snapshot.id)
            .map_err(|error| ApiError::internal(error.to_string()))?,
        None => Vec::new(),
    };
    let current_context_snapshot = match current_revision.as_ref() {
        Some(revision) => state
            .store
            .proposal_context_snapshot(&revision.id)
            .map_err(|error| ApiError::internal(error.to_string()))?,
        None => None,
    };
    let active_deferrals = match current_revision.as_ref() {
        Some(revision) => state
            .store
            .active_daily_evidence_deferrals(&profile.id, local_date, &revision.id)
            .map_err(|error| ApiError::internal(error.to_string()))?,
        None => Vec::new(),
    };
    let deferred_event_ids = active_deferrals
        .iter()
        .map(|deferral| deferral.event_id.as_str())
        .collect::<HashSet<_>>();
    let live_event_ids = evidence
        .events
        .iter()
        .filter(|event| !deferred_event_ids.contains(event.id.as_str()))
        .map(|event| event.id.as_str())
        .collect::<Vec<_>>();
    let live_manual_ids = manual_entries
        .iter()
        .map(|entry| entry.id.as_str())
        .collect::<Vec<_>>();
    let current_content = current_revision.as_ref().and_then(|revision| {
        serde_json::from_value::<DailyRevisionContent>(revision.content.clone()).ok()
    });
    let candidate_freshness = current_revision.as_ref().map(|_| {
        let stored_manual_ids = current_content
            .as_ref()
            .map(|content| content.manual_entry_ids.as_slice())
            .unwrap_or_default();
        let stored_event_ids = current_snapshot
            .as_ref()
            .map(|snapshot| snapshot.event_ids.as_slice())
            .unwrap_or_default();
        if !has_new_automated_evidence(&live_event_ids, stored_event_ids)
            && stored_manual_ids
                .iter()
                .map(String::as_str)
                .eq(live_manual_ids)
        {
            "current"
        } else {
            "update_available"
        }
    });
    let preview_markdown = current_content.as_ref().map(|content| {
        llm::render_daily_revision_preview(content, &manual_entries, &current_snapshot_evidence)
    });
    let expired_evidence_count = current_snapshot_evidence
        .iter()
        .filter(|item| !item.available)
        .count();
    let new_evidence_count = if current_revision.is_some() {
        current_snapshot
            .as_ref()
            .map(|snapshot| {
                live_event_ids
                    .iter()
                    .filter(|event_id| {
                        !snapshot.event_ids.iter().any(|stored| stored == **event_id)
                    })
                    .count()
            })
            .unwrap_or(live_event_ids.len())
    } else {
        0
    };
    let apply_status = state
        .store
        .latest_apply_operation(&profile.id, local_date)
        .map_err(|error| ApiError::internal(error.to_string()))?
        .as_ref()
        .map(public_apply_operation);
    Ok(Json(json!({
        "workspace_id": profile.id,
        "local_date": local_date,
        "timezone": window.timezone,
        "start_utc": window.start_utc,
        "end_utc": window.end_utc,
        "destination_path": window.destination_path,
        "day": frozen_day,
        "generation_attempt": state.store.latest_generation_attempt(&profile.id, local_date).map_err(|e| ApiError::internal(e.to_string()))?,
        "automated_evidence": {
            "events": evidence.events,
            "returned_count": event_count,
            "truncated": evidence.truncated,
            "limit": evidence.limit
        },
        "manual_entries": manual_entries,
        "current_revision": current_revision,
        "revision_reference_mode": current_context_snapshot.as_ref().and_then(|snapshot| snapshot.payload.get("reference_mode")).and_then(Value::as_str).unwrap_or("configured"),
        "current_snapshot": current_snapshot,
        "current_context_snapshot": current_context_snapshot.as_ref().map(public_context_snapshot),
        "current_snapshot_evidence": current_snapshot_evidence,
        "active_late_evidence_deferrals": active_deferrals,
        "candidate_freshness": candidate_freshness,
        "new_evidence_count": new_evidence_count,
        "expired_evidence_count": expired_evidence_count,
        "evidence_complete": expired_evidence_count == 0,
        "preview_markdown": preview_markdown,
        "apply_status": apply_status
    })))
}

async fn refocus_daily_context(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(date): AxumPath<String>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "logs:read", false)?;
    authorize_refocus(&state, &headers, "knowledge:read", false)?;
    let local_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("date must use YYYY-MM-DD"))?;
    let (profile, workspace) = active_refocus_context(&state)?;
    let revision = state
        .store
        .current_proposal_revision(&profile.id, local_date)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let Some(revision) = revision else {
        return Ok(Json(json!({
            "workspace_id": profile.id,
            "local_date": local_date,
            "status": "none",
            "mode": "exact_links_only",
            "workstreams": [],
            "message": "Generate a Daily candidate to capture Knowledge links."
        })));
    };
    let context_snapshot = state
        .store
        .proposal_context_snapshot(&revision.id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let Some(context_snapshot) = context_snapshot else {
        return Ok(Json(json!({
            "workspace_id": profile.id,
            "local_date": local_date,
            "revision_id": revision.id,
            "status": "none",
            "mode": "exact_links_only",
            "workstreams": [],
            "message": "This candidate did not use Knowledge links."
        })));
    };
    let diagnostics = context_snapshot.payload.get("diagnostics");
    if context_snapshot
        .payload
        .get("reference_mode")
        .and_then(Value::as_str)
        == Some("none")
    {
        return Ok(Json(
            json!({"workspace_id":profile.id,"local_date":local_date,"revision_id":revision.id,"status":"none","mode":"none","reference_mode":"none","workstreams":[],"message":"Generated without reference notes."}),
        ));
    }
    let generated_unavailable = diagnostics
        .and_then(|value| value.get("resolution_error_code"))
        .is_some();
    let mut status = if generated_unavailable {
        "unavailable"
    } else {
        "current"
    };
    let mut message = if generated_unavailable {
        "Knowledge resolution was unavailable when this candidate was generated."
    } else {
        "Knowledge links match the frozen candidate."
    };
    if !generated_unavailable {
        let freshness = current_context_freshness(
            &state,
            &profile,
            &workspace,
            local_date,
            &revision,
            &context_snapshot,
        )?;
        status = freshness.0;
        message = freshness.1;
    }
    let notes = context_snapshot
        .payload
        .get("used_notes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let note_titles = notes
        .iter()
        .filter_map(|note| {
            Some((
                note.get("path")?.as_str()?.to_owned(),
                note.get("title")?.as_str()?.to_owned(),
            ))
        })
        .collect::<HashMap<_, _>>();
    let excerpts = context_snapshot
        .payload
        .get("excerpts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|excerpt| Some((excerpt.get("id")?.as_str()?.to_owned(), excerpt.clone())))
        .collect::<HashMap<_, _>>();
    let workstream_excerpts = context_snapshot
        .payload
        .get("workstream_excerpts")
        .and_then(Value::as_object);
    let applied_exclusions = frozen_context_exclusions(Some(&context_snapshot));
    let attached_links = serde_json::from_value::<DailyRevisionContent>(revision.content.clone())
        .map(|content| {
            content
                .workstreams
                .into_iter()
                .map(|workstream| {
                    (
                        workstream.id,
                        workstream
                            .canonical_links
                            .into_iter()
                            .collect::<HashSet<_>>(),
                    )
                })
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    let mut grouped = BTreeMap::<String, BTreeMap<String, Value>>::new();
    for resolved in context_snapshot
        .payload
        .get("resolved_groups")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let Some(workstream_id) = resolved.get("canonical_group_id").and_then(Value::as_str) else {
            continue;
        };
        let Some(path) = resolved.get("canonical_note_path").and_then(Value::as_str) else {
            continue;
        };
        let canonical_link = resolved
            .get("canonical_link")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let attached = attached_links
            .get(workstream_id)
            .is_some_and(|links| links.contains(canonical_link));
        grouped
            .entry(workstream_id.to_owned())
            .or_default()
            .entry(path.to_owned())
            .or_insert_with(|| {
                let excerpt = workstream_excerpts
                    .and_then(|workstreams| workstreams.get(workstream_id))
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(Value::as_str)
                    .filter_map(|id| excerpts.get(id))
                    .find(|excerpt| {
                        excerpt.get("note_path").and_then(Value::as_str) == Some(path)
                    });
                json!({
                    "path": path,
                    "title": note_titles.get(path).cloned().unwrap_or_else(|| path.to_owned()),
                    "canonical_link": canonical_link,
                    "attached": attached,
                    "reason": resolved.get("reason").and_then(Value::as_str),
                    "matched_fields": resolved.get("matched_fields").and_then(Value::as_array).cloned().unwrap_or_default(),
                    "excluded": applied_exclusions.contains(&(workstream_id.to_owned(), path.to_owned())),
                    "excerpt": excerpt.map(|excerpt| json!({
                        "id": excerpt.get("id").and_then(Value::as_str),
                        "text": excerpt.get("text").and_then(Value::as_str),
                        "text_digest": excerpt.get("text_digest").and_then(Value::as_str),
                        "reason": excerpt.get("reason").and_then(Value::as_str),
                    })),
                })
            });
    }
    let workstreams = grouped
        .into_iter()
        .map(|(id, notes)| json!({"id": id, "notes": notes.into_values().collect::<Vec<_>>() }))
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "workspace_id": profile.id,
        "local_date": local_date,
        "revision_id": revision.id,
        "status": status,
        "mode": if context_snapshot.payload.get("schema_version").and_then(Value::as_u64) == Some(2) { "bounded_knowledge" } else { "exact_links_only" },
        "message": message,
        "snapshot": public_context_snapshot(&context_snapshot),
        "workstreams": workstreams,
    })))
}

fn current_context_freshness(
    state: &AppState,
    profile: &WorkspaceProfile,
    workspace: &InspectedWorkspace,
    _local_date: NaiveDate,
    revision: &ProposalRevision,
    context_snapshot: &log_inbox_core::models::ContextSnapshot,
) -> Result<(&'static str, &'static str), ApiError> {
    if context_snapshot
        .payload
        .get("reference_mode")
        .and_then(Value::as_str)
        == Some("none")
    {
        return Ok(("none", "Generated without reference notes."));
    }
    let used_note_paths = context_relevant_note_paths(revision, &context_snapshot.payload);
    let collections = state
        .store
        .list_knowledge_collections(&profile.id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let mappings = state
        .store
        .list_context_mappings(&profile.id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    if context_snapshot
        .payload
        .get("resolver_version")
        .and_then(Value::as_str)
        == Some("none")
    {
        return Ok(
            if collections.iter().any(|collection| collection.enabled)
                || mappings.iter().any(|mapping| mapping.enabled)
            {
                (
                    "changed",
                    "Reference notes are now configured. Regenerate to use them.",
                )
            } else {
                ("none", "This draft did not use reference notes.")
            },
        );
    }
    let fingerprints = match knowledge::current_fingerprints(workspace, &collections, &mappings) {
        Ok(fingerprints) => fingerprints,
        Err(_) => {
            return Ok((
                "unavailable",
                "Frozen links are preserved, but their current sources could not be checked.",
            ));
        }
    };
    if used_note_paths
        .iter()
        .any(|path| !fingerprints.note_paths.contains(path))
    {
        return Ok((
            "invalid",
            "A canonical note used by this revision was removed or excluded. Regenerate before Apply.",
        ));
    }
    let digest_matches = context_snapshot
        .payload
        .get("used_notes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|note| {
            Some((
                note.get("path")?.as_str()?,
                note.get("usable_digest")?.as_str()?,
            ))
        })
        .all(|(path, digest)| {
            fingerprints
                .usable_digests
                .get(path)
                .is_some_and(|current| current == digest)
        });
    let expected_resolver = if state
        .llm_config
        .as_ref()
        .is_some_and(llm::knowledge_text_stays_local)
    {
        knowledge::KNOWLEDGE_RESOLVER_VERSION
    } else {
        "exact-v1"
    };
    let resolver_matches = context_snapshot
        .payload
        .get("resolver_version")
        .and_then(Value::as_str)
        == Some(expected_resolver);
    let root_matches = context_snapshot
        .payload
        .get("root_binding")
        .and_then(Value::as_str)
        == Some(workspace.root_binding());
    let configuration_matches = context_snapshot
        .payload
        .get("configuration_digest")
        .and_then(Value::as_str)
        == Some(fingerprints.configuration_digest.as_str());
    let catalog_matches = context_snapshot
        .payload
        .get("catalog_digest")
        .and_then(Value::as_str)
        == Some(fingerprints.catalog_digest.as_str());
    let uses_excerpts = context_snapshot
        .payload
        .get("excerpts")
        .and_then(Value::as_array)
        .is_some_and(|excerpts| !excerpts.is_empty());
    if resolver_matches
        && root_matches
        && configuration_matches
        && catalog_matches
        && digest_matches
    {
        Ok(if uses_excerpts {
            (
                "current",
                "Knowledge links and excerpts match the frozen candidate.",
            )
        } else {
            ("current", "Knowledge links match the frozen candidate.")
        })
    } else {
        Ok((
            "changed",
            "Knowledge links, excerpts, or source metadata changed. Regenerate to use the latest setup.",
        ))
    }
}

fn context_relevant_note_paths(revision: &ProposalRevision, payload: &Value) -> Vec<String> {
    let excerpt_paths = payload
        .get("excerpts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|excerpt| excerpt.get("note_path").and_then(Value::as_str))
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    if excerpt_paths.is_empty() {
        attached_context_note_paths(revision, payload)
    } else {
        excerpt_paths.into_iter().collect()
    }
}

fn attached_context_note_paths(revision: &ProposalRevision, payload: &Value) -> Vec<String> {
    let attached = serde_json::from_value::<DailyRevisionContent>(revision.content.clone())
        .map(|content| {
            content
                .workstreams
                .into_iter()
                .map(|workstream| {
                    (
                        workstream.id,
                        workstream
                            .canonical_links
                            .into_iter()
                            .collect::<HashSet<_>>(),
                    )
                })
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    payload
        .get("resolved_groups")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|resolved| {
            let workstream_id = resolved.get("canonical_group_id")?.as_str()?;
            let canonical_link = resolved.get("canonical_link")?.as_str()?;
            if !attached
                .get(workstream_id)
                .is_some_and(|links| links.contains(canonical_link))
            {
                return None;
            }
            resolved
                .get("canonical_note_path")?
                .as_str()
                .map(str::to_owned)
        })
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn ensure_context_applyable(state: &AppState, local_date: NaiveDate) -> Result<(), ApiError> {
    let (profile, workspace) = active_refocus_context(state)?;
    let Some(revision) = state
        .store
        .current_proposal_revision(&profile.id, local_date)
        .map_err(|error| ApiError::internal(error.to_string()))?
    else {
        return Ok(());
    };
    let Some(context_snapshot) = state
        .store
        .proposal_context_snapshot(&revision.id)
        .map_err(|error| ApiError::internal(error.to_string()))?
    else {
        return Ok(());
    };
    if current_context_freshness(
        state,
        &profile,
        &workspace,
        local_date,
        &revision,
        &context_snapshot,
    )?
    .0 == "invalid"
    {
        return Err(ApiError::conflict(
            "a canonical note used by this candidate was removed or excluded; regenerate before Apply",
        ));
    }
    Ok(())
}

async fn refocus_daily_overview(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<DailyOverviewQuery>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "logs:read", false)?;
    let profile = state
        .store
        .active_workspace_profile()
        .map_err(|error| ApiError::internal(error.to_string()))?
        .ok_or_else(|| ApiError::conflict("review Daily settings first"))?;
    active_refocus_workspace(&state)?;
    let timezone = profile
        .timezone
        .parse::<chrono_tz::Tz>()
        .map_err(|_| ApiError::internal("the saved workspace timezone is invalid"))?;
    let today = Utc::now().with_timezone(&timezone).date_naive();
    let limit = query
        .limit
        .unwrap_or(
            state
                .store
                .recent_days_preference(&profile.id)
                .map_err(|error| ApiError::internal(error.to_string()))?,
        )
        .clamp(1, 31);
    let automation = state
        .store
        .daily_automation_settings(&profile.id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let scan_days = usize::from(
        automation
            .catch_up_days
            .max(automation.raw_retention_days)
            .min(90),
    )
    .max(limit);
    let dates = (0..scan_days)
        .map(|offset| today - Duration::days(offset as i64))
        .collect::<Vec<_>>();
    let overview_facts = state
        .store
        .daily_overview_facts(&profile.id, &profile.timezone, &dates)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let mut days = Vec::new();

    for facts in overview_facts {
        if days.len() >= limit {
            break;
        }
        if facts.local_date != today
            && facts.event_count == 0
            && facts.manual_entry_count == 0
            && facts.day.is_none()
            && facts.schedule_run.is_none()
        {
            continue;
        }
        let status = daily_overview_status(
            facts.local_date,
            today,
            facts.day.as_ref(),
            facts.revision.as_ref(),
            facts.apply_operation.as_ref(),
            facts.schedule_run.as_ref(),
            facts.event_count > 0,
            facts.manual_entry_count > 0,
            facts.new_evidence_count > 0 || facts.manual_entries_changed,
        );
        days.push(json!({
            "local_date": facts.local_date,
            "status": status,
            "event_count": facts.event_count,
            "manual_entry_count": facts.manual_entry_count,
            "generation_status": facts.day.as_ref().map(|day| day.generation_status.as_str()),
            "review_status": facts.day.as_ref().map(|day| day.review_status.as_str()),
            "freshness": facts.day.as_ref().map(|day| day.freshness.as_str()),
            "revision_number": facts.revision.as_ref().map(|revision| revision.revision_number),
            "new_evidence_count": facts.new_evidence_count,
            "manual_entries_changed": facts.manual_entries_changed,
            "expired_evidence_count": facts.expired_evidence_count,
            "evidence_complete": facts.expired_evidence_count == 0,
            "schedule_state": facts.schedule_run.as_ref().map(|run| run.state.as_str()),
            "schedule_attempts": facts.schedule_run.as_ref().map(|run| run.attempts),
            "schedule_retry_at": facts.schedule_run.as_ref().map(|run| run.next_attempt_at),
            "schedule_error": facts.schedule_run.as_ref().and_then(|run| run.last_error.as_deref()),
        }));
    }

    let missed_count = days.iter().filter(|day| day["status"] == "missed").count();
    let update_count = days
        .iter()
        .filter(|day| day["status"] == "update_available")
        .count();
    let failed_count = days
        .iter()
        .filter(|day| day["status"] == "generation_failed")
        .count();
    let intake_window =
        resolve_day(today, &profile.timezone).map_err(|e| ApiError::bad_request(e.to_string()))?;
    let intake = state
        .store
        .intake_summary(intake_window.start_utc, intake_window.end_utc)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(json!({
        "workspace_id": profile.id,
        "server_now": Utc::now(),
        "today": today,
        "timezone": profile.timezone,
        "recent_days": limit,
        "missed_count": missed_count,
        "update_count": update_count,
        "failed_count": failed_count,
        "active_generation": state.store.active_generation_attempt().map_err(|e| ApiError::internal(e.to_string()))?,
        "intake": intake,
        "window_start": today - Duration::days((scan_days - 1) as i64),
        "days": days,
    })))
}

#[allow(clippy::too_many_arguments)]
fn daily_overview_status(
    date: NaiveDate,
    today: NaiveDate,
    day: Option<&DailyDay>,
    revision: Option<&ProposalRevision>,
    apply: Option<&ApplyOperation>,
    schedule_run: Option<&log_inbox_core::models::DailyScheduleRun>,
    has_automated_input: bool,
    has_manual_input: bool,
    update_available: bool,
) -> &'static str {
    if apply.is_some_and(|operation| operation.state != "finalized") {
        "apply_attention"
    } else if apply.is_some_and(|operation| operation.state == "finalized")
        || day.is_some_and(|day| day.review_status == "applied")
    {
        if update_available {
            "update_available"
        } else {
            "applied"
        }
    } else if day.is_some_and(|day| day.review_status == "dismissed") {
        "dismissed"
    } else if update_available {
        "update_available"
    } else if revision.is_some() {
        "in_review"
    } else if day.is_some_and(|day| day.generation_status == "failed")
        || schedule_run.is_some_and(|run| run.state == "failed")
    {
        "generation_failed"
    } else if day.is_some_and(|day| matches!(day.generation_status.as_str(), "queued" | "running"))
    {
        "generating"
    } else if has_automated_input
        && date < today
        && !schedule_run.is_some_and(|run| matches!(run.state.as_str(), "pending" | "claimed"))
    {
        "missed"
    } else if has_manual_input {
        "notes_unreviewed"
    } else if schedule_run.is_some_and(|run| matches!(run.state.as_str(), "pending" | "claimed")) {
        "scheduled"
    } else {
        "not_started"
    }
}

async fn refocus_daily_apply_preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(date): AxumPath<String>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "logs:read", false)?;
    require_cutover_for_daily_mutation(&state)?;
    let local_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("date must use YYYY-MM-DD"))?;
    ensure_context_applyable(&state, local_date)?;
    let material = daily_apply_material(&state, local_date)?;
    let plan = material.plan;
    let updated_content_hash = daily_writer::digest(&plan.updated_content);
    Ok(Json(json!({
        "workspace_id": material.profile.id,
        "local_date": local_date,
        "destination_path": material.day.destination_path,
        "revision_id": material.revision.id,
        "revision_content_hash": material.revision.content_hash,
        "block_id": material.day.block_id,
        "will_create_note": !material.target_exists,
        "template_used": material.template_used,
        "previous_block": plan.previous_block,
        "next_block": plan.next_block,
        "expected_old_block_hash": plan.expected_old_block_hash,
        "intended_new_block_hash": plan.intended_new_block_hash,
        "expected_target_exists": material.target_exists,
        "expected_original_content_hash": daily_writer::digest(&material.original_content),
        "updated_content_hash": updated_content_hash
    })))
}

async fn refocus_daily_apply(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(date): AxumPath<String>,
    Json(input): Json<ApplyDailyRequest>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "vault:write", true)?;
    require_cutover_for_daily_mutation(&state)?;
    let local_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("date must use YYYY-MM-DD"))?;
    let profile = active_refocus_workspace(&state)?;
    let operation_id = apply_operation_id(&profile.id, local_date, &input);
    let _guard = state
        .apply_lock
        .lock()
        .map_err(|_| ApiError::internal("Daily Apply lock is unavailable"))?;
    let existing_operation = state
        .store
        .apply_operation(&operation_id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    if let Some(operation) = existing_operation
        .as_ref()
        .filter(|operation| operation.state == "finalized")
    {
        validate_apply_operation_approval(operation, &input)?;
        return Ok(Json(json!({
            "operation": public_apply_operation(operation),
            "destination_path": input.destination_path,
            "idempotent": true
        })));
    }
    if existing_operation.is_none() {
        ensure_context_applyable(&state, local_date)?;
    }

    let material = daily_apply_material(&state, local_date)?;
    validate_apply_material_approval(&material, &input)?;
    let operation = match existing_operation {
        Some(operation) => {
            validate_apply_operation_approval(&operation, &input)?;
            operation
        }
        None => {
            if material.plan.expected_old_block_hash != input.expected_old_block_hash {
                return Err(ApiError::conflict(
                    "the approved Daily block changed; preview Apply again",
                ));
            }
            state
                .store
                .prepare_apply_operation(&PrepareApplyOperation {
                    id: operation_id.clone(),
                    workspace_id: material.profile.id.clone(),
                    local_date,
                    revision_id: material.revision.id.clone(),
                    revision_content_hash: material.revision.content_hash.clone(),
                    destination_path: material.day.destination_path.clone(),
                    expected_old_block_hash: material.plan.expected_old_block_hash.clone(),
                    intended_new_block_hash: material.plan.intended_new_block_hash.clone(),
                    expected_target_exists: material.target_exists,
                    expected_original_content_hash: daily_writer::digest(
                        &material.original_content,
                    ),
                    intended_updated_content_hash: daily_writer::digest(
                        &material.plan.updated_content,
                    ),
                    temporary_name: daily_writer::temporary_name(&material.target, &operation_id)
                        .map_err(|error| ApiError::internal(error.to_string()))?,
                    recovery_payload: Some(material.original_content.clone()),
                    recovery_path: None,
                })
                .map_err(|error| ApiError::conflict(error.to_string()))?
        }
    };
    let operation = execute_daily_apply(&state, &material, operation)?;
    Ok(Json(json!({
        "operation": public_apply_operation(&operation),
        "destination_path": material.day.destination_path,
        "idempotent": false
    })))
}

fn public_apply_operation(operation: &ApplyOperation) -> Value {
    json!({
        "id": operation.id,
        "state": operation.state,
        "failure_reason": operation.failure_reason,
        "created_at": operation.created_at,
        "updated_at": operation.updated_at,
        "can_retry": matches!(operation.state.as_str(), "failed" | "reconciliation_required" | "prepared" | "writing" | "written")
    })
}

fn public_context_snapshot(snapshot: &log_inbox_core::models::ContextSnapshot) -> Value {
    let diagnostics = snapshot.payload.get("diagnostics");
    let count = |field: &str| {
        diagnostics
            .and_then(|value| value.get(field))
            .and_then(Value::as_u64)
            .unwrap_or_default()
    };
    json!({
        "id": snapshot.id,
        "snapshot_digest": snapshot.snapshot_digest,
        "created_at": snapshot.created_at,
        "resolver_version": snapshot.payload.get("resolver_version").and_then(Value::as_str),
        "used_note_count": snapshot.payload.get("used_notes").and_then(Value::as_array).map(Vec::len).unwrap_or_default(),
        "resolved_group_count": snapshot.payload.get("resolved_groups").and_then(Value::as_array).map(Vec::len).unwrap_or_default(),
        "excerpt_count": snapshot.payload.get("excerpts").and_then(Value::as_array).map(Vec::len).unwrap_or_default(),
        "diagnostics": {
            "missing_root_count": diagnostics.and_then(|value| value.get("missing_roots")).and_then(Value::as_array).map(Vec::len).unwrap_or_default(),
            "oversized_note_count": count("oversized_note_count"),
            "unreadable_note_count": count("unreadable_note_count"),
            "invalid_note_count": count("invalid_note_count"),
            "invalid_mapping_count": count("invalid_mapping_count"),
            "ambiguous_group_count": count("ambiguous_group_count"),
            "resolution_failed": diagnostics.and_then(|value| value.get("resolution_error_code")).is_some(),
        }
    })
}

async fn refocus_retry_daily_apply(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath((date, operation_id)): AxumPath<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "vault:write", true)?;
    require_cutover_for_daily_mutation(&state)?;
    let local_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("date must use YYYY-MM-DD"))?;
    let (profile, workspace) = active_refocus_context(&state)?;
    let _guard = state
        .apply_lock
        .lock()
        .map_err(|_| ApiError::internal("Daily Apply lock is unavailable"))?;
    let mut operation = state
        .store
        .apply_operation(&operation_id)
        .map_err(|error| ApiError::internal(error.to_string()))?
        .ok_or_else(|| ApiError::not_found("Apply operation was not found"))?;
    if operation.workspace_id != profile.id || operation.local_date != local_date {
        return Err(ApiError::not_found("Apply operation was not found"));
    }
    if operation.state == "finalized" {
        return Ok(Json(
            json!({"operation": public_apply_operation(&operation)}),
        ));
    }
    if matches!(
        operation.state.as_str(),
        "failed" | "reconciliation_required"
    ) {
        operation = state
            .store
            .transition_apply_operation(&operation.id, &operation.state, "prepared", None)
            .map_err(|error| ApiError::conflict(error.to_string()))?;
    }
    recover_daily_apply(&state, &workspace, operation.clone())
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    let operation = state
        .store
        .apply_operation(&operation.id)
        .map_err(|error| ApiError::internal(error.to_string()))?
        .ok_or_else(|| ApiError::internal("Apply operation disappeared"))?;
    Ok(Json(json!({
        "operation": public_apply_operation(&operation)
    })))
}

fn apply_operation_id(
    workspace_id: &str,
    local_date: NaiveDate,
    input: &ApplyDailyRequest,
) -> String {
    let identity = format!(
        "{workspace_id}\0{local_date}\0{}\0{}\0{}\0{}\0{}\0{}\0{}",
        input.expected_revision_id,
        input.expected_revision_content_hash,
        input.destination_path,
        input.intended_new_block_hash,
        input.expected_target_exists,
        input.expected_original_content_hash,
        input.expected_updated_content_hash
    );
    format!("apply_{}", daily_writer::digest(identity.as_bytes()))
}

fn validate_apply_operation_approval(
    operation: &ApplyOperation,
    input: &ApplyDailyRequest,
) -> Result<(), ApiError> {
    if operation.revision_id != input.expected_revision_id
        || operation.revision_content_hash != input.expected_revision_content_hash
        || operation.destination_path != input.destination_path
        || operation.expected_old_block_hash != input.expected_old_block_hash
        || operation.intended_new_block_hash != input.intended_new_block_hash
        || operation.expected_target_exists != Some(input.expected_target_exists)
        || operation.expected_original_content_hash.as_deref()
            != Some(input.expected_original_content_hash.as_str())
        || operation.intended_updated_content_hash.as_deref()
            != Some(input.expected_updated_content_hash.as_str())
    {
        return Err(ApiError::conflict(
            "Apply approval differs from the journaled operation",
        ));
    }
    Ok(())
}

fn validate_apply_material_approval(
    material: &DailyApplyMaterial,
    input: &ApplyDailyRequest,
) -> Result<(), ApiError> {
    if material.revision.id != input.expected_revision_id
        || material.revision.content_hash != input.expected_revision_content_hash
        || material.day.destination_path != input.destination_path
        || material.plan.intended_new_block_hash != input.intended_new_block_hash
        || material.target_exists != input.expected_target_exists
        || daily_writer::digest(&material.original_content) != input.expected_original_content_hash
        || daily_writer::digest(&material.plan.updated_content)
            != input.expected_updated_content_hash
    {
        return Err(ApiError::conflict(
            "the approved Daily preview changed; preview Apply again",
        ));
    }
    Ok(())
}

fn execute_daily_apply(
    state: &AppState,
    material: &DailyApplyMaterial,
    mut operation: ApplyOperation,
) -> Result<ApplyOperation, ApiError> {
    if matches!(
        operation.state.as_str(),
        "failed" | "reconciliation_required"
    ) {
        return Err(ApiError::conflict(format!(
            "Apply operation requires attention: {}",
            operation
                .failure_reason
                .as_deref()
                .unwrap_or(operation.state.as_str())
        )));
    }
    let (_, workspace) = active_refocus_context(state)?;
    let current_file = daily_writer::read_file(workspace.directory(), &material.target)
        .map_err(|error| ApiError::internal(format!("reading Daily target failed: {error}")))?;
    let current = current_file.as_deref().unwrap_or_default();
    let Some(expected_target_exists) = operation.expected_target_exists else {
        return Err(ApiError::conflict(
            "this legacy Apply operation lacks exact file identity and requires reconciliation",
        ));
    };
    let Some(expected_original_content_hash) = operation.expected_original_content_hash.clone()
    else {
        return Err(ApiError::conflict(
            "this Apply operation has incomplete file identity and requires reconciliation",
        ));
    };
    let Some(intended_updated_content_hash) = operation.intended_updated_content_hash.clone()
    else {
        return Err(ApiError::conflict(
            "this Apply operation has incomplete file identity and requires reconciliation",
        ));
    };
    let current_hash = daily_writer::digest(current);
    let current_is_intended =
        current_file.is_some() && current_hash == intended_updated_content_hash;
    let current_block_hash = if current_file.is_none() {
        None
    } else {
        daily_writer::managed_block_hash(current, &material.day.block_id)
            .map_err(ApiError::conflict)?
    };

    if current_is_intended
        && current_block_hash.as_deref() == Some(operation.intended_new_block_hash.as_str())
    {
        if operation.state == "prepared" {
            operation = transition_apply(state, &operation, "writing", None)?;
        }
        if operation.state == "writing" {
            operation = transition_apply(state, &operation, "written", None)?;
        }
    } else if matches!(operation.state.as_str(), "prepared" | "writing") {
        if current_file.is_some() != expected_target_exists
            || current_hash != expected_original_content_hash
        {
            transition_apply(
                state,
                &operation,
                "reconciliation_required",
                Some("the Daily target changed after approval"),
            )?;
            return Err(ApiError::conflict(
                "the Daily target changed after approval; no content was overwritten",
            ));
        }
        if operation.state == "prepared" {
            operation = transition_apply(state, &operation, "writing", None)?;
        }
        let resolved = workspace
            .resolve_markdown_path(
                std::path::Path::new(&operation.destination_path),
                MarkdownPathMode::MayCreate,
            )
            .map_err(|error| ApiError::conflict(error.to_string()))?;
        if resolved != workspace.canonical_root().join(&material.target) {
            transition_apply(
                state,
                &operation,
                "reconciliation_required",
                Some("the resolved Daily destination changed before writing"),
            )?;
            return Err(ApiError::conflict(
                "the resolved Daily destination changed; no content was written",
            ));
        }
        if let Err(error) = daily_writer::write_atomically(
            workspace.directory(),
            &material.target,
            &material.plan.updated_content,
            &operation.id,
        ) {
            let after = daily_writer::read_file(workspace.directory(), &material.target)
                .ok()
                .flatten();
            let after_bytes = after.as_deref().unwrap_or_default();
            if after.is_some() && daily_writer::digest(after_bytes) == intended_updated_content_hash
            {
                let message =
                    format!("Daily target was replaced but write durability is uncertain: {error}");
                transition_apply(state, &operation, "reconciliation_required", Some(&message))?;
                return Err(ApiError::internal(message));
            } else {
                let (state_name, message) = if after.is_some() == expected_target_exists
                    && daily_writer::digest(after_bytes) == expected_original_content_hash
                {
                    ("failed", format!("atomic Daily write failed: {error}"))
                } else {
                    (
                        "reconciliation_required",
                        format!("Daily target changed during a failed write: {error}"),
                    )
                };
                transition_apply(state, &operation, state_name, Some(&message))?;
                return Err(ApiError::internal(message));
            }
        } else {
            let written = daily_writer::read_file(workspace.directory(), &material.target)
                .map_err(|error| {
                    ApiError::internal(format!("verifying Daily target failed: {error}"))
                })?
                .ok_or_else(|| ApiError::conflict("the Daily target disappeared after writing"))?;
            let written_hash = daily_writer::managed_block_hash(&written, &material.day.block_id)
                .map_err(ApiError::conflict)?;
            if daily_writer::digest(&written) != intended_updated_content_hash
                || written_hash.as_deref() != Some(operation.intended_new_block_hash.as_str())
            {
                transition_apply(
                    state,
                    &operation,
                    "reconciliation_required",
                    Some("the written Daily block did not match the approved block"),
                )?;
                return Err(ApiError::conflict(
                    "the written Daily block needs reconciliation",
                ));
            }
            operation = transition_apply(state, &operation, "written", None)?;
        }
    } else if operation.state == "written" {
        transition_apply(
            state,
            &operation,
            "reconciliation_required",
            Some("the Daily target changed after it was written"),
        )?;
        return Err(ApiError::conflict(
            "the Daily target changed before database finalization",
        ));
    }

    state
        .store
        .finalize_apply_operation(
            &operation.id,
            &operation.revision_id,
            &operation.revision_content_hash,
        )
        .map_err(|error| ApiError::conflict(error.to_string()))
}

fn transition_apply(
    state: &AppState,
    operation: &ApplyOperation,
    next_state: &str,
    reason: Option<&str>,
) -> Result<ApplyOperation, ApiError> {
    state
        .store
        .transition_apply_operation(&operation.id, &operation.state, next_state, reason)
        .map_err(|error| ApiError::conflict(error.to_string()))
}

fn recover_daily_applies(state: &AppState) {
    let (profile, workspace) = match active_refocus_context(state) {
        Ok(context) => context,
        Err(_) => return,
    };
    let operations = match state.store.list_recoverable_apply_operations(100) {
        Ok(operations) => operations,
        Err(error) => {
            tracing::error!(%error, "listing unfinished Daily Apply operations failed");
            return;
        }
    };
    for operation in operations {
        if operation.workspace_id != profile.id {
            continue;
        }
        let result = recover_daily_apply(state, &workspace, operation.clone());
        if let Err(error) = result {
            tracing::error!(operation_id = %operation.id, %error, "recovering Daily Apply operation failed");
        }
    }
}

fn recover_daily_apply(
    state: &AppState,
    workspace: &InspectedWorkspace,
    mut operation: ApplyOperation,
) -> anyhow::Result<()> {
    let day = state
        .store
        .daily_day(&operation.workspace_id, operation.local_date)?
        .ok_or_else(|| anyhow::anyhow!("Daily day is missing"))?;
    workspace.resolve_markdown_path(
        std::path::Path::new(&operation.destination_path),
        MarkdownPathMode::MayCreate,
    )?;
    let target_path = PathBuf::from(&operation.destination_path);
    let target = target_path.as_path();
    let expected_target_exists = operation
        .expected_target_exists
        .ok_or_else(|| anyhow::anyhow!("Apply journal lacks target existence identity"))?;
    let expected_original_content_hash = operation
        .expected_original_content_hash
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Apply journal lacks original content identity"))?;
    let intended_updated_content_hash = operation
        .intended_updated_content_hash
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Apply journal lacks updated content identity"))?;
    let temporary_name = operation
        .temporary_name
        .clone()
        .ok_or_else(|| anyhow::anyhow!("Apply journal lacks temporary file identity"))?;
    let current = daily_writer::read_file(workspace.directory(), target)?;
    let current_bytes = current.as_deref().unwrap_or_default();
    let current_hash = daily_writer::digest(current_bytes);
    let current_block_hash = current
        .as_deref()
        .map(|bytes| daily_writer::managed_block_hash(bytes, &day.block_id))
        .transpose()
        .map_err(anyhow::Error::msg)?
        .flatten();
    let temporary = daily_writer::read_temporary(workspace.directory(), target, &temporary_name)?;

    if current.is_some()
        && current_hash == intended_updated_content_hash
        && current_block_hash.as_deref() == Some(operation.intended_new_block_hash.as_str())
    {
        if temporary.is_some() {
            daily_writer::remove_temporary(workspace.directory(), target, &temporary_name)?;
        }
        daily_writer::sync_parent(workspace.directory(), target)?;
        if operation.state == "prepared" {
            operation = state.store.transition_apply_operation(
                &operation.id,
                "prepared",
                "writing",
                None,
            )?;
        }
        if operation.state == "writing" {
            operation = state.store.transition_apply_operation(
                &operation.id,
                "writing",
                "written",
                None,
            )?;
        }
        return finalize_recovered_apply(state, &operation);
    }

    let current_is_original = current.is_some() == expected_target_exists
        && current_hash == expected_original_content_hash;
    if !current_is_original || operation.state == "written" {
        state.store.transition_apply_operation(
            &operation.id,
            &operation.state,
            "reconciliation_required",
            Some("startup recovery found a Daily target that differs from its exact journal identity"),
        )?;
        return Ok(());
    }
    if operation.state == "prepared" {
        operation =
            state
                .store
                .transition_apply_operation(&operation.id, "prepared", "writing", None)?;
    }

    let write_result = if let Some(temporary_bytes) = temporary {
        if daily_writer::digest(&temporary_bytes) == intended_updated_content_hash {
            daily_writer::commit_temporary(workspace.directory(), target, &temporary_name)
        } else {
            daily_writer::remove_temporary(workspace.directory(), target, &temporary_name)?;
            resume_recovery_write(
                state,
                workspace,
                &operation,
                target,
                &intended_updated_content_hash,
            )
        }
    } else {
        resume_recovery_write(
            state,
            workspace,
            &operation,
            target,
            &intended_updated_content_hash,
        )
    };
    if let Err(error) = write_result {
        if error.rename_completed()
            && daily_writer::sync_parent(workspace.directory(), target).is_ok()
        {
            // An explicit retry established directory-entry durability.
        } else {
            let next_state = if error.rename_completed() {
                "reconciliation_required"
            } else {
                "failed"
            };
            state.store.transition_apply_operation(
                &operation.id,
                &operation.state,
                next_state,
                Some(&format!("startup recovery write failed {error}")),
            )?;
            return Ok(());
        }
    }

    let written = daily_writer::read_file(workspace.directory(), target)?
        .ok_or_else(|| anyhow::anyhow!("Daily target disappeared after recovery write"))?;
    let written_block =
        daily_writer::managed_block_hash(&written, &day.block_id).map_err(anyhow::Error::msg)?;
    if daily_writer::digest(&written) != intended_updated_content_hash
        || written_block.as_deref() != Some(operation.intended_new_block_hash.as_str())
    {
        state.store.transition_apply_operation(
            &operation.id,
            &operation.state,
            "reconciliation_required",
            Some("startup recovery could not verify the exact approved Daily content"),
        )?;
        return Ok(());
    }
    operation =
        state
            .store
            .transition_apply_operation(&operation.id, "writing", "written", None)?;
    finalize_recovered_apply(state, &operation)
}

fn resume_recovery_write(
    state: &AppState,
    workspace: &InspectedWorkspace,
    operation: &ApplyOperation,
    target: &Path,
    intended_updated_content_hash: &str,
) -> Result<(), daily_writer::AtomicWriteError> {
    let material = daily_apply_material(state, operation.local_date).map_err(|error| {
        daily_writer::AtomicWriteError::BeforeRename(std::io::Error::other(error.message))
    })?;
    if daily_writer::digest(&material.plan.updated_content) != intended_updated_content_hash {
        return Err(daily_writer::AtomicWriteError::BeforeRename(
            std::io::Error::other("reviewed Daily content no longer matches the journal"),
        ));
    }
    daily_writer::write_atomically(
        workspace.directory(),
        target,
        &material.plan.updated_content,
        &operation.id,
    )
}

fn finalize_recovered_apply(state: &AppState, operation: &ApplyOperation) -> anyhow::Result<()> {
    if let Err(error) = state.store.finalize_apply_operation(
        &operation.id,
        &operation.revision_id,
        &operation.revision_content_hash,
    ) {
        state.store.transition_apply_operation(
            &operation.id,
            "written",
            "reconciliation_required",
            Some(&format!(
                "Daily file is written but finalization failed: {error}"
            )),
        )?;
    }
    Ok(())
}

fn daily_apply_material(
    state: &AppState,
    local_date: NaiveDate,
) -> Result<DailyApplyMaterial, ApiError> {
    let (profile, workspace) = active_refocus_context(state)?;
    let day = state
        .store
        .daily_day(&profile.id, local_date)
        .map_err(|error| ApiError::internal(error.to_string()))?
        .ok_or_else(|| ApiError::conflict("generate a Daily candidate before previewing Apply"))?;
    let revision = state
        .store
        .current_proposal_revision(&profile.id, local_date)
        .map_err(|error| ApiError::internal(error.to_string()))?
        .ok_or_else(|| ApiError::conflict("generate a Daily candidate before previewing Apply"))?;
    if day.current_revision_id.as_deref() != Some(revision.id.as_str()) {
        return Err(ApiError::conflict(
            "the Daily candidate changed; reload before previewing Apply",
        ));
    }
    let content = serde_json::from_value::<DailyRevisionContent>(revision.content.clone())
        .map_err(|error| ApiError::internal(format!("invalid stored Daily candidate: {error}")))?;
    let manual_entries = state
        .store
        .manual_daily_entries(&profile.id, local_date)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let snapshot = match revision.snapshot_id.as_deref() {
        Some(snapshot_id) => Some(
            state
                .store
                .evidence_snapshot(snapshot_id)
                .map_err(|error| ApiError::internal(error.to_string()))?
                .ok_or_else(|| ApiError::internal("the current evidence snapshot is missing"))?,
        ),
        None => None,
    };
    let snapshot_evidence = match snapshot.as_ref() {
        Some(snapshot) => state
            .store
            .snapshot_evidence(&snapshot.id)
            .map_err(|error| ApiError::internal(error.to_string()))?,
        None => Vec::new(),
    };
    let live = state
        .store
        .get_events_between(day.start_utc, day.end_utc, 500)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    if live.truncated {
        return Err(ApiError::unprocessable(
            "Daily evidence exceeds the supported 500-event Apply limit.",
        ));
    }
    let deferred_event_ids = state
        .store
        .active_daily_evidence_deferrals(&profile.id, local_date, &revision.id)
        .map_err(|error| ApiError::internal(error.to_string()))?
        .into_iter()
        .map(|deferral| deferral.event_id)
        .collect::<HashSet<_>>();
    let live_event_ids = live
        .events
        .iter()
        .filter(|event| !deferred_event_ids.contains(&event.id))
        .map(|event| event.id.as_str())
        .collect::<Vec<_>>();
    let snapshot_event_ids = snapshot
        .as_ref()
        .map(|snapshot| snapshot.event_ids.as_slice())
        .unwrap_or_default();
    let live_manual_ids = manual_entries
        .iter()
        .map(|entry| entry.id.as_str())
        .collect::<Vec<_>>();
    if has_new_automated_evidence(&live_event_ids, snapshot_event_ids)
        || !content
            .manual_entry_ids
            .iter()
            .map(String::as_str)
            .eq(live_manual_ids)
    {
        return Err(ApiError::conflict(
            "new Daily evidence or manual notes are available; regenerate before Apply",
        ));
    }
    workspace
        .resolve_markdown_path(
            std::path::Path::new(&day.destination_path),
            MarkdownPathMode::MayCreate,
        )
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let target = PathBuf::from(&day.destination_path);
    let original = daily_writer::read_file(workspace.directory(), &target)
        .map_err(|error| ApiError::internal(format!("reading Daily target failed: {error}")))?;
    let target_exists = original.is_some();
    let original_content = original.unwrap_or_default();
    let (initial_content, template_used) = if target_exists {
        (original_content.as_slice(), None)
    } else if day.template_revision.as_deref() == Some("none") {
        (&[][..], None)
    } else if let Some(template_revision) = day.template_revision.clone() {
        let template = state
            .store
            .daily_template_snapshot(&profile.id, local_date)
            .map_err(|error| ApiError::internal(error.to_string()))?
            .ok_or_else(|| ApiError::conflict("the frozen Daily template snapshot is missing"))?;
        if template.content_hash != template_revision
            || daily_writer::digest(&template.content) != template_revision
        {
            return Err(ApiError::conflict(
                "the frozen Daily template snapshot failed integrity verification",
            ));
        }
        let plan = daily_writer::plan_managed_block(
            Some(&template.content),
            &day.block_id,
            &llm::render_daily_revision_preview(&content, &manual_entries, &snapshot_evidence),
            &format!("Daily log {local_date}"),
        )
        .map_err(ApiError::conflict)?;
        return Ok(DailyApplyMaterial {
            profile,
            day,
            revision,
            target,
            target_exists,
            template_used: Some(template.template_path),
            original_content,
            plan,
        });
    } else {
        return Err(ApiError::conflict(
            "this legacy Daily record has no frozen template choice; regenerate it before Apply",
        ));
    };
    let markdown =
        llm::render_daily_revision_preview(&content, &manual_entries, &snapshot_evidence);
    let plan = daily_writer::plan_managed_block(
        Some(initial_content),
        &day.block_id,
        &markdown,
        &format!("Daily log {local_date}"),
    )
    .map_err(ApiError::conflict)?;
    Ok(DailyApplyMaterial {
        profile,
        day,
        revision,
        target,
        target_exists,
        template_used,
        original_content,
        plan,
    })
}

fn has_new_automated_evidence(live_event_ids: &[&str], snapshot_event_ids: &[String]) -> bool {
    live_event_ids
        .iter()
        .any(|event_id| !snapshot_event_ids.iter().any(|stored| stored == event_id))
}

fn ensure_refocus_daily_day(
    state: &AppState,
    profile: &WorkspaceProfile,
    workspace: &InspectedWorkspace,
    local_date: NaiveDate,
    destination_path: &str,
) -> Result<DailyDay, ApiError> {
    state
        .store
        .ensure_daily_day(local_date, destination_path, None)
        .map_err(generation::database_error)?;
    let template = match profile.template_path.as_deref() {
        Some(path) => {
            workspace
                .resolve_markdown_path(Path::new(path), MarkdownPathMode::ExistingFile)
                .map_err(|error| ApiError::bad_request(error.to_string()))?;
            let content = daily_writer::read_file(workspace.directory(), Path::new(path))
                .map_err(|error| {
                    ApiError::internal(format!("reading Daily template failed: {error}"))
                })?
                .ok_or_else(|| ApiError::conflict("the reviewed Daily template disappeared"))?;
            Some((path, content))
        }
        None => None,
    };
    state
        .store
        .freeze_daily_template(
            &profile.id,
            local_date,
            template
                .as_ref()
                .map(|(path, content)| (*path, content.as_slice())),
        )
        .map_err(generation::database_error)
}

async fn refocus_create_manual_entry(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(date): AxumPath<String>,
    Json(input): Json<ManualDailyEntryRequest>,
) -> Result<(StatusCode, Json<log_inbox_core::models::ManualDailyEntry>), ApiError> {
    authorize_refocus(&state, &headers, "review:write", true)?;
    let local_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("date must use YYYY-MM-DD"))?;
    let (profile, workspace) = active_refocus_context(&state)?;
    let frozen_day = state
        .store
        .daily_day(&profile.id, local_date)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let window = effective_daily_window(local_date, &profile, frozen_day.as_ref())
        .map_err(ApiError::bad_request)?;
    ensure_refocus_daily_day(
        &state,
        &profile,
        &workspace,
        local_date,
        &window.destination_path,
    )?;
    let entry = state
        .store
        .create_manual_daily_entry(&profile.id, local_date, &input.text, &input.references)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok((StatusCode::CREATED, Json(entry)))
}

async fn refocus_delete_manual_entry(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath((date, entry_id)): AxumPath<(String, String)>,
) -> Result<StatusCode, ApiError> {
    authorize_refocus(&state, &headers, "review:write", true)?;
    let local_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("date must use YYYY-MM-DD"))?;
    let (profile, _) = active_refocus_context(&state)?;
    state
        .store
        .delete_manual_daily_entry(&profile.id, local_date, &entry_id)
        .map_err(|error| ApiError::not_found(error.to_string()))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn refocus_create_activity_record(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(date): AxumPath<String>,
) -> Result<(StatusCode, Json<ProposalRevision>), ApiError> {
    authorize_refocus(&state, &headers, "draft:generate", true)?;
    let date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("date must use YYYY-MM-DD"))?;
    let _guard = state
        .daily_generation_lock
        .try_lock()
        .map_err(|_| ApiError::conflict("A draft is being prepared. Wait or cancel it first."))?;
    Ok((
        StatusCode::CREATED,
        Json(create_activity_record(&state, date)?),
    ))
}

fn create_activity_record(state: &AppState, date: NaiveDate) -> Result<ProposalRevision, ApiError> {
    require_cutover_for_daily_mutation(state)?;
    let (profile, workspace) = active_refocus_context(state)?;
    if state
        .store
        .current_proposal_revision(&profile.id, date)
        .map_err(generation::database_error)?
        .is_some()
    {
        return Err(ApiError::conflict(
            "This day already has a draft. It has not been replaced.",
        ));
    }
    let day = state
        .store
        .daily_day(&profile.id, date)
        .map_err(generation::database_error)?;
    if state
        .store
        .day_has_expired_snapshot_evidence(&profile.id, date)
        .map_err(generation::database_error)?
    {
        return Err(ApiError::conflict(
            "Some activity from a previous attempt has expired. A partial activity record cannot replace it.",
        ));
    }
    if day
        .as_ref()
        .is_some_and(|day| day.review_status == "dismissed")
    {
        return Err(ApiError::conflict(
            "Reopen this day before creating an activity record.",
        ));
    }
    let window =
        effective_daily_window(date, &profile, day.as_ref()).map_err(ApiError::bad_request)?;
    ensure_refocus_daily_day(state, &profile, &workspace, date, &window.destination_path)?;
    let evidence = state
        .store
        .get_events_between(window.start_utc, window.end_utc, 500)
        .map_err(generation::database_error)?;
    if evidence.truncated {
        return Err(ApiError::unprocessable(
            "This day exceeds the 500-event limit. No partial activity record was created.",
        ));
    }
    let manual = state
        .store
        .manual_daily_entries(&profile.id, date)
        .map_err(generation::database_error)?;
    if evidence.events.is_empty() && manual.is_empty() {
        return Err(ApiError::conflict(
            "This day has no activity or manual notes.",
        ));
    }
    let ids = evidence
        .events
        .iter()
        .map(|event| event.id.clone())
        .collect::<Vec<_>>();
    let snapshot = if ids.is_empty() {
        None
    } else {
        Some(
            state
                .store
                .create_evidence_snapshot(&profile.id, date, &ids)
                .map_err(generation::database_error)?,
        )
    };
    let content = serde_json::to_value(llm::activity_record(
        &evidence.events,
        manual.into_iter().map(|entry| entry.id).collect(),
    ))
    .map_err(|error| ApiError::internal(error.to_string()))?;
    state
        .store
        .create_generated_revision_if_current(
            &profile.id,
            date,
            snapshot.as_ref().map(|snapshot| snapshot.id.as_str()),
            None,
            "structured_edit",
            &content,
            None,
        )
        .map_err(generation::database_error)
}

async fn refocus_generate_daily(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(date): AxumPath<String>,
    input: Option<Json<GenerateDailyRequest>>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    authorize_refocus(&state, &headers, "draft:generate", true)?;
    let local_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("date must use YYYY-MM-DD"))?;
    let input = input.map(|Json(input)| input).unwrap_or_default();
    if input.reference_mode == generation::ReferenceMode::None && input.context_exclusions.is_some()
    {
        return Err(ApiError::bad_request(
            "Reference exclusions cannot be combined with reference_mode none.",
        ));
    }
    let context_adjustments = match (input.expected_revision_id, input.context_exclusions) {
        (Some(expected_revision_id), Some(exclusions)) => Some((
            expected_revision_id,
            validate_requested_context_exclusions(exclusions)?,
        )),
        (None, None) => None,
        _ => {
            return Err(ApiError::bad_request(
                "expected_revision_id and context_exclusions must be provided together",
            ));
        }
    };
    let guard = state
        .daily_generation_lock
        .clone()
        .try_lock_owned()
        .map_err(|_| {
            ApiError::conflict(
                "generation_busy: Another draft is being prepared. Wait or cancel it first.",
            )
        })?;
    let (attempt, value) =
        generation::begin_with_references(&state, local_date, "manual", input.reference_mode)?;
    tokio::spawn(async move {
        if let Err(error) = generation::run(
            &state,
            local_date,
            input.replace_edited,
            context_adjustments,
            attempt,
            guard,
        )
        .await
        {
            tracing::warn!(status = %error.status, "Daily generation ended without a candidate");
        }
    });
    Ok((StatusCode::ACCEPTED, Json(json!({"attempt": value}))))
}

async fn generate_context_comparison_arms(
    config: Option<&llm::LlmConfig>,
    context_snapshot: &log_inbox_core::models::ContextSnapshot,
    destination_path: &str,
    events: Vec<log_inbox_core::models::StoredLogEvent>,
    manual_entry_ids: Vec<String>,
) -> Result<(DailyRevisionContent, DailyRevisionContent), ApiError> {
    preflight_reference_context(&context_snapshot.payload)?;
    let mut with_context = knowledge::vault_context_from_snapshot(&context_snapshot.payload)
        .map_err(ApiError::conflict)?;
    if context_snapshot
        .payload
        .get("excerpts")
        .and_then(Value::as_array)
        .is_some_and(|excerpts| !excerpts.is_empty())
        && !config.is_some_and(llm::knowledge_text_stays_local)
    {
        return Err(ApiError::conflict(
            "this comparison contains frozen note excerpts and requires a local model",
        ));
    }
    if let Some(context) = with_context.as_object_mut() {
        context.insert(
            "daily_note".to_owned(),
            Value::String(destination_path.to_owned()),
        );
        context.insert(
            "link_context_revision".to_owned(),
            Value::String(context_snapshot.snapshot_digest.clone()),
        );
    }
    let without_context = json!({
        "candidate_notes": [],
        "workstream_links": {},
        "group_aliases": {},
        "daily_note": destination_path,
        "knowledge": {
            "resolver_version": "none",
            "context_is_background_only": true,
            "excerpts": [],
            "workstream_excerpts": {}
        }
    });
    let task = Some(
        "Create a concise, evidence-backed daily engineering record. Preserve distinct outcomes, decisions, trade-offs, validation, blockers, and follow-up."
            .to_owned(),
    );
    let with_args = llm::SuggestMarkdownSummaryArgs {
        vault_context: with_context,
        mode: "daily-consolidation".to_owned(),
        task: task.clone(),
    };
    let without_args = llm::SuggestMarkdownSummaryArgs {
        vault_context: without_context,
        mode: "daily-consolidation".to_owned(),
        task,
    };
    let with_proposal =
        llm::generate_automated_daily_summary(config, with_args.clone(), events.clone())
            .await
            .map_err(ApiError::unprocessable)?;
    let without_proposal =
        llm::generate_automated_daily_summary(config, without_args.clone(), events)
            .await
            .map_err(ApiError::unprocessable)?;
    let with_draft = with_proposal
        .structured_draft
        .ok_or_else(|| ApiError::internal("comparison generator omitted structured content"))?;
    let without_draft = without_proposal
        .structured_draft
        .ok_or_else(|| ApiError::internal("comparison generator omitted structured content"))?;
    Ok((
        llm::daily_revision_content(with_draft, manual_entry_ids.clone(), &with_args),
        llm::daily_revision_content(without_draft, manual_entry_ids, &without_args),
    ))
}

fn public_context_comparison(
    state: &AppState,
    comparison: &ContextComparison,
) -> Result<Value, ApiError> {
    let manual_entries = state
        .store
        .manual_daily_entries(&comparison.workspace_id, comparison.local_date)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let evidence = state
        .store
        .snapshot_evidence(&comparison.snapshot_id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let mut output = json!({
        "id": comparison.id,
        "source_revision_id": comparison.source_revision_id,
        "state": comparison.state,
        "created_at": comparison.created_at,
        "arms": [
            {
                "id": "a",
                "preview_markdown": llm::render_daily_revision_preview(
                    &comparison.arm_a_content,
                    &manual_entries,
                    &evidence,
                )
            },
            {
                "id": "b",
                "preview_markdown": llm::render_daily_revision_preview(
                    &comparison.arm_b_content,
                    &manual_entries,
                    &evidence,
                )
            }
        ]
    });
    if let Some(kind) = comparison.arm_a_kind.as_deref() {
        let (a, b) = if kind == "context" {
            ("with_context", "without_context")
        } else {
            ("without_context", "with_context")
        };
        output["assignment"] = json!({"a": a, "b": b});
    }
    if let Some(decision) = comparison.decision.as_ref() {
        output["decision"] = serde_json::to_value(decision)
            .map_err(|error| ApiError::internal(error.to_string()))?;
    }
    Ok(output)
}

async fn refocus_start_context_comparison(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(date): AxumPath<String>,
    Json(input): Json<StartContextComparisonRequest>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "draft:generate", true)?;
    require_cutover_for_daily_mutation(&state)?;
    let local_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("date must use YYYY-MM-DD"))?;
    let _generation_guard = state
        .daily_generation_lock
        .try_lock()
        .map_err(|_| ApiError::conflict("generation_busy: Another draft is being prepared."))?;
    let profile = active_refocus_workspace(&state)?;
    let day = state
        .store
        .daily_day(&profile.id, local_date)
        .map_err(|error| ApiError::internal(error.to_string()))?
        .ok_or_else(|| ApiError::conflict("generate a Daily candidate before comparing it"))?;
    let revision = state
        .store
        .current_proposal_revision(&profile.id, local_date)
        .map_err(|error| ApiError::internal(error.to_string()))?
        .ok_or_else(|| ApiError::conflict("generate a Daily candidate before comparing it"))?;
    if revision.id != input.expected_revision_id {
        return Err(ApiError::conflict(
            "the Daily candidate changed; start the comparison again",
        ));
    }
    if day.review_status != "in_review"
        || day.freshness != "current"
        || !matches!(revision.origin.as_str(), "generated" | "regenerated")
    {
        return Err(ApiError::conflict(
            "only a current, unedited Daily candidate can be compared",
        ));
    }
    let context_snapshot = state
        .store
        .proposal_context_snapshot(&revision.id)
        .map_err(|error| ApiError::internal(error.to_string()))?
        .ok_or_else(|| ApiError::conflict("this candidate did not use Knowledge context"))?;
    if context_snapshot
        .payload
        .get("used_notes")
        .and_then(Value::as_array)
        .is_none_or(Vec::is_empty)
    {
        return Err(ApiError::conflict(
            "this candidate has no matched Knowledge notes to compare",
        ));
    }
    if let Some(existing) = state
        .store
        .context_comparison_for_revision(&revision.id)
        .map_err(|error| ApiError::internal(error.to_string()))?
    {
        return public_context_comparison(&state, &existing).map(Json);
    }
    let snapshot_id = revision
        .snapshot_id
        .as_deref()
        .ok_or_else(|| ApiError::conflict("this candidate has no automated evidence"))?;
    let snapshot = state
        .store
        .evidence_snapshot(snapshot_id)
        .map_err(|error| ApiError::internal(error.to_string()))?
        .ok_or_else(|| ApiError::internal("the candidate evidence snapshot is missing"))?;
    let snapshot_evidence = state
        .store
        .snapshot_evidence(snapshot_id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    if snapshot_evidence.iter().any(|item| !item.available) {
        return Err(ApiError::conflict(
            "source evidence expired, so a fair comparison can no longer be generated",
        ));
    }
    let source_content = serde_json::from_value::<DailyRevisionContent>(revision.content.clone())
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let events = state
        .store
        .get_events_by_ids(&snapshot.event_ids)
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    let config = state
        .llm_config
        .as_ref()
        .ok_or_else(|| ApiError::unprocessable("Daily comparison requires a configured LLM"))?;
    let (mut attempt, _) = generation::begin(&state, local_date, "comparison")?;
    attempt.stage("requesting_model")?;
    let result = async {
    let (context_content, without_context_content) = tokio::select! {
        biased;
        _ = attempt.cancel.changed() => Err(ApiError::unprocessable("Generation canceled.")),
        _ = tokio::time::sleep_until(attempt.deadline) => Err(ApiError::unprocessable("Generation timed out.")),
        result = generate_context_comparison_arms(
        Some(config),
        &context_snapshot,
        &day.destination_path,
        events,
        source_content.manual_entry_ids,
        ) => result,
    }?;
    attempt.stage("saving")?;
    let comparison = state
        .store
        .create_context_comparison(
            &profile.id,
            local_date,
            &revision.id,
            &context_content,
            &without_context_content,
            &llm::generation_configuration_digest(config),
            &llm::daily_generation_contract_digest(),
        )
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    public_context_comparison(&state, &comparison).map(Json)
    }.await;
    attempt.finish(&result)?;
    result
}

async fn refocus_decide_context_comparison(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath((date, comparison_id)): AxumPath<(String, String)>,
    Json(input): Json<DecideContextComparisonRequest>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "review:write", true)?;
    require_cutover_for_daily_mutation(&state)?;
    let local_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("date must use YYYY-MM-DD"))?;
    let profile = active_refocus_workspace(&state)?;
    let comparison = state
        .store
        .context_comparison_for_revision(&input.expected_revision_id)
        .map_err(|error| ApiError::internal(error.to_string()))?
        .filter(|comparison| {
            comparison.id == comparison_id
                && comparison.workspace_id == profile.id
                && comparison.local_date == local_date
        })
        .ok_or_else(|| ApiError::not_found("Daily context comparison was not found"))?;
    let decided = state
        .store
        .decide_context_comparison(
            &comparison.id,
            &input.expected_revision_id,
            &input.continue_with,
            &input.usefulness,
            &input.less_editing,
            input.note.as_deref(),
        )
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    public_context_comparison(&state, &decided).map(Json)
}

fn validate_requested_context_exclusions(
    exclusions: Vec<KnowledgeExcerptExclusionRequest>,
) -> Result<BTreeSet<(String, String)>, ApiError> {
    if exclusions.len() > 64 {
        return Err(ApiError::bad_request(
            "context_exclusions cannot contain more than 64 entries",
        ));
    }
    let mut validated = BTreeSet::new();
    for exclusion in exclusions {
        if exclusion.workstream_id.trim() != exclusion.workstream_id
            || exclusion.workstream_id.is_empty()
            || exclusion.workstream_id.len() > 512
            || exclusion.note_path.trim() != exclusion.note_path
            || exclusion.note_path.is_empty()
            || exclusion.note_path.len() > 511
        {
            return Err(ApiError::bad_request("invalid Knowledge excerpt exclusion"));
        }
        if !validated.insert((exclusion.workstream_id, exclusion.note_path)) {
            return Err(ApiError::bad_request(
                "duplicate Knowledge excerpt exclusion",
            ));
        }
    }
    Ok(validated)
}

fn frozen_context_exclusions(
    snapshot: Option<&log_inbox_core::models::ContextSnapshot>,
) -> BTreeSet<(String, String)> {
    snapshot
        .and_then(|snapshot| snapshot.payload.get("applied_exclusions"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|exclusion| {
            Some((
                exclusion.get("workstream_id")?.as_str()?.to_owned(),
                exclusion.get("note_path")?.as_str()?.to_owned(),
            ))
        })
        .collect()
}

fn without_references_payload(workspace: &InspectedWorkspace) -> Value {
    json!({"schema_version":1,"resolver_version":"none","reference_mode":"none","root_binding":workspace.root_binding(),"workstream_links":{},"workstream_evidence":{},"diagnostics":{}})
}

fn preflight_reference_context(payload: &Value) -> Result<(), ApiError> {
    log_inbox_core::validate_context_snapshot_payload(payload)
        .map_err(|_| ApiError::invalid_references())?;
    knowledge::context_snapshot_digest(payload).map_err(|_| ApiError::invalid_references())?;
    Ok(())
}

#[cfg(test)]
async fn generate_daily_candidate(
    state: &AppState,
    local_date: NaiveDate,
    replace_edited: bool,
    requested_context_adjustments: Option<(String, BTreeSet<(String, String)>)>,
) -> Result<ProposalRevision, ApiError> {
    let guard = state
        .daily_generation_lock
        .clone()
        .try_lock_owned()
        .map_err(|_| ApiError::conflict("generation_busy: Another draft is being prepared."))?;
    let (attempt, _) = generation::begin(state, local_date, "scheduled")?;
    generation::run(
        state,
        local_date,
        replace_edited,
        requested_context_adjustments,
        attempt,
        guard,
    )
    .await
}

async fn generate_daily_candidate_inner(
    state: &AppState,
    local_date: NaiveDate,
    replace_edited: bool,
    requested_context_adjustments: Option<(String, BTreeSet<(String, String)>)>,
    attempt: &mut generation::Attempt,
) -> Result<ProposalRevision, ApiError> {
    require_cutover_for_daily_mutation(state)?;
    let (profile, workspace) = active_refocus_context(state)?;
    if profile.id != attempt.workspace_id {
        return Err(ApiError::conflict(
            "The active workspace changed before generation started.",
        ));
    }
    let frozen_day = state
        .store
        .daily_day(&profile.id, local_date)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    if frozen_day
        .as_ref()
        .is_some_and(|day| day.review_status == "dismissed")
    {
        return Err(ApiError::conflict(
            "Reopen this dismissed day before generating another candidate.",
        ));
    }
    let window = effective_daily_window(local_date, &profile, frozen_day.as_ref())
        .map_err(ApiError::bad_request)?;
    ensure_refocus_daily_day(
        state,
        &profile,
        &workspace,
        local_date,
        &window.destination_path,
    )?;
    let mut evidence = state
        .store
        .get_events_between(window.start_utc, window.end_utc, 500)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    if evidence.truncated {
        return Err(ApiError::unprocessable(
            "Daily evidence exceeds the supported 500-event preview limit.",
        ));
    }
    let manual_entry_ids = state
        .store
        .manual_daily_entries(&profile.id, local_date)
        .map_err(|error| ApiError::internal(error.to_string()))?
        .into_iter()
        .map(|entry| entry.id)
        .collect::<Vec<_>>();
    let current = state
        .store
        .current_proposal_revision(&profile.id, local_date)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    if let Some((expected_revision_id, _)) = requested_context_adjustments.as_ref()
        && current.as_ref().map(|revision| revision.id.as_str())
            != Some(expected_revision_id.as_str())
    {
        return Err(ApiError::conflict(
            "the Daily candidate changed; review its frozen Knowledge context again",
        ));
    }
    if let Some(current) = current.as_ref() {
        if current
            .content
            .get("schema_version")
            .and_then(Value::as_u64)
            == Some(2)
            && !replace_edited
        {
            return Err(ApiError::conflict(
                "Confirm replacement before generating an AI summary of this activity record.",
            ));
        }
        let deferred_event_ids = state
            .store
            .active_daily_evidence_deferrals(&profile.id, local_date, &current.id)
            .map_err(|error| ApiError::internal(error.to_string()))?
            .into_iter()
            .map(|deferral| deferral.event_id)
            .collect::<HashSet<_>>();
        evidence
            .events
            .retain(|event| !deferred_event_ids.contains(&event.id));
    }

    if let Some(current) = current.as_ref()
        && let Some(snapshot_id) = current.snapshot_id.as_deref()
    {
        let snapshot = state
            .store
            .evidence_snapshot(snapshot_id)
            .map_err(|error| ApiError::internal(error.to_string()))?
            .ok_or_else(|| ApiError::internal("the current evidence snapshot is missing"))?;
        let snapshot_evidence = state
            .store
            .snapshot_evidence(snapshot_id)
            .map_err(|error| ApiError::internal(error.to_string()))?;
        if snapshot_evidence.iter().any(|item| !item.available) {
            if requested_context_adjustments.is_some()
                || attempt.reference_mode == generation::ReferenceMode::None
            {
                return Err(ApiError::conflict(
                    "Source evidence for this candidate has expired, so its Knowledge context cannot be regenerated safely. The current revision is preserved.",
                ));
            }
            let mut content =
                serde_json::from_value::<DailyRevisionContent>(current.content.clone())
                    .map_err(|error| ApiError::internal(error.to_string()))?;
            let live_event_ids = evidence
                .events
                .iter()
                .map(|event| event.id.as_str())
                .collect::<Vec<_>>();
            let has_new_evidence = has_new_automated_evidence(&live_event_ids, &snapshot.event_ids);
            if !has_new_evidence && content.manual_entry_ids == manual_entry_ids {
                attempt.stage("saving")?;
                return Ok(current.clone());
            }
            if !has_new_evidence {
                content.manual_entry_ids = manual_entry_ids;
                let content = serde_json::to_value(content)
                    .map_err(|error| ApiError::internal(error.to_string()))?;
                let context_snapshot = state
                    .store
                    .proposal_context_snapshot(&current.id)
                    .map_err(|error| ApiError::internal(error.to_string()))?;
                attempt.stage("saving")?;
                return attempt.save(|| {
                    state.store.create_generated_revision_if_current(
                        &profile.id,
                        local_date,
                        Some(snapshot_id),
                        context_snapshot
                            .as_ref()
                            .map(|snapshot| snapshot.id.as_str()),
                        "structured_edit",
                        &content,
                        Some(&current.id),
                    )
                });
            }
            return Err(ApiError::conflict(
                "Some source evidence for this candidate has expired. Log Inbox will not replace a complete reviewed record from partial evidence. The existing revision is preserved; restore the missing source evidence before regenerating.",
            ));
        }
    }

    if evidence.events.is_empty() {
        if requested_context_adjustments.is_some() {
            return Err(ApiError::bad_request(
                "Knowledge excerpt adjustments require automated evidence",
            ));
        }
        if manual_entry_ids.is_empty() {
            return Err(ApiError::conflict(
                "This day has no evidence or manual notes.",
            ));
        }
        let content = serde_json::to_value(DailyRevisionContent {
            schema_version: 1,
            workstreams: Vec::new(),
            manual_entry_ids,
            open_questions: Vec::new(),
        })
        .map_err(|error| ApiError::internal(error.to_string()))?;
        if let Some(current) = current
            .as_ref()
            .filter(|current| current.snapshot_id.is_none() && current.content == content)
        {
            let snapshot = state
                .store
                .proposal_context_snapshot(&current.id)
                .map_err(|e| ApiError::internal(e.to_string()))?;
            let saved_mode = snapshot
                .as_ref()
                .and_then(|s| s.payload.get("reference_mode"))
                .and_then(Value::as_str)
                .unwrap_or("configured");
            if saved_mode == attempt.reference_mode.as_str() {
                attempt.stage("saving")?;
                return Ok(current.clone());
            }
        }
        attempt.stage("saving")?;
        let context = if attempt.reference_mode == generation::ReferenceMode::None {
            Some(attempt.save(|| {
                state.store.create_context_snapshot(
                    &profile.id,
                    local_date,
                    &without_references_payload(&workspace),
                )
            })?)
        } else {
            None
        };
        let revision = attempt.save(|| {
            state.store.create_generated_revision_if_current(
                &profile.id,
                local_date,
                None,
                context.as_ref().map(|snapshot| snapshot.id.as_str()),
                "manual",
                &content,
                current.as_ref().map(|r| r.id.as_str()),
            )
        })?;
        return Ok(revision);
    }

    let event_ids = evidence
        .events
        .iter()
        .map(|event| event.id.clone())
        .collect::<Vec<_>>();
    let snapshot = attempt.save(|| {
        state
            .store
            .create_evidence_snapshot(&profile.id, local_date, &event_ids)
    })?;
    let collections = if attempt.reference_mode == generation::ReferenceMode::None {
        Vec::new()
    } else {
        state
            .store
            .list_knowledge_collections(&profile.id)
            .map_err(|error| ApiError::internal(error.to_string()))?
    };
    let mappings = if attempt.reference_mode == generation::ReferenceMode::None {
        Vec::new()
    } else {
        state
            .store
            .list_context_mappings(&profile.id)
            .map_err(|error| ApiError::internal(error.to_string()))?
    };
    let current_context_snapshot = match current.as_ref() {
        Some(revision) => state
            .store
            .proposal_context_snapshot(&revision.id)
            .map_err(|error| ApiError::internal(error.to_string()))?,
        None => None,
    };
    let context_adjustment_requested = requested_context_adjustments.is_some();
    let context_exclusions = if attempt.reference_mode == generation::ReferenceMode::None {
        BTreeSet::new()
    } else {
        requested_context_adjustments
            .map(|(_, exclusions)| exclusions)
            .unwrap_or_else(|| frozen_context_exclusions(current_context_snapshot.as_ref()))
    };
    let (mut desired_context_payload, mut vault_context) = if attempt.reference_mode
        == generation::ReferenceMode::None
    {
        (
            Some(without_references_payload(&workspace)),
            json!({"candidate_notes":[],"workstream_links":{},"group_aliases":{}}),
        )
    } else {
        match knowledge::resolve_knowledge_with_adjustments(
            &workspace,
            &collections,
            &mappings,
            &evidence.events,
            state
                .llm_config
                .as_ref()
                .is_some_and(llm::knowledge_text_stays_local),
            &context_exclusions,
        ) {
            Ok(Some(resolution)) => (Some(resolution.snapshot_payload), resolution.vault_context),
            Ok(None) if context_adjustment_requested => {
                return Err(ApiError::conflict(
                    "Knowledge is no longer available for this candidate; review its context again",
                ));
            }
            Ok(None) => (
                None,
                json!({
                    "candidate_notes": [],
                    "workstream_links": {}
                }),
            ),
            Err(_) => return Err(ApiError::invalid_references()),
        }
    };
    let payload = desired_context_payload.get_or_insert_with(|| json!({"schema_version":1,"resolver_version":"none","root_binding":workspace.root_binding(),"workstream_links":{},"workstream_evidence":{},"diagnostics":{}}));
    payload["generation_contract"] = Value::String(llm::daily_generation_contract_digest());
    payload["reference_mode"] = Value::String(attempt.reference_mode.as_str().to_owned());
    if let Some(payload) = desired_context_payload.as_ref() {
        preflight_reference_context(payload)?;
    }
    let desired_context_digest = desired_context_payload
        .as_ref()
        .map(knowledge::context_snapshot_digest)
        .transpose()
        .map_err(ApiError::unprocessable)?;
    let context_is_current = current_context_snapshot
        .as_ref()
        .map(|snapshot| snapshot.snapshot_digest.as_str())
        == desired_context_digest.as_deref();
    if let Some(current) = current.as_ref()
        && current.snapshot_id.as_deref() == Some(snapshot.id.as_str())
        && context_is_current
    {
        attempt.stage("saving")?;
        let mut content = serde_json::from_value::<DailyRevisionContent>(current.content.clone())
            .map_err(|error| ApiError::internal(error.to_string()))?;
        if content.manual_entry_ids == manual_entry_ids {
            return Ok(current.clone());
        }
        content.manual_entry_ids = manual_entry_ids;
        let content =
            serde_json::to_value(content).map_err(|error| ApiError::internal(error.to_string()))?;
        return attempt.save(|| {
            state.store.create_generated_revision_if_current(
                &profile.id,
                local_date,
                Some(&snapshot.id),
                current_context_snapshot
                    .as_ref()
                    .map(|snapshot| snapshot.id.as_str()),
                "structured_edit",
                &content,
                Some(&current.id),
            )
        });
    }
    if current
        .as_ref()
        .is_some_and(|current| current.origin == "structured_edit")
        && !replace_edited
    {
        return Err(ApiError::conflict(
            "New evidence is available, but the current candidate has edits. Confirm replacement to regenerate.",
        ));
    }

    state
        .store
        .set_daily_generation_status(&profile.id, local_date, "running")
        .map_err(|error| ApiError::internal(error.to_string()))?;
    if let Some(context) = vault_context.as_object_mut() {
        context.insert(
            "daily_note".to_owned(),
            Value::String(window.destination_path.clone()),
        );
        if let Some(digest) = desired_context_digest.as_ref() {
            context.insert(
                "link_context_revision".to_owned(),
                Value::String(digest.clone()),
            );
        }
    }
    let args = llm::SuggestMarkdownSummaryArgs {
        vault_context,
        mode: "daily-consolidation".to_owned(),
        task: Some(
            "Create a concise, evidence-backed daily engineering record. Preserve distinct outcomes, decisions, trade-offs, validation, blockers, and follow-up."
                .to_owned(),
        ),
    };
    attempt.stage("requesting_model")?;
    let progress_store = state.store.clone();
    let progress_id = attempt.id.clone();
    let progress_retries = attempt.save_retries.clone();
    let progress_deadline = attempt.deadline;
    let generation_result = tokio::select! {
        biased;
        _ = attempt.cancel.changed() => Err("Generation canceled.".to_owned()),
        _ = tokio::time::sleep_until(attempt.deadline) => Err("Generation timed out.".to_owned()),
        result = llm::generate_automated_daily_summary_with_progress(
        state.llm_config.as_ref(),
        args.clone(),
        evidence.events,
        move |completed, total, fallback| generation::save_with_retries(&progress_id, progress_deadline, &progress_retries, || progress_store.update_generation_attempt_progress(&progress_id, completed, total, fallback)).map(|_| ()).map_err(|error| if error.failure_code == Some("database_busy") { "database_busy".to_owned() } else { error.message }),
        ) => result,
    };
    let proposal = match generation_result {
        Ok(proposal) => proposal,
        Err(error) => {
            state
                .store
                .set_daily_generation_status(&profile.id, local_date, "failed")
                .map_err(|store_error| ApiError::internal(store_error.to_string()))?;
            let code = if error == "Generation canceled." {
                "canceled"
            } else if error == "Generation timed out." {
                "timed_out"
            } else if error == "database_busy"
                || error.contains("database is locked")
                || error.contains("database is busy")
            {
                "database_busy"
            } else {
                "model_failed"
            };
            let error = if error == "database_busy" {
                "Database remained busy while recording progress. Retry this draft.".to_owned()
            } else {
                error
            };
            return Err(ApiError::unprocessable(error).with_code(code));
        }
    };
    attempt.stage("saving")?;
    let draft = proposal
        .structured_draft
        .ok_or_else(|| ApiError::internal("daily generator omitted structured content"))?;
    let content = serde_json::to_value(llm::daily_revision_content(draft, manual_entry_ids, &args))
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let origin = if current.is_some() {
        "regenerated"
    } else {
        "generated"
    };
    let revision = if let Some(payload) = desired_context_payload.as_ref() {
        let context_snapshot = attempt.save(|| {
            state
                .store
                .create_context_snapshot(&profile.id, local_date, payload)
        })?;
        attempt.save(|| {
            state.store.create_generated_revision_if_current(
                &profile.id,
                local_date,
                Some(&snapshot.id),
                Some(&context_snapshot.id),
                origin,
                &content,
                current.as_ref().map(|revision| revision.id.as_str()),
            )
        })
    } else {
        attempt.save(|| {
            state.store.create_generated_revision_if_current(
                &profile.id,
                local_date,
                Some(&snapshot.id),
                None,
                origin,
                &content,
                current.as_ref().map(|revision| revision.id.as_str()),
            )
        })
    }?;
    Ok(revision)
}

async fn refocus_decide_daily_evidence(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath((date, event_id)): AxumPath<(String, String)>,
    Json(input): Json<EvidenceDecisionRequest>,
) -> Result<Json<Vec<log_inbox_core::models::SnapshotEvidence>>, ApiError> {
    authorize_refocus(&state, &headers, "review:write", true)?;
    require_cutover_for_daily_mutation(&state)?;
    let snapshot = current_snapshot_for_review(&state, &date, &input.expected_revision_id)?;
    state
        .store
        .decide_snapshot_evidence(
            &snapshot.id,
            &event_id,
            &input.disposition,
            input.related_event_id.as_deref(),
            "owner",
            input.reason.as_deref(),
        )
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let evidence = state
        .store
        .snapshot_evidence(&snapshot.id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(Json(evidence))
}

async fn refocus_decide_daily_evidence_batch(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(date): AxumPath<String>,
    Json(input): Json<EvidenceDecisionBatchRequest>,
) -> Result<Json<Vec<log_inbox_core::models::SnapshotEvidence>>, ApiError> {
    authorize_refocus(&state, &headers, "review:write", true)?;
    require_cutover_for_daily_mutation(&state)?;
    let snapshot = current_snapshot_for_review(&state, &date, &input.expected_revision_id)?;
    let decisions = input
        .decisions
        .into_iter()
        .map(|decision| (decision.event_id, decision.disposition))
        .collect::<Vec<_>>();
    state
        .store
        .decide_snapshot_evidence_batch(&snapshot.id, &decisions, "owner")
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let evidence = state
        .store
        .snapshot_evidence(&snapshot.id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(Json(evidence))
}

async fn refocus_reopen_daily_evidence(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath((date, event_id)): AxumPath<(String, String)>,
    Json(input): Json<ExpectedRevisionRequest>,
) -> Result<Json<Vec<log_inbox_core::models::SnapshotEvidence>>, ApiError> {
    authorize_refocus(&state, &headers, "review:write", true)?;
    require_cutover_for_daily_mutation(&state)?;
    let snapshot = current_snapshot_for_review(&state, &date, &input.expected_revision_id)?;
    state
        .store
        .reopen_snapshot_evidence(&snapshot.id, &event_id)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let evidence = state
        .store
        .snapshot_evidence(&snapshot.id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(Json(evidence))
}

async fn refocus_defer_daily_evidence(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath((date, event_id)): AxumPath<(String, String)>,
    Json(input): Json<ExpectedRevisionRequest>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "review:write", true)?;
    require_cutover_for_daily_mutation(&state)?;
    let local_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("date must use YYYY-MM-DD"))?;
    let profile = active_refocus_workspace(&state)?;
    let deferral = state
        .store
        .defer_daily_evidence(
            &profile.id,
            local_date,
            &input.expected_revision_id,
            &event_id,
        )
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    Ok(Json(json!({ "deferral": deferral })))
}

async fn refocus_reopen_deferred_daily_evidence(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath((date, event_id)): AxumPath<(String, String)>,
    Json(input): Json<ExpectedRevisionRequest>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "review:write", true)?;
    require_cutover_for_daily_mutation(&state)?;
    let local_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("date must use YYYY-MM-DD"))?;
    let profile = active_refocus_workspace(&state)?;
    let deferral = state
        .store
        .reopen_deferred_daily_evidence(
            &profile.id,
            local_date,
            &input.expected_revision_id,
            &event_id,
        )
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    Ok(Json(json!({ "deferral": deferral })))
}

async fn refocus_dismiss_daily(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(date): AxumPath<String>,
    Json(input): Json<ExpectedRevisionRequest>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "review:write", true)?;
    require_cutover_for_daily_mutation(&state)?;
    let local_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("date must use YYYY-MM-DD"))?;
    let profile = active_refocus_workspace(&state)?;
    let dismissal = state
        .store
        .dismiss_daily_revision(&profile.id, local_date, &input.expected_revision_id)
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    Ok(Json(json!({ "dismissal": dismissal })))
}

async fn refocus_reopen_daily(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(date): AxumPath<String>,
    Json(input): Json<ExpectedRevisionRequest>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "review:write", true)?;
    require_cutover_for_daily_mutation(&state)?;
    let local_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("date must use YYYY-MM-DD"))?;
    let profile = active_refocus_workspace(&state)?;
    let dismissal = state
        .store
        .reopen_daily_revision(&profile.id, local_date, &input.expected_revision_id)
        .map_err(|error| ApiError::conflict(error.to_string()))?;
    let revision = state
        .store
        .current_proposal_revision(&profile.id, local_date)
        .map_err(|error| ApiError::internal(error.to_string()))?
        .ok_or_else(|| ApiError::internal("the reopened Daily revision is missing"))?;
    let expired_evidence_count = match revision.snapshot_id.as_deref() {
        Some(snapshot_id) => state
            .store
            .snapshot_evidence(snapshot_id)
            .map_err(|error| ApiError::internal(error.to_string()))?
            .iter()
            .filter(|evidence| !evidence.available)
            .count(),
        None => 0,
    };
    Ok(Json(json!({
        "dismissal": dismissal,
        "expired_evidence_count": expired_evidence_count,
        "evidence_complete": expired_evidence_count == 0,
    })))
}

fn current_snapshot_for_review(
    state: &AppState,
    date: &str,
    expected_revision_id: &str,
) -> Result<log_inbox_core::models::EvidenceSnapshot, ApiError> {
    let local_date = NaiveDate::parse_from_str(date, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("date must use YYYY-MM-DD"))?;
    let profile = active_refocus_workspace(state)?;
    let current = state
        .store
        .current_proposal_revision(&profile.id, local_date)
        .map_err(|error| ApiError::internal(error.to_string()))?
        .ok_or_else(|| ApiError::conflict("generate a candidate before reviewing evidence"))?;
    if current.id != expected_revision_id {
        return Err(ApiError::conflict(
            "the Daily candidate changed; reload and try again",
        ));
    }
    let snapshot_id = current
        .snapshot_id
        .ok_or_else(|| ApiError::conflict("manual-only candidates have no automated evidence"))?;
    state
        .store
        .evidence_snapshot(&snapshot_id)
        .map_err(|error| ApiError::internal(error.to_string()))?
        .ok_or_else(|| ApiError::internal("the current evidence snapshot is missing"))
}

async fn refocus_edit_daily_candidate(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(date): AxumPath<String>,
    Json(input): Json<EditDailyCandidateRequest>,
) -> Result<Json<ProposalRevision>, ApiError> {
    authorize_refocus(&state, &headers, "review:write", true)?;
    require_cutover_for_daily_mutation(&state)?;
    let local_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("date must use YYYY-MM-DD"))?;
    let profile = active_refocus_workspace(&state)?;
    let current = state
        .store
        .current_proposal_revision(&profile.id, local_date)
        .map_err(|error| ApiError::internal(error.to_string()))?
        .ok_or_else(|| ApiError::conflict("generate a candidate before editing it"))?;
    if current.id != input.expected_revision_id {
        return Err(ApiError::conflict(
            "the Daily candidate changed; reload and try again",
        ));
    }
    if current
        .content
        .get("schema_version")
        .and_then(Value::as_u64)
        != Some(input.content.schema_version)
    {
        return Err(ApiError::bad_request(
            "Editing cannot change the draft format.",
        ));
    }
    let content = serde_json::to_value(input.content)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let context_snapshot = state
        .store
        .proposal_context_snapshot(&current.id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let revision = if let Some(context_snapshot) = context_snapshot.as_ref() {
        state
            .store
            .create_proposal_revision_if_current_with_context(
                &profile.id,
                local_date,
                current.snapshot_id.as_deref(),
                &context_snapshot.id,
                "structured_edit",
                &content,
                &input.expected_revision_id,
            )
    } else {
        state.store.create_proposal_revision_if_current(
            &profile.id,
            local_date,
            current.snapshot_id.as_deref(),
            "structured_edit",
            &content,
            &input.expected_revision_id,
        )
    }
    .map_err(|error| {
        if error
            .to_string()
            .contains("current proposal revision changed")
        {
            ApiError::conflict("the Daily candidate changed; reload and try again")
        } else {
            ApiError::bad_request(error.to_string())
        }
    })?;
    Ok(Json(revision))
}

fn effective_daily_window(
    local_date: NaiveDate,
    profile: &log_inbox_core::models::WorkspaceProfile,
    frozen_day: Option<&log_inbox_core::models::DailyDay>,
) -> Result<EffectiveDailyWindow, String> {
    if let Some(day) = frozen_day {
        return Ok(EffectiveDailyWindow {
            timezone: day.timezone.clone(),
            start_utc: day.start_utc,
            end_utc: day.end_utc,
            destination_path: day.destination_path.clone(),
        });
    }
    let resolved = resolve_day(local_date, &profile.timezone).map_err(|error| error.to_string())?;
    let destination_path =
        render_daily_path(&profile.daily_root, &profile.daily_pattern, local_date)
            .map_err(|error| error.to_string())?;
    Ok(EffectiveDailyWindow {
        timezone: resolved.timezone,
        start_utc: resolved.start_utc,
        end_utc: resolved.end_utc,
        destination_path,
    })
}

async fn run_daily_scheduler(state: AppState) {
    let mut last_retention_run = None;
    loop {
        let now = Utc::now();
        if last_retention_run.is_none_or(|last| now - last >= Duration::hours(1)) {
            if let Err(error) = reconcile_retention(&state, now) {
                tracing::warn!(error = %error.message, "Daily retention maintenance failed");
            }
            last_retention_run = Some(now);
        }
        if let Err(error) = reconcile_daily_schedule(&state, now).await {
            tracing::warn!(error = %error.message, "Daily schedule reconciliation failed");
        }
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
    }
}

fn reconcile_retention(state: &AppState, now: DateTime<Utc>) -> Result<(), ApiError> {
    let Some(profile) = state
        .store
        .active_workspace_profile()
        .map_err(|error| ApiError::internal(error.to_string()))?
    else {
        return Ok(());
    };
    let settings = state
        .store
        .daily_automation_settings(&profile.id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    if settings.updated_at == DateTime::<Utc>::UNIX_EPOCH {
        return Ok(());
    }
    let report = state
        .store
        .run_retention_maintenance(&settings, now)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let migration_backups_deleted =
        migration::cleanup_expired_backups(&state.store, now, settings.recovery_retention_days)
            .map_err(|error| ApiError::internal(error.to_string()))?;
    let changed = report.raw_events_deleted
        + report.sessions_deleted
        + report.schedule_runs_deleted
        + report.reopened_dismissals_deleted
        + report.reopened_deferrals_deleted
        + report.stale_revisions_deleted
        + report.orphan_snapshots_deleted
        + report.finalized_recovery_scrubbed
        + report.imported_artifacts_deleted
        + migration_backups_deleted;
    if changed > 0 {
        tracing::info!(
            raw_events = report.raw_events_deleted,
            sessions = report.sessions_deleted,
            schedule_runs = report.schedule_runs_deleted,
            reopened_dismissals = report.reopened_dismissals_deleted,
            reopened_deferrals = report.reopened_deferrals_deleted,
            stale_revisions = report.stale_revisions_deleted,
            orphan_snapshots = report.orphan_snapshots_deleted,
            finalized_recovery = report.finalized_recovery_scrubbed,
            imported_artifacts = report.imported_artifacts_deleted,
            migration_backups = migration_backups_deleted,
            "Daily retention maintenance completed"
        );
    }
    Ok(())
}

async fn reconcile_daily_schedule(state: &AppState, now: DateTime<Utc>) -> Result<(), ApiError> {
    let Ok(guard) = state.daily_generation_lock.clone().try_lock_owned() else {
        return Ok(());
    };
    let (profile, workspace) = match active_refocus_context(state) {
        Ok(context) => context,
        Err(error) if error.status == StatusCode::CONFLICT => return Ok(()),
        Err(error) => return Err(error),
    };
    let settings = state
        .store
        .daily_automation_settings(&profile.id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    if !settings.enabled {
        return Ok(());
    }
    let timezone = profile
        .timezone
        .parse::<chrono_tz::Tz>()
        .map_err(|_| ApiError::internal("saved workspace timezone is invalid"))?;
    let generation_time = NaiveTime::parse_from_str(&settings.generation_time, "%H:%M")
        .map_err(|_| ApiError::internal("saved generation time is invalid"))?;
    let today = now.with_timezone(&timezone).date_naive();

    for days_ago in 1..=i64::from(settings.catch_up_days) {
        let local_date = today
            .checked_sub_signed(Duration::days(days_ago))
            .ok_or_else(|| ApiError::internal("catch-up date is outside the supported range"))?;
        let due_date = local_date
            .succ_opt()
            .ok_or_else(|| ApiError::internal("scheduled date has no following day"))?;
        let due_at = resolve_local_time(due_date, generation_time, &profile.timezone)
            .map_err(|error| ApiError::internal(error.to_string()))?;
        if due_at > now {
            continue;
        }
        let resolved = resolve_day(local_date, &profile.timezone)
            .map_err(|error| ApiError::internal(error.to_string()))?;
        let has_automated_evidence = !state
            .store
            .get_events_between(resolved.start_utc, resolved.end_utc, 1)
            .map_err(|error| ApiError::internal(error.to_string()))?
            .events
            .is_empty();
        let has_manual_entries = if state
            .store
            .daily_day(&profile.id, local_date)
            .map_err(|error| ApiError::internal(error.to_string()))?
            .is_some()
        {
            !state
                .store
                .manual_daily_entries(&profile.id, local_date)
                .map_err(|error| ApiError::internal(error.to_string()))?
                .is_empty()
        } else {
            false
        };
        if !has_automated_evidence && !has_manual_entries {
            continue;
        }

        let destination =
            render_daily_path(&profile.daily_root, &profile.daily_pattern, local_date)
                .map_err(|error| ApiError::internal(error.to_string()))?;
        ensure_refocus_daily_day(state, &profile, &workspace, local_date, &destination)?;
        state
            .store
            .enqueue_daily_schedule_run(
                &profile.id,
                local_date,
                due_at,
                &profile.timezone,
                &settings.updated_at.to_rfc3339(),
            )
            .map_err(|error| ApiError::internal(error.to_string()))?;
        let Some(claim) = state
            .store
            .claim_daily_schedule_run(&profile.id, local_date, now)
            .map_err(|error| ApiError::internal(error.to_string()))?
        else {
            continue;
        };
        let claim_token = claim
            .claim_token
            .as_deref()
            .ok_or_else(|| ApiError::internal("claimed schedule run has no claim token"))?;
        if state
            .store
            .current_proposal_revision(&profile.id, local_date)
            .map_err(|error| ApiError::internal(error.to_string()))?
            .is_some()
        {
            state
                .store
                .finish_daily_schedule_run(&profile.id, local_date, claim_token, None, Utc::now())
                .map_err(|error| ApiError::internal(error.to_string()))?;
            return Ok(());
        }
        state
            .store
            .set_daily_generation_status(&profile.id, local_date, "queued")
            .map_err(|error| ApiError::internal(error.to_string()))?;
        let (attempt, _) = generation::begin(state, local_date, "scheduled")?;
        match generation::run(state, local_date, false, None, attempt, guard).await {
            Ok(_) => {
                state
                    .store
                    .finish_daily_schedule_run(
                        &profile.id,
                        local_date,
                        claim_token,
                        None,
                        Utc::now(),
                    )
                    .map_err(|error| ApiError::internal(error.to_string()))?;
            }
            Err(error) if error.message == "Generation canceled." => {}
            Err(error)
                if error.status == StatusCode::CONFLICT
                    && error.message.contains("current candidate has edits") =>
            {
                state
                    .store
                    .finish_daily_schedule_run(
                        &profile.id,
                        local_date,
                        claim_token,
                        None,
                        Utc::now(),
                    )
                    .map_err(|error| ApiError::internal(error.to_string()))?;
            }
            Err(error) => {
                state
                    .store
                    .finish_daily_schedule_run(
                        &profile.id,
                        local_date,
                        claim_token,
                        Some(&bounded_schedule_error(&error.message)),
                        Utc::now(),
                    )
                    .map_err(|store_error| ApiError::internal(store_error.to_string()))?;
            }
        }
        // Process at most one claimed day per reconciliation. This keeps the UI and
        // model workload aligned around one terminal result before another day starts.
        return Ok(());
    }
    Ok(())
}

fn bounded_schedule_error(message: &str) -> String {
    if message.contains("LLM daily draft did not match the structured schema") {
        return "The local model returned an invalid Daily draft. Nothing was written; retry generation. If it repeats, check the configured model or disable automatic preparation until the model is corrected.".to_owned();
    }
    let sanitized = message
        .chars()
        .map(|character| {
            if character.is_control() && !matches!(character, '\n' | '\t') {
                ' '
            } else {
                character
            }
        })
        .take(512)
        .collect::<String>();
    if sanitized.trim().is_empty() {
        "Daily generation failed".to_owned()
    } else {
        sanitized
    }
}

fn authorize_refocus(
    state: &AppState,
    headers: &HeaderMap,
    scope: &str,
    require_csrf: bool,
) -> Result<log_inbox_core::models::DashboardSession, ApiError> {
    validate_request_boundary(&state.refocus, headers, require_csrf)?;
    let token = session_cookie(headers)
        .ok_or_else(|| ApiError::unauthorized("dashboard session cookie is missing"))?;
    let csrf = require_csrf
        .then(|| {
            headers
                .get("x-csrf-token")
                .and_then(|value| value.to_str().ok())
        })
        .flatten();
    if require_csrf && csrf.is_none() {
        return Err(ApiError::forbidden("CSRF token is required"));
    }
    state
        .store
        // The absolute expiry and cookie lifetime still enforce the selected session policy;
        // this refresh window must cover remembered sessions after an idle period.
        .authenticate_dashboard_session(token, csrf, scope, Utc::now(), Duration::days(30))
        .map_err(|_| ApiError::unauthorized("dashboard session is not authorized"))
}

fn validate_request_boundary(
    config: &RefocusConfig,
    headers: &HeaderMap,
    require_origin: bool,
) -> Result<(), ApiError> {
    let host = headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| ApiError::forbidden("Host header is required"))?;
    if !config.allowed_hosts.contains(host) {
        return Err(ApiError::forbidden("request Host is not allowed"));
    }
    if require_origin {
        let origin = headers
            .get(header::ORIGIN)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| ApiError::forbidden("Origin header is required"))?;
        if !config.allowed_origins.contains(origin) {
            return Err(ApiError::forbidden("request Origin is not allowed"));
        }
    }
    Ok(())
}

fn session_cookie(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .map(str::trim)
        .find_map(|cookie| cookie.strip_prefix("log_inbox_session="))
}

fn request_uses_https(headers: &HeaderMap) -> bool {
    headers
        .get(header::ORIGIN)
        .and_then(|value| value.to_str().ok())
        .is_some_and(|origin| origin.starts_with("https://"))
}

async fn dashboard_page() -> Html<&'static str> {
    Html(include_str!("../assets/daily.html"))
}

async fn dashboard_asset(AxumPath(name): AxumPath<String>) -> Response {
    let (content_type, body) = match name.as_str() {
        "daily.js" => ("text/javascript", include_str!("../assets/daily.js")),
        "daily-helpers.js" => (
            "text/javascript",
            include_str!("../assets/daily-helpers.js"),
        ),
        "daily-references.js" => (
            "text/javascript",
            include_str!("../assets/daily-references.js"),
        ),
        "daily-settings.js" => (
            "text/javascript",
            include_str!("../assets/daily-settings.js"),
        ),
        "daily-navigation.js" => (
            "text/javascript",
            include_str!("../assets/daily-navigation.js"),
        ),
        "daily-history.js" => (
            "text/javascript; charset=utf-8",
            include_str!("../assets/daily-history.js"),
        ),
        "daily.css" => ("text/css", include_str!("../assets/daily.css")),
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    (
        [
            (header::CONTENT_TYPE, content_type),
            (header::CACHE_CONTROL, "no-cache"),
        ],
        body,
    )
        .into_response()
}

async fn favicon() -> impl IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "image/x-icon"),
            (header::CACHE_CONTROL, "public, max-age=86400"),
        ],
        include_bytes!("../assets/favicon.ico").as_slice(),
    )
}

#[derive(Debug)]
struct ApiError {
    status: StatusCode,
    message: String,
    failure_code: Option<&'static str>,
}

impl ApiError {
    fn with_code(mut self, code: &'static str) -> Self {
        self.failure_code = Some(code);
        self
    }

    fn invalid_references() -> Self {
        Self::unprocessable("Reference notes could not be prepared. Retry without references; your notes and evidence are preserved.").with_code("reference_context_invalid")
    }
    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            failure_code: None,
            message: message.into(),
        }
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            failure_code: None,
            message: message.into(),
        }
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            failure_code: None,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        let message = message.into();
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            failure_code: if message.contains("database is locked")
                || message.contains("database is busy")
            {
                Some("database_busy")
            } else {
                None
            },
            message,
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            failure_code: None,
            message: message.into(),
        }
    }

    fn unprocessable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            failure_code: None,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            failure_code: None,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        if let Some(code) = self.failure_code {
            return (self.status, Json(json!({"error":self.message,"code":code}))).into_response();
        }
        if let Some(message) = self.message.strip_prefix("generation_busy: ") {
            return (
                self.status,
                Json(json!({"error": message, "code": "generation_busy"})),
            )
                .into_response();
        }
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

#[cfg(test)]
mod knowledge_destination_tests {
    #[test]
    fn knowledge_review_errors_are_reduced_to_safe_recovery_codes() {
        assert_eq!(
            super::public_knowledge_resolution_error_code(
                "Mapped reference note Products/Alpha.md is not available in the selected reference collections."
            ),
            "mapping_outside_collections"
        );
        assert_eq!(
            super::public_knowledge_resolution_error_code(
                "Knowledge catalog exceeds 2000 unique notes"
            ),
            "catalog_limit"
        );
        assert_eq!(
            super::public_knowledge_resolution_error_code("unexpected resolver failure"),
            "resolver_error"
        );
    }

    #[tokio::test]
    async fn dashboard_assets_are_embedded_and_explicitly_allowlisted() {
        for (name, content_type) in [
            ("daily.js", "text/javascript"),
            ("daily-navigation.js", "text/javascript"),
            ("daily-helpers.js", "text/javascript"),
            ("daily-references.js", "text/javascript"),
            ("daily-settings.js", "text/javascript"),
            ("daily.css", "text/css"),
        ] {
            let response = super::dashboard_asset(axum::extract::Path(name.to_owned())).await;
            assert_eq!(response.status(), axum::http::StatusCode::OK);
            assert_eq!(
                response.headers()[axum::http::header::CONTENT_TYPE],
                content_type
            );
            assert_eq!(
                response.headers()[axum::http::header::CACHE_CONTROL],
                "no-cache"
            );
            assert!(
                !axum::body::to_bytes(response.into_body(), usize::MAX)
                    .await
                    .unwrap()
                    .is_empty()
            );
        }
        let response =
            super::dashboard_asset(axum::extract::Path("../Cargo.toml".to_owned())).await;
        assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
    }
    use super::*;
    use tower::ServiceExt;

    #[test]
    fn only_live_ids_absent_from_the_snapshot_are_new_evidence() {
        let snapshot = vec!["expired".to_owned(), "still-live".to_owned()];
        assert!(!has_new_automated_evidence(&["still-live"], &snapshot));
        assert!(has_new_automated_evidence(
            &["still-live", "arrived-late"],
            &snapshot
        ));
    }

    #[test]
    fn overview_marks_only_unhandled_automated_past_days_as_missed() {
        let today = NaiveDate::from_ymd_opt(2026, 9, 10).unwrap();
        let yesterday = NaiveDate::from_ymd_opt(2026, 9, 9).unwrap();
        assert_eq!(
            daily_overview_status(yesterday, today, None, None, None, None, true, false, false),
            "missed"
        );
        assert_eq!(
            daily_overview_status(yesterday, today, None, None, None, None, false, true, false),
            "notes_unreviewed"
        );
        assert_eq!(
            daily_overview_status(today, today, None, None, None, None, false, false, false),
            "not_started"
        );
    }

    #[test]
    fn daily_context_projection_never_exposes_catalog_or_mapping_metadata() {
        let snapshot = log_inbox_core::models::ContextSnapshot {
            id: "context_1".to_owned(),
            workspace_id: "workspace_1".to_owned(),
            local_date: NaiveDate::from_ymd_opt(2026, 9, 9).unwrap(),
            snapshot_digest: "a".repeat(64),
            payload: json!({
                "schema_version": 1,
                "resolver_version": "exact-v1",
                "root_binding": "private-root-binding",
                "catalog_note_count": 42,
                "used_notes": [{"path": "Private/Alpha.md", "title": "Alpha"}],
                "resolved_groups": [{"canonical_note_path": "Private/Alpha.md"}],
                "mappings": [{"selectors": [{"field": "repo", "value": "secret"}]}],
                "workstream_links": {},
                "diagnostics": {"missing_roots": ["Private/Missing"]}
            }),
            created_at: Utc::now(),
        };
        let projected = public_context_snapshot(&snapshot);
        let text = projected.to_string();
        assert_eq!(projected["used_note_count"], 1);
        assert_eq!(projected["resolved_group_count"], 1);
        assert_eq!(projected["diagnostics"]["missing_root_count"], 1);
        for private in [
            "Private/Alpha.md",
            "Private/Missing",
            "private-root-binding",
            "secret",
        ] {
            assert!(!text.contains(private));
        }
    }

    #[test]
    fn open_context_comparison_projection_keeps_randomized_assignment_blind() {
        let state = test_state();
        let content = DailyRevisionContent {
            schema_version: 1,
            workstreams: Vec::new(),
            manual_entry_ids: Vec::new(),
            open_questions: Vec::new(),
        };
        let comparison = ContextComparison {
            id: "comparison_1".to_owned(),
            schema_version: 1,
            workspace_id: "workspace_1".to_owned(),
            local_date: NaiveDate::from_ymd_opt(2026, 9, 9).unwrap(),
            source_revision_id: "revision_1".to_owned(),
            snapshot_id: "snapshot_1".to_owned(),
            context_snapshot_id: "context_1".to_owned(),
            arm_a_content: content.clone(),
            arm_b_content: content,
            arm_a_kind: None,
            model_fingerprint: "a".repeat(64),
            contract_fingerprint: "b".repeat(64),
            state: "open".to_owned(),
            decision: None,
            created_at: Utc::now(),
        };
        let projection = public_context_comparison(&state, &comparison).expect("projection builds");
        assert!(projection.get("assignment").is_none());
        assert!(!projection.to_string().contains("with_context"));
        assert!(!projection.to_string().contains("without_context"));
    }

    fn test_state() -> AppState {
        let store = Store::open(
            std::env::temp_dir().join(format!("log-inbox-router-{}.sqlite3", uuid::Uuid::new_v4())),
        )
        .expect("test store opens");
        let workspace_root = std::env::temp_dir().join(format!(
            "log-inbox-router-workspace-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&workspace_root).expect("test workspace creates");
        AppState {
            store,
            llm_config: None,
            legacy_proposal_dir: None,
            legacy_support_files: Vec::new(),
            apply_lock: Arc::new(Mutex::new(())),
            knowledge_write_lock: Arc::new(Mutex::new(())),
            daily_generation_lock: Arc::new(tokio::sync::Mutex::new(())),
            generation_cancel: Arc::new(Mutex::new(None)),
            refocus: RefocusConfig {
                allowed_hosts: HashSet::from(["localhost:8788".to_owned()]),
                allowed_origins: HashSet::from(["http://localhost:8788".to_owned()]),
            },
            workspace: InspectedWorkspace::inspect(&workspace_root).expect("workspace inspects"),
        }
    }

    fn generation_test_state() -> (AppState, WorkspaceProfile, NaiveDate) {
        let state = test_state();
        let profile = state
            .store
            .save_active_workspace_profile(
                state.workspace.root_binding(),
                "UTC",
                "Work Log",
                "{date}.md",
                None,
                "markdown",
                None,
            )
            .unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 9, 9).unwrap();
        state
            .store
            .ensure_daily_day(date, "Work Log/2026-09-09.md", None)
            .unwrap();
        state
            .store
            .insert_event(log_inbox_core::models::LogEventInput {
                timestamp: Some("2026-09-09T10:00:00Z".parse().unwrap()),
                source: "codex/test".to_owned(),
                level: None,
                message: "Validated the generation lifecycle.".to_owned(),
                metadata: Some(serde_json::from_value(json!({"repo":"alpha"})).unwrap()),
                fingerprint: None,
            })
            .unwrap();
        (state, profile, date)
    }

    #[tokio::test]
    async fn history_and_dashboard_preferences_require_auth_and_scope_mutations() {
        let (state, profile, date) = generation_test_state();
        let credentials = generate_session_credentials();
        state
            .store
            .create_dashboard_session(
                &credentials,
                &["logs:read".into(), "settings:write".into()],
                Utc::now(),
                Duration::hours(1),
                Duration::hours(1),
            )
            .unwrap();
        let cookie = format!("log_inbox_session={}", credentials.session_token);
        let app = build_router(state.clone());
        let search = "/api/v2/history/search?q=Validated";
        assert_eq!(
            json_response(app.clone(), "GET", search, json!({}), None, None)
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let result =
            json_response(app.clone(), "GET", search, json!({}), Some(&cookie), None).await;
        assert_eq!(result.status(), StatusCode::OK);
        let result = response_json(result).await;
        assert_eq!(result["matches"][0]["local_date"], date.to_string());
        assert_eq!(result["matches"][0]["kind"], "activity");
        let id = result["matches"][0]["id"].as_str().unwrap();
        let target = format!("/api/v2/daily/{date}/activity/{id}");
        assert_eq!(
            json_response(app.clone(), "GET", &target, json!({}), Some(&cookie), None)
                .await
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            json_response(
                app.clone(),
                "GET",
                "/api/v2/history/search?q=",
                json!({}),
                Some(&cookie),
                None
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        let uri = "/api/v2/settings/dashboard";
        let defaults = response_json(
            json_response(app.clone(), "GET", uri, json!({}), Some(&cookie), None).await,
        )
        .await;
        assert_eq!(defaults["recent_days"], 10);
        assert_eq!(
            json_response(
                app.clone(),
                "PUT",
                uri,
                json!({"recent_days":30}),
                Some(&cookie),
                None
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            json_response(
                app.clone(),
                "PUT",
                uri,
                json!({"recent_days":9}),
                Some(&cookie),
                Some(&credentials.csrf_token)
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            json_response(
                app.clone(),
                "PUT",
                uri,
                json!({"recent_days":30}),
                Some(&cookie),
                Some(&credentials.csrf_token)
            )
            .await
            .status(),
            StatusCode::OK
        );
        assert_eq!(state.store.recent_days_preference(&profile.id).unwrap(), 30);
        let overview = response_json(
            json_response(
                app.clone(),
                "GET",
                "/api/v2/daily/overview",
                json!({}),
                Some(&cookie),
                None,
            )
            .await,
        )
        .await;
        assert_eq!(overview["recent_days"], 30);
        let overview = response_json(
            json_response(
                app.clone(),
                "GET",
                "/api/v2/daily/overview?limit=7",
                json!({}),
                Some(&cookie),
                None,
            )
            .await,
        )
        .await;
        assert_eq!(overview["recent_days"], 7);
        let readonly = generate_session_credentials();
        state
            .store
            .create_dashboard_session(
                &readonly,
                &["logs:read".into()],
                Utc::now(),
                Duration::hours(1),
                Duration::hours(1),
            )
            .unwrap();
        let readonly_cookie = format!("log_inbox_session={}", readonly.session_token);
        assert_eq!(
            json_response(
                app.clone(),
                "GET",
                search,
                json!({}),
                Some(&readonly_cookie),
                None
            )
            .await
            .status(),
            StatusCode::OK
        );
        assert_eq!(
            json_response(
                app,
                "PUT",
                uri,
                json!({"recent_days":7}),
                Some(&readonly_cookie),
                Some(&readonly.csrf_token)
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
    }

    #[tokio::test]
    async fn activity_record_works_without_model_and_uses_reviewed_apply() {
        let (state, profile, date) = generation_test_state();
        let manual = state
            .store
            .create_manual_daily_entry(&profile.id, date, "My own words.", &[])
            .unwrap();
        let credentials = generate_session_credentials();
        state
            .store
            .create_dashboard_session(
                &credentials,
                &[
                    "draft:generate".into(),
                    "logs:read".into(),
                    "review:write".into(),
                    "vault:write".into(),
                ],
                Utc::now(),
                Duration::hours(1),
                Duration::hours(1),
            )
            .unwrap();
        let cookie = format!("log_inbox_session={}", credentials.session_token);
        let app = build_router(state.clone());
        let uri = format!("/api/v2/daily/{date}/activity-record");
        let no_csrf =
            json_response(app.clone(), "POST", &uri, json!({}), Some(&cookie), None).await;
        assert_ne!(no_csrf.status(), StatusCode::CREATED);
        let guard = state.daily_generation_lock.lock().await;
        let busy = json_response(
            app.clone(),
            "POST",
            &uri,
            json!({}),
            Some(&cookie),
            Some(&credentials.csrf_token),
        )
        .await;
        assert_eq!(busy.status(), StatusCode::CONFLICT);
        drop(guard);
        let created = json_response(
            app.clone(),
            "POST",
            &uri,
            json!({}),
            Some(&cookie),
            Some(&credentials.csrf_token),
        )
        .await;
        let status = created.status();
        let revision = response_json(created).await;
        assert_eq!(status, StatusCode::CREATED, "{revision}");
        assert_eq!(revision["content"]["schema_version"], 2);
        assert_eq!(revision["content"]["manual_entry_ids"], json!([manual.id]));
        assert_eq!(
            revision["content"]["workstreams"][0]["activity"][0]["text"],
            "Validated the generation lifecycle."
        );
        assert!(!state.workspace.canonical_root().join("Work Log").exists());
        assert!(
            state
                .store
                .latest_generation_attempt(&profile.id, date)
                .unwrap()
                .is_none()
        );
        let duplicate = json_response(
            app.clone(),
            "POST",
            &uri,
            json!({}),
            Some(&cookie),
            Some(&credentials.csrf_token),
        )
        .await;
        assert_eq!(duplicate.status(), StatusCode::CONFLICT);
        let preview = json_response(
            app.clone(),
            "GET",
            &format!("/api/v2/daily/{date}/apply-preview"),
            json!({}),
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(preview.status(), StatusCode::OK);
        let preview = response_json(preview).await;
        assert!(
            preview["next_block"]
                .as_str()
                .unwrap()
                .contains("Activity record · not AI-summarized")
        );
        let approval = json!({"expected_revision_id":preview["revision_id"],"expected_revision_content_hash":preview["revision_content_hash"],"destination_path":preview["destination_path"],"expected_old_block_hash":preview["expected_old_block_hash"],"intended_new_block_hash":preview["intended_new_block_hash"],"expected_target_exists":preview["expected_target_exists"],"expected_original_content_hash":preview["expected_original_content_hash"],"expected_updated_content_hash":preview["updated_content_hash"]});
        let applied = json_response(
            app.clone(),
            "POST",
            &format!("/api/v2/daily/{date}/apply"),
            approval.clone(),
            Some(&cookie),
            Some(&credentials.csrf_token),
        )
        .await;
        assert_eq!(applied.status(), StatusCode::OK);
        let written = std::fs::read_to_string(
            state
                .workspace
                .canonical_root()
                .join(preview["destination_path"].as_str().unwrap()),
        )
        .unwrap();
        assert!(written.contains("My own words."));
        assert!(written.contains("Validated the generation lifecycle."));
        let reapplied = json_response(
            app,
            "POST",
            &format!("/api/v2/daily/{date}/apply"),
            approval,
            Some(&cookie),
            Some(&credentials.csrf_token),
        )
        .await;
        assert_eq!(response_json(reapplied).await["idempotent"], true);
    }

    #[test]
    fn activity_record_preserves_groups_and_rejects_inferred_fields() {
        let (state, profile, date) = generation_test_state();
        let mut events = state.store.all_events().unwrap();
        events[0].metadata.insert("task_id".into(), json!("task-a"));
        let mut earlier = events[0].clone();
        earlier.id = "earlier".into();
        earlier.timestamp -= Duration::minutes(1);
        earlier.message = "Earlier decision must remain.".into();
        events.push(earlier);
        let content = llm::activity_record(&events, vec![]);
        assert_eq!(content.workstreams.len(), 1);
        assert_eq!(
            content.workstreams[0].activity[0].text,
            "Earlier decision must remain."
        );
        assert_eq!(content.workstreams[0].activity.len(), 2);
        events[1]
            .metadata
            .insert("repo".into(), json!("another-repo"));
        assert_eq!(llm::activity_record(&events, vec![]).workstreams.len(), 2);
        let revision = create_activity_record(&state, date).unwrap();
        let snapshot = revision.snapshot_id.as_deref().unwrap();
        let mut invalid = revision.content.clone();
        invalid["workstreams"][0]["outcome"] = invalid["workstreams"][0]["activity"].clone();
        assert!(
            state
                .store
                .create_proposal_revision_if_current(
                    &profile.id,
                    date,
                    Some(snapshot),
                    "structured_edit",
                    &invalid,
                    &revision.id
                )
                .is_err()
        );
        let mut missing = revision.content.clone();
        missing["workstreams"][0]["activity"] = json!([]);
        assert!(
            state
                .store
                .create_proposal_revision_if_current(
                    &profile.id,
                    date,
                    Some(snapshot),
                    "structured_edit",
                    &missing,
                    &revision.id
                )
                .is_err()
        );
        let id = state.store.snapshot_evidence(snapshot).unwrap()[0]
            .event_id
            .clone();
        state
            .store
            .decide_snapshot_evidence(snapshot, &id, "omit", None, "owner", None)
            .unwrap();
        assert!(
            !daily_apply_material(&state, date)
                .unwrap()
                .plan
                .next_block
                .contains("Validated the generation lifecycle.")
        );
    }

    #[test]
    fn activity_record_refuses_expired_evidence_and_over_limit_days() {
        let (state, profile, date) = generation_test_state();
        let ids = state
            .store
            .all_events()
            .unwrap()
            .into_iter()
            .map(|event| event.id)
            .collect::<Vec<_>>();
        state
            .store
            .create_evidence_snapshot(&profile.id, date, &ids)
            .unwrap();
        let settings = state
            .store
            .save_daily_automation_settings(&profile.id, false, "00:15", 7, 1, 30, 30, None)
            .unwrap();
        state
            .store
            .run_retention_maintenance(&settings, Utc::now() + Duration::days(2))
            .unwrap();
        assert!(
            create_activity_record(&state, date)
                .unwrap_err()
                .message
                .contains("expired")
        );
        let (state, profile, date) = generation_test_state();
        for index in 0..500 {
            state
                .store
                .insert_event(log_inbox_core::models::LogEventInput {
                    timestamp: Some("2026-09-09T10:00:00Z".parse().unwrap()),
                    source: "codex/test".into(),
                    level: None,
                    message: format!("Activity {index}"),
                    metadata: None,
                    fingerprint: None,
                })
                .unwrap();
        }
        assert!(
            create_activity_record(&state, date)
                .unwrap_err()
                .message
                .contains("500-event")
        );
        assert!(
            state
                .store
                .current_proposal_revision(&profile.id, date)
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn generation_cancellation_and_timeout_are_terminal_without_a_revision() {
        for canceled in [true, false] {
            let (state, profile, date) = generation_test_state();
            let guard = state
                .daily_generation_lock
                .clone()
                .try_lock_owned()
                .unwrap();
            let (mut attempt, _) = generation::begin(&state, date, "manual").unwrap();
            if canceled {
                state
                    .generation_cancel
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .1
                    .send_replace(true);
            } else {
                attempt.deadline = tokio::time::Instant::now();
            }
            assert!(
                generation::run(&state, date, false, None, attempt, guard)
                    .await
                    .is_err()
            );
            let saved = state
                .store
                .latest_generation_attempt(&profile.id, date)
                .unwrap()
                .unwrap();
            assert_eq!(
                saved["state"],
                if canceled { "canceled" } else { "timed_out" }
            );
            assert!(
                state
                    .store
                    .current_proposal_revision(&profile.id, date)
                    .unwrap()
                    .is_none()
            );
            assert!(state.store.active_generation_attempt().unwrap().is_none());
            assert!(state.daily_generation_lock.try_lock().is_ok());
        }
    }

    #[test]
    fn apply_preview_includes_untouched_evidence_and_honors_omissions() {
        let (state, profile, date) = generation_test_state();
        state
            .store
            .freeze_daily_template(&profile.id, date, None)
            .unwrap();
        let ids = state
            .store
            .all_events()
            .unwrap()
            .into_iter()
            .map(|e| e.id)
            .collect::<Vec<_>>();
        let snapshot = state
            .store
            .create_evidence_snapshot(&profile.id, date, &ids)
            .unwrap();
        state.store.create_proposal_revision(&profile.id,date,Some(&snapshot.id),"generated",&json!({
            "schema_version":1,"manual_entry_ids":[],"open_questions":[],
            "workstreams":[{"id":"work","title":"Work","evidence_event_ids":ids,
                "outcome":[{"text":"Validated the generation lifecycle.","evidence_event_ids":ids}]}]
        })).unwrap();
        let included = daily_apply_material(&state, date).unwrap();
        assert!(
            included
                .plan
                .next_block
                .contains("Validated the generation lifecycle.")
        );
        assert!(
            state.store.snapshot_evidence(&snapshot.id).unwrap()[0]
                .disposition
                .is_none()
        );
        state
            .store
            .decide_snapshot_evidence(&snapshot.id, &ids[0], "omit", None, "owner", None)
            .unwrap();
        let omitted = daily_apply_material(&state, date).unwrap();
        assert!(
            !omitted
                .plan
                .next_block
                .contains("Validated the generation lifecycle.")
        );
    }

    #[tokio::test]
    async fn generation_gate_skips_scheduler_without_creating_attempts() {
        let (state, _, date) = generation_test_state();
        let _guard = state.daily_generation_lock.lock().await;
        assert_eq!(
            generate_daily_candidate(&state, date, false, None)
                .await
                .unwrap_err()
                .status,
            StatusCode::CONFLICT
        );
        reconcile_daily_schedule(&state, Utc::now()).await.unwrap();
        assert!(state.store.active_generation_attempt().unwrap().is_none());
    }

    #[test]
    fn dropped_generation_marks_day_interrupted_and_saving_rejects_pending_cancel() {
        let (state, profile, date) = generation_test_state();
        let (attempt, _) = generation::begin(&state, date, "manual").unwrap();
        state
            .store
            .set_daily_generation_status(&profile.id, date, "running")
            .unwrap();
        state
            .generation_cancel
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .1
            .send_replace(true);
        assert!(attempt.stage("saving").is_err());
        drop(attempt);
        assert_eq!(
            state
                .store
                .latest_generation_attempt(&profile.id, date)
                .unwrap()
                .unwrap()["state"],
            "interrupted"
        );
        assert_eq!(
            state
                .store
                .daily_day(&profile.id, date)
                .unwrap()
                .unwrap()
                .generation_status,
            "failed"
        );
    }

    #[test]
    fn malformed_reference_snapshot_is_rejected_before_model_work_and_exposes_recovery() {
        let (state, profile, date) = generation_test_state();
        let (attempt, _) = generation::begin(&state, date, "manual").unwrap();
        let result = preflight_reference_context(
            &json!({"schema_version":1,"workstream_links":{"repo:alpha":["not a canonical wikilink"]}}),
        );
        assert!(result.is_err());
        assert_eq!(attempt.finish(&result).unwrap(), "failed");
        let saved = state
            .store
            .latest_generation_attempt(&profile.id, date)
            .unwrap()
            .unwrap();
        assert_eq!(saved["failure_code"], "reference_context_invalid");
        assert_eq!(
            saved["recovery_actions"],
            json!(["retry_without_references"])
        );
        assert_eq!(saved["completed_groups"], 0);
        assert!(
            state
                .store
                .current_proposal_revision(&profile.id, date)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn generation_database_retry_budget_is_shared_and_stops_on_deadline_or_nonbusy_errors() {
        let busy = || {
            anyhow::Error::from(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_BUSY),
                None,
            ))
        };
        let budget = Mutex::new(0);
        assert_eq!(
            generation::database_error(busy()).failure_code,
            Some("database_busy")
        );
        assert_eq!(
            generation::database_error(anyhow::anyhow!("template changed")).failure_code,
            None
        );
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(60);
        let mut calls = 0;
        let value = generation::save_with_retries("test", deadline, &budget, || {
            calls += 1;
            if calls < 3 { Err(busy()) } else { Ok(42) }
        })
        .unwrap();
        assert_eq!(value, 42);
        assert_eq!(calls, 3);
        let mut exhausted_calls = 0;
        let error = generation::save_with_retries::<()>("test", deadline, &budget, || {
            exhausted_calls += 1;
            Err(busy())
        })
        .unwrap_err();
        assert_eq!(error.failure_code, Some("database_busy"));
        assert_eq!(exhausted_calls, 1);
        let mut expired_calls = 0;
        assert!(
            generation::save_with_retries::<()>(
                "test",
                tokio::time::Instant::now(),
                &Mutex::new(0),
                || {
                    expired_calls += 1;
                    Err(busy())
                }
            )
            .is_err()
        );
        assert_eq!(expired_calls, 1);
        let mut invalid_calls = 0;
        assert!(
            generation::save_with_retries::<()>("test", deadline, &Mutex::new(0), || {
                invalid_calls += 1;
                anyhow::bail!("invalid snapshot")
            })
            .is_err()
        );
        assert_eq!(invalid_calls, 1);
    }

    #[tokio::test]
    async fn cancel_endpoint_requires_csrf_and_refuses_cancellation_after_saving_starts() {
        let (state, _, date) = generation_test_state();
        let credentials = generate_session_credentials();
        state
            .store
            .create_dashboard_session(
                &credentials,
                &["draft:generate".to_owned(), "logs:read".to_owned()],
                Utc::now(),
                Duration::hours(1),
                Duration::hours(1),
            )
            .unwrap();
        let cookie = format!("log_inbox_session={}", credentials.session_token);
        let app = build_router(state.clone());
        for saving in [false, true] {
            let (attempt, value) = generation::begin(&state, date, "manual").unwrap();
            if saving {
                attempt.stage("saving").unwrap();
            }
            let uri = format!(
                "/api/v2/daily/{date}/generation/{}/cancel",
                value["id"].as_str().unwrap()
            );
            let unauthorized =
                json_response(app.clone(), "POST", &uri, json!({}), Some(&cookie), None).await;
            assert_ne!(unauthorized.status(), StatusCode::OK);
            let response = json_response(
                app.clone(),
                "POST",
                &uri,
                json!({}),
                Some(&cookie),
                Some(&credentials.csrf_token),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response_json(response).await["cancel_requested"], !saving);
            assert_eq!(*attempt.cancel.borrow(), !saving);
            drop(attempt);
        }
    }

    #[tokio::test]
    async fn pending_model_cancellation_timeout_and_concurrent_edit_preserve_existing_revision() {
        for scenario in ["cancel", "timeout", "edit", "without_references"] {
            let (mut state, profile, date) = generation_test_state();
            let captured = Arc::new(Mutex::new(None::<Value>));
            let collection = state
                .store
                .save_knowledge_collection(
                    None,
                    &profile.id,
                    "Product notes",
                    "Reference context",
                    &["notes".to_owned()],
                    &[],
                    true,
                    None,
                )
                .unwrap();
            std::fs::create_dir_all(state.workspace.canonical_root().join("notes")).unwrap();
            std::fs::write(
                state.workspace.canonical_root().join("notes/alpha.md"),
                "# Alpha\nReference excerpt must not leak into evidence-only generation.\n",
            )
            .unwrap();
            let entered = Arc::new(tokio::sync::Notify::new());
            let release = Arc::new(tokio::sync::Notify::new());
            let model = Router::new().route("/chat/completions", post({
                let entered = entered.clone();
                let release = release.clone();
                let captured = captured.clone();
                move |Json(body): Json<Value>| {
                    let entered = entered.clone();
                    let release = release.clone();
                    let captured = captured.clone();
                    async move {
                        *captured.lock().unwrap() = Some(body);
                        entered.notify_one();
                        release.notified().await;
                        Json(json!({"choices":[{"message":{"content":json!({"title":"Generation lifecycle", "outcome":["Validated generation lifecycle handling."],"decision":[],"trade_off":[],"validation":[],"blocker":[],"follow_up":[],"open_questions":[]}).to_string()}}]}))
                    }
                }
            }));
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            state.llm_config = Some(llm::LlmConfig::for_test(&format!(
                "http://{}",
                listener.local_addr().unwrap()
            )));
            let server = tokio::spawn(async move {
                axum::serve(listener, model).await.unwrap();
            });
            let note = state
                .store
                .create_manual_daily_entry(
                    &profile.id,
                    date,
                    "Owner note preserved during generation.",
                    &[],
                )
                .unwrap();
            let content = json!({"schema_version":1,"workstreams":[],"manual_entry_ids":[note.id],"open_questions":[]});
            let original = state
                .store
                .create_proposal_revision(&profile.id, date, None, "generated", &content)
                .unwrap();
            let mut preserved = original.id;
            let broken_mapping = if scenario == "without_references" {
                let mapping = state
                    .store
                    .save_context_mapping(&ContextMapping {
                        id: String::new(),
                        workspace_id: profile.id.clone(),
                        selectors: vec![LinkSelector {
                            field: "repo".to_owned(),
                            operator: "exact".to_owned(),
                            value: "alpha".to_owned(),
                        }],
                        canonical_note_path: "outside/missing.md".to_owned(),
                        enabled: true,
                        source_identity: None,
                        source_digest: None,
                        created_at: DateTime::<Utc>::UNIX_EPOCH,
                        updated_at: DateTime::<Utc>::UNIX_EPOCH,
                    })
                    .unwrap();
                let guard = state
                    .daily_generation_lock
                    .clone()
                    .try_lock_owned()
                    .unwrap();
                let (attempt, _) = generation::begin(&state, date, "manual").unwrap();
                let error = generation::run(&state, date, false, None, attempt, guard)
                    .await
                    .unwrap_err();
                assert_eq!(error.failure_code, Some("reference_context_invalid"));
                assert!(
                    captured.lock().unwrap().is_none(),
                    "bad reference config must fail before a model call"
                );
                assert_eq!(
                    state
                        .store
                        .current_proposal_revision(&profile.id, date)
                        .unwrap()
                        .unwrap()
                        .id,
                    preserved
                );
                Some(mapping)
            } else {
                None
            };
            let credentials = generate_session_credentials();
            state
                .store
                .create_dashboard_session(
                    &credentials,
                    &["draft:generate".to_owned(), "logs:read".to_owned()],
                    Utc::now(),
                    Duration::hours(1),
                    Duration::hours(1),
                )
                .unwrap();
            let guard = state
                .daily_generation_lock
                .clone()
                .try_lock_owned()
                .unwrap();
            let mode = if scenario == "without_references" {
                generation::ReferenceMode::None
            } else {
                generation::ReferenceMode::Configured
            };
            let (mut attempt, value) =
                generation::begin_with_references(&state, date, "manual", mode).unwrap();
            if scenario == "timeout" {
                // Start the timeout only after proving the model request is pending.
                // Paused Tokio time makes this independent of parallel SQLite/test load.
                attempt.deadline =
                    tokio::time::Instant::now() + std::time::Duration::from_secs(3600);
            }
            let worker_state = state.clone();
            let worker = tokio::spawn(async move {
                generation::run(&worker_state, date, false, None, attempt, guard).await
            });
            tokio::time::timeout(std::time::Duration::from_secs(15), entered.notified())
                .await
                .expect("model receives request");
            if scenario == "timeout" {
                tokio::time::pause();
                tokio::time::advance(std::time::Duration::from_secs(3601)).await;
            }
            if scenario == "cancel" {
                let app = build_router(state.clone());
                let uri = format!(
                    "/api/v2/daily/{date}/generation/{}/cancel",
                    value["id"].as_str().unwrap()
                );
                let cookie = format!("log_inbox_session={}", credentials.session_token);
                let response = json_response(
                    app,
                    "POST",
                    &uri,
                    json!({}),
                    Some(&cookie),
                    Some(&credentials.csrf_token),
                )
                .await;
                assert_eq!(response.status(), StatusCode::OK);
                assert_eq!(response_json(response).await["cancel_requested"], true);
            } else if scenario == "edit" {
                preserved = state
                    .store
                    .create_proposal_revision_if_current(
                        &profile.id,
                        date,
                        None,
                        "structured_edit",
                        &content,
                        &preserved,
                    )
                    .unwrap()
                    .id;
                release.notify_one();
            } else if scenario == "without_references" {
                let body = captured.lock().unwrap().clone().unwrap().to_string();
                assert!(!body.contains("Reference excerpt must not leak"));
                assert!(!body.contains("[[notes/alpha]]"));
                release.notify_one();
            }
            let result = tokio::time::timeout(std::time::Duration::from_secs(3), worker)
                .await
                .unwrap()
                .unwrap();
            if scenario == "without_references" {
                let revision = result.unwrap();
                let snapshot = state
                    .store
                    .proposal_context_snapshot(&revision.id)
                    .unwrap()
                    .unwrap();
                assert_eq!(snapshot.payload["reference_mode"], "none");
                let content: DailyRevisionContent =
                    serde_json::from_value(revision.content).unwrap();
                assert!(
                    content
                        .workstreams
                        .iter()
                        .all(|stream| stream.canonical_links.is_empty())
                );
                assert_eq!(
                    state
                        .store
                        .knowledge_collection(&collection.id)
                        .unwrap()
                        .unwrap(),
                    collection
                );
                assert_eq!(
                    state.store.list_context_mappings(&profile.id).unwrap(),
                    broken_mapping.into_iter().collect::<Vec<_>>()
                );
                assert_eq!(
                    state
                        .store
                        .latest_generation_attempt(&profile.id, date)
                        .unwrap()
                        .unwrap()["completed_groups"],
                    1
                );
                server.abort();
                continue;
            }
            assert!(result.is_err(), "{scenario}");
            let saved = state
                .store
                .latest_generation_attempt(&profile.id, date)
                .unwrap()
                .unwrap();
            assert_eq!(
                saved["state"],
                match scenario {
                    "cancel" => "canceled",
                    "timeout" => "timed_out",
                    _ => "failed",
                }
            );
            assert_eq!(
                state
                    .store
                    .current_proposal_revision(&profile.id, date)
                    .unwrap()
                    .unwrap()
                    .id,
                preserved
            );
            if scenario == "edit" {
                assert!(result.unwrap_err().message.contains("changed"));
            }
            assert!(state.daily_generation_lock.try_lock().is_ok());
            server.abort();
            if scenario == "timeout" {
                tokio::time::resume();
            }
        }
    }

    async fn route_status(app: Router, method: &str, uri: &str, body: &str) -> StatusCode {
        app.oneshot(
            Request::builder()
                .method(method)
                .uri(uri)
                .header(header::HOST, "localhost:8788")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_owned()))
                .expect("request builds"),
        )
        .await
        .expect("router responds")
        .status()
    }

    async fn json_response(
        app: Router,
        method: &str,
        uri: &str,
        body: Value,
        cookie: Option<&str>,
        csrf: Option<&str>,
    ) -> Response {
        let mut request = Request::builder()
            .method(method)
            .uri(uri)
            .header(header::HOST, "localhost:8788")
            .header(header::ORIGIN, "http://localhost:8788")
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(cookie) = cookie {
            request = request.header(header::COOKIE, cookie);
        }
        if let Some(csrf) = csrf {
            request = request.header("X-CSRF-Token", csrf);
        }
        app.oneshot(
            request
                .body(Body::from(body.to_string()))
                .expect("request builds"),
        )
        .await
        .expect("router responds")
    }

    async fn response_json(response: Response) -> Value {
        let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
            .await
            .expect("response body reads");
        serde_json::from_slice(&bytes).expect("response is JSON")
    }

    async fn wait_for_generation(app: &Router, cookie: &str, date: &str) {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let response = json_response(
                    app.clone(),
                    "GET",
                    &format!("/api/v2/daily/{date}"),
                    json!({}),
                    Some(cookie),
                    None,
                )
                .await;
                let day = response_json(response).await;
                if day["generation_attempt"]["state"] != "running" {
                    assert_eq!(day["generation_attempt"]["state"], "succeeded", "{day}");
                    return;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("generation completes promptly");
    }

    #[tokio::test]
    async fn legacy_routes_are_not_exposed() {
        let refocused = build_router(test_state());
        assert_eq!(
            route_status(refocused.clone(), "GET", "/api/dashboard", "").await,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            route_status(refocused.clone(), "POST", "/mcp", "{}").await,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            route_status(refocused.clone(), "GET", "/api/v2/auth/session", "").await,
            StatusCode::UNAUTHORIZED
        );
        let refocused = build_router(test_state());
        assert_eq!(
            route_status(refocused.clone(), "GET", "/api/v2/migration/cutover", "").await,
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            route_status(refocused.clone(), "GET", "/api/vault/connection", "").await,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            route_status(refocused, "GET", "/api/knowledge", "").await,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn login_can_issue_a_thirty_day_device_session() {
        let state = test_state();
        state
            .store
            .set_owner_secret_hash(&hash_owner_secret("owner-secret-for-tests").unwrap())
            .unwrap();
        let app = build_router(state);
        let response = json_response(
            app.clone(),
            "POST",
            "/api/v2/auth/login",
            json!({"owner_secret": "owner-secret-for-tests", "remember_me": true}),
            None,
            None,
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(
            response
                .headers()
                .get(header::SET_COOKIE)
                .unwrap()
                .to_str()
                .unwrap()
                .contains("Max-Age=2592000")
        );
        let cookie = response
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let login_body = response_json(response).await;
        assert_eq!(login_body["expires_in_seconds"], 2_592_000);
        let restored = json_response(
            app,
            "GET",
            "/api/v2/auth/session",
            Value::Null,
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(restored.status(), StatusCode::OK);
        let restored_body = response_json(restored).await;
        assert_eq!(restored_body["authenticated"], true);
        assert_ne!(restored_body["csrf_token"], login_body["csrf_token"]);
        assert!(
            restored_body["csrf_token"]
                .as_str()
                .unwrap()
                .starts_with("csrf_")
        );
    }

    #[tokio::test]
    async fn workspace_settings_require_csrf_and_preserve_the_active_profile_id() {
        let state = test_state();
        state
            .store
            .set_owner_secret_hash(&hash_owner_secret("owner-secret-for-tests").unwrap())
            .unwrap();
        let app = build_router(state);
        let login = json_response(
            app.clone(),
            "POST",
            "/api/v2/auth/login",
            json!({"owner_secret": "owner-secret-for-tests"}),
            None,
            None,
        )
        .await;
        assert_eq!(login.status(), StatusCode::OK);
        let cookie = login
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let csrf = response_json(login).await["csrf_token"]
            .as_str()
            .unwrap()
            .to_owned();
        let settings = json!({
            "timezone": "Europe/Stockholm",
            "daily_root": "Work Log",
            "daily_pattern": "{year}/{month_name}/Daily {date}.md",
            "template_path": null,
            "link_style": "markdown"
        });

        let rejected = json_response(
            app.clone(),
            "POST",
            "/api/v2/settings/workspace/preview",
            settings.clone(),
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(rejected.status(), StatusCode::FORBIDDEN);

        let preview = json_response(
            app.clone(),
            "POST",
            "/api/v2/settings/workspace/preview",
            settings,
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(preview.status(), StatusCode::OK);
        let preview = response_json(preview).await;
        let saved = json_response(
            app.clone(),
            "PUT",
            "/api/v2/settings/workspace",
            json!({
                "settings": preview["settings"],
                "preview_digest": preview["preview_digest"],
                "expected_profile_id": null,
                "expected_updated_at": null
            }),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(saved.status(), StatusCode::OK);
        let first = response_json(saved).await;
        let profile_id = first["active_profile"]["id"].clone();

        let second_preview = json_response(
            app.clone(),
            "POST",
            "/api/v2/settings/workspace/preview",
            json!({
                "timezone": "UTC",
                "daily_root": "Journal",
                "daily_pattern": "{date}.md",
                "template_path": null,
                "link_style": "wikilink"
            }),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        let second_preview = response_json(second_preview).await;
        let settings_read = json_response(
            app.clone(),
            "GET",
            "/api/v2/settings/workspace",
            json!({}),
            Some(&cookie),
            None,
        )
        .await;
        let settings_read = response_json(settings_read).await;
        let updated = json_response(
            app.clone(),
            "PUT",
            "/api/v2/settings/workspace",
            json!({
                "settings": second_preview["settings"],
                "preview_digest": second_preview["preview_digest"],
                "expected_profile_id": settings_read["active_profile"]["id"],
                "expected_updated_at": settings_read["active_profile"]["updated_at"]
            }),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(updated.status(), StatusCode::OK);
        assert_eq!(
            response_json(updated).await["active_profile"]["id"],
            profile_id
        );
    }

    #[tokio::test]
    async fn knowledge_collections_require_reviewed_scope_and_optimistic_changes() {
        let state = test_state();
        std::fs::create_dir_all(state.workspace.canonical_root().join("Products/Archive")).unwrap();
        std::fs::write(
            state
                .workspace
                .canonical_root()
                .join("Products/Overview.md"),
            "# Product\nprivate body",
        )
        .unwrap();
        std::fs::write(
            state
                .workspace
                .canonical_root()
                .join("Products/Archive/Old.md"),
            "old",
        )
        .unwrap();
        state
            .store
            .set_owner_secret_hash(&hash_owner_secret("owner-secret-for-tests").unwrap())
            .unwrap();
        state
            .store
            .save_active_workspace_profile(
                state.workspace.root_binding(),
                "UTC",
                "Work Log",
                "{date}.md",
                None,
                "markdown",
                None,
            )
            .unwrap();
        let limited = generate_session_credentials();
        state
            .store
            .create_dashboard_session(
                &limited,
                &["knowledge:read".to_owned()],
                Utc::now(),
                Duration::minutes(30),
                Duration::hours(8),
            )
            .unwrap();
        let app = build_router(state);
        assert_eq!(
            route_status(app.clone(), "GET", "/api/v2/knowledge/collections", "").await,
            StatusCode::UNAUTHORIZED
        );
        let login = json_response(
            app.clone(),
            "POST",
            "/api/v2/auth/login",
            json!({"owner_secret": "owner-secret-for-tests"}),
            None,
            None,
        )
        .await;
        let cookie = login.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let csrf = response_json(login).await["csrf_token"]
            .as_str()
            .unwrap()
            .to_owned();
        let draft = json!({
            "label": " Product context ",
            "purpose": " Product behavior and decisions ",
            "roots": ["Products"],
            "exclusions": ["Products/Archive"],
            "enabled": true
        });

        let limited_cookie = format!("log_inbox_session={}", limited.session_token);
        let limited_preview = json_response(
            app.clone(),
            "POST",
            "/api/v2/knowledge/collections/preview",
            draft.clone(),
            Some(&limited_cookie),
            Some(&limited.csrf_token),
        )
        .await;
        assert_eq!(limited_preview.status(), StatusCode::OK);
        let limited_preview = response_json(limited_preview).await;
        let limited_write = json_response(
            app.clone(),
            "POST",
            "/api/v2/knowledge/collections",
            json!({
                "collection": limited_preview["collection"],
                "preview_digest": limited_preview["preview_digest"],
                "expected_updated_at": null
            }),
            Some(&limited_cookie),
            Some(&limited.csrf_token),
        )
        .await;
        assert_eq!(limited_write.status(), StatusCode::UNAUTHORIZED);

        let missing_csrf = json_response(
            app.clone(),
            "POST",
            "/api/v2/knowledge/collections/preview",
            draft.clone(),
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(missing_csrf.status(), StatusCode::FORBIDDEN);
        let preview = json_response(
            app.clone(),
            "POST",
            "/api/v2/knowledge/collections/preview",
            draft,
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(preview.status(), StatusCode::OK);
        let preview = response_json(preview).await;
        assert_eq!(preview["collection"]["label"], "Product context");
        assert_eq!(preview["matched_note_count"], 1);
        assert_eq!(preview["eligible_note_count"], 1);
        assert_eq!(preview["missing_roots"], json!([]));
        assert_eq!(preview["changes_saved"], false);
        assert!(!preview.to_string().contains("private body"));

        let wrong_digest = json_response(
            app.clone(),
            "POST",
            "/api/v2/knowledge/collections",
            json!({
                "collection": preview["collection"],
                "preview_digest": "wrong",
                "expected_updated_at": null
            }),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(wrong_digest.status(), StatusCode::CONFLICT);
        let created = json_response(
            app.clone(),
            "POST",
            "/api/v2/knowledge/collections",
            json!({
                "collection": preview["collection"],
                "preview_digest": preview["preview_digest"],
                "expected_updated_at": null
            }),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(created.status(), StatusCode::CREATED);
        let created = response_json(created).await;
        let id = created["collection"]["id"].as_str().unwrap();
        let created_at = created["collection"]["updated_at"].clone();

        let listed = json_response(
            app.clone(),
            "GET",
            "/api/v2/knowledge/collections",
            json!({}),
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(listed.status(), StatusCode::OK);
        assert_eq!(response_json(listed).await["count"], 1);
        let folders = json_response(
            app.clone(),
            "GET",
            "/api/v2/knowledge/folders?query=prod&limit=5",
            json!({}),
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(folders.status(), StatusCode::OK);
        assert_eq!(
            response_json(folders).await["folders"],
            json!(["Products", "Products/Archive"])
        );

        let changed_draft = json!({
            "label": "Product context",
            "purpose": "Product behavior and decisions",
            "roots": ["Products"],
            "exclusions": ["Products/Archive"],
            "enabled": false
        });
        let changed_preview = json_response(
            app.clone(),
            "POST",
            "/api/v2/knowledge/collections/preview",
            changed_draft,
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        let changed_preview = response_json(changed_preview).await;
        let stale = json_response(
            app.clone(),
            "PUT",
            &format!("/api/v2/knowledge/collections/{id}"),
            json!({
                "collection": changed_preview["collection"],
                "preview_digest": changed_preview["preview_digest"],
                "expected_updated_at": DateTime::<Utc>::UNIX_EPOCH
            }),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(stale.status(), StatusCode::CONFLICT);
        let updated = json_response(
            app.clone(),
            "PUT",
            &format!("/api/v2/knowledge/collections/{id}"),
            json!({
                "collection": changed_preview["collection"],
                "preview_digest": changed_preview["preview_digest"],
                "expected_updated_at": created_at
            }),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(updated.status(), StatusCode::OK);
        let updated_at = response_json(updated).await["collection"]["updated_at"].clone();

        let stale_delete = json_response(
            app.clone(),
            "DELETE",
            &format!("/api/v2/knowledge/collections/{id}"),
            json!({"expected_updated_at": DateTime::<Utc>::UNIX_EPOCH}),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(stale_delete.status(), StatusCode::CONFLICT);
        let deleted = json_response(
            app.clone(),
            "DELETE",
            &format!("/api/v2/knowledge/collections/{id}"),
            json!({"expected_updated_at": updated_at}),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(deleted.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn knowledge_review_links_and_ignores_curated_names_without_browsing_files() {
        let mut state = test_state();
        state.llm_config = Some(llm::LlmConfig::for_test("http://127.0.0.1:11434/v1"));
        std::fs::create_dir_all(state.workspace.canonical_root().join("Products")).unwrap();
        std::fs::write(
            state.workspace.canonical_root().join("Products/Alpha.md"),
            "# Alpha\nprivate product details",
        )
        .unwrap();
        state
            .store
            .set_owner_secret_hash(&hash_owner_secret("owner-secret-for-tests").unwrap())
            .unwrap();
        let profile = state
            .store
            .save_active_workspace_profile(
                state.workspace.root_binding(),
                "UTC",
                "Work Log",
                "{date}.md",
                None,
                "markdown",
                None,
            )
            .unwrap();
        state
            .store
            .save_knowledge_collection(
                None,
                &profile.id,
                "Products",
                "Canonical product notes",
                &["Products".to_owned()],
                &[],
                true,
                None,
            )
            .unwrap();
        let event = state
            .store
            .insert_event(log_inbox_core::models::LogEventInput {
                timestamp: Some("2026-09-09T12:00:00Z".parse().unwrap()),
                source: "codex/private-host".to_owned(),
                level: Some("info".to_owned()),
                message: "private event body".to_owned(),
                metadata: Some(serde_json::Map::from_iter([
                    ("product".to_owned(), json!("Unmapped Product")),
                    ("branch".to_owned(), json!("private-branch")),
                ])),
                fingerprint: None,
            })
            .unwrap();
        let limited = generate_session_credentials();
        state
            .store
            .create_dashboard_session(
                &limited,
                &["knowledge:read".to_owned()],
                Utc::now(),
                Duration::minutes(30),
                Duration::hours(8),
            )
            .unwrap();
        let app = build_router(state.clone());
        let limited_cookie = format!("log_inbox_session={}", limited.session_token);
        let limited_review = json_response(
            app.clone(),
            "GET",
            "/api/v2/knowledge/review",
            json!({}),
            Some(&limited_cookie),
            None,
        )
        .await;
        assert_eq!(limited_review.status(), StatusCode::UNAUTHORIZED);
        let limited_context = json_response(
            app.clone(),
            "GET",
            "/api/v2/daily/2026-09-09/context",
            json!({}),
            Some(&limited_cookie),
            None,
        )
        .await;
        assert_eq!(limited_context.status(), StatusCode::UNAUTHORIZED);

        let login = json_response(
            app.clone(),
            "POST",
            "/api/v2/auth/login",
            json!({"owner_secret": "owner-secret-for-tests"}),
            None,
            None,
        )
        .await;
        let cookie = login.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let csrf = response_json(login).await["csrf_token"]
            .as_str()
            .unwrap()
            .to_owned();
        let notes = json_response(
            app.clone(),
            "GET",
            "/api/v2/knowledge/notes?query=alpha",
            json!({}),
            Some(&cookie),
            None,
        )
        .await;
        let notes = response_json(notes).await;
        assert_eq!(notes["notes"][0]["path"], "Products/Alpha.md");
        assert!(!notes.to_string().contains("private product details"));

        let review = json_response(
            app.clone(),
            "GET",
            "/api/v2/knowledge/review",
            json!({}),
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(review.status(), StatusCode::OK);
        let review = response_json(review).await;
        assert_eq!(review["review_status"], "ready");
        assert_eq!(review["unresolved"]["identities"][0]["field"], "product");
        assert_eq!(
            review["unresolved"]["identities"][0]["value"],
            "Unmapped Product"
        );
        let review_text = review.to_string();
        assert!(!review_text.contains("private event body"));
        assert!(!review_text.contains("private-branch"));
        assert!(!review_text.contains("codex/private-host"));

        let outside = json_response(
            app.clone(),
            "POST",
            "/api/v2/knowledge/mappings",
            json!({
                "mapping": {"field": "product", "value": "Unmapped Product", "canonical_note_path": "Elsewhere.md"},
                "expected_updated_at": null
            }),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(outside.status(), StatusCode::CONFLICT);
        let created = json_response(
            app.clone(),
            "POST",
            "/api/v2/knowledge/mappings",
            json!({
                "mapping": {"field": "product", "value": "Unmapped Product", "canonical_note_path": "Products/Alpha.md"},
                "expected_updated_at": null
            }),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(created.status(), StatusCode::CREATED);
        let created = response_json(created).await;
        let mapping_id = created["mapping"]["id"].as_str().unwrap();
        let mapping_updated_at = created["mapping"]["updated_at"].clone();
        let duplicate = json_response(
            app.clone(),
            "POST",
            "/api/v2/knowledge/mappings",
            json!({
                "mapping": {"field": "product", "value": "unmapped product", "canonical_note_path": "Products/Alpha.md"},
                "expected_updated_at": null
            }),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(duplicate.status(), StatusCode::CONFLICT);
        let linked_review = json_response(
            app.clone(),
            "GET",
            "/api/v2/knowledge/review",
            json!({}),
            Some(&cookie),
            None,
        )
        .await;
        let linked_review = response_json(linked_review).await;
        assert_eq!(linked_review["mappings"].as_array().unwrap().len(), 1);
        assert!(
            linked_review["unresolved"]["identities"]
                .as_array()
                .unwrap()
                .is_empty()
        );

        let date = NaiveDate::from_ymd_opt(2026, 9, 9).unwrap();
        state
            .store
            .ensure_daily_day(date, "Work Log/2026-09-09.md", None)
            .unwrap();
        let evidence_snapshot = state
            .store
            .create_evidence_snapshot(&profile.id, date, std::slice::from_ref(&event.id))
            .unwrap();
        let collections = state.store.list_knowledge_collections(&profile.id).unwrap();
        let mappings = state.store.list_context_mappings(&profile.id).unwrap();
        let resolution = knowledge::resolve_knowledge_with_excerpts(
            &state.workspace,
            &collections,
            &mappings,
            std::slice::from_ref(&event),
            true,
        )
        .unwrap()
        .unwrap();
        let context_snapshot = state
            .store
            .create_context_snapshot(&profile.id, date, &resolution.snapshot_payload)
            .unwrap();
        let workstream_id = resolution.snapshot_payload["resolved_groups"][0]["canonical_group_id"]
            .as_str()
            .unwrap();
        state
            .store
            .create_proposal_revision_with_context(
                &profile.id,
                date,
                Some(&evidence_snapshot.id),
                &context_snapshot.id,
                "generated",
                &json!({
                    "schema_version": 1,
                    "workstreams": [{
                        "id": workstream_id,
                        "title": "Alpha work",
                        "evidence_event_ids": [event.id],
                        "canonical_links": ["[[Products/Alpha]]"],
                        "outcome": [{"text": "Completed Alpha work.", "evidence_event_ids": [event.id]}],
                        "decision": [], "trade_off": [], "validation": [], "blocker": [], "follow_up": []
                    }],
                    "manual_entry_ids": [],
                    "open_questions": []
                }),
            )
            .unwrap();
        state
            .store
            .decide_snapshot_evidence(
                &evidence_snapshot.id,
                &event.id,
                "include",
                None,
                "owner",
                None,
            )
            .unwrap();
        let context = json_response(
            app.clone(),
            "GET",
            "/api/v2/daily/2026-09-09/context",
            json!({}),
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(context.status(), StatusCode::OK);
        let context = response_json(context).await;
        assert_eq!(context["status"], "current");
        assert_eq!(
            context["workstreams"][0]["notes"][0]["path"],
            "Products/Alpha.md"
        );
        assert_eq!(context["workstreams"][0]["notes"][0]["attached"], true);
        let context_text = context.to_string();
        assert!(context_text.contains("private product details"));
        assert!(!context_text.contains("private event body"));

        std::fs::write(
            state.workspace.canonical_root().join("Products/Alpha.md"),
            "# Alpha renamed\nprivate product details",
        )
        .unwrap();
        let changed_context = json_response(
            app.clone(),
            "GET",
            "/api/v2/daily/2026-09-09/context",
            json!({}),
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(response_json(changed_context).await["status"], "changed");
        std::fs::remove_file(state.workspace.canonical_root().join("Products/Alpha.md")).unwrap();
        let invalid_context = json_response(
            app.clone(),
            "GET",
            "/api/v2/daily/2026-09-09/context",
            json!({}),
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(response_json(invalid_context).await["status"], "invalid");
        let stale_apply = json_response(
            app.clone(),
            "GET",
            "/api/v2/daily/2026-09-09/apply-preview",
            json!({}),
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(stale_apply.status(), StatusCode::CONFLICT);
        assert!(
            response_json(stale_apply).await["error"]
                .as_str()
                .unwrap()
                .contains("removed or excluded")
        );

        let stale = json_response(
            app.clone(),
            "PUT",
            &format!("/api/v2/knowledge/mappings/{mapping_id}"),
            json!({
                "mapping": {"field": "product", "value": "Unmapped Product", "canonical_note_path": "Products/Alpha.md", "enabled": false},
                "expected_updated_at": DateTime::<Utc>::UNIX_EPOCH
            }),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(stale.status(), StatusCode::CONFLICT);
        let deleted = json_response(
            app.clone(),
            "DELETE",
            &format!("/api/v2/knowledge/mappings/{mapping_id}"),
            json!({"expected_updated_at": mapping_updated_at}),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(deleted.status(), StatusCode::NO_CONTENT);

        let ignored = json_response(
            app.clone(),
            "POST",
            "/api/v2/knowledge/ignored",
            json!({"field": "product", "value": "Unmapped Product"}),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(ignored.status(), StatusCode::CREATED);
        let ignored = response_json(ignored).await;
        let ignored_id = ignored["ignored"]["id"].as_str().unwrap();
        let ignored_review = json_response(
            app.clone(),
            "GET",
            "/api/v2/knowledge/review",
            json!({}),
            Some(&cookie),
            None,
        )
        .await;
        let ignored_review = response_json(ignored_review).await;
        assert_eq!(ignored_review["ignored"].as_array().unwrap().len(), 1);
        assert!(
            ignored_review["unresolved"]["identities"]
                .as_array()
                .unwrap()
                .is_empty()
        );
        let reopened = json_response(
            app.clone(),
            "DELETE",
            &format!("/api/v2/knowledge/ignored/{ignored_id}"),
            json!({}),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(reopened.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn automation_settings_are_explicit_and_optimistically_saved() {
        let state = test_state();
        state
            .store
            .set_owner_secret_hash(&hash_owner_secret("owner-secret-for-tests").unwrap())
            .unwrap();
        let profile = state
            .store
            .save_active_workspace_profile(
                state.workspace.root_binding(),
                "Europe/Stockholm",
                "Work Log",
                "{date}.md",
                None,
                "markdown",
                None,
            )
            .unwrap();
        let app = build_router(state);
        let login = json_response(
            app.clone(),
            "POST",
            "/api/v2/auth/login",
            json!({"owner_secret": "owner-secret-for-tests"}),
            None,
            None,
        )
        .await;
        let cookie = login.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let csrf = response_json(login).await["csrf_token"]
            .as_str()
            .unwrap()
            .to_owned();

        let defaults = json_response(
            app.clone(),
            "GET",
            "/api/v2/settings/automation",
            json!({}),
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(defaults.status(), StatusCode::OK);
        let defaults = response_json(defaults).await;
        assert_eq!(defaults["saved"], false);
        assert_eq!(defaults["settings"]["enabled"], false);
        assert_eq!(defaults["settings"]["generation_time"], "00:15");
        assert_eq!(defaults["writes_markdown_automatically"], false);

        let save = json_response(
            app.clone(),
            "PUT",
            "/api/v2/settings/automation",
            json!({
                "enabled": true,
                "generation_time": "01:30",
                "catch_up_days": 14,
                "raw_retention_days": 30,
                "audit_retention_days": 45,
                "recovery_retention_days": 60,
                "expected_updated_at": null
            }),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(save.status(), StatusCode::OK);
        let saved = response_json(save).await;
        assert_eq!(saved["settings"]["workspace_id"], profile.id);
        assert_eq!(saved["settings"]["generation_time"], "01:30");
        assert_eq!(saved["writes_markdown_automatically"], false);

        let stale = json_response(
            app,
            "PUT",
            "/api/v2/settings/automation",
            json!({
                "enabled": false,
                "generation_time": "00:15",
                "catch_up_days": 7,
                "raw_retention_days": 30,
                "audit_retention_days": 30,
                "recovery_retention_days": 30,
                "expected_updated_at": null
            }),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(stale.status(), StatusCode::CONFLICT);
    }

    #[tokio::test]
    async fn scheduler_prepares_only_nonempty_due_days_without_writing_markdown() {
        let state = test_state();
        let profile = state
            .store
            .save_active_workspace_profile(
                state.workspace.root_binding(),
                "Europe/Stockholm",
                "Work Log",
                "{date}.md",
                None,
                "markdown",
                None,
            )
            .unwrap();
        state
            .store
            .save_daily_automation_settings(&profile.id, true, "00:15", 7, 30, 30, 30, None)
            .unwrap();
        state
            .store
            .insert_event(log_inbox_core::models::LogEventInput {
                timestamp: Some("2026-09-08T10:00:00Z".parse().unwrap()),
                source: "codex/test".to_owned(),
                level: Some("info".to_owned()),
                message: "Completed the scheduler test.".to_owned(),
                metadata: Some(serde_json::Map::from_iter([(
                    "task_id".to_owned(),
                    Value::from("scheduler-test"),
                )])),
                fingerprint: None,
            })
            .unwrap();

        reconcile_daily_schedule(&state, "2026-09-09T10:00:00Z".parse().unwrap())
            .await
            .unwrap();

        let runs = state.store.daily_schedule_runs(&profile.id, 14).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].local_date.to_string(), "2026-09-08");
        assert_eq!(runs[0].state, "failed");
        assert!(
            runs[0]
                .last_error
                .as_deref()
                .unwrap()
                .contains("requires a configured LLM")
        );
        assert_eq!(
            state
                .store
                .daily_day(&profile.id, NaiveDate::from_ymd_opt(2026, 9, 8).unwrap())
                .unwrap()
                .unwrap()
                .generation_status,
            "failed"
        );
        assert!(
            state
                .store
                .daily_day(&profile.id, NaiveDate::from_ymd_opt(2026, 9, 7).unwrap())
                .unwrap()
                .is_none()
        );
        assert!(!state.workspace.canonical_root().join("Work Log").exists());
    }

    #[tokio::test]
    async fn regeneration_refuses_to_replace_expired_evidence_with_a_partial_day() {
        let state = test_state();
        let profile = state
            .store
            .save_active_workspace_profile(
                state.workspace.root_binding(),
                "UTC",
                "Work Log",
                "{date}.md",
                None,
                "markdown",
                None,
            )
            .unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 9, 9).unwrap();
        state
            .store
            .ensure_daily_day(date, "Work Log/2026-09-09.md", None)
            .unwrap();
        let original = state
            .store
            .insert_event(log_inbox_core::models::LogEventInput {
                timestamp: Some("2026-09-09T10:00:00Z".parse().unwrap()),
                source: "codex/test".to_owned(),
                level: Some("info".to_owned()),
                message: "Recorded the original decision.".to_owned(),
                metadata: None,
                fingerprint: None,
            })
            .unwrap();
        let snapshot = state
            .store
            .create_evidence_snapshot(&profile.id, date, std::slice::from_ref(&original.id))
            .unwrap();
        let original_revision = state
            .store
            .create_proposal_revision(
                &profile.id,
                date,
                Some(&snapshot.id),
                "generated",
                &json!({
                    "schema_version": 1,
                    "manual_entry_ids": [],
                    "open_questions": [],
                    "workstreams": [{
                        "id": "source:codex%2Ftest|task:retention",
                        "title": "Retention safety",
                        "evidence_event_ids": [original.id.clone()],
                        "canonical_links": [],
                        "outcome": [{"text": "Preserved the original decision.", "evidence_event_ids": [original.id.clone()]}],
                        "decision": [], "trade_off": [], "validation": [], "blocker": [], "follow_up": []
                    }]
                }),
            )
            .unwrap();
        let retention = state
            .store
            .save_daily_automation_settings(&profile.id, false, "00:15", 7, 1, 30, 30, None)
            .unwrap();
        state
            .store
            .run_retention_maintenance(&retention, Utc::now() + Duration::days(2))
            .unwrap();
        assert!(
            state
                .store
                .snapshot_evidence(&snapshot.id)
                .unwrap()
                .iter()
                .all(|item| !item.available)
        );
        let unchanged = generate_daily_candidate(&state, date, false, None)
            .await
            .expect("an unchanged expired candidate remains usable");
        assert_eq!(unchanged.id, original_revision.id);
        let manual = state
            .store
            .create_manual_daily_entry(
                &profile.id,
                date,
                "Added owner-authored context after expiry.",
                &[],
            )
            .unwrap();
        let manual_revision = generate_daily_candidate(&state, date, false, None)
            .await
            .expect("manual notes can be attached without partial regeneration");
        assert_eq!(manual_revision.snapshot_id, Some(snapshot.id.clone()));
        assert_eq!(manual_revision.origin, "structured_edit");
        assert!(
            serde_json::from_value::<DailyRevisionContent>(manual_revision.content.clone())
                .unwrap()
                .manual_entry_ids
                .contains(&manual.id)
        );
        let late = state
            .store
            .insert_event(log_inbox_core::models::LogEventInput {
                timestamp: Some("2026-09-09T11:00:00Z".parse().unwrap()),
                source: "codex/test".to_owned(),
                level: Some("info".to_owned()),
                message: "Arrived after the original evidence expired.".to_owned(),
                metadata: None,
                fingerprint: None,
            })
            .unwrap();

        let error = generate_daily_candidate(&state, date, true, None)
            .await
            .expect_err("partial regeneration must be refused");
        assert_eq!(error.status, StatusCode::CONFLICT);
        assert!(error.message.contains("expired"));
        assert_eq!(
            state
                .store
                .current_proposal_revision(&profile.id, date)
                .unwrap()
                .unwrap()
                .id,
            manual_revision.id
        );
        state
            .store
            .defer_daily_evidence(&profile.id, date, &manual_revision.id, &late.id)
            .expect("owner can leave the exact late event for later");
        let preserved = generate_daily_candidate(&state, date, false, None)
            .await
            .expect("deferred late evidence no longer blocks the preserved revision");
        assert_eq!(preserved.id, manual_revision.id);
    }

    #[tokio::test]
    async fn apply_preview_is_exact_read_only_and_rejects_stale_manual_evidence() {
        let state = test_state();
        state
            .store
            .set_owner_secret_hash(&hash_owner_secret("owner-secret-for-tests").unwrap())
            .unwrap();
        let workspace_root = state.workspace.canonical_root().to_owned();
        let app = build_router(state);
        let login = json_response(
            app.clone(),
            "POST",
            "/api/v2/auth/login",
            json!({"owner_secret": "owner-secret-for-tests"}),
            None,
            None,
        )
        .await;
        let cookie = login
            .headers()
            .get(header::SET_COOKIE)
            .unwrap()
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        let csrf = response_json(login).await["csrf_token"]
            .as_str()
            .unwrap()
            .to_owned();
        let settings = json!({
            "timezone": "UTC",
            "daily_root": "Work Log",
            "daily_pattern": "{year}/{month_name}/Daily {date}.md",
            "template_path": null,
            "link_style": "markdown"
        });
        let preview = json_response(
            app.clone(),
            "POST",
            "/api/v2/settings/workspace/preview",
            settings,
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        let preview = response_json(preview).await;
        let saved = json_response(
            app.clone(),
            "PUT",
            "/api/v2/settings/workspace",
            json!({
                "settings": preview["settings"],
                "preview_digest": preview["preview_digest"],
                "expected_profile_id": null,
                "expected_updated_at": null
            }),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(saved.status(), StatusCode::OK);

        let manual = json_response(
            app.clone(),
            "POST",
            "/api/v2/daily/2026-09-09/manual",
            json!({"text": "Reviewed the safe Apply boundary.", "references": []}),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(manual.status(), StatusCode::CREATED);
        let generated = json_response(
            app.clone(),
            "POST",
            "/api/v2/daily/2026-09-09/generate",
            json!({}),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(generated.status(), StatusCode::ACCEPTED);
        wait_for_generation(&app, &cookie, "2026-09-09").await;

        let apply_preview = json_response(
            app.clone(),
            "GET",
            "/api/v2/daily/2026-09-09/apply-preview",
            json!({}),
            Some(&cookie),
            None,
        )
        .await;
        let apply_preview_status = apply_preview.status();
        let apply_preview = response_json(apply_preview).await;
        assert_eq!(apply_preview_status, StatusCode::OK, "{apply_preview}");
        assert_eq!(apply_preview["will_create_note"], true);
        assert!(
            apply_preview["next_block"]
                .as_str()
                .unwrap()
                .contains("Reviewed the safe Apply boundary.")
        );
        assert!(!workspace_root.join("Work Log").exists());

        let approval = json!({
            "expected_revision_id": apply_preview["revision_id"],
            "expected_revision_content_hash": apply_preview["revision_content_hash"],
            "destination_path": apply_preview["destination_path"],
            "expected_old_block_hash": apply_preview["expected_old_block_hash"],
            "intended_new_block_hash": apply_preview["intended_new_block_hash"],
            "expected_target_exists": apply_preview["expected_target_exists"],
            "expected_original_content_hash": apply_preview["expected_original_content_hash"],
            "expected_updated_content_hash": apply_preview["updated_content_hash"]
        });
        let applied = json_response(
            app.clone(),
            "POST",
            "/api/v2/daily/2026-09-09/apply",
            approval.clone(),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(applied.status(), StatusCode::OK);
        let applied = response_json(applied).await;
        assert_eq!(applied["operation"]["state"], "finalized");
        assert!(applied["operation"].get("recovery_payload").is_none());
        assert!(applied["operation"].get("recovery_path").is_none());
        assert!(applied["operation"].get("temporary_name").is_none());
        let destination = apply_preview["destination_path"].as_str().unwrap();
        let written = std::fs::read_to_string(workspace_root.join(destination)).unwrap();
        assert!(written.contains("Reviewed the safe Apply boundary."));
        let reapplied = json_response(
            app.clone(),
            "POST",
            "/api/v2/daily/2026-09-09/apply",
            approval,
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(reapplied.status(), StatusCode::OK);
        assert_eq!(response_json(reapplied).await["idempotent"], true);

        let newer_manual = json_response(
            app.clone(),
            "POST",
            "/api/v2/daily/2026-09-09/manual",
            json!({"text": "This makes the candidate stale.", "references": []}),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(newer_manual.status(), StatusCode::CREATED);
        let stale = json_response(
            app.clone(),
            "GET",
            "/api/v2/daily/2026-09-09/apply-preview",
            json!({}),
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(stale.status(), StatusCode::CONFLICT);

        let regenerated = json_response(
            app.clone(),
            "POST",
            "/api/v2/daily/2026-09-09/generate",
            json!({}),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(regenerated.status(), StatusCode::ACCEPTED);
        wait_for_generation(&app, &cookie, "2026-09-09").await;
        let newer_preview = json_response(
            app.clone(),
            "GET",
            "/api/v2/daily/2026-09-09/apply-preview",
            json!({}),
            Some(&cookie),
            None,
        )
        .await;
        assert_eq!(newer_preview.status(), StatusCode::OK);
        let newer_preview = response_json(newer_preview).await;
        let target = workspace_root.join(destination);
        let mut externally_edited = std::fs::read(&target).unwrap();
        externally_edited.extend_from_slice(b"\nUser edit after preview.\n");
        std::fs::write(&target, &externally_edited).unwrap();
        let rejected_apply = json_response(
            app.clone(),
            "POST",
            "/api/v2/daily/2026-09-09/apply",
            json!({
                "expected_revision_id": newer_preview["revision_id"],
                "expected_revision_content_hash": newer_preview["revision_content_hash"],
                "destination_path": newer_preview["destination_path"],
                "expected_old_block_hash": newer_preview["expected_old_block_hash"],
                "intended_new_block_hash": newer_preview["intended_new_block_hash"],
                "expected_target_exists": newer_preview["expected_target_exists"],
                "expected_original_content_hash": newer_preview["expected_original_content_hash"],
                "expected_updated_content_hash": newer_preview["updated_content_hash"]
            }),
            Some(&cookie),
            Some(&csrf),
        )
        .await;
        assert_eq!(rejected_apply.status(), StatusCode::CONFLICT);
        assert_eq!(std::fs::read(&target).unwrap(), externally_edited);
    }

    #[test]
    fn startup_recovery_commits_an_exact_journaled_temporary() {
        let state = test_state();
        let workspace = &state.workspace;
        let profile = state
            .store
            .save_active_workspace_profile(
                workspace.root_binding(),
                "UTC",
                "Work Log",
                "{date}.md",
                None,
                "markdown",
                None,
            )
            .unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 9, 9).unwrap();
        let day = state
            .store
            .ensure_daily_day(date, "Work Log/2026-09-09.md", None)
            .unwrap();
        let revision = state
            .store
            .create_proposal_revision(
                &profile.id,
                date,
                None,
                "advanced_markdown",
                &json!("Reviewed Daily"),
            )
            .unwrap();
        let plan = daily_writer::plan_managed_block(None, &day.block_id, "Reviewed Daily", "Daily")
            .unwrap();
        let operation = state
            .store
            .prepare_apply_operation(&PrepareApplyOperation {
                id: "apply_recovery_test".to_owned(),
                workspace_id: profile.id.clone(),
                local_date: date,
                revision_id: revision.id.clone(),
                revision_content_hash: revision.content_hash.clone(),
                destination_path: day.destination_path.clone(),
                expected_old_block_hash: None,
                intended_new_block_hash: plan.intended_new_block_hash.clone(),
                expected_target_exists: false,
                expected_original_content_hash: daily_writer::digest(b""),
                intended_updated_content_hash: daily_writer::digest(&plan.updated_content),
                temporary_name: daily_writer::temporary_name(
                    std::path::Path::new(&day.destination_path),
                    "apply_recovery_test",
                )
                .unwrap(),
                recovery_payload: Some(Vec::new()),
                recovery_path: None,
            })
            .unwrap();
        state
            .store
            .transition_apply_operation(&operation.id, "prepared", "writing", None)
            .unwrap();
        let target = workspace.canonical_root().join(&day.destination_path);
        std::fs::create_dir_all(target.parent().unwrap()).unwrap();
        std::fs::write(
            target.parent().unwrap().join(
                operation
                    .temporary_name
                    .as_deref()
                    .expect("temporary identity exists"),
            ),
            &plan.updated_content,
        )
        .unwrap();

        recover_daily_applies(&state);

        assert_eq!(
            state
                .store
                .apply_operation(&operation.id)
                .unwrap()
                .unwrap()
                .state,
            "finalized"
        );
        assert_eq!(
            state
                .store
                .daily_day(&profile.id, date)
                .unwrap()
                .unwrap()
                .review_status,
            "applied"
        );
        assert_eq!(std::fs::read(target).unwrap(), plan.updated_content);
    }

    #[test]
    fn active_workspace_checks_detect_replacement_after_startup() {
        let state = test_state();
        let root = state.workspace.canonical_root().to_owned();
        state
            .store
            .save_active_workspace_profile(
                state.workspace.root_binding(),
                "UTC",
                "Work Log",
                "{date}.md",
                None,
                "markdown",
                None,
            )
            .unwrap();
        let previous = root.with_extension("replaced");
        std::fs::rename(&root, &previous).unwrap();
        std::fs::create_dir(&root).unwrap();

        let error = active_refocus_workspace(&state)
            .expect_err("a replacement at the same configured path must require review");
        assert_eq!(error.status, StatusCode::CONFLICT);
        assert!(error.message.contains("mounted workspace has changed"));

        std::fs::remove_dir_all(root).unwrap();
        std::fs::remove_dir_all(previous).unwrap();
    }

    #[test]
    fn refocus_boundary_rejects_untrusted_hosts_and_origins() {
        let config = RefocusConfig {
            allowed_hosts: HashSet::from(["localhost:8788".to_owned()]),
            allowed_origins: HashSet::from(["http://localhost:8788".to_owned()]),
        };
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "localhost:8788".parse().unwrap());
        headers.insert(header::ORIGIN, "http://localhost:8788".parse().unwrap());
        assert!(validate_request_boundary(&config, &headers, true).is_ok());
        headers.insert(header::ORIGIN, "https://attacker.example".parse().unwrap());
        assert!(validate_request_boundary(&config, &headers, true).is_err());
        headers.insert(header::ORIGIN, "http://localhost:8788".parse().unwrap());
        headers.insert(header::HOST, "attacker.example".parse().unwrap());
        assert!(validate_request_boundary(&config, &headers, false).is_err());
    }

    #[test]
    fn extracts_only_the_named_dashboard_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            "theme=dark; log_inbox_session=session_test; other=value"
                .parse()
                .unwrap(),
        );
        assert_eq!(session_cookie(&headers), Some("session_test"));
    }

    #[test]
    fn cookie_security_follows_the_validated_request_origin() {
        let mut headers = HeaderMap::new();
        headers.insert(header::ORIGIN, "http://localhost:8788".parse().unwrap());
        assert!(!request_uses_https(&headers));
        headers.insert(header::ORIGIN, "https://logs.example.test".parse().unwrap());
        assert!(request_uses_https(&headers));
    }

    #[test]
    fn frozen_daily_windows_ignore_later_workspace_timezone_changes() {
        let date = NaiveDate::from_ymd_opt(2026, 3, 29).unwrap();
        let created_at: DateTime<Utc> = "2026-01-01T00:00:00Z".parse().unwrap();
        let profile = log_inbox_core::models::WorkspaceProfile {
            id: "workspace_test".to_owned(),
            status: "active".to_owned(),
            root_binding: "root".to_owned(),
            timezone: "America/New_York".to_owned(),
            daily_root: "Journal".to_owned(),
            daily_pattern: "{date}.md".to_owned(),
            template_path: None,
            link_style: "markdown".to_owned(),
            created_at,
            updated_at: created_at,
        };
        let stockholm = resolve_day(date, "Europe/Stockholm").unwrap();
        let frozen = log_inbox_core::models::DailyDay {
            workspace_id: profile.id.clone(),
            local_date: date,
            timezone: stockholm.timezone.clone(),
            start_utc: stockholm.start_utc,
            end_utc: stockholm.end_utc,
            destination_path: "Old Journal/2026-03-29.md".to_owned(),
            template_revision: None,
            block_id: "day_test".to_owned(),
            generation_status: "ready".to_owned(),
            review_status: "in_review".to_owned(),
            freshness: "current".to_owned(),
            current_revision_id: Some("revision_test".to_owned()),
            created_at,
            updated_at: created_at,
        };

        let effective = effective_daily_window(date, &profile, Some(&frozen)).unwrap();

        assert_eq!(effective.timezone, "Europe/Stockholm");
        assert_eq!(effective.start_utc, stockholm.start_utc);
        assert_eq!(effective.end_utc, stockholm.end_utc);
        assert_eq!(effective.destination_path, frozen.destination_path);
    }
}
