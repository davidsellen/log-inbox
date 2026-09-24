use super::*;

#[derive(Deserialize)]
pub(super) struct SearchQuery {
    q: String,
    cursor: Option<String>,
}

pub(super) async fn search(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<SearchQuery>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "logs:read", false)?;
    let profile = active_refocus_workspace(&state)?;
    if query.q.trim().is_empty()
        || query.q.chars().count() > 200
        || query.cursor.as_ref().is_some_and(|cursor| {
            cursor.len() > 2048
                || serde_json::from_str::<(std::cmp::Reverse<NaiveDate>, String, String, String)>(
                    cursor,
                )
                .is_err()
        })
    {
        return Err(ApiError::bad_request(
            "Enter 1–200 characters and a valid search cursor",
        ));
    }
    // Keep potentially long reads off the asynchronous HTTP executor; no writer
    // lock or generation lock is held while searching.
    let page = tokio::task::spawn_blocking(move || {
        state.store.search_history(
            &profile.id,
            &profile.timezone,
            &query.q,
            query.cursor.as_deref(),
        )
    })
    .await
    .map_err(|_| ApiError::internal("History search stopped"))?
    .map_err(|error| ApiError::internal(error.to_string()))?;
    Ok(Json(json!(page)))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Preferences {
    recent_days: usize,
}

pub(super) async fn preferences(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "logs:read", false)?;
    let profile = active_refocus_workspace(&state)?;
    let days = state
        .store
        .recent_days_preference(&profile.id)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(json!({"recent_days": days})))
}

pub(super) async fn save_preferences(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Preferences>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "settings:write", true)?;
    let profile = active_refocus_workspace(&state)?;
    if ![7, 10, 14, 30].contains(&input.recent_days) {
        return Err(ApiError::bad_request(
            "Recent days must be 7, 10, 14, or 30",
        ));
    }
    state
        .store
        .save_recent_days_preference(&profile.id, input.recent_days)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    Ok(Json(json!({"recent_days": input.recent_days})))
}

// A search result may refer to evidence beyond the initial 500-item day preview.
pub(super) async fn activity(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath((date, id)): AxumPath<(String, String)>,
) -> Result<Json<Value>, ApiError> {
    authorize_refocus(&state, &headers, "logs:read", false)?;
    let profile = active_refocus_workspace(&state)?;
    let date = NaiveDate::parse_from_str(&date, "%Y-%m-%d")
        .map_err(|_| ApiError::bad_request("Invalid date"))?;
    let day = state
        .store
        .daily_day(&profile.id, date)
        .map_err(|e| ApiError::internal(e.to_string()))?;
    let window =
        effective_daily_window(date, &profile, day.as_ref()).map_err(ApiError::bad_request)?;
    let event = state
        .store
        .get_events_by_ids(&[id])
        .map_err(|e| ApiError::internal(e.to_string()))?
        .into_iter()
        .find(|event| event.timestamp >= window.start_utc && event.timestamp < window.end_utc);
    match event {
        Some(event) => Ok(Json(json!(event))),
        None => Err(ApiError::not_found(
            "This activity is no longer available on this day",
        )),
    }
}
