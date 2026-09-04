use crate::{llm, proposal_inbox::ProposalInbox, vault_context::VaultContextProvider};
use log_inbox_core::{models::DailyConsolidationJob, store::Store};
use serde_json::Value;
use std::{
    collections::{BTreeMap, HashSet},
    time::Duration,
};

const POLL_INTERVAL: Duration = Duration::from_millis(500);
pub const DEFAULT_DAILY_CONSOLIDATION_PROMPT: &str = r#"Create a concise daily engineering report from the complete selected day.

For each distinct workstream:
- Use a concise level-three Markdown heading. Include the most specific allowed canonical link when one clearly applies.
- Merge events sharing a task_id or session_id into one story. Treat the latest terminal event as authoritative over earlier start or progress events.
- Write 1-3 factual bullets covering the final outcome, important decision or diagnosis, validation, and any blocker or follow-up.

Keep unrelated repositories or tasks separate. Prefer outcomes over chronology. Omit duplicate lifecycle updates, superseded claims, transport details, and empty start messages. Do not invent conclusions that are absent from the evidence."#;

pub fn migrate_prompt_preference(store: &Store) -> anyhow::Result<()> {
    let preferences = store.get_preferences()?;
    if preferences.contains_key("daily_consolidation_prompt") {
        return Ok(());
    }

    store.set_preferences(&BTreeMap::from([(
        "daily_consolidation_prompt".to_owned(),
        configured_daily_prompt(&preferences),
    )]))?;
    Ok(())
}

pub async fn run(
    store: Store,
    llm_config: Option<llm::LlmConfig>,
    inbox: ProposalInbox,
    vault_context: VaultContextProvider,
) {
    loop {
        match store.claim_next_daily_consolidation() {
            Ok(Some(job)) => {
                if let Err(error) =
                    process_job(&store, llm_config.as_ref(), &inbox, &vault_context, &job).await
                {
                    tracing::error!(job_id = %job.id, %error, "daily consolidation failed");
                    let cancelled = store
                        .daily_consolidation_cancel_requested(&job.id)
                        .unwrap_or(false);
                    let result = if cancelled {
                        store.finish_daily_consolidation(&job.id, "cancelled", None, None)
                    } else {
                        store.finish_daily_consolidation(&job.id, "failed", None, Some(&error))
                    };
                    if let Err(store_error) = result {
                        tracing::error!(job_id = %job.id, %store_error, "recording consolidation failure failed");
                    }
                }
            }
            Ok(None) => tokio::time::sleep(POLL_INTERVAL).await,
            Err(error) => {
                tracing::error!(%error, "claiming daily consolidation failed");
                tokio::time::sleep(POLL_INTERVAL).await;
            }
        }
    }
}

