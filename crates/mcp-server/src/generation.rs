use super::*;

pub(super) type CancelState = Arc<Mutex<Option<(String, tokio::sync::watch::Sender<bool>)>>>;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ReferenceMode {
    #[default]
    Configured,
    None,
}

impl ReferenceMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Configured => "configured",
            Self::None => "none",
        }
    }
}

pub(super) struct Attempt {
    pub id: String,
    pub deadline: tokio::time::Instant,
    pub cancel: tokio::sync::watch::Receiver<bool>,
    state: AppState,
    date: NaiveDate,
    pub workspace_id: String,
    pub reference_mode: ReferenceMode,
    operation: String,
    stage_clock: Mutex<(String, std::time::Instant)>,
    pub save_retries: Arc<Mutex<u8>>,
}

pub(super) fn database_error(error: anyhow::Error) -> ApiError {
    if let Some(code) = log_inbox_core::sqlite_busy_code(&error) {
        tracing::warn!(
            sqlite_extended_code = code,
            "Daily database operation remained busy"
        );
        ApiError::conflict(
            "The database is busy. Retry preparation; your notes and previous draft are unchanged.",
        )
        .with_code("database_busy")
    } else {
        ApiError::conflict(error.to_string())
    }
}

pub(super) fn save_with_retries<T>(
    id: &str,
    deadline: tokio::time::Instant,
    retry_budget: &Mutex<u8>,
    mut operation: impl FnMut() -> anyhow::Result<T>,
) -> Result<T, ApiError> {
    loop {
        match operation() {
            Ok(value) => return Ok(value),
            Err(error) => {
                let Some(sqlite_code) = log_inbox_core::sqlite_busy_code(&error) else {
                    return Err(ApiError::internal(error.to_string()));
                };
                let mut retries = retry_budget
                    .lock()
                    .map_err(|_| ApiError::internal("generation controller unavailable"))?;
                if *retries >= 2 || tokio::time::Instant::now() >= deadline {
                    return Err(ApiError::internal(
                        "Database remained busy while saving. Retry this draft.",
                    )
                    .with_code("database_busy"));
                }
                *retries += 1;
                tracing::warn!(
                    attempt_id = id,
                    sqlite_extended_code = sqlite_code,
                    retry = *retries,
                    "Retrying generation database save without repeating inference"
                );
            }
        }
    }
}

impl Attempt {
    pub fn save<T>(&self, operation: impl FnMut() -> anyhow::Result<T>) -> Result<T, ApiError> {
        save_with_retries(&self.id, self.deadline, &self.save_retries, operation)
    }
    pub fn finish<T>(&self, result: &Result<T, ApiError>) -> Result<&'static str, ApiError> {
        self.log_stage("complete");
        let terminal = match result {
            Ok(_) => "succeeded",
            Err(e) if e.message == "Generation canceled." => "canceled",
            Err(e) if e.message == "Generation timed out." => "timed_out",
            Err(_) => "failed",
        };
        let error = result
            .as_ref()
            .err()
            .map(|e| bounded_schedule_error(&e.message));
        let failure_code = result.as_ref().err().map(|e| {
            e.failure_code.unwrap_or(match terminal {
                "canceled" => "canceled",
                "timed_out" => "timed_out",
                _ => "generation_failed",
            })
        });
        self.save(|| {
            self.state.store.finish_generation_attempt_with_failure(
                &self.id,
                terminal,
                error.as_deref(),
                failure_code,
                Utc::now(),
            )
        })?;
        Ok(terminal)
    }
    pub fn stage(&self, stage: &str) -> Result<(), ApiError> {
        let _cancel_guard = self
            .state
            .generation_cancel
            .lock()
            .map_err(|_| ApiError::internal("generation controller unavailable"))?;
        if matches!(stage, "requesting_model" | "saving") {
            if *self.cancel.borrow() {
                return Err(ApiError::unprocessable("Generation canceled."));
            }
            if tokio::time::Instant::now() >= self.deadline {
                return Err(ApiError::unprocessable("Generation timed out."));
            }
        }
        self.state
            .store
            .update_generation_attempt_stage(&self.id, stage)
            .map_err(|e| ApiError::internal(e.to_string()))?;
        self.log_stage(stage);
        Ok(())
    }

    fn log_stage(&self, next: &str) {
        if let Ok(mut clock) = self.stage_clock.lock() {
            tracing::info!(attempt_id = %self.id, stage = %clock.0, elapsed_ms = clock.1.elapsed().as_millis() as u64, "Generation stage finished");
            *clock = (next.to_owned(), std::time::Instant::now());
        }
    }
}

