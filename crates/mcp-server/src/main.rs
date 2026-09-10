use axum::{
    Json, Router,
    body::Body,
    extract::{Path as AxumPath, Query, State},
    http::{HeaderMap, Request, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post, put},
};
use chrono::{DateTime, Duration, NaiveDate, NaiveTime, Utc};
use log_inbox_core::{
    auth::{
        DASHBOARD_SCOPES, generate_session_credentials, hash_owner_secret, verify_owner_secret,
    },
    daily::{render_daily_path, resolve_day, resolve_local_time},
    models::{
        ApplyOperation, DailyDay, DailyRevisionContent, PrepareApplyOperation, ProposalRevision,
        WorkspaceProfile,
    },
    settings::Settings,
    store::Store,
    workspace::{InspectedWorkspace, MarkdownPathMode},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashSet,
    env,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod daily_writer;
mod llm;
mod migration;

#[derive(Clone)]
struct AppState {
    store: Store,
    llm_config: Option<llm::LlmConfig>,
    legacy_proposal_dir: Option<PathBuf>,
    legacy_support_files: Vec<(String, PathBuf)>,
    apply_lock: Arc<Mutex<()>>,
    daily_generation_lock: Arc<tokio::sync::Mutex<()>>,
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
struct ExpectedRevisionRequest {
    expected_revision_id: String,
}

#[derive(Debug, Deserialize)]
struct EditDailyCandidateRequest {
    expected_revision_id: String,
    content: DailyRevisionContent,
}

#[derive(Debug, Default, Deserialize)]
struct GenerateDailyRequest {
    #[serde(default)]
    replace_edited: bool,
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
        daily_generation_lock: Arc::new(tokio::sync::Mutex::new(())),
        refocus,
        workspace,
    };
    recover_daily_applies(&state);
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
            "/api/v2/migration/cutover",
            get(refocus_cutover_report).post(refocus_commit_cutover),
        )
        .route("/api/v2/daily/overview", get(refocus_daily_overview))
        .route("/api/v2/daily/{date}", get(refocus_daily_day))
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
            "/api/v2/daily/{date}/manual",
            post(refocus_create_manual_entry),
        )
        .route(
            "/api/v2/daily/{date}/dismiss",
            post(refocus_dismiss_daily).delete(refocus_reopen_daily),
        )
        .route(
            "/api/v2/daily/{date}/evidence/{event_id}",
            put(refocus_decide_daily_evidence).delete(refocus_reopen_daily_evidence),
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
    state
        .store
        .create_dashboard_session(
            &credentials,
            &DASHBOARD_SCOPES
                .iter()
                .map(|scope| (*scope).to_owned())
                .collect::<Vec<_>>(),
            Utc::now(),
            Duration::minutes(30),
            Duration::hours(8),
        )
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let secure = if request_uses_https(&headers) {
        "; Secure"
    } else {
        ""
    };
    let cookie = format!(
        "log_inbox_session={}; Path=/; HttpOnly; SameSite=Strict; Max-Age=28800{secure}",
        credentials.session_token
    );
    Ok((
        [(header::SET_COOKIE, cookie)],
        Json(json!({ "csrf_token": credentials.csrf_token, "expires_in_seconds": 28800 })),
    )
        .into_response())
}

