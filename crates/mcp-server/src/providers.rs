//! Owner-managed compatible providers. Stored credentials never enter API projections.
use super::*;
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, AeadCore, KeyInit, OsRng, Payload},
};
use llm::{LlmConfig, RequestProfile};
use std::net::{IpAddr, SocketAddr};
use uuid::Uuid;

#[derive(Clone, Serialize, Deserialize)]
struct Connection {
    id: String,
    name: String,
    base_url: String,
    #[serde(default)]
    version: u64,
    #[serde(default)]
    credential: Option<Value>,
    #[serde(default)]
    consent: bool,
    #[serde(default)]
    models: Vec<Model>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Model {
    id: String,
    profile: RequestProfile,
    tested: bool,
}
#[derive(Default, Serialize, Deserialize)]
struct Settings {
    #[serde(default)]
    connections: Vec<Connection>,
    #[serde(default)]
    default: Option<Selection>,
    #[serde(default)]
    environment_consent: Option<String>,
    #[serde(default)]
    environment_models: Option<EnvironmentModels>,
}
#[derive(Default, Serialize, Deserialize)]
struct EnvironmentModels {
    base_url: String,
    models: Vec<Model>,
}
#[derive(Clone, Serialize, Deserialize)]
struct Selection {
    connection_id: String,
    model: String,
}
#[derive(Deserialize)]
struct SaveConnection {
    name: String,
    base_url: String,
    api_key: Option<String>,
}
#[derive(Deserialize)]
struct ModelInput {
    model: String,
}
#[derive(Deserialize)]
struct ConsentInput {
    consent: bool,
}

pub(super) struct ResolvedModel {
    pub config: LlmConfig,
    pub provenance: Value,
    pub fingerprint: String,
}

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v2/settings/generation", get(metadata))
        .route("/api/v2/settings/generation/history", get(history))
        .route("/api/v2/settings/generation/connections", post(create))
        .route(
            "/api/v2/settings/generation/connections/{id}",
            axum::routing::put(update).delete(remove),
        )
        .route(
            "/api/v2/settings/generation/connections/{id}/consent",
            axum::routing::put(consent),
        )
        .route(
            "/api/v2/settings/generation/connections/{id}/models",
            get(discover).post(discover),
        )
        .route(
            "/api/v2/settings/generation/connections/{id}/models/test",
            post(test_model),
        )
        .route(
            "/api/v2/settings/generation/default",
            axum::routing::put(set_default),
        )
}
fn load(state: &AppState, workspace: &str) -> Result<Settings, ApiError> {
    let value = state
        .store
        .generation_settings(workspace)
        .map_err(internal)?;
    serde_json::from_value(value).map_err(internal)
}
fn save(state: &AppState, workspace: &str, settings: &Settings) -> Result<(), ApiError> {
    state
        .store
        .save_generation_settings(
            workspace,
            &serde_json::to_value(settings).map_err(internal)?,
        )
        .map_err(internal)
}
fn internal(error: impl std::fmt::Display) -> ApiError {
    ApiError::internal(error.to_string())
}
fn mutation_lock() -> &'static Mutex<()> {
    static LOCK: std::sync::OnceLock<Mutex<()>> = std::sync::OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}