impl Drop for Attempt {
    fn drop(&mut self) {
        // Also covers task panic/cancellation; completed attempts are unchanged.
        if self
            .state
            .store
            .finish_generation_attempt_with_failure(
                &self.id,
                "interrupted",
                Some("Generation interrupted. Retry when ready."),
                Some("interrupted"),
                Utc::now(),
            )
            .unwrap_or(false)
            && self.operation != "comparison"
        {
            let _ = self.state.store.set_daily_generation_status(
                &self.workspace_id,
                self.date,
                "failed",
            );
        }
        if let Ok(mut active) = self.state.generation_cancel.lock()
            && active.as_ref().is_some_and(|(id, _)| id == &self.id)
        {
            *active = None;
        }
    }
}

pub(super) fn begin(
    state: &AppState,
    date: NaiveDate,
    operation: &str,
) -> Result<(Attempt, Value), ApiError> {
    begin_with_references(state, date, operation, ReferenceMode::Configured)
}

pub(super) fn begin_with_references(
    state: &AppState,
    date: NaiveDate,
    operation: &str,
    reference_mode: ReferenceMode,
) -> Result<(Attempt, Value), ApiError> {
    let profile = active_refocus_workspace(state)?;
    let timeout = state
        .llm_config
        .as_ref()
        .map(llm::LlmConfig::timeout_seconds)
        .unwrap_or(1200);
    let value = state
        .store
        .create_generation_attempt_with_references(
            &profile.id,
            date,
            operation,
            timeout,
            reference_mode.as_str(),
            Utc::now(),
        )
        .map_err(database_error)?;
    let id = value["id"]
        .as_str()
        .ok_or_else(|| ApiError::internal("generation attempt has no ID"))?
        .to_owned();
    let (sender, cancel) = tokio::sync::watch::channel(false);
    *state
        .generation_cancel
        .lock()
        .map_err(|_| ApiError::internal("generation controller unavailable"))? =
        Some((id.clone(), sender));
    Ok((
        Attempt {
            id,
            deadline: tokio::time::Instant::now() + std::time::Duration::from_secs(timeout),
            cancel,
            state: state.clone(),
            date,
            workspace_id: profile.id,
            reference_mode,
            operation: operation.to_owned(),
            stage_clock: Mutex::new(("preparing".to_owned(), std::time::Instant::now())),
            save_retries: Arc::new(Mutex::new(0)),
        },
        value,
    ))
}

pub(super) async fn run(
    state: &AppState,
    date: NaiveDate,
    replace: bool,
    context: Option<(String, BTreeSet<(String, String)>)>,
    mut attempt: Attempt,
    _guard: tokio::sync::OwnedMutexGuard<()>,
) -> Result<ProposalRevision, ApiError> {
    let result = generate_daily_candidate_inner(state, date, replace, context, &mut attempt).await;
    let terminal = attempt.finish(&result)?;
    if state
        .store
        .daily_day(&attempt.workspace_id, date)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .is_some()
    {
        state
            .store
            .set_daily_generation_status(
                &attempt.workspace_id,
                date,
                if result.is_ok() { "ready" } else { "failed" },
            )
            .map_err(|e| ApiError::internal(e.to_string()))?;
    }
    if result.is_err() && terminal == "canceled" {
        state
            .store
            .cancel_daily_schedule_run(&attempt.workspace_id, date, Utc::now())
            .map_err(|e| ApiError::internal(e.to_string()))?;
    }
    result
}

pub(super) async fn cancel(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath((date, id)): AxumPath<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "draft:generate", true)?;
    let date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("date must use YYYY-MM-DD"))?;
    let profile = active_refocus_workspace(&state)?;
    let active = state
        .generation_cancel
        .lock()
        .map_err(|_| ApiError::internal("generation controller unavailable"))?;
    let attempt = state
        .store
        .latest_generation_attempt(&profile.id, date)
        .map_err(|e| ApiError::internal(e.to_string()))?
        .filter(|a| a["id"].as_str() == Some(&id))
        .ok_or_else(|| ApiError::not_found("generation attempt not found"))?;
    let mut requested = false;
    if attempt["finished_at"].is_null()
        && attempt["stage"] != "saving"
        && let Some((_, sender)) = active.as_ref().filter(|(active_id, _)| active_id == &id)
    {
        sender.send_replace(true);
        requested = true;
    }
    Ok(Json(
        json!({"attempt": attempt, "cancel_requested": requested}),
    ))
}