async fn refocus_session(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    let session = authorize_refocus(&state, &headers, "logs:read", false)?;
    Ok(Json(
        json!({ "authenticated": true, "scopes": session.scopes, "absolute_expires_at": session.absolute_expires_at }),
    ))
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
    Ok(Json(json!({
        "settings": settings,
        "saved": true,
        "writes_markdown_automatically": false
    })))
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
    migration::commit_cutover(
        &state.store,
        &profile,
        &workspace,
        state.legacy_proposal_dir.as_deref(),
        &state.legacy_support_files,
        &request,
    )
    .map(Json)
    .map_err(|error| ApiError::conflict(error.to_string()))
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
    let live_event_ids = evidence
        .events
        .iter()
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
        "automated_evidence": {
            "events": evidence.events,
            "returned_count": event_count,
            "truncated": evidence.truncated,
            "limit": evidence.limit
        },
        "manual_entries": manual_entries,
        "current_revision": current_revision,
        "current_snapshot": current_snapshot,
        "current_snapshot_evidence": current_snapshot_evidence,
        "candidate_freshness": candidate_freshness,
        "new_evidence_count": new_evidence_count,
        "expired_evidence_count": expired_evidence_count,
        "evidence_complete": expired_evidence_count == 0,
        "preview_markdown": preview_markdown,
        "apply_status": apply_status
    })))
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
    let limit = query.limit.unwrap_or(14).clamp(1, 31);
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
    Ok(Json(json!({
        "workspace_id": profile.id,
        "server_now": Utc::now(),
        "today": today,
        "timezone": profile.timezone,
        "missed_count": missed_count,
        "update_count": update_count,
        "failed_count": failed_count,
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
    } else if day.is_some_and(|day| day.generation_status == "failed")
        || schedule_run.is_some_and(|run| run.state == "failed")
    {
        "generation_failed"
    } else if update_available || day.is_some_and(|day| day.freshness == "update_available") {
        "update_available"
    } else if apply.is_some_and(|operation| operation.state == "finalized")
        || day.is_some_and(|day| day.review_status == "applied")
    {
        "applied"
    } else if day.is_some_and(|day| day.review_status == "dismissed") {
        "dismissed"
    } else if day.is_some_and(|day| matches!(day.generation_status.as_str(), "queued" | "running"))
    {
        "generating"
    } else if revision.is_some() {
        "in_review"
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
    if let Some(operation) = state
        .store
        .apply_operation(&operation_id)
        .map_err(|error| ApiError::internal(error.to_string()))?
        .filter(|operation| operation.state == "finalized")
    {
        validate_apply_operation_approval(&operation, &input)?;
        return Ok(Json(json!({
            "operation": public_apply_operation(&operation),
            "destination_path": input.destination_path,
            "idempotent": true
        })));
    }

    let material = daily_apply_material(&state, local_date)?;
    validate_apply_material_approval(&material, &input)?;
    let operation = match state
        .store
        .apply_operation(&operation_id)
        .map_err(|error| ApiError::internal(error.to_string()))?
    {
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
    if snapshot_evidence
        .iter()
        .any(|item| item.disposition.is_none())
    {
        return Err(ApiError::conflict(
            "review every automated evidence item before previewing Apply",
        ));
    }
    let live = state
        .store
        .get_events_between(day.start_utc, day.end_utc, 500)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    if live.truncated {
        return Err(ApiError::unprocessable(
            "Daily evidence exceeds the supported 500-event Apply limit.",
        ));
    }
    let live_event_ids = live
        .events
        .iter()
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
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
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
        .map_err(|error| ApiError::conflict(error.to_string()))
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

async fn refocus_generate_daily(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(date): AxumPath<String>,
    input: Option<Json<GenerateDailyRequest>>,
) -> Result<Json<ProposalRevision>, ApiError> {
    authorize_refocus(&state, &headers, "draft:generate", true)?;
    let local_date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("date must use YYYY-MM-DD"))?;
    let replace_edited = input
        .map(|Json(input)| input.replace_edited)
        .unwrap_or(false);
    generate_daily_candidate(&state, local_date, replace_edited)
        .await
        .map(Json)
}

async fn generate_daily_candidate(
    state: &AppState,
    local_date: NaiveDate,
    replace_edited: bool,
) -> Result<ProposalRevision, ApiError> {
    require_cutover_for_daily_mutation(state)?;
    let _generation_guard = state.daily_generation_lock.lock().await;
    let (profile, workspace) = active_refocus_context(state)?;
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
    let evidence = state
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
                return Ok(current.clone());
            }
            if !has_new_evidence {
                content.manual_entry_ids = manual_entry_ids;
                let content = serde_json::to_value(content)
                    .map_err(|error| ApiError::internal(error.to_string()))?;
                return state
                    .store
                    .create_proposal_revision_if_current(
                        &profile.id,
                        local_date,
                        Some(snapshot_id),
                        "structured_edit",
                        &content,
                        &current.id,
                    )
                    .map_err(|error| ApiError::conflict(error.to_string()));
            }
            return Err(ApiError::conflict(
                "Some source evidence for this candidate has expired. Log Inbox will not replace a complete reviewed record from partial evidence. The existing revision is preserved; restore the missing source evidence before regenerating.",
            ));
        }
    }

    if evidence.events.is_empty() {
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
            return Ok(current.clone());
        }
        let revision = state
            .store
            .create_proposal_revision(&profile.id, local_date, None, "manual", &content)
            .map_err(|error| ApiError::internal(error.to_string()))?;
        return Ok(revision);
    }

    let event_ids = evidence
        .events
        .iter()
        .map(|event| event.id.clone())
        .collect::<Vec<_>>();
    let snapshot = state
        .store
        .create_evidence_snapshot(&profile.id, local_date, &event_ids)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    if let Some(current) = current.as_ref()
        && current.snapshot_id.as_deref() == Some(snapshot.id.as_str())
    {
        let mut content = serde_json::from_value::<DailyRevisionContent>(current.content.clone())
            .map_err(|error| ApiError::internal(error.to_string()))?;
        if content.manual_entry_ids == manual_entry_ids {
            return Ok(current.clone());
        }
        content.manual_entry_ids = manual_entry_ids;
        let content =
            serde_json::to_value(content).map_err(|error| ApiError::internal(error.to_string()))?;
        let revised = state
            .store
            .create_proposal_revision_if_current(
                &profile.id,
                local_date,
                Some(&snapshot.id),
                "structured_edit",
                &content,
                &current.id,
            )
            .map_err(|error| ApiError::conflict(error.to_string()))?;
        return Ok(revised);
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
    let args = llm::SuggestMarkdownSummaryArgs {
        vault_context: json!({
            "daily_note": window.destination_path,
            "candidate_notes": [],
            "workstream_links": {}
        }),
        mode: "daily-consolidation".to_owned(),
        task: Some(
            "Create a concise, evidence-backed daily engineering record. Preserve distinct outcomes, decisions, trade-offs, validation, blockers, and follow-up."
                .to_owned(),
        ),
    };
    let proposal = match llm::generate_automated_daily_summary(
        state.llm_config.as_ref(),
        args,
        evidence.events,
    )
    .await
    {
        Ok(proposal) => proposal,
        Err(error) => {
            state
                .store
                .set_daily_generation_status(&profile.id, local_date, "failed")
                .map_err(|store_error| ApiError::internal(store_error.to_string()))?;
            return Err(ApiError::unprocessable(error));
        }
    };
    let draft = proposal
        .structured_draft
        .ok_or_else(|| ApiError::internal("daily generator omitted structured content"))?;
    let content = json!({
        "schema_version": 1,
        "workstreams": draft.workstreams,
        "manual_entry_ids": manual_entry_ids,
        "open_questions": draft.open_questions
    });
    let origin = if current.is_some() {
        "regenerated"
    } else {
        "generated"
    };
    let revision = state
        .store
        .create_proposal_revision(
            &profile.id,
            local_date,
            Some(&snapshot.id),
            origin,
            &content,
        )
        .map_err(|error| ApiError::internal(error.to_string()))?;
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
    let content = serde_json::to_value(input.content)
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let revision = state
        .store
        .create_proposal_revision_if_current(
            &profile.id,
            local_date,
            current.snapshot_id.as_deref(),
            "structured_edit",
            &content,
            &input.expected_revision_id,
        )
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
            continue;
        }
        state
            .store
            .set_daily_generation_status(&profile.id, local_date, "queued")
            .map_err(|error| ApiError::internal(error.to_string()))?;
        match generate_daily_candidate(state, local_date, false).await {
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
    }
    Ok(())
}

