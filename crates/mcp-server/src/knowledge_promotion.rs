//! Explicit KB preview/confirmation over the shared capability-based atomic writer.
use super::*;
use daily_writer::ManagedBlockKind;
use log_inbox_core::{
    knowledge_capture::KnowledgeCandidate, knowledge_promotion::KnowledgePromotion,
};

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v2/knowledge/candidates/{id}/promotion/preview",
            post(preview),
        )
        .route("/api/v2/knowledge/promotions", get(list))
        .route(
            "/api/v2/knowledge/promotions/{id}",
            get(detail).delete(discard),
        )
        .route("/api/v2/knowledge/promotions/{id}/confirm", post(confirm))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PreviewRequest {
    candidate_hash: String,
    destination_path: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfirmRequest {
    preview_hash: String,
}

fn conflict(e: impl std::fmt::Display) -> ApiError {
    ApiError::conflict(e.to_string())
}

fn authorize(state: &AppState, headers: &HeaderMap, mutation: bool) -> Result<(), ApiError> {
    authorize_refocus(state, headers, "knowledge:read", mutation)?;
    if mutation {
        authorize_refocus(state, headers, "vault:write", true)?;
    }
    Ok(())
}

fn target_configuration(
    state: &AppState,
    profile: &WorkspaceProfile,
    workspace: &InspectedWorkspace,
    path: &str,
) -> Result<String, ApiError> {
    let collections = state
        .store
        .list_knowledge_collections(&profile.id)
        .map_err(conflict)?;
    knowledge::find_note_option(workspace, &collections, path)
        .map_err(conflict)?
        .ok_or_else(|| {
            conflict("Choose an existing Markdown note in an enabled Reference collection")
        })?;
    workspace
        .resolve_markdown_path(Path::new(path), MarkdownPathMode::ExistingFile)
        .map_err(conflict)?;
    knowledge::configuration_digest(workspace, &collections, &[]).map_err(conflict)
}

// Submissions are plain text. They cannot create Markdown links, HTML or managed markers.
fn plain(value: &str) -> String {
    let mut text = String::new();
    for c in value.chars() {
        match c {
            '<' => text.push_str("&lt;"),
            '>' => text.push_str("&gt;"),
            '&' => text.push_str("&amp;"),
            '\\' | '`' | '*' | '_' | '[' | ']' | '(' | ')' | '#' | '!' | '|' => {
                text.push('\\');
                text.push(c);
            }
            _ => text.push(c),
        }
    }
    text
}

fn render(candidate: &KnowledgeCandidate) -> String {
    let s = &candidate.submission;
    let mut out = format!(
        "## {}\n\n{}\n\nProduct: {} · Modules: {}\n\nPlan: {} · Revision: {}\n\nOwner reviewed for KB inclusion. Claims retain their reported scope.\n",
        plain(&s.title),
        plain(&s.summary),
        plain(&s.product),
        s.modules
            .iter()
            .map(|m| plain(m))
            .collect::<Vec<_>>()
            .join(", "),
        plain(&s.plan_id),
        plain(&s.plan_revision)
    );
    for claim in &s.claims {
        let kind = serde_json::to_value(&claim.kind).expect("claim enum serializes");
        out.push_str(&format!(
            "\n- Reported {}: {}",
            kind.as_str().unwrap_or("claim"),
            plain(&claim.text)
        ));
        if let Some(scope) = &claim.verification_scope {
            out.push_str(&format!(" Scope: {}", plain(scope)));
        }
        if let Some(a) = &claim.reported_acceptance {
            out.push_str(&format!(
                " Reported acceptance: {} via {} at {}.",
                plain(&a.actor),
                plain(&a.source),
                a.at
            ));
        }
    }
    for (label, values) in [
        ("Decisions", &s.decisions),
        ("Constraints", &s.constraints),
        ("Open questions", &s.questions),
        ("Commits", &s.commits),
        ("Pull requests", &s.pull_requests),
    ] {
        if !values.is_empty() {
            out.push_str(&format!("\n\n### {label}\n"));
            for value in values {
                out.push_str(&format!("\n- {}", plain(value)));
            }
        }
    }
    if !s.artifacts.is_empty() {
        out.push_str("\n\n### Referenced artifacts\n");
        for artifact in &s.artifacts {
            out.push_str(&format!(
                "\n- {} · SHA-256 {}",
                plain(&artifact.path),
                artifact.sha256
            ));
        }
    }
    out.push_str(&format!(
        "\n\nSource: {} · Event: {} · Event SHA-256: {}\nCandidate: {} · Content SHA-256: {}\n",
        plain(&candidate.producer_source),
        candidate.source_event_id,
        candidate.source_event_digest,
        candidate.id,
        candidate.candidate_hash
    ));
    out
}

async fn preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Json(input): Json<PreviewRequest>,
) -> Result<Json<Value>, ApiError> {
    authorize(&state, &headers, true)?;
    let _guard = state.knowledge_write_lock.lock().map_err(conflict)?;
    let (profile, workspace) = active_refocus_context(&state)?;
    let candidate = state
        .store
        .require_promotable_knowledge_candidate(&profile.id, &id, &input.candidate_hash)
        .map_err(conflict)?;
    let configuration_digest =
        target_configuration(&state, &profile, &workspace, &input.destination_path)?;
    let original =
        daily_writer::read_file(workspace.directory(), Path::new(&input.destination_path))
            .map_err(conflict)?
            .ok_or_else(|| conflict("KB target must already exist"))?;
    let identity = serde_json::to_vec(&json!([
        profile.id,
        candidate.producer_source,
        candidate.submission.plan_id
    ]))
    .map_err(conflict)?;
    let block_id = daily_writer::digest(&identity);
    let plan = daily_writer::plan_block(
        ManagedBlockKind::Knowledge,
        Some(&original),
        &block_id,
        &render(&candidate),
        "",
    )
    .map_err(conflict)?;
    knowledge::validate_note_update_budget(
        &workspace,
        &state
            .store
            .list_knowledge_collections(&profile.id)
            .map_err(conflict)?,
        &input.destination_path,
        plan.updated_content.len(),
    )
    .map_err(conflict)?;
    let op = KnowledgePromotion {
        operation_id: String::new(),
        workspace_id: profile.id,
        candidate_id: id,
        candidate_hash: candidate.candidate_hash,
        approval_digest: daily_writer::digest(
            &serde_json::to_vec(&candidate.review).map_err(conflict)?,
        ),
        destination_path: input.destination_path,
        configuration_digest,
        block_id,
        original_hash: daily_writer::digest(&original),
        updated_hash: daily_writer::digest(&plan.updated_content),
        block_hash: plan.intended_new_block_hash,
        previous_block: plan.previous_block,
        next_block: plan.next_block,
        preview_hash: String::new(),
        created_at: String::new(),
        state: "prepared".into(),
        failure_reason: None,
        original_content: original,
        updated_content: plan.updated_content,
    };
    Ok(Json(
        serde_json::to_value(
            state
                .store
                .prepare_knowledge_promotion(op)
                .map_err(conflict)?,
        )
        .map_err(conflict)?,
    ))
}

