//! Dashboard-only materialization and review of producer submissions.
use super::*;
use log_inbox_core::knowledge_capture::KnowledgeDispositionKind;

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v2/knowledge/submissions", get(submissions))
        .route("/api/v2/knowledge/suggestions", get(suggestions))
        .route(
            "/api/v2/knowledge/suggestions/{id}/capture",
            post(capture_suggestion),
        )
        .route(
            "/api/v2/knowledge/candidates",
            get(candidates).post(materialize),
        )
        .route("/api/v2/knowledge/candidates/{id}", get(candidate))
        .route("/api/v2/knowledge/candidates/{id}/review", post(review))
}

// Store diagnostics must never echo producer payloads into dashboard errors.
fn capture_error(error: anyhow::Error) -> ApiError {
    let message = error.to_string();
    if message.contains("not found")
        || message.contains("not available")
        || message.contains("missing or expired")
        || message.contains("does not belong to workspace")
    {
        ApiError::not_found("The submission or candidate is unavailable.")
    } else if message.contains("conflict")
        || message.contains("stale")
        || message.contains("superseded")
        || message.contains("evidence")
        || message.contains("already reviewed")
        || message.contains("hash changed")
        || message.contains("immutable owner disposition")
        || message.contains("already bound")
    {
        ApiError::conflict(
            "The candidate changed or is no longer eligible. Reload before reviewing.",
        )
    } else {
        ApiError::unprocessable("The submission or review is invalid.")
    }
}

async fn submissions(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "logs:read", false)?;
    let profile = active_refocus_workspace(&state)?;
    let result = state
        .store
        .list_knowledge_submission_events(&profile.id, 100)
        .map_err(|_| ApiError::internal("Submissions could not be loaded."))?;
    let considered_count = result.events.len();
    let events = result.events;
    Ok(Json(
        json!({"events": events, "considered_count": considered_count, "limit": result.limit, "truncated": result.truncated}),
    ))
}

async fn suggestions(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "logs:read", false)?;
    let profile = active_refocus_workspace(&state)?;
    let result = state
        .store
        .list_knowledge_suggestions(&profile.id, 100)
        .map_err(capture_error)?;
    Ok(Json(serde_json::to_value(result).map_err(|_| {
        ApiError::internal("Suggestions could not be loaded.")
    })?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CaptureSuggestionInput {
    expected_suggestion_hash: String,
    product: String,
    modules: Vec<String>,
}
async fn capture_suggestion(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Json(input): Json<CaptureSuggestionInput>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "review:write", true)?;
    authorize_refocus(&state, &headers, "logs:read", false)?;
    let profile = active_refocus_workspace(&state)?;
    let _guard = state
        .knowledge_write_lock
        .lock()
        .map_err(|_| ApiError::internal("Knowledge review lock unavailable."))?;
    let candidate = state
        .store
        .capture_knowledge_suggestion(
            &profile.id,
            &id,
            &input.expected_suggestion_hash,
            &input.product,
            &input.modules,
        )
        .map_err(capture_error)?;
    Ok(Json(json!({"candidate":candidate})))
}

async fn candidates(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "knowledge:read", false)?;
    let profile = active_refocus_workspace(&state)?;
    let candidates = state
        .store
        .list_knowledge_candidates(&profile.id, 100)
        .map_err(capture_error)?;
    let truncated = candidates.len() == 100;
    Ok(Json(
        json!({"candidates": candidates, "limit":100, "truncated":truncated}),
    ))
}