async fn process_job(
    store: &Store,
    llm_config: Option<&llm::LlmConfig>,
    inbox: &ProposalInbox,
    vault_context: &VaultContextProvider,
    job: &DailyConsolidationJob,
) -> Result<(), String> {
    if let Some(existing) = inbox
        .list()?
        .into_iter()
        .find(|proposal| proposal.consolidation_job_id.as_deref() == Some(job.id.as_str()))
    {
        return store
            .finish_daily_consolidation(&job.id, "completed", Some(&existing.proposal_id), None)
            .map_err(|error| error.to_string());
    }

    let events = store
        .get_daily_consolidation_events(&job.id)
        .map_err(|error| error.to_string())?;
    if events.len() != job.event_count {
        return Err(format!(
            "consolidation snapshot expected {} events but loaded {}",
            job.event_count,
            events.len()
        ));
    }
    let event_ids = events
        .iter()
        .map(|event| event.id.clone())
        .collect::<Vec<_>>();
    let selected_ids = event_ids.iter().map(String::as_str).collect::<HashSet<_>>();
    let supersedes_proposal_ids = inbox
        .list()?
        .into_iter()
        .filter(|proposal| {
            proposal.target_note == job.target_note
                && proposal.consolidation_job_id.as_deref() != Some(job.id.as_str())
                && !proposal.evidence_event_ids.is_empty()
                && proposal
                    .evidence_event_ids
                    .iter()
                    .all(|event_id| selected_ids.contains(event_id.as_str()))
        })
        .map(|proposal| proposal.proposal_id)
        .collect::<Vec<_>>();

    let mut context = vault_context.for_events(&events)?;
    let Some(context_object) = context.as_object_mut() else {
        return Err("vault context must be a JSON object".to_owned());
    };
    context_object.insert(
        "daily_note".to_owned(),
        Value::String(job.target_note.clone()),
    );
    let preferences = store.get_preferences().map_err(|error| error.to_string())?;
    let task = configured_daily_prompt(&preferences);
    let args = llm::SuggestMarkdownSummaryArgs {
        event_ids: event_ids.clone(),
        vault_context: context,
        mode: "daily-consolidation".to_owned(),
        task: Some(task),
    };

    let summary = llm::suggest_markdown_summary(llm_config, args, events);
    tokio::pin!(summary);
    let mut proposal = loop {
        tokio::select! {
            result = &mut summary => break result?,
            _ = tokio::time::sleep(POLL_INTERVAL) => {
                if store.daily_consolidation_cancel_requested(&job.id).map_err(|error| error.to_string())? {
                    store.finish_daily_consolidation(&job.id, "cancelled", None, None).map_err(|error| error.to_string())?;
                    return Ok(());
                }
            }
        }
    };
    if store
        .daily_consolidation_cancel_requested(&job.id)
        .map_err(|error| error.to_string())?
    {
        store
            .finish_daily_consolidation(&job.id, "cancelled", None, None)
            .map_err(|error| error.to_string())?;
        return Ok(());
    }

    proposal.supersedes_proposal_ids = supersedes_proposal_ids;
    proposal.consolidation_job_id = Some(job.id.clone());
    let staged = inbox.stage(&proposal).map_err(|error| error.to_string())?;
    if store
        .daily_consolidation_cancel_requested(&job.id)
        .map_err(|error| error.to_string())?
    {
        inbox.discard_if_present(&staged.proposal_id)?;
        store
            .finish_daily_consolidation(&job.id, "cancelled", None, None)
            .map_err(|error| error.to_string())?;
        return Ok(());
    }
    if let Err(error) = store.mark_staged(&event_ids, &staged.proposal_id) {
        let _ = inbox.discard_if_present(&staged.proposal_id);
        return Err(error.to_string());
    }
    store
        .finish_daily_consolidation(&job.id, "completed", Some(&staged.proposal_id), None)
        .map_err(|error| error.to_string())?;
    tracing::info!(
        job_id = %job.id,
        proposal_id = %staged.proposal_id,
        event_count = event_ids.len(),
        "daily consolidation completed"
    );
    Ok(())
}

pub fn configured_daily_prompt(preferences: &BTreeMap<String, String>) -> String {
    if let Some(prompt) = preferences
        .get("daily_consolidation_prompt")
        .filter(|value| !value.trim().is_empty())
    {
        return prompt.trim().to_owned();
    }

    let mut prompt = DEFAULT_DAILY_CONSOLIDATION_PROMPT.to_owned();
    if let Some(legacy) = preferences
        .get("consolidation_instructions")
        .filter(|value| !value.trim().is_empty())
    {
        prompt.push_str("\n\nAdditional preferences:\n");
        prompt.push_str(legacy.trim());
    }
    prompt
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uses_saved_prompt_before_legacy_or_default_values() {
        let preferences = BTreeMap::from([
            (
                "daily_consolidation_prompt".to_owned(),
                " Current prompt ".to_owned(),
            ),
            (
                "consolidation_instructions".to_owned(),
                "Legacy prompt".to_owned(),
            ),
        ]);

        assert_eq!(configured_daily_prompt(&preferences), "Current prompt");
    }

    #[test]
    fn migrates_legacy_prompt_and_defaults_when_empty() {
        let legacy = BTreeMap::from([(
            "consolidation_instructions".to_owned(),
            "Legacy prompt".to_owned(),
        )]);
        assert!(configured_daily_prompt(&legacy).starts_with(DEFAULT_DAILY_CONSOLIDATION_PROMPT));
        assert!(
            configured_daily_prompt(&legacy).ends_with("Additional preferences:\nLegacy prompt")
        );
        assert_eq!(
            configured_daily_prompt(&BTreeMap::new()),
            DEFAULT_DAILY_CONSOLIDATION_PROMPT
        );
    }
}