async fn list(State(state): State<AppState>, headers: HeaderMap) -> Result<Json<Value>, ApiError> {
    authorize(&state, &headers, false)?;
    let profile = active_refocus_workspace(&state)?;
    Ok(Json(
        json!({"operations":state.store.list_knowledge_promotions(&profile.id).map_err(conflict)?,"limit":100}),
    ))
}
async fn detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, ApiError> {
    authorize(&state, &headers, false)?;
    let profile = active_refocus_workspace(&state)?;
    Ok(Json(
        serde_json::to_value(
            state
                .store
                .knowledge_promotion(&profile.id, &id)
                .map_err(conflict)?
                .ok_or_else(|| conflict("KB operation not found"))?,
        )
        .map_err(conflict)?,
    ))
}
async fn discard(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
) -> Result<Json<Value>, ApiError> {
    authorize(&state, &headers, true)?;
    let _guard = state.knowledge_write_lock.lock().map_err(conflict)?;
    let _apply = state.apply_lock.lock().map_err(conflict)?;
    let (profile, workspace) = active_refocus_context(&state)?;
    let op = state
        .store
        .knowledge_promotion(&profile.id, &id)
        .map_err(conflict)?
        .ok_or_else(|| conflict("KB operation not found"))?;
    if matches!(op.state.as_str(), "writing" | "conflict") {
        let target = Path::new(&op.destination_path);
        workspace
            .resolve_markdown_path(target, MarkdownPathMode::ExistingFile)
            .map_err(conflict)?;
        let current = daily_writer::read_file(workspace.directory(), target)
            .map_err(conflict)?
            .ok_or_else(|| conflict("KB target disappeared; reconcile manually"))?;
        if op.state == "writing" && daily_writer::digest(&current) != op.original_hash {
            return Err(conflict(
                "Recover the completed write or reconcile the changed note before discarding",
            ));
        }
        let temporary = daily_writer::temporary_name(target, &op.operation_id).map_err(conflict)?;
        if let Some(bytes) = daily_writer::read_temporary(workspace.directory(), target, &temporary)
            .map_err(conflict)?
        {
            if daily_writer::digest(&bytes) != op.updated_hash {
                return Err(conflict("KB temporary file differs; reconcile manually"));
            }
            daily_writer::remove_temporary(workspace.directory(), target, &temporary)
                .map_err(conflict)?;
            daily_writer::sync_parent(workspace.directory(), target).map_err(conflict)?;
        }
    }
    state
        .store
        .set_knowledge_promotion_state(&op, "cancelled", None)
        .map_err(conflict)?;
    Ok(Json(json!({"state":"cancelled"})))
}