#[cfg(test)]
thread_local! {
    // Current-thread HTTP tests inject a file source without mutating process environment.
    static TEST_KEY_FILE: std::cell::RefCell<Option<PathBuf>> = const { std::cell::RefCell::new(None) };
}
fn key() -> Result<[u8; 32], ApiError> {
    #[cfg(test)]
    if let Some(path) = TEST_KEY_FILE.with(|path| path.borrow().clone()) {
        return key_from_file(&path);
    }
    let path = env::var("LOG_INBOX_LLM_MASTER_KEY_FILE")
        .map_err(|_| recovery("Set LOG_INBOX_LLM_MASTER_KEY_FILE to store provider credentials"))?;
    key_from_file(std::path::Path::new(&path))
}
fn key_from_file(path: &std::path::Path) -> Result<[u8; 32], ApiError> {
    let bytes =
        std::fs::read(path).map_err(|_| recovery("Cannot read provider credential master key"))?;
    if let Ok(key) = <[u8; 32]>::try_from(bytes.as_slice()) {
        return Ok(key);
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| recovery("Master key must be 32 raw bytes or 64 hexadecimal characters"))?;
    decode_hex(text.trim())
        .and_then(|bytes| bytes.try_into().ok())
        .ok_or_else(|| recovery("Master key must be 32 raw bytes or 64 hexadecimal characters"))
}
fn recovery(message: &str) -> ApiError {
    ApiError::conflict(format!("{message}. Open Settings → Generation to recover."))
        .with_code("generation_configuration")
}
fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
fn decode_hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}
fn encrypt_with(key: &[u8; 32], identity: &str, secret: &str) -> Result<Value, ApiError> {
    let nonce = XChaCha20Poly1305::generate_nonce(&mut OsRng);
    let ciphertext = XChaCha20Poly1305::new(key.into())
        .encrypt(
            &nonce,
            Payload {
                msg: secret.as_bytes(),
                aad: identity.as_bytes(),
            },
        )
        .map_err(|_| recovery("Could not encrypt provider credential"))?;
    Ok(json!({"nonce":encode_hex(&nonce),"ciphertext":encode_hex(&ciphertext)}))
}
fn decrypt_with(key: &[u8; 32], identity: &str, value: &Value) -> Result<String, ApiError> {
    let nonce = value["nonce"]
        .as_str()
        .and_then(decode_hex)
        .filter(|n| n.len() == 24)
        .ok_or_else(|| recovery("Stored credential is invalid"))?;
    let ciphertext = value["ciphertext"]
        .as_str()
        .and_then(decode_hex)
        .ok_or_else(|| recovery("Stored credential is invalid"))?;
    let plaintext = XChaCha20Poly1305::new(key.into()).decrypt(XNonce::from_slice(&nonce),Payload {msg:&ciphertext,aad:identity.as_bytes()}).map_err(|_|recovery("Provider key could not be decrypted; restore the master key or re-enter this credential"))?;
    String::from_utf8(plaintext).map_err(|_| recovery("Stored credential is invalid"))
}
fn identity(workspace: &str, connection: &Connection) -> String {
    format!("{workspace}:{}:{}", connection.id, connection.base_url)
}
fn validate_url(base: &str) -> Result<reqwest::Url, ApiError> {
    let url = reqwest::Url::parse(base)
        .map_err(|_| ApiError::bad_request("Enter an absolute compatible API endpoint"))?;
    let host = url
        .host_str()
        .ok_or_else(|| ApiError::bad_request("Endpoint needs a host"))?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ApiError::bad_request(
            "Endpoint must not contain credentials, query parameters, or fragments",
        ));
    }
    let local = local_host(host);
    if url.scheme() != "https" && !(url.scheme() == "http" && local) {
        return Err(ApiError::bad_request("Nonlocal endpoints require HTTPS"));
    }
    if let Ok(ip) = host.trim_matches(['[', ']']).parse::<IpAddr>()
        && !allowed_ip(ip, local, host.eq_ignore_ascii_case("ollama"))
    {
        return Err(ApiError::bad_request("Endpoint address is not permitted"));
    }
    Ok(url)
}
fn local_host(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost")
        || host.eq_ignore_ascii_case("ollama")
        || host
            .trim_matches(['[', ']'])
            .parse::<IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}
