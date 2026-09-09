use axum::{
    Json, Router,
    body::Body,
    extract::{Path as AxumPath, State},
    http::{HeaderMap, Request, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post, put},
};
use chrono::{DateTime, Datelike, Duration, NaiveDate, Utc};
use log_inbox_core::{
    auth::{
        DASHBOARD_SCOPES, generate_session_credentials, hash_owner_secret, verify_owner_secret,
    },
    daily::{render_daily_path, resolve_day},
    models::{
        ApplyOperation, DailyConsolidationJob, DailyDay, DailyRevisionContent, IgnoredLinkIdentity,
        LinkSelector, LogEventInput, LogQuery, PrepareApplyOperation, ProposalRevision,
        VaultLinkRule, WorkspaceProfile,
    },
    settings::Settings,
    store::Store,
    workspace::{InspectedWorkspace, MarkdownPathMode},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    env,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod auto_stage;
mod daily_consolidation;
mod daily_writer;
mod llm;
mod proposal_inbox;
mod vault_context;

#[derive(Clone)]
struct AppState {
    store: Store,
    llm_config: Option<llm::LlmConfig>,
    proposal_inbox: Option<proposal_inbox::ProposalInbox>,
    daily_notes_dir: Option<PathBuf>,
    daily_notes_display_path: Option<String>,
    vault_context: vault_context::VaultContextProvider,
    apply_lock: Arc<Mutex<()>>,
    daily_generation_lock: Arc<tokio::sync::Mutex<()>>,
    refocus: Option<RefocusConfig>,
    workspace: Option<InspectedWorkspace>,
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
    fn from_env(store: &Store) -> anyhow::Result<Option<Self>> {
        if env::var("LOG_INBOX_REFOCUS_ENABLED").as_deref() != Ok("1") {
            return Ok(None);
        }
        let owner_secret = env::var("LOG_INBOX_OWNER_SECRET").map_err(|_| {
            anyhow::anyhow!("LOG_INBOX_OWNER_SECRET is required when refocus is enabled")
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
        Ok(Some(Self {
            allowed_hosts,
            allowed_origins,
        }))
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

#[derive(Debug, Deserialize)]
struct UpdateProposalRequest {
    markdown: String,
    expected_revision: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DashboardPreferences {
    ingest_url: String,
    agent_name: String,
    source_prefix: String,
    default_host: String,
    extra_instructions: String,
    #[serde(default, alias = "consolidation_instructions")]
    daily_consolidation_prompt: String,
}

#[derive(Debug, Deserialize)]
struct DailyConsolidationRequest {
    start: DateTime<Utc>,
    end: DateTime<Utc>,
    target_note: String,
}

#[derive(Debug, Serialize)]
struct DashboardData {
    preferences: DashboardPreferences,
    instructions: String,
    proposals: Vec<proposal_inbox::PendingProposal>,
    consolidations: Vec<DailyConsolidationJob>,
}

#[derive(Debug, Deserialize)]
struct LinkRuleInput {
    #[serde(default)]
    id: Option<String>,
    selectors: Vec<LinkSelector>,
    target_note_id: String,
    #[serde(default = "default_true")]
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct IgnoreIdentityInput {
    field: String,
    value: String,
}

#[derive(Debug, Serialize)]
struct LinkingData {
    catalog: vault_context::VaultCatalog,
    rules: Vec<VaultLinkRule>,
    observed: Vec<vault_context::ObservedIdentity>,
    ignored: Vec<IgnoredLinkIdentity>,
    event_count: usize,
}

#[derive(Debug, Deserialize)]
struct ManualLogInput {
    message: String,
    timestamp: String,
    #[serde(default)]
    vault_note_ids: Vec<String>,
    #[serde(default)]
    work_item: Option<String>,
    #[serde(default)]
    pull_request: Option<String>,
}

#[derive(Debug, Serialize)]
struct ManualLogOptions {
    notes: Vec<vault_context::VaultNote>,
    recent_note_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct BrowserVaultFile {
    path: String,
    contents: String,
}

#[derive(Debug, Deserialize)]
struct BrowserVaultSyncInput {
    #[serde(default)]
    vault_id: String,
    name: String,
    files: Vec<BrowserVaultFile>,
    #[serde(default, alias = "tree_paths")]
    markdown_paths: Vec<String>,
    #[serde(default)]
    folder_paths: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct KnowledgeDestination {
    role: String,
    base_path: String,
    path_template: String,
    write_mode: String,
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct KnowledgeDestinationDraft {
    role: String,
    base_path: String,
    path_template: String,
    #[serde(default = "default_true")]
    enabled: bool,
}

#[derive(Debug, Deserialize)]
struct KnowledgeStructureInput {
    destinations: Vec<KnowledgeDestinationDraft>,
    catalog_revision: String,
    #[serde(default)]
    example_date: Option<String>,
}

#[derive(Debug, Deserialize)]
struct BrowserApplyInput {
    current_content: String,
}

#[derive(Debug, Deserialize)]
struct BrowserApplyAcknowledgement {
    acknowledgement_token: String,
    verified_revision: String,
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
    let refocus_enabled = refocus.is_some();
    let workspace = if refocus_enabled {
        let configured_root = env::var_os("LOG_INBOX_WORKSPACE_DIR")
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/workspace"));
        Some(InspectedWorkspace::inspect(&configured_root)?)
    } else {
        None
    };
    if !refocus_enabled {
        daily_consolidation::migrate_prompt_preference(&store)?;
        store.recover_daily_consolidations()?;
    }
    let vault_context = vault_context::VaultContextProvider::from_env();
    if !refocus_enabled && let Some(saved) = store.get_preferences()?.get("browser_vault_catalog") {
        match serde_json::from_str::<vault_context::VaultCatalog>(saved) {
            Ok(mut catalog) => {
                if catalog.vault_id.is_empty() {
                    catalog.vault_id = format!(
                        "browser-legacy:{}",
                        catalog.root.as_deref().unwrap_or("vault")
                    );
                }
                vault_context
                    .set_browser_catalog(Some(catalog))
                    .map_err(anyhow::Error::msg)?;
            }
            Err(error) => tracing::warn!(%error, "ignoring invalid saved browser vault catalog"),
        }
    }
    let state = AppState {
        store,
        llm_config: llm::LlmConfig::from_env(),
        proposal_inbox: (!refocus_enabled)
            .then(proposal_inbox::ProposalInbox::from_env)
            .flatten(),
        daily_notes_dir: (!refocus_enabled)
            .then(|| {
                env::var_os("LOG_INBOX_DAILY_NOTES_DIR")
                    .filter(|path| !path.is_empty())
                    .map(PathBuf::from)
            })
            .flatten(),
        daily_notes_display_path: (!refocus_enabled)
            .then(|| {
                env::var("LOG_INBOX_DAILY_NOTES_DISPLAY_PATH")
                    .ok()
                    .filter(|path| !path.trim().is_empty())
            })
            .flatten(),
        vault_context,
        apply_lock: Arc::new(Mutex::new(())),
        daily_generation_lock: Arc::new(tokio::sync::Mutex::new(())),
        refocus,
        workspace,
    };
    if refocus_enabled {
        recover_daily_applies(&state);
    }

    if !refocus_enabled {
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

        if let Some(inbox) = state.proposal_inbox.clone() {
            tokio::spawn(daily_consolidation::run(
                state.store.clone(),
                state.llm_config.clone(),
                inbox,
                state.vault_context.clone(),
            ));
        }
    }

    let app = build_router(state);

    let addr: SocketAddr = "0.0.0.0:8788".parse()?;
    tracing::info!(%addr, "starting mcp server");
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

fn build_router(state: AppState) -> Router {
    let app = Router::new()
        .route("/", get(dashboard_page))
        .route("/favicon.ico", get(favicon))
        .route("/health", get(health));

    let app = if state.refocus.is_some() {
        app.route("/api/v2/auth/login", post(refocus_login))
            .route("/api/v2/auth/session", get(refocus_session))
            .route("/api/v2/auth/logout", post(refocus_logout))
            .route(
                "/api/v2/settings/workspace",
                get(refocus_workspace_settings),
            )
            .route(
                "/api/v2/settings/workspace/preview",
                post(refocus_preview_workspace_settings),
            )
            .route(
                "/api/v2/settings/workspace",
                put(refocus_save_workspace_settings),
            )
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
                "/api/v2/daily/{date}/evidence/{event_id}",
                put(refocus_decide_daily_evidence).delete(refocus_reopen_daily_evidence),
            )
            .route(
                "/api/v2/daily/{date}/candidate",
                put(refocus_edit_daily_candidate),
            )
    } else {
        app.route("/api/dashboard", get(dashboard_data))
            .route("/api/logs/manual/options", get(manual_log_options))
            .route("/api/logs/manual", post(create_manual_log))
            .route("/api/vault/connection", get(vault_connection))
            .route("/api/knowledge", get(knowledge_data))
            .route(
                "/api/knowledge/structure/preview",
                post(preview_knowledge_structure),
            )
            .route("/api/knowledge/structure", put(save_knowledge_structure))
            .route(
                "/api/vault/browser/catalog",
                put(sync_browser_vault).delete(disconnect_browser_vault),
            )
            .route("/api/preferences", put(save_preferences))
            .route("/api/linking", get(linking_data))
            .route("/api/linking/scan", post(linking_data))
            .route("/api/linking/rules", post(create_link_rule))
            .route("/api/linking/ignored", put(ignore_link_identity))
            .route(
                "/api/linking/ignored/{ignored_id}",
                axum::routing::delete(restore_ignored_identity),
            )
            .route(
                "/api/linking/rules/{rule_id}",
                put(update_link_rule).delete(delete_link_rule),
            )
            .route(
                "/api/proposals/{proposal_id}",
                put(update_dashboard_proposal),
            )
            .route(
                "/api/proposals/{proposal_id}/apply",
                post(apply_dashboard_proposal),
            )
            .route(
                "/api/proposals/{proposal_id}/browser-apply/prepare",
                post(prepare_browser_apply),
            )
            .route(
                "/api/proposals/{proposal_id}/browser-apply/acknowledge",
                post(acknowledge_browser_apply),
            )
            .route(
                "/api/proposals/{proposal_id}/discard",
                post(discard_dashboard_proposal),
            )
            .route(
                "/api/proposals/{proposal_id}/regenerate",
                post(regenerate_dashboard_proposal),
            )
            .route("/api/consolidations/daily", post(consolidate_dashboard_day))
            .route(
                "/api/consolidations/{job_id}",
                get(get_dashboard_consolidation),
            )
            .route(
                "/api/consolidations/{job_id}/cancel",
                post(cancel_dashboard_consolidation),
            )
            .route("/mcp", post(mcp))
    };

    app.layer(middleware::from_fn(log_request_response))
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
    let config = state
        .refocus
        .as_ref()
        .ok_or_else(|| ApiError::not_found("refocused API is disabled"))?;
    validate_request_boundary(config, &headers, true)?;
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

fn inspect_refocus_workspace(state: &AppState) -> Result<InspectedWorkspace, ApiError> {
    let configured = state
        .workspace
        .as_ref()
        .ok_or_else(|| ApiError::internal("workspace root is not configured"))?;
    InspectedWorkspace::inspect(configured.canonical_root()).map_err(|error| {
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
            .map(|snapshot| {
                snapshot
                    .event_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        if stored_event_ids == live_event_ids
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
        "preview_markdown": preview_markdown,
        "apply_status": apply_status
    })))
}

async fn refocus_daily_apply_preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(date): AxumPath<String>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "logs:read", false)?;
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
        .map(|snapshot| {
            snapshot
                .event_ids
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let live_manual_ids = manual_entries
        .iter()
        .map(|entry| entry.id.as_str())
        .collect::<Vec<_>>();
    if live_event_ids != snapshot_event_ids
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
    let _generation_guard = state.daily_generation_lock.lock().await;
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
        if let Some(current) = state
            .store
            .current_proposal_revision(&profile.id, local_date)
            .map_err(|error| ApiError::internal(error.to_string()))?
            .filter(|current| current.snapshot_id.is_none() && current.content == content)
        {
            return Ok(Json(current));
        }
        let revision = state
            .store
            .create_proposal_revision(&profile.id, local_date, None, "manual", &content)
            .map_err(|error| ApiError::internal(error.to_string()))?;
        return Ok(Json(revision));
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
    let current = state
        .store
        .current_proposal_revision(&profile.id, local_date)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    if let Some(current) = current.as_ref()
        && current.snapshot_id.as_deref() == Some(snapshot.id.as_str())
    {
        let mut content = serde_json::from_value::<DailyRevisionContent>(current.content.clone())
            .map_err(|error| ApiError::internal(error.to_string()))?;
        if content.manual_entry_ids == manual_entry_ids {
            return Ok(Json(current.clone()));
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
        return Ok(Json(revised));
    }
    let replace_edited = input
        .map(|Json(input)| input.replace_edited)
        .unwrap_or(false);
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
        event_ids,
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
    Ok(Json(revision))
}

async fn refocus_decide_daily_evidence(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath((date, event_id)): AxumPath<(String, String)>,
    Json(input): Json<EvidenceDecisionRequest>,
) -> Result<Json<Vec<log_inbox_core::models::SnapshotEvidence>>, ApiError> {
    authorize_refocus(&state, &headers, "review:write", true)?;
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

fn authorize_refocus(
    state: &AppState,
    headers: &HeaderMap,
    scope: &str,
    require_csrf: bool,
) -> Result<log_inbox_core::models::DashboardSession, ApiError> {
    let config = state
        .refocus
        .as_ref()
        .ok_or_else(|| ApiError::not_found("refocused API is disabled"))?;
    validate_request_boundary(config, headers, require_csrf)?;
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

async fn dashboard_page(State(state): State<AppState>) -> Html<&'static str> {
    if state.refocus.is_some() {
        Html(include_str!("../assets/daily.html"))
    } else {
        Html(include_str!("../assets/dashboard.html"))
    }
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

async fn dashboard_data(State(state): State<AppState>) -> Result<Json<DashboardData>, ApiError> {
    let preferences = DashboardPreferences::load(&state.store)?;
    let mut proposals = state
        .proposal_inbox
        .as_ref()
        .map_or_else(|| Ok(Vec::new()), proposal_inbox::ProposalInbox::list)
        .map_err(ApiError::internal)?;
    let rules = state
        .store
        .list_link_rules()
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let revision = state
        .vault_context
        .for_events(&[], &rules)
        .map_err(ApiError::internal)?["link_context_revision"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    for proposal in &mut proposals {
        let events = state
            .store
            .get_events_by_ids(&proposal.evidence_event_ids)
            .map_err(|error| ApiError::internal(error.to_string()))?;
        proposal.evidence_start = events.iter().map(|event| event.timestamp).min();
        proposal.evidence_end = events.iter().map(|event| event.timestamp).max();
        if proposal.link_context_revision == revision {
            continue;
        }
        if proposal.link_candidates.is_empty() {
            proposal.stale = true;
            continue;
        }
        let context = state
            .vault_context
            .for_events(&events, &rules)
            .map_err(ApiError::internal)?;
        let mut current = context["candidate_notes"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();
        current.sort();
        let mut previous = proposal.link_candidates.clone();
        previous.sort();
        proposal.stale = previous.is_empty() || previous != current;
    }
    let instructions = render_agent_instructions(&preferences);
    let consolidations = state
        .store
        .list_daily_consolidations(20)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(Json(DashboardData {
        preferences,
        instructions,
        proposals,
        consolidations,
    }))
}

async fn manual_log_options(
    State(state): State<AppState>,
) -> Result<Json<ManualLogOptions>, ApiError> {
    let catalog = state.vault_context.catalog().map_err(ApiError::internal)?;
    let valid = catalog
        .notes
        .iter()
        .map(|note| note.id.as_str())
        .collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    let recent_note_ids = state
        .store
        .all_events()
        .map_err(|error| ApiError::internal(error.to_string()))?
        .into_iter()
        .filter(|event| event.metadata.get("entry_kind").and_then(Value::as_str) == Some("manual"))
        .flat_map(|event| {
            event
                .metadata
                .get("canonical_note_candidates")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|value| value.as_str().map(ToOwned::to_owned))
        .filter(|id| valid.contains(id.as_str()) && seen.insert(id.clone()))
        .take(5)
        .collect();
    Ok(Json(ManualLogOptions {
        notes: catalog.notes,
        recent_note_ids,
    }))
}

async fn vault_connection(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let browser = state
        .vault_context
        .browser_catalog()
        .map_err(ApiError::internal)?;
    let catalog = state.vault_context.catalog().map_err(ApiError::internal)?;
    Ok(Json(json!({
        "mode": if browser.is_some() { "browser" } else if catalog.configured { "mounted" } else { "unconfigured" },
        "vault_id": catalog.vault_id,
        "name": catalog.root,
        "revision": catalog.revision,
        "note_count": catalog.notes.len(),
        "daily_notes_path": state.daily_notes_display_path.as_deref().or_else(|| state.daily_notes_dir.as_deref().and_then(|path| path.to_str())),
    })))
}

const KNOWLEDGE_ROLES: &[&str] = &[
    "daily_activity",
    "product_knowledge",
    "engineering_knowledge",
    "decision_records",
    "feature_recaps",
];

fn destination_preference_key(vault_id: &str) -> String {
    format!("knowledge_destinations_v1:{vault_id}")
}

fn destination_write_mode(role: &str) -> Option<&'static str> {
    match role {
        "daily_activity" => Some("managed_daily_note"),
        "product_knowledge" | "engineering_knowledge" => Some("reference_root"),
        "decision_records" => Some("adjacent_to_canonical"),
        "feature_recaps" => Some("managed_section"),
        _ => None,
    }
}

fn normalized_destination_path(value: &str) -> Result<String, ApiError> {
    let normalized = value.trim().trim_matches('/').replace('\\', "/");
    if normalized.is_empty() {
        return Ok(String::new());
    }
    let path = PathBuf::from(&normalized);
    if path.is_absolute()
        || path
            .components()
            .any(|component| !matches!(component, std::path::Component::Normal(_)))
    {
        return Err(ApiError::bad_request(
            "destination paths must remain relative to the vault",
        ));
    }
    let lower = normalized.to_ascii_lowercase();
    if lower.starts_with('.')
        || lower == "00 inbox"
        || lower.starts_with("00 inbox/")
        || lower.contains("/log inbox/pending")
        || lower == "log inbox/pending"
    {
        return Err(ApiError::bad_request(
            "system and proposal-inbox folders cannot be knowledge destinations",
        ));
    }
    Ok(normalized)
}

fn validate_path_template(role: &str, value: &str) -> Result<String, ApiError> {
    let template = value.trim().replace('\\', "/");
    if matches!(
        role,
        "product_knowledge" | "engineering_knowledge" | "feature_recaps"
    ) && !template.is_empty()
    {
        return Err(ApiError::bad_request(
            "this destination role does not use a path template",
        ));
    }
    if role == "daily_activity" && !template.to_ascii_lowercase().ends_with(".md") {
        return Err(ApiError::bad_request(
            "daily activity templates must resolve to a Markdown filename",
        ));
    }
    if template.len() > 300 || template.starts_with('/') || template.contains("..") {
        return Err(ApiError::bad_request("invalid destination path template"));
    }
    let mut remainder = template.clone();
    for token in [
        "{year}",
        "{month}",
        "{month_name}",
        "{day}",
        "{date}",
        "{slug}",
    ] {
        remainder = remainder.replace(token, "");
    }
    if remainder.contains('{') || remainder.contains('}') {
        return Err(ApiError::bad_request(
            "destination template contains an unsupported token",
        ));
    }
    Ok(template)
}

fn vault_folders(catalog: &vault_context::VaultCatalog) -> Vec<String> {
    let mut folders = BTreeSet::new();
    folders.insert(String::new());
    folders.extend(catalog.folder_paths.iter().cloned());
    for path in &catalog.markdown_paths {
        let mut current = String::new();
        for part in path
            .split('/')
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .skip(1)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            current = if current.is_empty() {
                part.to_owned()
            } else {
                format!("{current}/{part}")
            };
            folders.insert(current.clone());
        }
    }
    folders.into_iter().collect()
}

fn normalize_knowledge_structure(
    input: &KnowledgeStructureInput,
) -> Result<Vec<KnowledgeDestination>, ApiError> {
    if input.destinations.len() != KNOWLEDGE_ROLES.len() {
        return Err(ApiError::bad_request(
            "the structure must contain each knowledge destination exactly once",
        ));
    }
    let mut seen = HashSet::new();
    let mut destinations = Vec::with_capacity(input.destinations.len());
    for draft in &input.destinations {
        if !KNOWLEDGE_ROLES.contains(&draft.role.as_str()) || !seen.insert(draft.role.as_str()) {
            return Err(ApiError::bad_request(
                "the structure contains an unknown or duplicate destination",
            ));
        }
        destinations.push(KnowledgeDestination {
            role: draft.role.clone(),
            base_path: normalized_destination_path(&draft.base_path)?,
            path_template: if !draft.enabled && draft.path_template.trim().is_empty() {
                String::new()
            } else {
                validate_path_template(&draft.role, &draft.path_template)?
            },
            write_mode: destination_write_mode(&draft.role).unwrap().to_owned(),
            enabled: draft.enabled,
        });
    }
    Ok(destinations)
}

fn required_static_folders(destination: &KnowledgeDestination) -> Vec<String> {
    if !destination.enabled {
        return Vec::new();
    }
    let mut required = Vec::new();
    let mut current = String::new();
    for part in destination
        .base_path
        .split('/')
        .filter(|part| !part.is_empty())
    {
        current = if current.is_empty() {
            part.to_owned()
        } else {
            format!("{current}/{part}")
        };
        required.push(current.clone());
    }
    for part in destination
        .path_template
        .split('/')
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .skip(1)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        if part.contains('{') || part.contains('}') || part.is_empty() {
            break;
        }
        current = if current.is_empty() {
            part.to_owned()
        } else {
            format!("{current}/{part}")
        };
        required.push(current.clone());
    }
    required
}

fn resolve_destination_example(destination: &KnowledgeDestination, date: NaiveDate) -> String {
    if !destination.enabled {
        return "Disabled".to_owned();
    }
    if destination.role == "feature_recaps" {
        return "Mapped canonical feature or system note".to_owned();
    }
    let month_name = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ][date.month0() as usize];
    let resolved = destination
        .path_template
        .replace("{year}", &date.year().to_string())
        .replace("{month}", &format!("{:02}", date.month()))
        .replace("{month_name}", month_name)
        .replace("{day}", &date.day().to_string())
        .replace("{date}", &date.format("%Y-%m-%d").to_string())
        .replace("{slug}", "decision-title");
    let path = [destination.base_path.as_str(), resolved.as_str()]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("/");
    if destination.role == "decision_records" && destination.base_path.is_empty() {
        format!("<mapped subject>/{path}")
    } else if path.is_empty() {
        "Vault root".to_owned()
    } else {
        path
    }
}

fn knowledge_structure_preview_value(
    catalog: &vault_context::VaultCatalog,
    destinations: &[KnowledgeDestination],
    example_date: NaiveDate,
    can_create_folders: bool,
) -> Value {
    let folders = vault_folders(catalog).into_iter().collect::<HashSet<_>>();
    let mut reused = BTreeSet::new();
    let mut missing = BTreeSet::new();
    for destination in destinations {
        for path in required_static_folders(destination) {
            if folders.contains(&path) {
                reused.insert(path);
            } else {
                missing.insert(path);
            }
        }
    }
    let examples = destinations
        .iter()
        .map(|destination| {
            (
                destination.role.clone(),
                resolve_destination_example(destination, example_date),
            )
        })
        .collect::<BTreeMap<_, _>>();
    json!({
        "destinations": destinations,
        "examples": examples,
        "existing_folders": reused,
        "missing_folders": missing,
        "can_create_folders": can_create_folders,
        "catalog_revision": catalog.revision,
    })
}

fn folder_label(path: &str) -> String {
    path.rsplit('/')
        .next()
        .unwrap_or(path)
        .trim_start_matches(|character: char| {
            character.is_ascii_digit() || character == ' ' || character == '-' || character == '_'
        })
        .to_ascii_lowercase()
}

fn suggested_folder(folders: &[String], terms: &[&str]) -> Option<String> {
    folders
        .iter()
        .filter(|path| !path.is_empty())
        .find(|path| terms.iter().any(|term| folder_label(path).contains(term)))
        .cloned()
}

async fn knowledge_data(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    let catalog = state.vault_context.catalog().map_err(ApiError::internal)?;
    let preferences = state
        .store
        .get_preferences()
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let destinations = preferences
        .get(&destination_preference_key(&catalog.vault_id))
        .and_then(|value| serde_json::from_str::<Vec<KnowledgeDestination>>(value).ok())
        .unwrap_or_default();
    let folders = vault_folders(&catalog);
    let daily = suggested_folder(&folders, &["work log", "daily"]);
    let products = suggested_folder(&folders, &["products", "product"]);
    let engineering = suggested_folder(&folders, &["engineering"]);
    let suggestions = vec![
        KnowledgeDestination {
            role: "daily_activity".to_owned(),
            base_path: daily.unwrap_or_default(),
            path_template: "{year}/{month_name}/Daily log {month_name} {day}.md".to_owned(),
            write_mode: "managed_daily_note".to_owned(),
            enabled: true,
        },
        KnowledgeDestination {
            role: "product_knowledge".to_owned(),
            base_path: products.unwrap_or_default(),
            path_template: String::new(),
            write_mode: "reference_root".to_owned(),
            enabled: true,
        },
        KnowledgeDestination {
            role: "engineering_knowledge".to_owned(),
            base_path: engineering.unwrap_or_default(),
            path_template: String::new(),
            write_mode: "reference_root".to_owned(),
            enabled: true,
        },
        KnowledgeDestination {
            role: "decision_records".to_owned(),
            base_path: String::new(),
            path_template: "Decisions/{date} {slug}.md".to_owned(),
            write_mode: "adjacent_to_canonical".to_owned(),
            enabled: false,
        },
        KnowledgeDestination {
            role: "feature_recaps".to_owned(),
            base_path: String::new(),
            path_template: String::new(),
            write_mode: "managed_section".to_owned(),
            enabled: false,
        },
    ];
    let protected_paths = folders
        .iter()
        .filter(|path| {
            let lower = path.to_ascii_lowercase();
            lower == "00 inbox" || lower.starts_with("00 inbox/") || lower.ends_with("/pending")
        })
        .cloned()
        .collect::<Vec<_>>();
    Ok(Json(json!({
        "vault": { "id": catalog.vault_id, "name": catalog.root, "revision": catalog.revision, "total_markdown_files": catalog.markdown_paths.len(), "linkable_notes": catalog.notes.len() },
        "destinations": destinations,
        "suggestions": suggestions,
        "folders": folders,
        "protected_paths": protected_paths,
    })))
}

async fn preview_knowledge_structure(
    State(state): State<AppState>,
    Json(input): Json<KnowledgeStructureInput>,
) -> Result<Json<Value>, ApiError> {
    let catalog = state.vault_context.catalog().map_err(ApiError::internal)?;
    if input.catalog_revision != catalog.revision {
        return Err(ApiError::conflict(
            "the vault changed; rescan and review the structure again",
        ));
    }
    let destinations = normalize_knowledge_structure(&input)?;
    let example_date = match input.example_date.as_deref() {
        Some(value) => NaiveDate::parse_from_str(value, "%Y-%m-%d")
            .map_err(|_| ApiError::bad_request("example date must use YYYY-MM-DD"))?,
        None => Utc::now().date_naive(),
    };
    let can_create_folders = state
        .vault_context
        .browser_catalog()
        .map_err(ApiError::internal)?
        .is_some();
    Ok(Json(knowledge_structure_preview_value(
        &catalog,
        &destinations,
        example_date,
        can_create_folders,
    )))
}

async fn save_knowledge_structure(
    State(state): State<AppState>,
    Json(input): Json<KnowledgeStructureInput>,
) -> Result<Json<Value>, ApiError> {
    let catalog = state.vault_context.catalog().map_err(ApiError::internal)?;
    if input.catalog_revision != catalog.revision {
        return Err(ApiError::conflict(
            "the vault changed; rescan and review the structure again",
        ));
    }
    let destinations = normalize_knowledge_structure(&input)?;
    let key = destination_preference_key(&catalog.vault_id);
    state
        .store
        .set_preferences(&BTreeMap::from([(
            key,
            serde_json::to_string(&destinations)
                .map_err(|error| ApiError::internal(error.to_string()))?,
        )]))
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(Json(json!({ "destinations": destinations })))
}

async fn sync_browser_vault(
    State(state): State<AppState>,
    Json(input): Json<BrowserVaultSyncInput>,
) -> Result<Json<vault_context::VaultCatalog>, ApiError> {
    if input.vault_id.len() > 200 {
        return Err(ApiError::bad_request(
            "vault ID must contain no more than 200 bytes",
        ));
    }
    if input.name.trim().is_empty() || input.name.len() > 200 {
        return Err(ApiError::bad_request(
            "vault name must contain 1 to 200 bytes",
        ));
    }
    if input.files.len() > 500 {
        return Err(ApiError::bad_request(
            "vault scan is limited to 500 Markdown files",
        ));
    }
    if input.markdown_paths.len() > 2_000 {
        return Err(ApiError::bad_request(
            "vault scan is limited to 2000 Markdown paths",
        ));
    }
    if input.folder_paths.len() > 2_000 {
        return Err(ApiError::bad_request(
            "vault scan is limited to 2000 folder paths",
        ));
    }
    let mut files = Vec::with_capacity(input.files.len());
    let mut seen_paths = HashSet::new();
    for file in input.files {
        let path = PathBuf::from(file.path.trim());
        if path.as_os_str().is_empty()
            || path.is_absolute()
            || path
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err(ApiError::bad_request(
                "vault file path must remain relative",
            ));
        }
        if file.contents.len() > 1024 * 1024 {
            return Err(ApiError::bad_request(format!(
                "vault note is larger than 1 MiB: {}",
                path.display()
            )));
        }
        let normalized_path = path.to_string_lossy().replace('\\', "/");
        if !seen_paths.insert(normalized_path.to_ascii_lowercase()) {
            return Err(ApiError::conflict(format!(
                "vault scan contains a duplicate or case-colliding path: {normalized_path}"
            )));
        }
        files.push((normalized_path, file.contents));
    }
    let mut markdown_paths = Vec::with_capacity(input.markdown_paths.len());
    let mut seen_markdown_paths = HashSet::new();
    for value in input.markdown_paths {
        let path = PathBuf::from(value.trim());
        if path.as_os_str().is_empty()
            || path.is_absolute()
            || path.extension().and_then(|value| value.to_str()) != Some("md")
            || path
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err(ApiError::bad_request(
                "vault catalog paths must be relative Markdown paths",
            ));
        }
        let normalized_path = path.to_string_lossy().replace('\\', "/");
        if !seen_markdown_paths.insert(normalized_path.to_ascii_lowercase()) {
            return Err(ApiError::conflict(format!(
                "vault catalog contains a duplicate or case-colliding Markdown path: {normalized_path}"
            )));
        }
        markdown_paths.push(normalized_path);
    }
    if markdown_paths.is_empty() {
        markdown_paths.extend(files.iter().map(|(path, _)| path.clone()));
    }
    let mut folder_paths = vec![String::new()];
    let mut seen_folder_paths = HashSet::from([String::new()]);
    for value in input.folder_paths {
        let path = PathBuf::from(value.trim());
        if path.as_os_str().is_empty()
            || path.is_absolute()
            || path
                .components()
                .any(|component| !matches!(component, std::path::Component::Normal(_)))
        {
            return Err(ApiError::bad_request(
                "vault folder paths must remain relative",
            ));
        }
        let normalized_path = path.to_string_lossy().replace('\\', "/");
        if !seen_folder_paths.insert(normalized_path.to_ascii_lowercase()) {
            return Err(ApiError::conflict(format!(
                "vault scan contains a duplicate or case-colliding folder: {normalized_path}"
            )));
        }
        folder_paths.push(normalized_path);
    }
    folder_paths.sort();
    let vault_id = if input.vault_id.trim().is_empty() {
        format!("browser-legacy:{}", input.name.trim())
    } else {
        input.vault_id.trim().to_owned()
    };
    let catalog = state
        .vault_context
        .catalog_from_browser_files(
            &vault_id,
            input.name.trim(),
            &files,
            markdown_paths,
            folder_paths,
        )
        .map_err(ApiError::bad_request)?;
    state
        .store
        .set_preferences(&BTreeMap::from([(
            "browser_vault_catalog".to_owned(),
            serde_json::to_string(&catalog)
                .map_err(|error| ApiError::internal(error.to_string()))?,
        )]))
        .map_err(|error| ApiError::internal(error.to_string()))?;
    state
        .vault_context
        .set_browser_catalog(Some(catalog.clone()))
        .map_err(ApiError::internal)?;
    Ok(Json(catalog))
}

async fn disconnect_browser_vault(State(state): State<AppState>) -> Result<StatusCode, ApiError> {
    state
        .store
        .delete_preference("browser_vault_catalog")
        .map_err(|error| ApiError::internal(error.to_string()))?;
    state
        .vault_context
        .set_browser_catalog(None)
        .map_err(ApiError::internal)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn create_manual_log(
    State(state): State<AppState>,
    Json(input): Json<ManualLogInput>,
) -> Result<(StatusCode, Json<Value>), ApiError> {
    let message = input.message.trim();
    if message.is_empty() || message.len() > 4_000 {
        return Err(ApiError::bad_request(
            "work description must contain 1 to 4000 bytes",
        ));
    }
    if input.vault_note_ids.len() > 8 {
        return Err(ApiError::bad_request("select no more than 8 vault notes"));
    }
    let parsed_timestamp = DateTime::parse_from_rfc3339(input.timestamp.trim())
        .map_err(|_| ApiError::bad_request("timestamp must include a timezone offset"))?;
    let day = parsed_timestamp.format("%Y-%m-%d").to_string();
    let timestamp = parsed_timestamp.with_timezone(&Utc);
    let catalog = state.vault_context.catalog().map_err(ApiError::internal)?;
    let mut note_ids = Vec::new();
    let mut seen_note_ids = HashSet::new();
    for id in input.vault_note_ids {
        let id = id.trim().to_owned();
        if !id.is_empty() && seen_note_ids.insert(id.clone()) {
            note_ids.push(id);
        }
    }
    if let Some(stale) = note_ids
        .iter()
        .find(|id| !catalog.notes.iter().any(|note| &note.id == *id))
    {
        return Err(ApiError::bad_request(format!(
            "vault note is no longer available: {stale}"
        )));
    }
    let work_item = validate_manual_reference(input.work_item, "work item")?;
    let pull_request = validate_manual_reference(input.pull_request, "pull request")?;
    let task_id = format!("manual_{}", uuid::Uuid::new_v4().simple());
    let primary_note = note_ids.first().cloned();
    let mut metadata = serde_json::Map::new();
    metadata.insert("agent".to_owned(), Value::String("human".to_owned()));
    metadata.insert("entry_kind".to_owned(), Value::String("manual".to_owned()));
    metadata.insert(
        "event_type".to_owned(),
        Value::String("complete".to_owned()),
    );
    metadata.insert("status".to_owned(), Value::String("completed".to_owned()));
    metadata.insert("task_id".to_owned(), Value::String(task_id.clone()));
    metadata.insert("session_id".to_owned(), Value::String(task_id));
    metadata.insert("sequence".to_owned(), Value::from(1));
    metadata.insert(
        "canonical_note_candidates".to_owned(),
        Value::Array(note_ids.iter().cloned().map(Value::String).collect()),
    );
    if let Some(workstream) = primary_note {
        metadata.insert("workstream".to_owned(), Value::String(workstream));
    }
    if let Some(value) = work_item {
        metadata.insert("work_item".to_owned(), Value::String(value));
    }
    if let Some(value) = pull_request {
        metadata.insert("pull_request".to_owned(), Value::String(value));
    }
    let event = state
        .store
        .insert_event(LogEventInput {
            source: "manual/dashboard".to_owned(),
            level: Some("info".to_owned()),
            timestamp: Some(timestamp),
            message: message.to_owned(),
            metadata: Some(metadata),
            fingerprint: None,
        })
        .map_err(|error| ApiError::bad_request(error.to_string()))?;
    Ok((
        StatusCode::CREATED,
        Json(json!({ "id": event.id, "timestamp": event.timestamp, "day": day })),
    ))
}

fn validate_manual_reference(
    value: Option<String>,
    label: &str,
) -> Result<Option<String>, ApiError> {
    let Some(value) = value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
    else {
        return Ok(None);
    };
    if value.len() > 2_048 {
        return Err(ApiError::bad_request(format!("{label} is too long")));
    }
    if value.contains("://") {
        let parsed = reqwest::Url::parse(&value)
            .map_err(|_| ApiError::bad_request(format!("{label} URL is invalid")))?;
        if !matches!(parsed.scheme(), "http" | "https") {
            return Err(ApiError::bad_request(format!(
                "{label} URL must use http or https"
            )));
        }
    }
    Ok(Some(value))
}

async fn save_preferences(
    State(state): State<AppState>,
    Json(preferences): Json<DashboardPreferences>,
) -> Result<Json<Value>, ApiError> {
    preferences.validate()?;
    state
        .store
        .set_preferences(&preferences.to_map())
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(Json(json!({
        "preferences": preferences,
        "instructions": render_agent_instructions(&preferences),
    })))
}

async fn linking_data(State(state): State<AppState>) -> Result<Json<LinkingData>, ApiError> {
    let catalog = state.vault_context.catalog().map_err(ApiError::internal)?;
    let rules = state
        .store
        .list_link_rules()
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let events = state
        .store
        .all_events()
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let observed = state
        .vault_context
        .observed(&events, &rules)
        .map_err(ApiError::internal)?;
    let ignored = state
        .store
        .list_ignored_link_identities()
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(Json(LinkingData {
        catalog,
        rules,
        observed,
        ignored,
        event_count: events.len(),
    }))
}

async fn ignore_link_identity(
    State(state): State<AppState>,
    Json(input): Json<IgnoreIdentityInput>,
) -> Result<Json<IgnoredLinkIdentity>, ApiError> {
    let field = input.field.trim();
    let value = input.value.trim();
    if value.is_empty() {
        return Err(ApiError::bad_request("identifier value is required"));
    }
    if !vault_context::supports_selector_field(field) {
        return Err(ApiError::bad_request("unsupported identifier type"));
    }
    let rules = state
        .store
        .list_link_rules()
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let events = state
        .store
        .all_events()
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let observed = state
        .vault_context
        .observed(&events, &rules)
        .map_err(ApiError::internal)?;
    let normalized = vault_context::normalized_identity(value);
    let candidate = observed
        .iter()
        .find(|item| {
            item.field == field && vault_context::normalized_identity(&item.value) == normalized
        })
        .ok_or_else(|| ApiError::not_found("identifier not found"))?;
    if candidate.status != "unresolved" {
        return Err(ApiError::conflict(
            "only unresolved identifiers can be ignored",
        ));
    }
    state
        .store
        .ignore_link_identity(field, &candidate.value, &normalized)
        .map(Json)
        .map_err(|error| ApiError::internal(error.to_string()))
}

async fn restore_ignored_identity(
    State(state): State<AppState>,
    AxumPath(ignored_id): AxumPath<String>,
) -> Result<StatusCode, ApiError> {
    if state
        .store
        .restore_ignored_link_identity(&ignored_id)
        .map_err(|error| ApiError::internal(error.to_string()))?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("ignored identifier not found"))
    }
}

async fn create_link_rule(
    State(state): State<AppState>,
    Json(input): Json<LinkRuleInput>,
) -> Result<(StatusCode, Json<VaultLinkRule>), ApiError> {
    let now = Utc::now();
    let id = input
        .id
        .unwrap_or_else(|| format!("rule_{}", uuid::Uuid::new_v4().simple()));
    validate_rule_id(&id)?;
    let rule = VaultLinkRule {
        id,
        selectors: input.selectors,
        target_note_id: input.target_note_id,
        enabled: input.enabled,
        created_at: now,
        updated_at: now,
    };
    save_link_rule(&state, &rule)?;
    Ok((StatusCode::CREATED, Json(rule)))
}

async fn update_link_rule(
    State(state): State<AppState>,
    AxumPath(rule_id): AxumPath<String>,
    Json(input): Json<LinkRuleInput>,
) -> Result<Json<VaultLinkRule>, ApiError> {
    validate_rule_id(&rule_id)?;
    let existing = state
        .store
        .list_link_rules()
        .map_err(|error| ApiError::internal(error.to_string()))?
        .into_iter()
        .find(|rule| rule.id == rule_id)
        .ok_or_else(|| ApiError::not_found("mapping not found"))?;
    let rule = VaultLinkRule {
        id: rule_id,
        selectors: input.selectors,
        target_note_id: input.target_note_id,
        enabled: input.enabled,
        created_at: existing.created_at,
        updated_at: Utc::now(),
    };
    save_link_rule(&state, &rule)?;
    Ok(Json(rule))
}

async fn delete_link_rule(
    State(state): State<AppState>,
    AxumPath(rule_id): AxumPath<String>,
) -> Result<StatusCode, ApiError> {
    validate_rule_id(&rule_id)?;
    if state
        .store
        .delete_link_rule(&rule_id)
        .map_err(|error| ApiError::internal(error.to_string()))?
    {
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::not_found("mapping not found"))
    }
}

fn save_link_rule(state: &AppState, rule: &VaultLinkRule) -> Result<(), ApiError> {
    let catalog = state.vault_context.catalog().map_err(ApiError::internal)?;
    vault_context::validate_rule(rule, &catalog).map_err(ApiError::bad_request)?;
    let duplicate = state
        .store
        .list_link_rules()
        .map_err(|error| ApiError::internal(error.to_string()))?
        .into_iter()
        .any(|existing| {
            existing.id != rule.id
                && existing.selectors == rule.selectors
                && existing.target_note_id == rule.target_note_id
        });
    if duplicate {
        return Err(ApiError::conflict("an identical mapping already exists"));
    }
    state
        .store
        .save_link_rule(rule)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let ignored = state
        .store
        .list_ignored_link_identities()
        .map_err(|error| ApiError::internal(error.to_string()))?;
    for selector in &rule.selectors {
        let selector_value = vault_context::normalized_identity(&selector.value);
        for identity in ignored.iter().filter(|identity| {
            identity.field == selector.field
                && if selector.operator == "prefix" {
                    identity.normalized_value.starts_with(&selector_value)
                } else {
                    identity.normalized_value == selector_value
                }
        }) {
            state
                .store
                .restore_ignored_link_identity(&identity.id)
                .map_err(|error| ApiError::internal(error.to_string()))?;
        }
    }
    Ok(())
}

fn validate_rule_id(id: &str) -> Result<(), ApiError> {
    if id.len() <= 100
        && !id.is_empty()
        && id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '_' | '-'))
    {
        Ok(())
    } else {
        Err(ApiError::bad_request("invalid mapping ID"))
    }
}

async fn apply_dashboard_proposal(
    State(state): State<AppState>,
    AxumPath(proposal_id): AxumPath<String>,
) -> Result<Json<Value>, ApiError> {
    let applied = apply_proposal(&state, &proposal_id).map_err(ApiError::bad_request)?;
    Ok(Json(json!(applied)))
}

async fn prepare_browser_apply(
    State(state): State<AppState>,
    AxumPath(proposal_id): AxumPath<String>,
    Json(input): Json<BrowserApplyInput>,
) -> Result<Json<proposal_inbox::BrowserApplyPlan>, ApiError> {
    if input.current_content.len() > 2 * 1024 * 1024 {
        return Err(ApiError::bad_request(
            "daily note exceeds the 2 MiB browser apply limit",
        ));
    }
    let inbox = state.proposal_inbox.as_ref().ok_or_else(|| {
        ApiError::bad_request("proposal inbox is not configured; set LOG_INBOX_PROPOSAL_DIR")
    })?;
    inbox
        .prepare_browser_apply(&proposal_id, &input.current_content)
        .map(Json)
        .map_err(ApiError::bad_request)
}

async fn acknowledge_browser_apply(
    State(state): State<AppState>,
    AxumPath(proposal_id): AxumPath<String>,
    Json(input): Json<BrowserApplyAcknowledgement>,
) -> Result<Json<Value>, ApiError> {
    let _guard = state
        .apply_lock
        .lock()
        .map_err(|_| ApiError::internal("proposal operation lock is poisoned"))?;
    let inbox = state.proposal_inbox.as_ref().ok_or_else(|| {
        ApiError::bad_request("proposal inbox is not configured; set LOG_INBOX_PROPOSAL_DIR")
    })?;
    let selected = inbox.get(&proposal_id).map_err(ApiError::bad_request)?;
    let expected_token = format!(
        "{proposal_id}:{}:{}",
        selected.revision, input.verified_revision
    );
    if input.acknowledgement_token != expected_token {
        return Err(ApiError::conflict(
            "proposal or browser write changed since the result was prepared",
        ));
    }
    let selected_event_ids = selected
        .evidence_event_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut covered = selected
        .supersedes_proposal_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    for proposal in inbox.list().map_err(ApiError::internal)? {
        if proposal.proposal_id != proposal_id
            && proposal.target_note == selected.target_note
            && !proposal.evidence_event_ids.is_empty()
            && proposal
                .evidence_event_ids
                .iter()
                .all(|event_id| selected_event_ids.contains(event_id.as_str()))
        {
            covered.insert(proposal.proposal_id);
        }
    }
    state
        .store
        .mark_reviewed(
            &selected.evidence_event_ids,
            &format!("browser-vault/{}.md", selected.target_note),
            "proposal-browser-apply",
        )
        .map_err(|error| ApiError::internal(error.to_string()))?;
    for superseded_id in &covered {
        if superseded_id != &proposal_id {
            inbox
                .discard_if_present(superseded_id)
                .map_err(ApiError::internal)?;
        }
    }
    inbox.discard(&proposal_id).map_err(ApiError::internal)?;
    Ok(Json(
        json!({ "proposal_id": proposal_id, "status": "applied", "proposal_removed": true }),
    ))
}

async fn update_dashboard_proposal(
    State(state): State<AppState>,
    AxumPath(proposal_id): AxumPath<String>,
    Json(request): Json<UpdateProposalRequest>,
) -> Result<Json<proposal_inbox::PendingProposal>, ApiError> {
    let inbox = state.proposal_inbox.as_ref().ok_or_else(|| {
        ApiError::bad_request("proposal inbox is not configured; set LOG_INBOX_PROPOSAL_DIR")
    })?;
    match inbox.update_markdown(&proposal_id, &request.markdown, &request.expected_revision) {
        Ok(proposal) => Ok(Json(proposal)),
        Err(error) if error == "proposal changed since it was opened" => {
            Err(ApiError::conflict(error))
        }
        Err(error) => Err(ApiError::bad_request(error)),
    }
}

async fn discard_dashboard_proposal(
    State(state): State<AppState>,
    AxumPath(proposal_id): AxumPath<String>,
) -> Result<Json<Value>, ApiError> {
    let _guard = state
        .apply_lock
        .lock()
        .map_err(|_| ApiError::internal("proposal operation lock is poisoned"))?;
    let inbox = state.proposal_inbox.as_ref().ok_or_else(|| {
        ApiError::bad_request("proposal inbox is not configured; set LOG_INBOX_PROPOSAL_DIR")
    })?;
    let proposal = inbox.get(&proposal_id).map_err(ApiError::bad_request)?;
    state
        .store
        .mark_reviewed(
            &proposal.evidence_event_ids,
            "discarded proposal",
            "dashboard-discard",
        )
        .map_err(|error| ApiError::internal(error.to_string()))?;
    inbox.discard(&proposal_id).map_err(ApiError::internal)?;
    Ok(Json(json!({
        "proposal_id": proposal_id,
        "status": "discarded",
        "evidence_event_ids": proposal.evidence_event_ids,
    })))
}

async fn regenerate_dashboard_proposal(
    State(state): State<AppState>,
    AxumPath(proposal_id): AxumPath<String>,
) -> Result<(StatusCode, Json<DailyConsolidationJob>), ApiError> {
    let inbox = state
        .proposal_inbox
        .as_ref()
        .ok_or_else(|| ApiError::bad_request("proposal inbox is not configured"))?;
    let proposal = inbox.get(&proposal_id).map_err(ApiError::bad_request)?;
    let source_job_id = proposal.consolidation_job_id.ok_or_else(|| {
        ApiError::bad_request("only daily consolidation proposals can be regenerated")
    })?;
    let source_job = state
        .store
        .get_daily_consolidation_job(&source_job_id)
        .map_err(|error| ApiError::internal(error.to_string()))?
        .ok_or_else(|| ApiError::not_found("source consolidation job not found"))?;
    let events = state
        .store
        .get_daily_consolidation_events(&source_job_id)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let event_ids = events
        .iter()
        .map(|event| event.id.clone())
        .collect::<Vec<_>>();
    let rules = state
        .store
        .list_link_rules()
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let context = state
        .vault_context
        .for_events(&events, &rules)
        .map_err(ApiError::internal)?;
    let revision = context
        .get("link_context_revision")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let job = state
        .store
        .enqueue_daily_consolidation(
            source_job.start,
            source_job.end,
            &source_job.target_note,
            revision,
            &event_ids,
        )
        .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok((StatusCode::ACCEPTED, Json(job)))
}

async fn consolidate_dashboard_day(
    State(state): State<AppState>,
    Json(request): Json<DailyConsolidationRequest>,
) -> Result<(StatusCode, Json<DailyConsolidationJob>), ApiError> {
    validate_daily_consolidation_request(&request)?;
    let result = state
        .store
        .get_events_between(request.start, request.end, 500)
        .map_err(|error| ApiError::internal(error.to_string()))?;
    if result.events.is_empty() {
        return Err(ApiError::bad_request("no log events exist for this day"));
    }
    if result.truncated {
        return Err(ApiError::conflict(
            "this day contains more than 500 events; narrow the source data before consolidation",
        ));
    }
    let inbox = state.proposal_inbox.as_ref().ok_or_else(|| {
        ApiError::bad_request("proposal inbox is not configured; set LOG_INBOX_PROPOSAL_DIR")
    })?;
    let event_ids = result
        .events
        .iter()
        .map(|event| event.id.clone())
        .collect::<Vec<_>>();
    let rules = state
        .store
        .list_link_rules()
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let context = state
        .vault_context
        .for_events(&result.events, &rules)
        .map_err(ApiError::internal)?;
    let revision = context
        .get("link_context_revision")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let mut job = state
        .store
        .enqueue_daily_consolidation(
            request.start,
            request.end,
            &request.target_note,
            revision,
            &event_ids,
        )
        .map_err(|error| ApiError::internal(error.to_string()))?;
    let result_is_missing = job.status == "completed"
        && job
            .proposal_id
            .as_deref()
            .is_none_or(|proposal_id| inbox.get(proposal_id).is_err());
    if matches!(job.status.as_str(), "failed" | "cancelled") || result_is_missing {
        job = state
            .store
            .requeue_daily_consolidation(&job.id)
            .map_err(|error| ApiError::internal(error.to_string()))?
            .ok_or_else(|| ApiError::internal("daily consolidation job disappeared"))?;
    }
    let status = if matches!(
        job.status.as_str(),
        "pending" | "running" | "cancel_requested"
    ) {
        StatusCode::ACCEPTED
    } else {
        StatusCode::OK
    };
    Ok((status, Json(job)))
}

async fn cancel_dashboard_consolidation(
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Json<DailyConsolidationJob>, ApiError> {
    let job = state
        .store
        .request_daily_consolidation_cancel(&job_id)
        .map_err(|error| ApiError::internal(error.to_string()))?
        .ok_or_else(|| ApiError::not_found("daily consolidation job not found"))?;
    Ok(Json(job))
}

async fn get_dashboard_consolidation(
    State(state): State<AppState>,
    AxumPath(job_id): AxumPath<String>,
) -> Result<Json<DailyConsolidationJob>, ApiError> {
    let job = state
        .store
        .get_daily_consolidation_job(&job_id)
        .map_err(|error| ApiError::internal(error.to_string()))?
        .ok_or_else(|| ApiError::not_found("daily consolidation job not found"))?;
    Ok(Json(job))
}

fn validate_daily_consolidation_request(
    request: &DailyConsolidationRequest,
) -> Result<(), ApiError> {
    if request.start >= request.end || request.end - request.start > Duration::hours(27) {
        return Err(ApiError::bad_request(
            "daily consolidation requires a positive time window of at most 27 hours",
        ));
    }
    if request.target_note.trim().is_empty()
        || request
            .target_note
            .chars()
            .any(|character| matches!(character, '/' | '\\' | '\0'))
        || request.target_note.contains("..")
        || request.target_note.len() > 200
    {
        return Err(ApiError::bad_request(
            "target note must be a plain Markdown filename",
        ));
    }
    Ok(())
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
            let rules = state
                .store
                .list_link_rules()
                .map_err(|error| error.to_string())?;
            enrich_vault_context(&mut args, state.vault_context.for_events(&events, &rules)?);
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
            let rules = state
                .store
                .list_link_rules()
                .map_err(|error| error.to_string())?;
            enrich_vault_context(&mut args, state.vault_context.for_events(&events, &rules)?);
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
            let applied = apply_proposal(state, &args.proposal_id)?;
            Ok(tool_text(json!(applied)))
        }
        _ => Err(format!("unknown tool {name}")),
    }
}

fn apply_proposal(
    state: &AppState,
    proposal_id: &str,
) -> Result<proposal_inbox::AppliedProposal, String> {
    let _guard = state
        .apply_lock
        .lock()
        .map_err(|_| "daily-note apply lock is poisoned".to_owned())?;
    let inbox = state
        .proposal_inbox
        .as_ref()
        .ok_or_else(|| "proposal inbox is not configured; set LOG_INBOX_PROPOSAL_DIR".to_owned())?;
    let daily_notes_dir = state.daily_notes_dir.as_deref().ok_or_else(|| {
        "daily notes directory is not configured; set LOG_INBOX_DAILY_NOTES_DIR".to_owned()
    })?;
    let selected = inbox.get(proposal_id)?;
    let selected_event_ids = selected
        .evidence_event_ids
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let mut covered_proposals = selected
        .supersedes_proposal_ids
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    for proposal in inbox.list()? {
        if proposal.proposal_id != proposal_id
            && proposal.target_note == selected.target_note
            && !proposal.evidence_event_ids.is_empty()
            && proposal
                .evidence_event_ids
                .iter()
                .all(|event_id| selected_event_ids.contains(event_id.as_str()))
        {
            covered_proposals.insert(proposal.proposal_id);
        }
    }
    let mut applied = inbox.apply(proposal_id, daily_notes_dir)?;
    state
        .store
        .mark_reviewed(
            &applied.evidence_event_ids,
            &applied.daily_path.display().to_string(),
            "proposal-apply",
        )
        .map_err(|error| error.to_string())?;
    applied.supersedes_proposal_ids = covered_proposals.into_iter().collect();
    for superseded_id in &applied.supersedes_proposal_ids {
        if superseded_id != proposal_id {
            inbox.discard_if_present(superseded_id)?;
        }
    }
    inbox.discard(proposal_id)?;
    applied.proposal_removed = true;
    Ok(applied)
}

impl DashboardPreferences {
    fn load(store: &Store) -> Result<Self, ApiError> {
        let values = store
            .get_preferences()
            .map_err(|error| ApiError::internal(error.to_string()))?;
        Ok(Self {
            ingest_url: preference(
                &values,
                "ingest_url",
                env::var("LOG_INBOX_PUBLIC_INGEST_URL")
                    .unwrap_or_else(|_| "http://127.0.0.1:8787".to_owned()),
            ),
            agent_name: preference(&values, "agent_name", "codex"),
            source_prefix: preference(&values, "source_prefix", "codex"),
            default_host: preference(&values, "default_host", "windows"),
            extra_instructions: preference(&values, "extra_instructions", ""),
            daily_consolidation_prompt: daily_consolidation::configured_daily_prompt(&values),
        })
    }

    fn validate(&self) -> Result<(), ApiError> {
        if !(self.ingest_url.starts_with("http://") || self.ingest_url.starts_with("https://")) {
            return Err(ApiError::bad_request(
                "ingest URL must begin with http:// or https://",
            ));
        }
        for (name, value, maximum) in [
            ("ingest URL", &self.ingest_url, 500),
            ("agent name", &self.agent_name, 100),
            ("source prefix", &self.source_prefix, 100),
            ("default host", &self.default_host, 100),
            ("extra instructions", &self.extra_instructions, 4000),
            (
                "daily consolidation prompt",
                &self.daily_consolidation_prompt,
                4000,
            ),
        ] {
            if value.len() > maximum {
                return Err(ApiError::bad_request(format!(
                    "{name} exceeds {maximum} bytes"
                )));
            }
        }
        if self.agent_name.trim().is_empty() || self.source_prefix.trim().is_empty() {
            return Err(ApiError::bad_request(
                "agent name and source prefix are required",
            ));
        }
        Ok(())
    }

    fn to_map(&self) -> BTreeMap<String, String> {
        BTreeMap::from([
            ("ingest_url".to_owned(), self.ingest_url.clone()),
            ("agent_name".to_owned(), self.agent_name.clone()),
            ("source_prefix".to_owned(), self.source_prefix.clone()),
            ("default_host".to_owned(), self.default_host.clone()),
            (
                "extra_instructions".to_owned(),
                self.extra_instructions.clone(),
            ),
            (
                "daily_consolidation_prompt".to_owned(),
                self.daily_consolidation_prompt.clone(),
            ),
        ])
    }
}

fn preference<T: Into<String>>(values: &BTreeMap<String, String>, key: &str, default: T) -> String {
    values.get(key).cloned().unwrap_or_else(|| default.into())
}

fn render_agent_instructions(preferences: &DashboardPreferences) -> String {
    let mut instructions = include_str!("../assets/agent-instructions.md")
        .replace(
            "{{INGEST_URL}}",
            preferences.ingest_url.trim_end_matches('/'),
        )
        .replace("{{AGENT_NAME}}", preferences.agent_name.trim())
        .replace("{{SOURCE_PREFIX}}", preferences.source_prefix.trim())
        .replace("{{DEFAULT_HOST}}", preferences.default_host.trim());
    if !preferences.extra_instructions.trim().is_empty() {
        instructions.push_str("\n\n### Local additions\n\n");
        instructions.push_str(preferences.extra_instructions.trim());
        instructions.push('\n');
    }
    instructions
}

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

fn default_true() -> bool {
    true
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

#[cfg(test)]
mod knowledge_destination_tests {
    use super::*;
    use tower::ServiceExt;

    fn test_state(refocused: bool) -> AppState {
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
            proposal_inbox: None,
            daily_notes_dir: None,
            daily_notes_display_path: None,
            vault_context: vault_context::VaultContextProvider::from_env(),
            apply_lock: Arc::new(Mutex::new(())),
            daily_generation_lock: Arc::new(tokio::sync::Mutex::new(())),
            refocus: refocused.then(|| RefocusConfig {
                allowed_hosts: HashSet::from(["localhost:8788".to_owned()]),
                allowed_origins: HashSet::from(["http://localhost:8788".to_owned()]),
            }),
            workspace: refocused
                .then(|| InspectedWorkspace::inspect(&workspace_root).expect("workspace inspects")),
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
    async fn refocus_and_legacy_routes_never_coexist() {
        let refocused = build_router(test_state(true));
        assert_eq!(
            route_status(refocused.clone(), "GET", "/api/dashboard", "").await,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            route_status(refocused.clone(), "POST", "/mcp", "{}").await,
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            route_status(refocused, "GET", "/api/v2/auth/session", "").await,
            StatusCode::UNAUTHORIZED
        );

        let legacy = build_router(test_state(false));
        assert_eq!(
            route_status(legacy.clone(), "GET", "/api/v2/auth/session", "").await,
            StatusCode::NOT_FOUND
        );
        assert_ne!(
            route_status(legacy, "POST", "/mcp", "{}").await,
            StatusCode::NOT_FOUND
        );
    }

    #[tokio::test]
    async fn workspace_settings_require_csrf_and_preserve_the_active_profile_id() {
        let state = test_state(true);
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
    async fn apply_preview_is_exact_read_only_and_rejects_stale_manual_evidence() {
        let state = test_state(true);
        state
            .store
            .set_owner_secret_hash(&hash_owner_secret("owner-secret-for-tests").unwrap())
            .unwrap();
        let workspace_root = state
            .workspace
            .as_ref()
            .unwrap()
            .canonical_root()
            .to_owned();
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
        let state = test_state(true);
        let workspace = state.workspace.as_ref().unwrap();
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
        let state = test_state(true);
        let root = state
            .workspace
            .as_ref()
            .unwrap()
            .canonical_root()
            .to_owned();
        state
            .store
            .save_active_workspace_profile(
                state.workspace.as_ref().unwrap().root_binding(),
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

    #[test]
    fn treats_numbered_and_unnumbered_folder_names_as_user_owned() {
        assert_eq!(folder_label("01 Work Log"), "work log");
        assert_eq!(folder_label("Daily Notes"), "daily notes");
        assert_eq!(folder_label("03 Products"), "products");
        assert_eq!(folder_label("Knowledge/Product Areas"), "product areas");
    }

    #[test]
    fn blocks_operational_paths_and_unknown_template_tokens() {
        assert!(normalized_destination_path("00 Inbox/Log Inbox/pending").is_err());
        assert!(normalized_destination_path("../outside").is_err());
        assert!(normalized_destination_path("Knowledge/Engineering").is_ok());
        assert!(
            validate_path_template("daily_activity", "{year}/{month_name}/Daily {day}.md").is_ok()
        );
        assert!(validate_path_template("daily_activity", "{quarter}/Daily.md").is_err());
    }

    #[test]
    fn previews_only_missing_static_folders() {
        let input = KnowledgeStructureInput {
            destinations: vec![
                KnowledgeDestinationDraft {
                    role: "daily_activity".to_owned(),
                    base_path: "Work Log".to_owned(),
                    path_template: "Archive/{year}/Daily {day}.md".to_owned(),
                    enabled: true,
                },
                KnowledgeDestinationDraft {
                    role: "product_knowledge".to_owned(),
                    base_path: "Products/Platform".to_owned(),
                    path_template: String::new(),
                    enabled: true,
                },
                KnowledgeDestinationDraft {
                    role: "engineering_knowledge".to_owned(),
                    base_path: "Engineering".to_owned(),
                    path_template: String::new(),
                    enabled: true,
                },
                KnowledgeDestinationDraft {
                    role: "decision_records".to_owned(),
                    base_path: String::new(),
                    path_template: "Decisions/{date} {slug}.md".to_owned(),
                    enabled: false,
                },
                KnowledgeDestinationDraft {
                    role: "feature_recaps".to_owned(),
                    base_path: String::new(),
                    path_template: String::new(),
                    enabled: false,
                },
            ],
            catalog_revision: "revision".to_owned(),
            example_date: Some("2026-09-09".to_owned()),
        };
        let Ok(destinations) = normalize_knowledge_structure(&input) else {
            panic!("valid structure");
        };
        let catalog = vault_context::VaultCatalog {
            vault_id: "test".to_owned(),
            configured: true,
            root: Some("Vault".to_owned()),
            revision: "revision".to_owned(),
            notes: Vec::new(),
            markdown_paths: Vec::new(),
            folder_paths: vec![String::new(), "Work Log".to_owned(), "Products".to_owned()],
        };
        let preview = knowledge_structure_preview_value(
            &catalog,
            &destinations,
            NaiveDate::from_ymd_opt(2026, 9, 9).unwrap(),
            true,
        );
        assert_eq!(preview["existing_folders"], json!(["Products", "Work Log"]));
        assert_eq!(
            preview["missing_folders"],
            json!(["Engineering", "Products/Platform", "Work Log/Archive"])
        );
        assert_eq!(
            preview["examples"]["daily_activity"],
            "Work Log/Archive/2026/Daily 9.md"
        );
    }

    #[test]
    fn rejects_incomplete_or_duplicate_structures() {
        let input = KnowledgeStructureInput {
            destinations: vec![KnowledgeDestinationDraft {
                role: "daily_activity".to_owned(),
                base_path: "Work Log".to_owned(),
                path_template: "Daily.md".to_owned(),
                enabled: true,
            }],
            catalog_revision: "revision".to_owned(),
            example_date: None,
        };
        assert!(normalize_knowledge_structure(&input).is_err());
    }
}