async fn confirm(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(id): AxumPath<String>,
    Json(input): Json<ConfirmRequest>,
) -> Result<Json<Value>, ApiError> {
    authorize(&state, &headers, true)?;
    let _knowledge = state.knowledge_write_lock.lock().map_err(conflict)?;
    let _apply = state.apply_lock.lock().map_err(conflict)?;
    let (profile, workspace) = active_refocus_context(&state)?;
    let op = state
        .store
        .knowledge_promotion(&profile.id, &id)
        .map_err(conflict)?
        .ok_or_else(|| conflict("KB operation not found"))?;
    if op.preview_hash != input.preview_hash {
        return Err(conflict(
            "KB preview changed; review the exact update again",
        ));
    }
    execute(&state, &workspace, &profile, op).map(|op| Json(json!(op)))
}

fn execute(
    state: &AppState,
    workspace: &InspectedWorkspace,
    profile: &WorkspaceProfile,
    mut op: KnowledgePromotion,
) -> Result<KnowledgePromotion, ApiError> {
    if op.state == "finalized" {
        return Ok(op);
    }
    if !matches!(op.state.as_str(), "prepared" | "writing") {
        return Err(conflict("KB operation requires a new preview"));
    }
    let target = Path::new(&op.destination_path);
    workspace
        .resolve_markdown_path(target, MarkdownPathMode::ExistingFile)
        .map_err(conflict)?;
    let current = daily_writer::read_file(workspace.directory(), target)
        .map_err(conflict)?
        .ok_or_else(|| conflict("KB target disappeared"))?;
    let current_hash = daily_writer::digest(&current);
    // A completed rename is recovery, not a new authorization. Preserve the frozen approval.
    if op.state == "writing"
        && current_hash == op.updated_hash
        && daily_writer::block_hash(ManagedBlockKind::Knowledge, &current, &op.block_id)
            .map_err(conflict)?
            .as_deref()
            == Some(&op.block_hash)
    {
        daily_writer::sync_parent(workspace.directory(), target).map_err(conflict)?;
        state
            .store
            .set_knowledge_promotion_state(&op, "finalized", None)
            .map_err(conflict)?;
        op.state = "finalized".into();
        return Ok(op);
    }
    state
        .store
        .require_promotable_knowledge_candidate(&profile.id, &op.candidate_id, &op.candidate_hash)
        .map_err(conflict)?;
    state
        .store
        .ensure_no_pending_daily_write(&profile.id, &op.destination_path)
        .map_err(conflict)?;
    if target_configuration(state, profile, workspace, &op.destination_path)?
        != op.configuration_digest
    {
        return Err(conflict(
            "Reference configuration changed; create a new KB preview",
        ));
    }
    if current_hash != op.original_hash {
        if op.state == "prepared" {
            state
                .store
                .set_knowledge_promotion_state(&op, "writing", None)
                .map_err(conflict)?;
            op.state = "writing".into();
        }
        state
            .store
            .set_knowledge_promotion_state(
                &op,
                "conflict",
                Some("Canonical note changed after preview; nothing overwritten"),
            )
            .map_err(conflict)?;
        return Err(conflict(
            "Canonical note changed after preview; nothing overwritten",
        ));
    }
    knowledge::validate_note_update_budget(
        workspace,
        &state
            .store
            .list_knowledge_collections(&profile.id)
            .map_err(conflict)?,
        &op.destination_path,
        op.updated_content.len(),
    )
    .map_err(conflict)?;
    if op.state == "prepared" {
        state
            .store
            .set_knowledge_promotion_state(&op, "writing", None)
            .map_err(conflict)?;
        op.state = "writing".into();
    }
    // A crash may leave our exact durable temporary file. Never reuse unrelated bytes.
    let temporary = daily_writer::temporary_name(target, &op.operation_id).map_err(conflict)?;
    if let Some(staged) =
        daily_writer::read_temporary(workspace.directory(), target, &temporary).map_err(conflict)?
    {
        if daily_writer::digest(&staged) != op.updated_hash {
            return Err(conflict(
                "KB temporary file differs; manual reconciliation required",
            ));
        }
        daily_writer::commit_temporary(workspace.directory(), target, &temporary)
            .map_err(conflict)?;
    } else {
        daily_writer::write_atomically(
            workspace.directory(),
            target,
            &op.updated_content,
            &op.operation_id,
        )
        .map_err(conflict)?;
    }
    let written = daily_writer::read_file(workspace.directory(), target)
        .map_err(conflict)?
        .ok_or_else(|| conflict("KB target missing after write"))?;
    if daily_writer::digest(&written) != op.updated_hash {
        return Err(conflict(
            "KB note changed during verification; recover this operation",
        ));
    }
    daily_writer::sync_parent(workspace.directory(), target).map_err(conflict)?;
    state
        .store
        .set_knowledge_promotion_state(&op, "finalized", None)
        .map_err(conflict)?;
    op.state = "finalized".into();
    Ok(op)
}