async fn candidate(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "knowledge:read", false)?;
    let profile = active_refocus_workspace(&state)?;
    let candidate = state
        .store
        .knowledge_candidate(&profile.id, &id)
        .map_err(capture_error)?
        .ok_or_else(|| ApiError::not_found("Candidate not found."))?;
    Ok(Json(json!({"candidate":candidate})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MaterializeInput {
    source_event_id: String,
}

async fn materialize(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<MaterializeInput>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "review:write", true)?;
    authorize_refocus(&state, &headers, "logs:read", false)?;
    active_refocus_workspace(&state)?;
    let _guard = state
        .knowledge_write_lock
        .lock()
        .map_err(|_| ApiError::internal("Knowledge review lock unavailable."))?;
    let candidate = state
        .store
        .materialize_knowledge_submission(&input.source_event_id)
        .map_err(capture_error)?;
    Ok(Json(json!({"candidate":candidate})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewInput {
    expected_candidate_hash: String,
    disposition: KnowledgeDispositionKind,
    reason: String,
}

async fn review(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Json(input): Json<ReviewInput>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "knowledge:read", true)?;
    authorize_refocus(&state, &headers, "review:write", true)?;
    let profile = active_refocus_workspace(&state)?;
    let _guard = state
        .knowledge_write_lock
        .lock()
        .map_err(|_| ApiError::internal("Knowledge review lock unavailable."))?;
    let disposition = state
        .store
        .review_knowledge_candidate(
            &profile.id,
            &id,
            &input.expected_candidate_hash,
            input.disposition,
            &input.reason,
        )
        .map_err(capture_error)?;
    Ok(Json(json!({"review":disposition})))
}

#[cfg(test)]
mod tests {
    use super::*;
    use log_inbox_core::models::LogEventInput;
    use tower::ServiceExt;

    fn state() -> AppState {
        let path = std::env::temp_dir().join(format!("kb-api-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&path).unwrap();
        let workspace = InspectedWorkspace::inspect(&path).unwrap();
        let store = Store::open(path.join("store.sqlite3")).unwrap();
        store
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
        AppState {
            store,
            workspace,
            llm_config: None,
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
        }
    }
    fn session(state: &AppState, scopes: &[&str]) -> log_inbox_core::auth::SessionCredentials {
        let credentials = generate_session_credentials();
        state
            .store
            .create_dashboard_session(
                &credentials,
                &scopes.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                Utc::now(),
                Duration::hours(1),
                Duration::hours(1),
            )
            .unwrap();
        credentials
    }
    async fn request(
        app: Router,
        method: &str,
        path: &str,
        body: Value,
        session: Option<&log_inbox_core::auth::SessionCredentials>,
        csrf: bool,
        boundary: (&str, &str),
    ) -> Response {
        let mut request = Request::builder()
            .method(method)
            .uri(path)
            .header(header::HOST, boundary.0)
            .header(header::ORIGIN, boundary.1)
            .header(header::CONTENT_TYPE, "application/json");
        if let Some(session) = session {
            request = request.header(
                header::COOKIE,
                format!("log_inbox_session={}", session.session_token),
            );
            if csrf {
                request = request.header("X-CSRF-Token", &session.csrf_token);
            }
        }
        app.oneshot(request.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap()
    }
    async fn call(
        app: Router,
        method: &str,
        path: &str,
        body: Value,
        credentials: &log_inbox_core::auth::SessionCredentials,
    ) -> Response {
        request(
            app,
            method,
            path,
            body,
            Some(credentials),
            true,
            ("localhost:8788", "http://localhost:8788"),
        )
        .await
    }
    async fn json(response: Response) -> Value {
        serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), 1_000_000)
                .await
                .unwrap(),
        )
        .unwrap()
    }

    #[tokio::test]
    async fn knowledge_activity_suggestion_api_keeps_owner_and_source_boundaries() {
        let state = state();
        let owner = session(&state, &["logs:read", "knowledge:read", "review:write"]);
        let reader = session(&state, &["logs:read"]);
        let reviewer = session(&state, &["review:write"]);
        let e = state
            .store
            .insert_event(LogEventInput {
                source: "codex/host".into(),
                level: None,
                timestamp: None,
                message: "Investigation complete; implementation not started".into(),
                metadata: Some(
                    json!({"event_type":"complete", "outcome":"Likely cause; unverified"})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
                fingerprint: None,
            })
            .unwrap();
        let app = build_router(state.clone());
        let list = "/api/v2/knowledge/suggestions";
        assert_eq!(
            call(app.clone(), "GET", list, json!({}), &reviewer)
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let suggestion =
            json(call(app.clone(), "GET", list, json!({}), &reader).await).await["suggestions"][0]
                .clone();
        assert_eq!(suggestion["submission"]["claims"][0]["kind"], "reported");
        let path = format!("{}/{}/capture", list, e.id);
        let body = json!({"expected_suggestion_hash":suggestion["suggestion_hash"], "product":"Log Inbox", "modules":["knowledge"]});
        for credentials in [&reader, &reviewer] {
            assert_eq!(
                call(app.clone(), "POST", &path, body.clone(), credentials)
                    .await
                    .status(),
                StatusCode::UNAUTHORIZED
            );
        }
        assert_eq!(
            request(
                app.clone(),
                "POST",
                &path,
                body.clone(),
                None,
                false,
                ("localhost:8788", "http://localhost:8788")
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        for (csrf, host, origin) in [
            (false, "localhost:8788", "http://localhost:8788"),
            (true, "hostile.test", "http://localhost:8788"),
            (true, "localhost:8788", "http://hostile.test"),
        ] {
            assert_eq!(
                request(
                    app.clone(),
                    "POST",
                    &path,
                    body.clone(),
                    Some(&owner),
                    csrf,
                    (host, origin)
                )
                .await
                .status(),
                StatusCode::FORBIDDEN
            );
        }
        let mut stale = body.clone();
        stale["expected_suggestion_hash"] = json!("stale");
        assert_eq!(
            call(app.clone(), "POST", &path, stale, &owner)
                .await
                .status(),
            StatusCode::CONFLICT
        );
        let mut missing = body.clone();
        missing["product"] = json!("");
        assert_eq!(
            call(app.clone(), "POST", &path, missing, &owner)
                .await
                .status(),
            StatusCode::UNPROCESSABLE_ENTITY
        );
        let candidate =
            json(call(app.clone(), "POST", &path, body.clone(), &owner).await).await["candidate"]
                .clone();
        assert!(candidate["review"].is_null());
        assert_eq!(candidate["source_event_id"], e.id);
        assert_eq!(
            json(call(app.clone(), "POST", &path, body, &owner).await).await["candidate"]["id"],
            candidate["id"]
        );
        assert!(!state.workspace.canonical_root().join("Daily").exists());
    }

    #[tokio::test]
    async fn knowledge_capture_api_requires_owner_scopes_csrf_and_exact_review() {
        let state = state();
        let owner = session(&state, &["logs:read", "knowledge:read", "review:write"]);
        let reader = session(&state, &["knowledge:read"]);
        let reviewer = session(&state, &["review:write"]);
        let payload = json!({"schema_version":1,"submission_key":"s1","plan_id":"p1","plan_revision":"r1","product":"Log Inbox","modules":["knowledge"],"title":"Capture original plans","summary":"Accepted plan; not implementation evidence.","claims":[{"kind":"accepted","text":"Owner accepted plan"}]});
        let insert = |payload| {
            state
                .store
                .insert_event(LogEventInput {
                    source: "codex/test".into(),
                    level: None,
                    timestamp: None,
                    message: "Completed planning".into(),
                    metadata: Some(
                        json!({"knowledge_submission":payload})
                            .as_object()
                            .unwrap()
                            .clone(),
                    ),
                    fingerprint: None,
                })
                .unwrap()
        };
        let event = insert(payload.clone());
        let app = build_router(state.clone());
        let path = "/api/v2/knowledge/candidates";
        let body = json!({"source_event_id":event.id});
        assert_eq!(
            request(
                app.clone(),
                "POST",
                path,
                body.clone(),
                None,
                false,
                ("localhost:8788", "http://localhost:8788")
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        let bearer_only = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header(header::HOST, "localhost:8788")
                    .header(header::ORIGIN, "http://localhost:8788")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header(header::AUTHORIZATION, "Bearer ingestion-key")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(bearer_only.status(), StatusCode::UNAUTHORIZED);
        for session in [&reader, &reviewer] {
            assert_eq!(
                call(app.clone(), "POST", path, body.clone(), session)
                    .await
                    .status(),
                StatusCode::UNAUTHORIZED
            );
        }
        for (csrf, host, origin) in [
            (false, "localhost:8788", "http://localhost:8788"),
            (true, "hostile.test", "http://localhost:8788"),
            (true, "localhost:8788", "http://hostile.test"),
        ] {
            assert_eq!(
                request(
                    app.clone(),
                    "POST",
                    path,
                    body.clone(),
                    Some(&owner),
                    csrf,
                    (host, origin)
                )
                .await
                .status(),
                StatusCode::FORBIDDEN
            );
        }
        let candidate =
            json(call(app.clone(), "POST", path, body.clone(), &owner).await).await["candidate"]
                .clone();
        assert!(candidate["review"].is_null());
        assert_eq!(
            json(call(app.clone(), "POST", path, body, &owner).await).await["candidate"]["id"],
            candidate["id"]
        );
        let review_path = format!("{}/{}/review", path, candidate["id"].as_str().unwrap());
        let review = json!({"expected_candidate_hash":"stale", "disposition":"accept", "reason":"Reviewed original evidence"});
        assert_eq!(
            call(app.clone(), "POST", &review_path, review.clone(), &owner)
                .await
                .status(),
            StatusCode::CONFLICT
        );
        let review = json!({"expected_candidate_hash":candidate["candidate_hash"], "disposition":"accept", "reason":"Reviewed original evidence"});
        assert_eq!(
            call(app.clone(), "POST", &review_path, review.clone(), &reviewer)
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            call(app.clone(), "POST", &review_path, review.clone(), &owner)
                .await
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            call(app.clone(), "POST", &review_path, review, &owner)
                .await
                .status(),
            StatusCode::OK
        );
        assert_eq!(call(app.clone(), "POST", &review_path, json!({"expected_candidate_hash":candidate["candidate_hash"], "disposition":"reject", "reason":"Changed disposition"}), &owner).await.status(), StatusCode::CONFLICT);
        let mut conflicting = payload;
        conflicting["summary"] = json!("Conflicting payload under the same key");
        let conflict = insert(conflicting);
        assert_eq!(
            call(
                app.clone(),
                "POST",
                path,
                json!({"source_event_id":conflict.id}),
                &owner
            )
            .await
            .status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            call(app.clone(), "GET", path, json!({}), &reader)
                .await
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            call(
                app.clone(),
                "GET",
                "/api/v2/knowledge/submissions",
                json!({}),
                &reader
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        assert!(!state.workspace.canonical_root().join("Daily").exists());
    }
}