fn allowed_ip(ip: IpAddr, local: bool, compose: bool) -> bool {
    if local && !compose {
        return ip.is_loopback();
    }
    match ip {
        IpAddr::V4(ip) => {
            if ip.is_loopback() {
                return local;
            }
            if ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_broadcast()
                || ip.octets()[0] == 0
                || ip.octets() == [168, 63, 129, 16]
                || (ip.octets()[0] == 192 && ip.octets()[1] == 0 && ip.octets()[2] == 0)
            {
                return false;
            }
            if compose {
                return ip.is_private();
            }
            !ip.is_private()
                && ip.octets()[0] < 224
                && !(ip.octets()[0] == 100 && (64..=127).contains(&ip.octets()[1]))
                && !(ip.octets()[0] == 198 && (18..=19).contains(&ip.octets()[1]))
        }
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return allowed_ip(IpAddr::V4(v4), local, compose);
            }
            if ip.is_loopback() {
                return local;
            }
            !local
                && !ip.is_unspecified()
                && !ip.is_multicast()
                && (ip.segments()[0] & 0xe000) == 0x2000
        }
    }
}
async fn pin(config: &mut LlmConfig) -> Result<(), ApiError> {
    let url = validate_url(&config.base_url)?;
    let host = url
        .host_str()
        .expect("validated host")
        .trim_matches(['[', ']']);
    let port = url.port_or_known_default().expect("validated scheme");
    let addresses: Vec<SocketAddr> = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        tokio::net::lookup_host((host, port)),
    )
    .await
    .map_err(|_| ApiError::unprocessable("Model host resolution timed out"))?
    .map_err(|_| ApiError::unprocessable("Model host could not be resolved"))?
    .collect();
    if addresses.is_empty()
        || addresses.iter().any(|address| {
            !allowed_ip(
                address.ip(),
                local_host(host),
                host.eq_ignore_ascii_case("ollama"),
            )
        })
    {
        return Err(ApiError::bad_request(
            "Model host resolved to a prohibited address",
        ));
    }
    config.pinned_addresses = addresses;
    Ok(())
}
fn config_for(
    state: &AppState,
    workspace: &str,
    settings: &Settings,
    selection: &Selection,
    require_test: bool,
) -> Result<(LlmConfig, String), ApiError> {
    if selection.model.trim().is_empty() {
        return Err(ApiError::bad_request("Select a model"));
    }
    if selection.connection_id == "environment" {
        let mut config = state
            .llm_config
            .clone()
            .ok_or_else(|| recovery("Environment connection is unavailable"))?;
        if !llm::knowledge_text_stays_local(&config)
            && settings.environment_consent.as_deref() != Some(config.base_url.as_str())
        {
            return Err(recovery(
                "Confirm cloud consent for the environment connection",
            ));
        }
        let tested = settings
            .environment_models
            .as_ref()
            .filter(|saved| saved.base_url == config.base_url)
            .and_then(|saved| {
                saved
                    .models
                    .iter()
                    .find(|model| model.id == selection.model && model.tested)
            });
        if require_test && selection.model != config.model && tested.is_none() {
            return Err(ApiError::bad_request(
                "Test this model in Settings before using it",
            ));
        }
        if let Some(tested) = tested {
            config.profile = tested.profile;
        }
        config.model = selection.model.clone();
        return Ok((config, "Environment".to_owned()));
    }
    let connection = settings
        .connections
        .iter()
        .find(|connection| connection.id == selection.connection_id)
        .ok_or_else(|| recovery("Selected connection no longer exists; choose another model"))?;
    let url = validate_url(&connection.base_url)?;
    if !local_host(url.host_str().expect("host")) && !connection.consent {
        return Err(recovery("Confirm cloud consent before sending activity"));
    }
    let model = connection
        .models
        .iter()
        .find(|model| model.id == selection.model && model.tested);
    if require_test && model.is_none() {
        return Err(ApiError::bad_request(
            "Test this model in Settings before using it",
        ));
    }
    let credential = connection
        .credential
        .as_ref()
        .map(|value| {
            key().and_then(|key| decrypt_with(&key, &identity(workspace, connection), value))
        })
        .transpose()?;
    Ok((
        LlmConfig::connection(
            connection.base_url.clone(),
            credential,
            selection.model.clone(),
            model.map(|m| m.profile).unwrap_or_default(),
        ),
        connection.name.clone(),
    ))
}
pub(super) async fn resolve(
    state: &AppState,
    workspace: &str,
    connection_id: Option<&str>,
    model: Option<&str>,
) -> Result<Option<ResolvedModel>, ApiError> {
    let settings = load(state, workspace)?;
    let selection = match (connection_id, model) {
        (Some(connection_id), Some(model)) => Selection {
            connection_id: connection_id.to_owned(),
            model: model.to_owned(),
        },
        (None, None) => match settings.default.clone() {
            Some(selection) => selection,
            None => match &state.llm_config {
                Some(config) => Selection {
                    connection_id: "environment".to_owned(),
                    model: config.model.clone(),
                },
                None => return Ok(None),
            },
        },
        _ => {
            return Err(ApiError::bad_request(
                "Provide both connection_id and model",
            ));
        }
    };
    let (mut config, name) = config_for(state, workspace, &settings, &selection, true)?;
    pin(&mut config).await?;
    let fingerprint = llm::generation_configuration_digest(&config);
    let version = settings
        .connections
        .iter()
        .find(|connection| connection.id == selection.connection_id)
        .map(|connection| connection.version);
    let provenance = json!({"connection_version":version,"connection_id":selection.connection_id,"provider":name,"model":selection.model,"profile":config.profile,"configuration_digest":fingerprint});
    Ok(Some(ResolvedModel {
        config,
        provenance,
        fingerprint,
    }))
}
async fn history(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "logs:read", false)?;
    let workspace = active_refocus_workspace(&state)?;
    let attempts = state
        .store
        .generation_history(&workspace.id, 100)
        .map_err(internal)?;
    Ok(Json(json!({"attempts":attempts,"limit":100})))
}
async fn metadata(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "logs:read", false)?;
    let workspace = active_refocus_workspace(&state)?;
    let settings = load(&state, &workspace.id)?;
    let mut connections = Vec::new();
    if let Some(config) = &state.llm_config {
        let mut models = vec![Model {
            id: config.model.clone(),
            profile: config.profile,
            tested: true,
        }];
        if let Some(saved) = settings
            .environment_models
            .as_ref()
            .filter(|saved| saved.base_url == config.base_url)
        {
            models.retain(|initial| !saved.models.iter().any(|model| model.id == initial.id));
            models.extend(saved.models.clone());
        }
        connections.push(json!({"id":"environment","name":"Environment","base_url":config.base_url,"read_only":true,"credential_present":config.api_key.is_some(),"external":!llm::knowledge_text_stays_local(config),"consent":llm::knowledge_text_stays_local(config)||settings.environment_consent.as_deref()==Some(&config.base_url),"models":models}));
    }
    for connection in &settings.connections {
        connections.push(json!({"id":connection.id,"name":connection.name,"base_url":connection.base_url,"read_only":false,"credential_present":connection.credential.is_some(),"external":validate_url(&connection.base_url).map(|url|!local_host(url.host_str().unwrap_or(""))).unwrap_or(true),"consent":connection.consent,"models":connection.models}));
    }
    let default = settings.default.or_else(|| {
        state.llm_config.as_ref().map(|config| Selection {
            connection_id: "environment".to_owned(),
            model: config.model.clone(),
        })
    });
    Ok(Json(
        json!({"connections":connections,"default":default,"credential_storage_available":key().is_ok()}),
    ))
}
async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<SaveConnection>,
) -> Result<Json<Value>, ApiError> {
    save_connection(state, headers, None, input)
}
async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(input): Json<SaveConnection>,
) -> Result<Json<Value>, ApiError> {
    save_connection(state, headers, Some(id), input)
}
fn save_connection(
    state: AppState,
    headers: HeaderMap,
    id: Option<String>,
    input: SaveConnection,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "settings:write", true)?;
    let workspace = active_refocus_workspace(&state)?;
    let url = validate_url(input.base_url.trim())?;
    if input.name.trim().is_empty()
        || input.name.len() > 100
        || input.base_url.len() > 2048
        || input.api_key.as_ref().is_some_and(|key| key.len() > 8192)
    {
        return Err(ApiError::bad_request(
            "Connection name, endpoint or credential has an invalid length",
        ));
    }
    let _lock = mutation_lock().lock().map_err(internal)?;
    let mut settings = load(&state, &workspace.id)?;
    let updating = id.is_some();
    let id = id.unwrap_or_else(|| format!("provider_{}", Uuid::new_v4().simple()));
    if id == "environment" {
        return Err(ApiError::bad_request("Environment connection is read-only"));
    }
    let previous = settings
        .connections
        .iter()
        .find(|connection| connection.id == id)
        .cloned();
    if updating && previous.is_none() {
        return Err(ApiError::bad_request("Unknown connection"));
    }
    let base_url = url.as_str().trim_end_matches('/').to_owned();
    let endpoint_changed = previous
        .as_ref()
        .is_some_and(|old| old.base_url != base_url);
    let mut connection = Connection {
        id: id.clone(),
        name: input.name.trim().to_owned(),
        base_url,
        version: previous
            .as_ref()
            .map(|old| old.version.saturating_add(1))
            .unwrap_or(1),
        credential: None,
        consent: previous.as_ref().is_some_and(|old| old.consent) && !endpoint_changed,
        models: if endpoint_changed {
            vec![]
        } else {
            previous
                .as_ref()
                .map(|old| old.models.clone())
                .unwrap_or_default()
        },
    };
    connection.credential = match input.api_key {
        Some(secret) if !secret.is_empty() => Some(encrypt_with(
            &key()?,
            &identity(&workspace.id, &connection),
            &secret,
        )?),
        Some(_) => None,
        None => match previous
            .as_ref()
            .and_then(|old| old.credential.as_ref().map(|value| (old, value)))
        {
            Some((old, value)) if endpoint_changed => Some(encrypt_with(
                &key()?,
                &identity(&workspace.id, &connection),
                &decrypt_with(&key()?, &identity(&workspace.id, old), value)?,
            )?),
            Some((_, value)) => Some(value.clone()),
            None => None,
        },
    };
    settings.connections.retain(|old| old.id != id);
    settings.connections.push(connection);
    save(&state, &workspace.id, &settings)?;
    Ok(Json(json!({"id":id,"saved":true})))
}
async fn remove(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "settings:write", true)?;
    let workspace = active_refocus_workspace(&state)?;
    let _lock = mutation_lock().lock().map_err(internal)?;
    let mut settings = load(&state, &workspace.id)?;
    if id == "environment"
        || settings
            .default
            .as_ref()
            .is_some_and(|selection| selection.connection_id == id)
    {
        return Err(ApiError::conflict(
            "Select another default before removing this connection",
        ));
    }
    settings
        .connections
        .retain(|connection| connection.id != id);
    save(&state, &workspace.id, &settings)?;
    Ok(Json(json!({"deleted":true})))
}
async fn consent(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(input): Json<ConsentInput>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "settings:write", true)?;
    let workspace = active_refocus_workspace(&state)?;
    let _lock = mutation_lock().lock().map_err(internal)?;
    let mut settings = load(&state, &workspace.id)?;
    if id == "environment" {
        settings.environment_consent = if input.consent {
            Some(
                state
                    .llm_config
                    .as_ref()
                    .ok_or_else(|| ApiError::bad_request("Environment connection is unavailable"))?
                    .base_url
                    .clone(),
            )
        } else {
            None
        };
    } else {
        settings
            .connections
            .iter_mut()
            .find(|connection| connection.id == id)
            .ok_or_else(|| ApiError::bad_request("Unknown connection"))?
            .consent = input.consent;
    }
    save(&state, &workspace.id, &settings)?;
    Ok(Json(json!({"saved":true})))
}
async fn set_default(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(selection): Json<Selection>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "settings:write", true)?;
    let workspace = active_refocus_workspace(&state)?;
    let _lock = mutation_lock().lock().map_err(internal)?;
    let mut settings = load(&state, &workspace.id)?;
    config_for(&state, &workspace.id, &settings, &selection, true)?;
    settings.default = Some(selection);
    save(&state, &workspace.id, &settings)?;
    Ok(Json(json!({"saved":true})))
}
async fn discover(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
) -> Result<Json<Value>, ApiError> {
    // Discovery sends credentials; require CSRF even for this read-shaped operation.
    authorize_refocus(&state, &headers, "settings:write", true)?;
    let workspace = active_refocus_workspace(&state)?;
    let settings = load(&state, &workspace.id)?;
    let model = if id == "environment" {
        state
            .llm_config
            .as_ref()
            .map(|config| config.model.clone())
            .unwrap_or_default()
    } else {
        "discovery".to_owned()
    };
    let (mut config, _) = config_for(
        &state,
        &workspace.id,
        &settings,
        &Selection {
            connection_id: id,
            model,
        },
        false,
    )?;
    pin(&mut config).await?;
    let client = config
        .client()
        .map_err(|_| ApiError::unprocessable("Cannot create provider client"))?;
    let mut request = client
        .get(format!("{}/models", config.base_url))
        .timeout(std::time::Duration::from_secs(15));
    if let Some(key) = config.api_key {
        request = request.bearer_auth(key);
    }
    let mut response = request.send().await.map_err(|error| {
        provider_error(if error.is_timeout() {
            "timed out"
        } else {
            "connection failed"
        })
    })?;
    if !response.status().is_success() {
        return Err(provider_error(&format!("HTTP {}", response.status())));
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| provider_error("connection failed"))?
    {
        if bytes.len() + chunk.len() > 256 * 1024 {
            return Err(provider_error("model list too large"));
        }
        bytes.extend_from_slice(&chunk);
    }
    let value: Value =
        serde_json::from_slice(&bytes).map_err(|_| provider_error("invalid model list"))?;
    let models = value["data"]
        .as_array()
        .ok_or_else(|| provider_error("invalid model list"))?
        .iter()
        .filter_map(|item| item["id"].as_str())
        .filter(|id| id.len() <= 256)
        .collect::<Vec<_>>();
    Ok(Json(json!({"models":models})))
}
fn provider_error(error: &str) -> ApiError {
    let (message, code) = if error.contains("401") || error.contains("403") {
        (
            "Provider authentication failed; check the API key",
            "provider_authentication",
        )
    } else if error.contains("404") {
        (
            "Provider or model was not found; check endpoint and model ID",
            "provider_model_missing",
        )
    } else if error.contains("timed out") {
        (
            "Provider test timed out; try again when the model is ready",
            "provider_timeout",
        )
    } else if error.contains("HTTP 5")
        || error.contains("429")
        || error.contains("connection failed")
    {
        (
            "Provider is temporarily unavailable; try again",
            "provider_unavailable",
        )
    } else {
        (
            "Model does not support the Daily structured response contract",
            "provider_incompatible",
        )
    };
    ApiError::unprocessable(message).with_code(code)
}
async fn test_model(
    State(state): State<AppState>,
    headers: HeaderMap,
    axum::extract::Path(id): axum::extract::Path<String>,
    Json(input): Json<ModelInput>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "settings:write", true)?;
    if input.model.trim().is_empty() || input.model.len() > 256 {
        return Err(ApiError::bad_request(
            "Enter a model ID up to 256 characters",
        ));
    }
    let workspace = active_refocus_workspace(&state)?;
    let settings = load(&state, &workspace.id)?;
    let (mut config, _) = config_for(
        &state,
        &workspace.id,
        &settings,
        &Selection {
            connection_id: id.clone(),
            model: input.model.clone(),
        },
        false,
    )?;
    pin(&mut config).await?;
    let base = config.base_url.clone();
    let tested_version = settings
        .connections
        .iter()
        .find(|connection| connection.id == id)
        .map(|connection| connection.version);
    let mut last_error = String::new();
    for profile in [
        RequestProfile::MaxTokens,
        RequestProfile::MaxCompletionTokens,
    ] {
        config.profile = profile;
        match llm::test_compatibility(&config).await {
            Ok(()) => {
                last_error.clear();
                break;
            }
            Err(error) => last_error = error,
        }
    }
    if !last_error.is_empty() {
        return Err(provider_error(&last_error));
    }
    {
        let _lock = mutation_lock().lock().map_err(internal)?;
        let mut settings = load(&state, &workspace.id)?;
        let models = if id == "environment" {
            if settings
                .environment_models
                .as_ref()
                .is_none_or(|saved| saved.base_url != base)
            {
                settings.environment_models = Some(EnvironmentModels {
                    base_url: base,
                    models: vec![],
                });
            }
            &mut settings
                .environment_models
                .as_mut()
                .expect("initialized environment models")
                .models
        } else {
            &mut settings
                .connections
                .iter_mut()
                .find(|connection| {
                    connection.id == id
                        && connection.base_url == base
                        && Some(connection.version) == tested_version
                })
                .ok_or_else(|| ApiError::conflict("Connection changed during test; test it again"))?
                .models
        };
        models.retain(|model| model.id != input.model);
        models.push(Model {
            id: input.model.clone(),
            profile: config.profile,
            tested: true,
        });
        save(&state, &workspace.id, &settings)?;
    }
    Ok(Json(
        json!({"tested":true,"model":input.model,"profile":config.profile}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture() -> (AppState, HeaderMap, String) {
        let root = std::env::temp_dir().join(format!("provider-api-{}", Uuid::new_v4()));
        std::fs::create_dir_all(root.join("workspace")).unwrap();
        let workspace = InspectedWorkspace::inspect(&root.join("workspace")).unwrap();
        let store = Store::open(root.join("store.sqlite")).unwrap();
        let profile = store
            .save_active_workspace_profile(
                workspace.root_binding(),
                "UTC",
                "Daily",
                "{date}.md",
                None,
                "markdown",
                None,
            )
            .unwrap();
        let credentials = generate_session_credentials();
        store
            .create_dashboard_session(
                &credentials,
                &["logs:read".into(), "settings:write".into()],
                Utc::now(),
                Duration::hours(1),
                Duration::hours(1),
            )
            .unwrap();
        let state = AppState {
            store,
            llm_config: Some(LlmConfig::for_test("http://127.0.0.1:11434/v1")),
            legacy_proposal_dir: None,
            legacy_support_files: vec![],
            apply_lock: Arc::new(Mutex::new(())),
            knowledge_write_lock: Arc::new(Mutex::new(())),
            daily_generation_lock: Arc::new(tokio::sync::Mutex::new(())),
            generation_cancel: Arc::new(Mutex::new(None)),
            refocus: RefocusConfig {
                allowed_hosts: HashSet::from(["localhost:8788".into()]),
                allowed_origins: HashSet::from(["http://localhost:8788".into()]),
            },
            workspace,
        };
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "localhost:8788".parse().unwrap());
        headers.insert(header::ORIGIN, "http://localhost:8788".parse().unwrap());
        headers.insert(
            header::COOKIE,
            format!("log_inbox_session={}", credentials.session_token)
                .parse()
                .unwrap(),
        );
        headers.insert("x-csrf-token", credentials.csrf_token.parse().unwrap());
        (state, headers, profile.id)
    }
    #[tokio::test]
    async fn generation_history_http_requires_owner_session_and_omits_private_fields() {
        use tower::ServiceExt;
        let (state, headers, workspace) = fixture();
        let attempt = state
            .store
            .create_generation_attempt(
                &workspace,
                NaiveDate::from_ymd_opt(2026, 10, 15).unwrap(),
                "manual",
                1200,
                Utc::now(),
            )
            .unwrap();
        let id = attempt["id"].as_str().unwrap();
        state.store.set_generation_attempt_provenance(id,&json!({"provider":"Local","model":"test-model","api_key":"private-http-history-sentinel"})).unwrap();
        state
            .store
            .finish_generation_attempt(
                id,
                "failed",
                Some("private-http-history-sentinel"),
                Utc::now(),
            )
            .unwrap();
        let scoped = generate_session_credentials();
        state
            .store
            .create_dashboard_session(
                &scoped,
                &["settings:write".to_owned()],
                Utc::now(),
                Duration::hours(1),
                Duration::hours(1),
            )
            .unwrap();
        let app = routes().with_state(state);
        let make_request = |authenticated: bool| {
            let mut request = axum::http::Request::builder()
                .uri("/api/v2/settings/generation/history")
                .header(header::HOST, "localhost:8788");
            if authenticated {
                request.headers_mut().unwrap().extend(headers.clone());
            }
            request.body(axum::body::Body::empty()).unwrap()
        };
        let response = app.clone().oneshot(make_request(false)).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let mut denied = make_request(true);
        denied.headers_mut().insert(
            header::COOKIE,
            format!("log_inbox_session={}", scoped.session_token)
                .parse()
                .unwrap(),
        );
        let response = app.clone().oneshot(denied).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        let response = app.oneshot(make_request(true)).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let value: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(value["limit"], 100);
        assert_eq!(
            value["attempts"][0]["model_selection"]["model"],
            "test-model"
        );
        assert!(value["attempts"][0].get("error").is_none());
        assert!(
            !std::str::from_utf8(&bytes)
                .unwrap()
                .contains("private-http-history-sentinel")
        );
    }
    #[tokio::test(flavor = "current_thread")]
    async fn missing_and_wrong_master_key_files_recover_through_http_without_losing_drafts() {
        use tower::ServiceExt;
        let (state, headers, workspace) = fixture();
        let directory = state.store.database_path().parent().unwrap();
        let key_path = directory.join("provider-master-key");
        struct ResetKeySource;
        impl Drop for ResetKeySource {
            fn drop(&mut self) {
                TEST_KEY_FILE.with(|path| *path.borrow_mut() = None);
            }
        }
        TEST_KEY_FILE.with(|path| *path.borrow_mut() = Some(key_path.clone()));
        let _reset = ResetKeySource;
        let encryption_key = [17; 32];
        let secret = "secret-provider-token-for-recovery-test";
        let mut connection = Connection {
            id: "encrypted-provider".to_owned(),
            name: "Encrypted provider".to_owned(),
            base_url: "http://localhost:11434/v1".to_owned(),
            version: 1,
            credential: None,
            consent: false,
            models: vec![Model {
                id: "model".to_owned(),
                profile: RequestProfile::MaxTokens,
                tested: true,
            }],
        };
        connection.credential = Some(
            encrypt_with(&encryption_key, &identity(&workspace, &connection), secret).unwrap(),
        );
        save(
            &state,
            &workspace,
            &Settings {
                connections: vec![connection],
                ..Settings::default()
            },
        )
        .unwrap();
        let date = NaiveDate::from_ymd_opt(2026, 10, 15).unwrap();
        state
            .store
            .ensure_daily_day(date, "Daily/2026-10-15.md", None)
            .unwrap();
        let note = state
            .store
            .create_manual_daily_entry(&workspace, date, "Existing reviewed draft", &[])
            .unwrap();
        let content = json!({"schema_version":1,"workstreams":[],"manual_entry_ids":[note.id],"open_questions":[]});
        let revision = state
            .store
            .create_proposal_revision(&workspace, date, None, "manual", &content)
            .unwrap();
        let app = routes().with_state(state.clone());
        async fn request(
            app: Router,
            headers: &HeaderMap,
            method: &str,
            path: &str,
            body: Value,
        ) -> (StatusCode, Value) {
            let mut request = axum::http::Request::builder()
                .method(method)
                .uri(path)
                .header(header::CONTENT_TYPE, "application/json");
            request.headers_mut().unwrap().extend(headers.clone());
            let response = app
                .oneshot(
                    request
                        .body(axum::body::Body::from(body.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            let status = response.status();
            let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
                .await
                .unwrap();
            (status, serde_json::from_slice(&bytes).unwrap())
        }
        let selection = json!({"connection_id":"encrypted-provider","model":"model"});
        for wrong_file in [false, true] {
            if wrong_file {
                std::fs::write(&key_path, [18; 32]).unwrap();
            }
            let (status, error) = request(
                app.clone(),
                &headers,
                "PUT",
                "/api/v2/settings/generation/default",
                selection.clone(),
            )
            .await;
            assert_eq!(status, StatusCode::CONFLICT);
            assert_eq!(error["code"], "generation_configuration");
            assert!(error["error"].as_str().unwrap().contains("Settings"));
            assert!(!error.to_string().contains(secret));
            assert!(!error.to_string().contains(key_path.to_str().unwrap()));
            assert_eq!(
                state
                    .store
                    .daily_day(&workspace, date)
                    .unwrap()
                    .unwrap()
                    .current_revision_id,
                Some(revision.id.clone())
            );
            assert_eq!(
                state.store.manual_daily_entries(&workspace, date).unwrap()[0].text,
                "Existing reviewed draft"
            );
            assert!(load(&state, &workspace).unwrap().default.is_none());
        }
        std::fs::write(&key_path, encode_hex(&encryption_key)).unwrap();
        let (status, _) = request(
            app.clone(),
            &headers,
            "PUT",
            "/api/v2/settings/generation/default",
            selection,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, metadata) = request(
            app,
            &headers,
            "GET",
            "/api/v2/settings/generation",
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(metadata["credential_storage_available"], true);
        assert!(!metadata.to_string().contains(secret));
        assert_eq!(
            load(&state, &workspace)
                .unwrap()
                .default
                .unwrap()
                .connection_id,
            "encrypted-provider"
        );
    }
    #[tokio::test]
    async fn settings_require_csrf_and_preserve_environment_model_selection() {
        let (state, headers, workspace) = fixture();
        let mut no_csrf = headers.clone();
        no_csrf.remove("x-csrf-token");
        let input = || SaveConnection {
            name: "Local".into(),
            base_url: "http://localhost:11434/v1".into(),
            api_key: None,
        };
        assert!(save_connection(state.clone(), no_csrf, None, input()).is_err());
        let saved = save_connection(state.clone(), headers.clone(), None, input())
            .unwrap()
            .0;
        let id = saved["id"].as_str().unwrap();
        let mut settings = load(&state, &workspace).unwrap();
        settings.environment_models = Some(EnvironmentModels {
            base_url: state.llm_config.as_ref().unwrap().base_url.clone(),
            models: vec![Model {
                id: "alternate".into(),
                profile: RequestProfile::MaxCompletionTokens,
                tested: true,
            }],
        });
        save(&state, &workspace, &settings).unwrap();
        let selection = Selection {
            connection_id: "environment".into(),
            model: "alternate".into(),
        };
        let (config, _) = config_for(&state, &workspace, &settings, &selection, true).unwrap();
        assert_eq!(config.model, "alternate");
        assert_eq!(config.profile, RequestProfile::MaxCompletionTokens);
        let _ = set_default(State(state.clone()), headers.clone(), Json(selection))
            .await
            .unwrap();
        let visible = metadata(State(state.clone()), headers.clone())
            .await
            .unwrap()
            .0;
        assert_eq!(visible["default"]["model"], "alternate");
        assert!(
            visible["connections"]
                .as_array()
                .unwrap()
                .iter()
                .all(|connection| connection.get("credential").is_none())
        );
        assert!(
            remove(
                State(state.clone()),
                headers.clone(),
                axum::extract::Path("environment".to_owned())
            )
            .await
            .is_err()
        );
        let _ = remove(
            State(state.clone()),
            headers,
            axum::extract::Path(id.to_owned()),
        )
        .await
        .unwrap();
    }
    #[test]
    fn cloud_consent_is_required_and_invalidated_by_endpoint_changes() {
        let (state, headers, workspace) = fixture();
        let saved = save_connection(
            state.clone(),
            headers.clone(),
            None,
            SaveConnection {
                name: "Cloud".into(),
                base_url: "https://api.example.com/v1".into(),
                api_key: None,
            },
        )
        .unwrap()
        .0;
        let id = saved["id"].as_str().unwrap();
        let mut settings = load(&state, &workspace).unwrap();
        let selection = Selection {
            connection_id: id.to_owned(),
            model: "cloud".into(),
        };
        assert!(config_for(&state, &workspace, &settings, &selection, false).is_err());
        settings.connections[0].consent = true;
        settings.connections[0].models.push(Model {
            id: "cloud".into(),
            profile: RequestProfile::MaxTokens,
            tested: true,
        });
        save(&state, &workspace, &settings).unwrap();
        assert!(config_for(&state, &workspace, &settings, &selection, true).is_ok());
        let _ = save_connection(
            state.clone(),
            headers,
            Some(id.to_owned()),
            SaveConnection {
                name: "Cloud".into(),
                base_url: "https://other.example.com/v1".into(),
                api_key: None,
            },
        )
        .unwrap();
        let settings = load(&state, &workspace).unwrap();
        assert!(!settings.connections[0].consent);
        assert!(settings.connections[0].models.is_empty());
    }
    #[tokio::test]
    async fn compatibility_uses_production_contract_and_request_profiles() {
        use axum::routing::post;
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let counted = calls.clone();
        let app=Router::new().route("/chat/completions",post(move |Json(body):Json<Value>| {
            let counted=counted.clone();
            async move {
                counted.fetch_add(1,std::sync::atomic::Ordering::SeqCst);
                assert_eq!(body["response_format"]["type"],"json_schema");
                assert!(body["messages"][1]["content"].as_str().unwrap().contains("synthetic connection test"));
                if body.get("max_tokens").is_some() {
                    assert!(body.get("temperature").is_some());
                    return (StatusCode::BAD_REQUEST,Json(json!({"error":"use completion tokens"})));
                }
                assert_eq!(body["max_completion_tokens"],512);
                assert!(body.get("temperature").is_none());
                (StatusCode::OK,Json(json!({"choices":[{"message":{"content":json!({"title":"Connection validation","outcome":["Validated synthetic connection"],"decision":["Selected the standard library"],"trade_off":[],"validation":["Validated the connection test"],"blocker":[],"follow_up":[],"open_questions":[]}).to_string()}}]})))
            }
        }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let mut config = LlmConfig::connection(
            format!("http://{address}"),
            None,
            "synthetic-model".to_owned(),
            RequestProfile::MaxTokens,
        );
        pin(&mut config).await.unwrap();
        assert!(llm::test_compatibility(&config).await.is_err());
        config.profile = RequestProfile::MaxCompletionTokens;
        llm::test_compatibility(&config).await.unwrap();
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 2);
        server.abort();
    }
    #[test]
    fn credentials_are_authenticated_and_bound_to_connection() {
        let key = [7; 32];
        let secret = "private-provider-credential";
        let first = encrypt_with(&key, "workspace:one", secret).unwrap();
        let second = encrypt_with(&key, "workspace:one", secret).unwrap();
        assert_ne!(first, second);
        assert!(!first.to_string().contains(secret));
        assert_eq!(decrypt_with(&key, "workspace:one", &first).unwrap(), secret);
        assert!(decrypt_with(&key, "workspace:two", &first).is_err());
        assert!(decrypt_with(&[8; 32], "workspace:one", &first).is_err());
    }
    #[test]
    fn endpoint_policy_preserves_local_and_blocks_unsafe_destinations() {
        for endpoint in [
            "http://localhost:11434/v1",
            "http://127.0.0.1:3000/v1",
            "http://ollama:11434/v1",
            "https://api.example.com/v1",
        ] {
            assert!(validate_url(endpoint).is_ok(), "{endpoint}");
        }
        for endpoint in [
            "http://api.example.com/v1",
            "https://user:pass@api.example.com/v1",
            "https://169.254.169.254/v1",
            "https://10.0.0.1/v1",
            "https://[::ffff:169.254.169.254]/v1",
            "https://api.example.com/v1?api_key=secret",
        ] {
            assert!(validate_url(endpoint).is_err(), "{endpoint}");
        }
        assert!(!allowed_ip("10.0.0.1".parse().unwrap(), false, false));
        assert!(!allowed_ip("169.254.169.254".parse().unwrap(), true, true));
        assert!(!allowed_ip(
            "::ffff:127.0.0.1".parse().unwrap(),
            false,
            false
        ));
        assert!(allowed_ip("172.20.0.2".parse().unwrap(), true, true));
    }
}
