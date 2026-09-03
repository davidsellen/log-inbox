use crate::{llm, proposal_inbox::ProposalInbox, vault_context::VaultContextProvider};
use log_inbox_core::{models::DailyConsolidationJob, store::Store};
use serde_json::Value;
use std::{collections::HashSet, time::Duration};

const POLL_INTERVAL: Duration = Duration::from_millis(500);

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
    let mut task = "Consolidate the complete selected day into a readable daily report. Merge start, progress, and completion events for the same workstream; remove duplicates and trivial transport messages; organize distinct workstreams under concise level-three Markdown headings with 1-3 factual bullets each; retain decisions, outcomes, validation, blockers, and useful canonical links.".to_owned();
    if let Some(instructions) = preferences
        .get("consolidation_instructions")
        .filter(|value| !value.trim().is_empty())
    {
        task.push_str("\n\nUser preferences for this report:\n");
        task.push_str(instructions.trim());
    }
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