#[cfg(test)]
mod tests {
    use super::*;
    use log_inbox_core::{
        knowledge_capture::{KnowledgeCandidate, KnowledgeDispositionKind},
        models::LogEventInput,
    };
    use tower::ServiceExt;
    fn fixture() -> (
        AppState,
        KnowledgeCandidate,
        log_inbox_core::auth::SessionCredentials,
    ) {
        let path = env::temp_dir().join(format!("kb-promote-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(path.join("Products")).unwrap();
        std::fs::write(path.join("Products/Inbox.md"),"---\ntitle: Inbox\n---\n# Inbox\nHuman prose.\n<!-- log-inbox:daily:existing:begin -->\nDaily text\n<!-- log-inbox:daily:existing:end -->\n").unwrap();
        let workspace = InspectedWorkspace::inspect(&path).unwrap();
        let store = Store::open(path.join("store.sqlite3")).unwrap();
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
        store
            .save_knowledge_collection(
                None,
                &profile.id,
                "Products",
                "Canonical notes",
                &["Products".into()],
                &[],
                true,
                None,
            )
            .unwrap();
        let payload = json!({"schema_version":1,"submission_key":"one","plan_id":"kb","plan_revision":"1","product":"Log Inbox","modules":["knowledge"],"title":"Accepted work","summary":"Plan only <script>bad()</script> [[Injected]] <!-- log-inbox:daily:x:begin -->","claims":[{"kind":"accepted","text":"Plan accepted; implementation pending"}]});
        let event = store
            .insert_event(LogEventInput {
                source: "codex/test".into(),
                message: "Planning complete".into(),
                timestamp: None,
                level: None,
                metadata: Some(
                    json!({"knowledge_submission":payload})
                        .as_object()
                        .unwrap()
                        .clone(),
                ),
                fingerprint: None,
            })
            .unwrap();
        let candidate = store.materialize_knowledge_submission(&event.id).unwrap();
        store
            .review_knowledge_candidate(
                &profile.id,
                &candidate.id,
                &candidate.candidate_hash,
                KnowledgeDispositionKind::Accept,
                "Reviewed original plan",
            )
            .unwrap();
        let credentials = generate_session_credentials();
        store
            .create_dashboard_session(
                &credentials,
                &["knowledge:read".into(), "vault:write".into()],
                Utc::now(),
                Duration::hours(1),
                Duration::hours(1),
            )
            .unwrap();
        (
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
            },
            candidate,
            credentials,
        )
    }
    async fn call(
        state: &AppState,
        credentials: &log_inbox_core::auth::SessionCredentials,
        path: &str,
        body: Value,
    ) -> Response {
        build_router(state.clone())
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(path)
                    .header(header::HOST, "localhost:8788")
                    .header(header::ORIGIN, "http://localhost:8788")
                    .header(
                        header::COOKIE,
                        format!("log_inbox_session={}", credentials.session_token),
                    )
                    .header("x-csrf-token", &credentials.csrf_token)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap()
    }
    async fn preview_op(
        state: &AppState,
        candidate: &KnowledgeCandidate,
        credentials: &log_inbox_core::auth::SessionCredentials,
    ) -> KnowledgePromotion {
        let response=call(state,credentials,&format!("/api/v2/knowledge/candidates/{}/promotion/preview",candidate.id),json!({"candidate_hash":candidate.candidate_hash,"destination_path":"Products/Inbox.md"})).await;
        let status = response.status();
        let body = axum::body::to_bytes(response.into_body(), 1_000_000)
            .await
            .unwrap();
        assert_eq!(status, StatusCode::OK, "{}", String::from_utf8_lossy(&body));
        serde_json::from_slice(&body).unwrap()
    }
    #[tokio::test]
    async fn preview_confirm_preserves_note_and_daily_block_and_retries_exactly() {
        let (state, candidate, credentials) = fixture();
        let target = state.workspace.canonical_root().join("Products/Inbox.md");
        let original = std::fs::read(&target).unwrap();
        let preview = preview_op(&state, &candidate, &credentials).await;
        assert_eq!(std::fs::read(&target).unwrap(), original);
        assert!(!preview.next_block.contains("<script>"));
        assert!(!preview.next_block.contains("[[Injected]]"));
        assert!(preview.next_block.contains("Reported accepted"));
        let repeated = preview_op(&state, &candidate, &credentials).await;
        assert_eq!(preview.operation_id, repeated.operation_id);
        let path = format!(
            "/api/v2/knowledge/promotions/{}/confirm",
            preview.operation_id
        );
        for _ in 0..2 {
            assert_eq!(
                call(
                    &state,
                    &credentials,
                    &path,
                    json!({"preview_hash":preview.preview_hash})
                )
                .await
                .status(),
                StatusCode::OK
            );
        }
        let updated = std::fs::read_to_string(&target).unwrap();
        assert!(updated.starts_with(std::str::from_utf8(&original).unwrap()));
        assert_eq!(updated.matches("log-inbox:knowledge:").count(), 2);
        assert!(
            state
                .store
                .daily_day(&candidate.workspace_id, "2026-10-19".parse().unwrap())
                .unwrap()
                .is_none()
        );
        assert_eq!(
            state
                .store
                .knowledge_promotion(&candidate.workspace_id, &preview.operation_id)
                .unwrap()
                .unwrap()
                .state,
            "finalized"
        );
    }
    #[tokio::test]
    async fn stale_files_wrong_hash_protected_targets_and_missing_write_scope_cannot_write() {
        let (state, candidate, credentials) = fixture();
        let op = preview_op(&state, &candidate, &credentials).await;
        let path = format!("/api/v2/knowledge/promotions/{}/confirm", op.operation_id);
        assert_eq!(
            call(&state, &credentials, &path, json!({"preview_hash":"wrong"}))
                .await
                .status(),
            StatusCode::CONFLICT
        );
        let target = state.workspace.canonical_root().join("Products/Inbox.md");
        std::fs::write(&target, "Owner changed note").unwrap();
        assert_eq!(
            call(
                &state,
                &credentials,
                &path,
                json!({"preview_hash":op.preview_hash})
            )
            .await
            .status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            std::fs::read_to_string(&target).unwrap(),
            "Owner changed note"
        );
        let preview_path = format!(
            "/api/v2/knowledge/candidates/{}/promotion/preview",
            candidate.id
        );
        for target in [
            "../outside.md",
            ".git/config.md",
            "Daily/2026-10-19.md",
            "Products/Missing.md",
        ] {
            assert_ne!(
                call(
                    &state,
                    &credentials,
                    &preview_path,
                    json!({"candidate_hash":candidate.candidate_hash,"destination_path":target})
                )
                .await
                .status(),
                StatusCode::OK
            );
        }
        let reader = generate_session_credentials();
        state
            .store
            .create_dashboard_session(
                &reader,
                &["knowledge:read".into()],
                Utc::now(),
                Duration::hours(1),
                Duration::hours(1),
            )
            .unwrap();
        assert_eq!(call(&state,&reader,&preview_path,json!({"candidate_hash":candidate.candidate_hash,"destination_path":"Products/Inbox.md"})).await.status(),StatusCode::UNAUTHORIZED);
    }
    #[tokio::test]
    async fn restart_recovers_staged_and_already_renamed_writes_without_duplicates() {
        for renamed in [false, true] {
            let (mut state, candidate, credentials) = fixture();
            let op = preview_op(&state, &candidate, &credentials).await;
            let full = state
                .store
                .knowledge_promotion(&candidate.workspace_id, &op.operation_id)
                .unwrap()
                .unwrap();
            state
                .store
                .set_knowledge_promotion_state(&full, "writing", None)
                .unwrap();
            let target = Path::new(&full.destination_path);
            if renamed {
                daily_writer::write_atomically(
                    state.workspace.directory(),
                    target,
                    &full.updated_content,
                    &full.operation_id,
                )
                .unwrap();
            } else {
                let temporary = daily_writer::temporary_name(target, &full.operation_id).unwrap();
                std::fs::write(
                    state
                        .workspace
                        .canonical_root()
                        .join("Products")
                        .join(temporary),
                    &full.updated_content,
                )
                .unwrap();
            }
            state.store = Store::open(state.store.database_path().to_owned()).unwrap();
            let response = call(
                &state,
                &credentials,
                &format!("/api/v2/knowledge/promotions/{}/confirm", op.operation_id),
                json!({"preview_hash":op.preview_hash}),
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                std::fs::read(state.workspace.canonical_root().join(target)).unwrap(),
                full.updated_content
            );
            assert_eq!(
                state
                    .store
                    .list_knowledge_promotions(&candidate.workspace_id)
                    .unwrap()[0]
                    .state,
                "finalized"
            );
        }
    }
    #[tokio::test]
    async fn expired_sources_and_changed_collections_block_fresh_writes_but_not_rename_recovery() {
        let (state, candidate, credentials) = fixture();
        let op = preview_op(&state, &candidate, &credentials).await;
        let full = state
            .store
            .knowledge_promotion(&candidate.workspace_id, &op.operation_id)
            .unwrap()
            .unwrap();
        let conn = rusqlite::Connection::open(state.store.database_path()).unwrap();
        conn.execute(
            "DELETE FROM log_events WHERE id=?1",
            [&candidate.source_event_id],
        )
        .unwrap();
        let path = format!("/api/v2/knowledge/promotions/{}/confirm", op.operation_id);
        assert_eq!(
            call(
                &state,
                &credentials,
                &path,
                json!({"preview_hash":op.preview_hash})
            )
            .await
            .status(),
            StatusCode::CONFLICT
        );
        state
            .store
            .set_knowledge_promotion_state(&full, "writing", None)
            .unwrap();
        daily_writer::write_atomically(
            state.workspace.directory(),
            Path::new(&full.destination_path),
            &full.updated_content,
            &full.operation_id,
        )
        .unwrap();
        assert_eq!(
            call(
                &state,
                &credentials,
                &path,
                json!({"preview_hash":op.preview_hash})
            )
            .await
            .status(),
            StatusCode::OK
        );
        let (state, candidate, credentials) = fixture();
        let op = preview_op(&state, &candidate, &credentials).await;
        let collection = state
            .store
            .list_knowledge_collections(&candidate.workspace_id)
            .unwrap()
            .remove(0);
        state
            .store
            .delete_knowledge_collection(
                &collection.id,
                &candidate.workspace_id,
                collection.updated_at,
            )
            .unwrap();
        assert_eq!(
            call(
                &state,
                &credentials,
                &format!("/api/v2/knowledge/promotions/{}/confirm", op.operation_id),
                json!({"preview_hash":op.preview_hash})
            )
            .await
            .status(),
            StatusCode::CONFLICT
        );
    }
    #[tokio::test]
    async fn two_prepared_destinations_cannot_duplicate_one_plan() {
        let (state, candidate, credentials) = fixture();
        std::fs::write(
            state.workspace.canonical_root().join("Products/Other.md"),
            "# Other\nHuman text\n",
        )
        .unwrap();
        let first = preview_op(&state, &candidate, &credentials).await;
        let response=call(&state,&credentials,&format!("/api/v2/knowledge/candidates/{}/promotion/preview",candidate.id),json!({"candidate_hash":candidate.candidate_hash,"destination_path":"Products/Other.md"})).await;
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(response.into_body(), 1_000_000)
            .await
            .unwrap();
        let second: KnowledgePromotion = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(
            call(
                &state,
                &credentials,
                &format!(
                    "/api/v2/knowledge/promotions/{}/confirm",
                    first.operation_id
                ),
                json!({"preview_hash":first.preview_hash})
            )
            .await
            .status(),
            StatusCode::OK
        );
        assert_eq!(
            call(
                &state,
                &credentials,
                &format!(
                    "/api/v2/knowledge/promotions/{}/confirm",
                    second.operation_id
                ),
                json!({"preview_hash":second.preview_hash})
            )
            .await
            .status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            std::fs::read_to_string(state.workspace.canonical_root().join("Products/Other.md"))
                .unwrap(),
            "# Other\nHuman text\n"
        );
    }
    #[tokio::test]
    async fn unwritten_expired_operation_can_cancel_and_release_note_without_writing() {
        let (state, candidate, credentials) = fixture();
        let op = preview_op(&state, &candidate, &credentials).await;
        let mut full = state
            .store
            .knowledge_promotion(&candidate.workspace_id, &op.operation_id)
            .unwrap()
            .unwrap();
        state
            .store
            .set_knowledge_promotion_state(&full, "writing", None)
            .unwrap();
        full.state = "writing".into();
        let target = Path::new(&full.destination_path);
        let temporary = daily_writer::temporary_name(target, &full.operation_id).unwrap();
        std::fs::write(
            state
                .workspace
                .canonical_root()
                .join("Products")
                .join(&temporary),
            &full.updated_content,
        )
        .unwrap();
        let conn = rusqlite::Connection::open(state.store.database_path()).unwrap();
        conn.execute(
            "DELETE FROM log_events WHERE id=?1",
            [&candidate.source_event_id],
        )
        .unwrap();
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
        let result = discard(
            State(state.clone()),
            headers,
            AxumPath(op.operation_id.clone()),
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(
            state
                .store
                .knowledge_promotion(&candidate.workspace_id, &op.operation_id)
                .unwrap()
                .unwrap()
                .state,
            "cancelled"
        );
        assert_eq!(
            std::fs::read(state.workspace.canonical_root().join(target)).unwrap(),
            full.original_content
        );
        assert!(
            !state
                .workspace
                .canonical_root()
                .join("Products")
                .join(temporary)
                .exists()
        );
        assert!(
            state
                .store
                .ensure_no_pending_knowledge_write(&candidate.workspace_id, &full.destination_path)
                .is_ok()
        );
        let listed = state
            .store
            .list_knowledge_promotions(&candidate.workspace_id)
            .unwrap();
        assert!(listed[0].original_content.is_empty() && listed[0].updated_content.is_empty());
    }
    #[tokio::test]
    async fn daily_reconciliation_retains_write_ownership_until_finalized() {
        let (state, candidate, credentials) = fixture();
        let date = NaiveDate::from_ymd_opt(2026, 10, 19).unwrap();
        let day = state
            .store
            .ensure_daily_day(date, "Products/Inbox.md", None)
            .unwrap();
        let revision = state
            .store
            .create_proposal_revision(
                &candidate.workspace_id,
                date,
                None,
                "advanced_markdown",
                &json!("Reviewed Daily"),
            )
            .unwrap();
        let target = Path::new(&day.destination_path);
        let current = daily_writer::read_file(state.workspace.directory(), target)
            .unwrap()
            .unwrap();
        let plan =
            daily_writer::plan_managed_block(Some(&current), &day.block_id, "Reviewed Daily", "")
                .unwrap();
        let operation = state
            .store
            .prepare_apply_operation(&PrepareApplyOperation {
                id: "daily_pending_reconciliation".into(),
                workspace_id: candidate.workspace_id.clone(),
                local_date: date,
                revision_id: revision.id,
                revision_content_hash: revision.content_hash,
                destination_path: day.destination_path.clone(),
                expected_old_block_hash: plan.expected_old_block_hash,
                intended_new_block_hash: plan.intended_new_block_hash,
                expected_target_exists: true,
                expected_original_content_hash: daily_writer::digest(&current),
                intended_updated_content_hash: daily_writer::digest(&plan.updated_content),
                temporary_name: daily_writer::temporary_name(
                    target,
                    "daily_pending_reconciliation",
                )
                .unwrap(),
                recovery_payload: Some(current),
                recovery_path: None,
            })
            .unwrap();
        state
            .store
            .transition_apply_operation(&operation.id, "prepared", "writing", None)
            .unwrap();
        daily_writer::write_atomically(
            state.workspace.directory(),
            target,
            &plan.updated_content,
            &operation.id,
        )
        .unwrap();
        state
            .store
            .transition_apply_operation(
                &operation.id,
                "writing",
                "reconciliation_required",
                Some("Simulated sync uncertainty after rename"),
            )
            .unwrap();
        let kb = preview_op(&state, &candidate, &credentials).await;
        assert_eq!(
            call(
                &state,
                &credentials,
                &format!("/api/v2/knowledge/promotions/{}/confirm", kb.operation_id),
                json!({"preview_hash":kb.preview_hash})
            )
            .await
            .status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            daily_writer::read_file(state.workspace.directory(), target)
                .unwrap()
                .unwrap(),
            plan.updated_content
        );
        for blocked in [
            "prepared",
            "writing",
            "written",
            "failed",
            "reconciliation_required",
        ] {
            // All five states retain the same Daily ownership contract.
            let conn = rusqlite::Connection::open(state.store.database_path()).unwrap();
            conn.execute(
                "UPDATE apply_operations SET state=?1 WHERE id=?2",
                rusqlite::params![blocked, operation.id],
            )
            .unwrap();
            assert!(
                state
                    .store
                    .ensure_no_pending_daily_write(&candidate.workspace_id, &day.destination_path)
                    .is_err()
            );
        }
        let conn = rusqlite::Connection::open(state.store.database_path()).unwrap();
        conn.execute(
            "UPDATE apply_operations SET state='finalized' WHERE id=?1",
            [&operation.id],
        )
        .unwrap();
        assert!(
            state
                .store
                .ensure_no_pending_daily_write(&candidate.workspace_id, &day.destination_path)
                .is_ok()
        );
    }
    #[tokio::test]
    async fn daily_apply_cannot_prepare_while_knowledge_owns_the_note() {
        let (state, candidate, credentials) = fixture();
        let date = NaiveDate::from_ymd_opt(2000, 1, 1).unwrap();
        state
            .store
            .ensure_daily_day(date, "Products/Inbox.md", None)
            .unwrap();
        let manual = state
            .store
            .create_manual_daily_entry(
                &candidate.workspace_id,
                date,
                "Reviewed owner activity",
                &[],
            )
            .unwrap();
        state.store.create_proposal_revision(&candidate.workspace_id,date,None,"manual",&json!({"schema_version":1,"workstreams":[],"manual_entry_ids":[manual.id],"open_questions":[]})).unwrap();
        let material =
            daily_apply_material(&state, date).unwrap_or_else(|e| panic!("{}", e.message));
        let kb = preview_op(&state, &candidate, &credentials).await;
        let full = state
            .store
            .knowledge_promotion(&candidate.workspace_id, &kb.operation_id)
            .unwrap()
            .unwrap();
        state
            .store
            .set_knowledge_promotion_state(&full, "writing", None)
            .unwrap();
        let body = json!({"expected_revision_id":material.revision.id,"expected_revision_content_hash":material.revision.content_hash,"destination_path":material.day.destination_path,"expected_old_block_hash":material.plan.expected_old_block_hash,"intended_new_block_hash":material.plan.intended_new_block_hash,"expected_target_exists":material.target_exists,"expected_original_content_hash":daily_writer::digest(&material.original_content),"expected_updated_content_hash":daily_writer::digest(&material.plan.updated_content)});
        let response = call(&state, &credentials, "/api/v2/daily/2000-01-01/apply", body).await;
        assert_eq!(response.status(), StatusCode::CONFLICT);
        let response = axum::body::to_bytes(response.into_body(), 1_000_000)
            .await
            .unwrap();
        assert!(String::from_utf8_lossy(&response).contains("Knowledge write"));
        assert!(
            state
                .store
                .list_unfinished_apply_operations(100)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            call(
                &state,
                &credentials,
                &format!("/api/v2/knowledge/promotions/{}/confirm", kb.operation_id),
                json!({"preview_hash":kb.preview_hash})
            )
            .await
            .status(),
            StatusCode::OK
        );
    }
    #[test]
    fn canonical_updates_keep_aggregate_reference_budget_with_overlapping_collections() {
        let (state, candidate, _) = fixture();
        let root = state.workspace.canonical_root();
        let limit = 1024 * 1024;
        for n in 0..31 {
            std::fs::write(
                root.join(format!("Products/filler{n}.md")),
                vec![b'x'; limit],
            )
            .unwrap();
        }
        std::fs::write(root.join("Products/final.md"), vec![b'x'; limit - 100]).unwrap();
        let collections = state
            .store
            .list_knowledge_collections(&candidate.workspace_id)
            .unwrap();
        let mut overlap = collections.clone();
        overlap.extend(collections);
        assert!(
            knowledge::validate_note_update_budget(
                &state.workspace,
                &overlap,
                "Products/Inbox.md",
                50
            )
            .is_ok()
        );
        assert!(
            knowledge::validate_note_update_budget(
                &state.workspace,
                &overlap,
                "Products/Inbox.md",
                200
            )
            .unwrap_err()
            .contains("catalog size")
        );
        assert!(
            knowledge::validate_note_update_budget(
                &state.workspace,
                &overlap,
                "Products/Inbox.md",
                limit + 1
            )
            .unwrap_err()
            .contains("note size")
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
