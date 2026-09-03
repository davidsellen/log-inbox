use crate::{llm, proposal_inbox::ProposalInbox};
use chrono::{Duration as ChronoDuration, Utc};
use log_inbox_core::{models::StoredLogEvent, store::Store};
use serde_json::{Value, json};
use std::{cmp::Ordering, collections::BTreeMap, env, time::Duration};

#[cfg(test)]
use serde_json::Map;

#[derive(Debug, Clone)]
pub struct AutoStageConfig {
    interval: Duration,
    quiet_period: ChronoDuration,
    batch_size: usize,
}

impl AutoStageConfig {
    pub fn from_env() -> Option<Self> {
        let interval_seconds = env_u64("LOG_INBOX_AUTO_STAGE_INTERVAL_SECONDS", 0);
        if interval_seconds == 0 {
            return None;
        }

        Some(Self {
            interval: Duration::from_secs(interval_seconds),
            quiet_period: ChronoDuration::seconds(env_i64(
                "LOG_INBOX_AUTO_STAGE_QUIET_SECONDS",
                30,
            )),
            batch_size: env_usize("LOG_INBOX_AUTO_STAGE_BATCH_SIZE", 100).clamp(1, 500),
        })
    }
}

pub async fn run(
    config: AutoStageConfig,
    store: Store,
    llm_config: Option<llm::LlmConfig>,
    inbox: ProposalInbox,
) {
    let mut ticker = tokio::time::interval(config.interval);
    loop {
        ticker.tick().await;
        if let Err(error) = stage_ready_groups(&config, &store, llm_config.as_ref(), &inbox).await {
            tracing::error!(%error, "automatic proposal staging failed");
        }
    }
}

async fn stage_ready_groups(
    config: &AutoStageConfig,
    store: &Store,
    llm_config: Option<&llm::LlmConfig>,
    inbox: &ProposalInbox,
) -> Result<(), String> {
    let now = Utc::now();
    let events = store
        .get_unstaged_events(now, config.batch_size)
        .map_err(|error| error.to_string())?;

    for events in group_events(events).into_values() {
        let latest_received_at = events
            .iter()
            .map(|event| event.received_at)
            .max()
            .expect("event groups are non-empty");
        if latest_received_at > now - config.quiet_period {
            continue;
        }

        let event_ids = events
            .iter()
            .map(|event| event.id.clone())
            .collect::<Vec<_>>();
        let args = llm::SuggestMarkdownSummaryArgs {
            event_ids: event_ids.clone(),
            vault_context: automatic_vault_context(&events),
            mode: "daily-note".to_owned(),
            task: Some(
                "Consolidate this completed activity group into a concise reviewable daily-log proposal."
                    .to_owned(),
            ),
        };
        let proposal = llm::suggest_markdown_summary(llm_config, args, events).await?;
        let staged = inbox.stage(&proposal).map_err(|error| error.to_string())?;
        store
            .mark_staged(&event_ids, &staged.proposal_id)
            .map_err(|error| error.to_string())?;
        tracing::info!(
            proposal_id = %staged.proposal_id,
            event_count = event_ids.len(),
            path = %staged.path.display(),
            "automatically staged Markdown proposal"
        );
    }

    Ok(())
}

fn group_events(events: Vec<StoredLogEvent>) -> BTreeMap<String, Vec<StoredLogEvent>> {
    let mut groups = BTreeMap::new();
    for event in events {
        groups
            .entry(group_key(&event))
            .or_insert_with(Vec::new)
            .push(event);
    }
    for events in groups.values_mut() {
        events.sort_by(event_order);
    }
    groups
}

fn event_order(left: &StoredLogEvent, right: &StoredLogEvent) -> Ordering {
    match (
        left.metadata.get("sequence").and_then(Value::as_i64),
        right.metadata.get("sequence").and_then(Value::as_i64),
    ) {
        (Some(left), Some(right)) => left.cmp(&right),
        _ => left
            .timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.received_at.cmp(&right.received_at)),
    }
}

fn group_key(event: &StoredLogEvent) -> String {
    for key in ["task_id", "session_id"] {
        if let Some(value) = event.metadata.get(key).and_then(Value::as_str) {
            return format!("{key}:{value}");
        }
    }
    if let Some(fingerprint) = &event.fingerprint {
        return format!("fingerprint:{fingerprint}");
    }
    format!("event:{}", event.id)
}

fn automatic_vault_context(events: &[StoredLogEvent]) -> Value {
    let candidate_notes = events
        .iter()
        .filter_map(|event| event.metadata.get("canonical_note"))
        .filter_map(Value::as_str)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    let note_date = events
        .iter()
        .map(|event| event.timestamp)
        .max()
        .unwrap_or_else(Utc::now);

    json!({
        "daily_note": format!("Daily log {}", note_date.format("%b %-d")),
        "candidate_notes": candidate_notes,
    })
}

fn env_u64(name: &str, default: u64) -> u64 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn env_i64(name: &str, default: i64) -> i64 {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

fn env_usize(name: &str, default: usize) -> usize {
    env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(id: &str, metadata: Map<String, Value>) -> StoredLogEvent {
        StoredLogEvent {
            id: id.to_owned(),
            received_at: Utc::now(),
            timestamp: Utc::now(),
            source: "codex/test".to_owned(),
            level: "info".to_owned(),
            message: "activity".to_owned(),
            metadata,
            fingerprint: None,
            truncated: false,
            reviewed: false,
        }
    }

    #[test]
    fn groups_task_events_without_merging_unrelated_events() {
        let groups = group_events(vec![
            event(
                "evt_2",
                Map::from_iter([
                    ("task_id".to_owned(), Value::from("task_1")),
                    ("sequence".to_owned(), Value::from(2)),
                ]),
            ),
            event(
                "evt_1",
                Map::from_iter([
                    ("task_id".to_owned(), Value::from("task_1")),
                    ("sequence".to_owned(), Value::from(1)),
                ]),
            ),
            event("evt_3", Map::new()),
        ]);

        assert_eq!(groups.len(), 2);
        assert_eq!(groups["task_id:task_1"].len(), 2);
        assert_eq!(groups["task_id:task_1"][0].id, "evt_1");
        assert_eq!(groups["event:evt_3"].len(), 1);
    }
}