fn bounded_schedule_error(message: &str) -> String {
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
        .authenticate_dashboard_session(token, csrf, scope, Utc::now(), Duration::minutes(30))
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
}

impl ApiError {
    fn unauthorized(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNAUTHORIZED,
            message: message.into(),
        }
    }

    fn forbidden(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::FORBIDDEN,
            message: message.into(),
        }
    }

    fn bad_request(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::BAD_REQUEST,
            message: message.into(),
        }
    }

    fn internal(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::INTERNAL_SERVER_ERROR,
            message: message.into(),
        }
    }

    fn conflict(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::CONFLICT,
            message: message.into(),
        }
    }

    fn unprocessable(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::UNPROCESSABLE_ENTITY,
            message: message.into(),
        }
    }

    fn not_found(message: impl Into<String>) -> Self {
        Self {
            status: StatusCode::NOT_FOUND,
            message: message.into(),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (self.status, Json(json!({ "error": self.message }))).into_response()
    }
}

#[cfg(test)]
mod knowledge_destination_tests {
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
            daily_generation_lock: Arc::new(tokio::sync::Mutex::new(())),
            refocus: RefocusConfig {
                allowed_hosts: HashSet::from(["localhost:8788".to_owned()]),
                allowed_origins: HashSet::from(["http://localhost:8788".to_owned()]),
            },
            workspace: InspectedWorkspace::inspect(&workspace_root).expect("workspace inspects"),
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
        let unchanged = generate_daily_candidate(&state, date, false)
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
        let manual_revision = generate_daily_candidate(&state, date, false)
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
        state
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

        let error = generate_daily_candidate(&state, date, true)
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
        assert_eq!(generated.status(), StatusCode::OK);

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
        assert_eq!(regenerated.status(), StatusCode::OK);
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
