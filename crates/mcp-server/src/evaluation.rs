//! Opt-in, read-only real-day evaluation. Uses the Store's shared content validators;
//! no database, API, or Markdown writer is used.

use crate::{knowledge, llm};
use anyhow::{Context, Result, ensure};
use log_inbox_core::{
    models::{ContextMapping, KnowledgeCollection, StoredLogEvent},
    validate_daily_evidence_content,
    workspace::InspectedWorkspace,
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    path::Path,
    time::Instant,
};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvaluationInput {
    workspace_root: String,
    collections: Vec<KnowledgeCollection>,
    mappings: Vec<ContextMapping>,
    cases: Vec<EvaluationCase>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EvaluationCase {
    date: String,
    events: Vec<StoredLogEvent>,
    snapshot_id: String,
    baseline_revision: Value,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn value_digest(value: &Value) -> Result<String> {
    Ok(digest(&serde_json::to_vec(value)?))
}

fn validate_candidate_for_save(
    content: &log_inbox_core::models::DailyRevisionContent,
    events: &[StoredLogEvent],
    context_payload: Option<&Value>,
) -> Result<()> {
    ensure!(
        content.schema_version == 1
            && content.manual_entry_ids.is_empty()
            && content
                .workstreams
                .iter()
                .all(|work| work.activity.is_empty()),
        "evaluation requires an automated summary without manual activity"
    );
    let ids = events
        .iter()
        .map(|event| event.id.clone())
        .collect::<Vec<_>>();
    validate_daily_evidence_content(&content.workstreams, &ids, context_payload)
}

/// Publish complete JSON only, using a hard link for atomic no-overwrite creation.
fn write_new_json(path: &Path, value: &Value) -> Result<()> {
    use std::os::unix::fs::OpenOptionsExt;
    let temporary = path.with_extension(format!("{}.tmp", uuid::Uuid::new_v4()));
    let result = (|| -> Result<()> {
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temporary)?;
        file.write_all(&serde_json::to_vec_pretty(value)?)?;
        file.sync_all()?;
        fs::hard_link(&temporary, path)
            .context("artifact already exists or cannot be published")?;
        fs::File::open(path.parent().context("artifact parent")?)?.sync_all()?;
        Ok(())
    })();
    let _ = fs::remove_file(&temporary);
    result
}

fn read_json(path: &Path) -> Result<Value> {
    ensure!(
        fs::symlink_metadata(path)?.file_type().is_file(),
        "artifact must be a regular file"
    );
    Ok(serde_json::from_slice(&read_bounded(path)?)?)
}

fn read_bounded(path: &Path) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(16 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    ensure!(
        bytes.len() <= 16 * 1024 * 1024,
        "evaluation input exceeds 16 MiB"
    );
    Ok(bytes)
}

fn validate_frozen_cases(manifest: &Value, input: &EvaluationInput) -> Result<()> {
    let cases = manifest["cases"]
        .as_array()
        .context("manifest cases missing")?;
    ensure!(
        cases.len() == input.cases.len(),
        "manifest case count mismatch"
    );
    for (frozen, case) in cases.iter().zip(&input.cases) {
        ensure!(
            frozen["date"] == case.date && frozen["snapshot_id"] == case.snapshot_id,
            "manifest case identity mismatch"
        );
        ensure!(
            frozen["event_ids"]
                == json!(
                    case.events
                        .iter()
                        .map(|event| &event.id)
                        .collect::<Vec<_>>()
                ),
            "frozen evidence IDs mismatch"
        );
        ensure!(
            frozen["events_sha256"] == digest(&serde_json::to_vec(&case.events)?),
            "frozen evidence hash mismatch"
        );
        ensure!(
            frozen["baseline_revision"] == case.baseline_revision,
            "frozen baseline mismatch"
        );
        ensure!(
            frozen["knowledge"]["vault_context"].is_object()
                && frozen["knowledge"]["snapshot_payload"].is_object(),
            "frozen Knowledge missing"
        );
        ensure!(
            frozen["knowledge_sha256"] == value_digest(&frozen["knowledge"])?,
            "frozen knowledge hash mismatch"
        );
    }
    Ok(())
}

/// Pure validation: replay never inspects a workspace or resolves current notes.
fn validate_replay(manifest: &Value, input: &EvaluationInput, identity: &Value) -> Result<()> {
    for field in [
        "fixture_sha256",
        "model",
        "request_profile",
        "timeout_seconds",
    ] {
        let declared_model_change = field == "model"
            && identity["model_change_from"].is_string()
            && identity["model_change_from"] == manifest["identity"]["model"];
        ensure!(
            !identity[field].is_null()
                && (manifest["identity"][field] == identity[field] || declared_model_change),
            "frozen manifest fixture/model/profile/timeout mismatch"
        );
    }
    validate_frozen_cases(manifest, input)
}

fn without_knowledge() -> Value {
    json!({
        "candidate_notes": [], "workstream_links": {}, "group_aliases": {},
        "knowledge": {"resolver_version": "none", "context_is_background_only": true,
            "excerpts": [], "workstream_excerpts": {}}
    })
}

fn validate_extraction_artifact(
    artifact: &Value,
    manifest_sha256: &str,
    case: &EvaluationCase,
) -> Result<()> {
    ensure!(
        artifact["manifest_sha256"] == manifest_sha256
            && artifact["date"] == case.date
            && artifact["events_sha256"] == digest(&serde_json::to_vec(&case.events)?)
            && matches!(
                artifact["status"].as_str(),
                Some("succeeded" | "failed" | "timed_out")
            ),
        "existing source extraction does not match evaluation"
    );
    ensure!(
        (artifact["status"] == "succeeded" && artifact["source_facts"].is_object())
            || (artifact["status"] != "succeeded" && artifact["source_facts"].is_null()),
        "source extraction status/content mismatch"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires private October 13–14 fixtures, output directory, and explicit local model"]
async fn evaluate_real_days() -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let input_path =
        std::env::var("LOG_INBOX_EVALUATION_INPUT").context("set private fixture path")?;
    let output_path =
        std::env::var("LOG_INBOX_EVALUATION_OUTPUT").context("set private output directory")?;
    ensure!(
        std::env::var("LOG_INBOX_LLM_MODEL").is_ok(),
        "explicit model required"
    );
    let config = llm::LlmConfig::from_env().context("explicit local LLM configuration required")?;
    ensure!(
        llm::knowledge_text_stays_local(&config),
        "evaluation requires a local model"
    );
    let endpoint = reqwest::Url::parse(&config.base_url)?;
    ensure!(
        endpoint.username().is_empty()
            && endpoint.password().is_none()
            && endpoint.query().is_none()
            && endpoint.fragment().is_none(),
        "plain local endpoint required"
    );
    let bytes = read_bounded(Path::new(&input_path))?;
    let input: EvaluationInput =
        serde_json::from_slice(&bytes).context("invalid evaluation fixture")?;
    ensure!(input.cases.len() == 2, "provide exactly October 13 and 14");
    for (case, date) in input.cases.iter().zip(["2026-10-13", "2026-10-14"]) {
        ensure!(
            case.date == date,
            "cases must be ordered October 13, October 14"
        );
        ensure!(
            !case.events.is_empty() && case.events.len() <= 500,
            "invalid event count"
        );
        ensure!(!case.snapshot_id.is_empty(), "snapshot identity required");
        let ids: std::collections::BTreeSet<_> =
            case.events.iter().map(|event| &event.id).collect();
        ensure!(ids.len() == case.events.len(), "duplicate evidence IDs");
    }
    let workspace_root = fs::canonicalize(&input.workspace_root)?;
    let output = Path::new(&output_path);
    if !output.exists() {
        fs::DirBuilder::new().mode(0o700).create(output)?;
    }
    ensure!(
        fs::symlink_metadata(output)?.file_type().is_dir(),
        "output must be a real directory"
    );
    let output = fs::canonicalize(output)?;
    ensure!(
        !output.starts_with(&workspace_root),
        "output must be outside the vault"
    );
    ensure!(
        !output.starts_with(fs::canonicalize(std::env::current_dir()?)?),
        "output must be outside the checkout"
    );
    ensure!(
        fs::metadata(&output)?.permissions().mode() & 0o077 == 0,
        "output directory must be private (0700)"
    );

    let mut identity = json!({
        "schema_version": 1,
        "pipeline": "verified-source-cards-then-source-only-topic-plan-and-readable-highlights",
        "extraction_reuse": "one verified card/topic ledger per date shared by both Knowledge arms",
        "source_preparation": "deterministic exact cards plus source-only proposed topic partition",
        "fixture_sha256": digest(&bytes),
        "model": config.model,
        "request_profile": config.profile,
        "configuration_digest": llm::generation_configuration_digest(&config),
        "contract_digest": llm::daily_generation_contract_digest(),
        "timeout_seconds": config.timeout_seconds(),
        "order": [
            {"date": "2026-10-13", "with_knowledge": false},
            {"date": "2026-10-13", "with_knowledge": true},
            {"date": "2026-10-14", "with_knowledge": true},
            {"date": "2026-10-14", "with_knowledge": false}
        ]
    });
    if let Ok(baseline_model) = std::env::var("LOG_INBOX_EVALUATION_BASELINE_MODEL") {
        identity["model_change_from"] = json!(baseline_model);
        identity["model_change_scope"] =
            json!("explicit model variation; original evidence and Knowledge unchanged");
    }
    let replay = std::env::var("LOG_INBOX_EVALUATION_FROZEN_MANIFEST")
        .ok()
        .map(|path| read_json(Path::new(&path)))
        .transpose()?;
    if let Some(replay) = &replay {
        validate_replay(replay, &input, &identity)?;
        identity["frozen_manifest_sha256"] = json!(value_digest(replay)?);
    }
    let manifest_path = output.join("manifest.json");
    let manifest = if manifest_path.exists() {
        let manifest = read_json(&manifest_path)?;
        ensure!(
            manifest["identity"] == identity,
            "existing evaluation has different input/model/contract"
        );
        validate_frozen_cases(&manifest, &input)?;
        if let Some(replay) = &replay {
            ensure!(
                manifest["cases"] == replay["cases"]
                    && manifest["frozen_replay"]
                        == json!({
                            "manifest_sha256": value_digest(replay)?, "original_identity": replay["identity"]
                        }),
                "existing evaluation differs from frozen replay"
            );
        }
        manifest
    } else if let Some(replay) = &replay {
        let manifest = json!({
            "identity": identity,
            "cases": replay["cases"],
            "frozen_replay": {"manifest_sha256": value_digest(replay)?, "original_identity": replay["identity"]}
        });
        write_new_json(&manifest_path, &manifest)?;
        manifest
    } else {
        let workspace = InspectedWorkspace::inspect(&workspace_root)?;
        let mut cases = Vec::new();
        for case in &input.cases {
            let resolution = knowledge::resolve_knowledge(
                &workspace,
                &input.collections,
                &input.mappings,
                &case.events,
            )
            .map_err(|_| {
                anyhow::anyhow!(
                    "Knowledge resolution failed; review selected collections and mappings"
                )
            })?
            .context("Knowledge evaluation requires enabled reviewed context")?;
            let frozen = json!({"vault_context": resolution.vault_context, "snapshot_payload": resolution.snapshot_payload});
            cases.push(json!({
                "date": case.date, "snapshot_id": case.snapshot_id,
                "event_ids": case.events.iter().map(|event| &event.id).collect::<Vec<_>>(),
                "events_sha256": digest(&serde_json::to_vec(&case.events)?),
                "baseline_revision": case.baseline_revision,
                "knowledge_sha256": value_digest(&frozen)?, "knowledge": frozen
            }));
        }
        let manifest = json!({"identity": identity, "cases": cases});
        write_new_json(&manifest_path, &manifest)?;
        manifest
    };
    let manifest_sha256 = value_digest(&manifest)?;
    for (index, case) in input.cases.iter().enumerate() {
        // Preparation is independent of Knowledge. Freeze raw cards and the
        // source-only proposed topic partition once for both composition arms.
        let extraction_path = output.join(format!("{}-source-facts.json", case.date));
        let extraction = if extraction_path.exists() {
            let prior = read_json(&extraction_path)?;
            validate_extraction_artifact(&prior, &manifest_sha256, case)?;
            prior
        } else {
            let started = Instant::now();
            println!(
                "evaluation date={} stage=source_extraction started=1",
                case.date
            );
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(config.timeout_seconds()),
                llm::extract_daily_sources(
                    &config,
                    &llm::SuggestMarkdownSummaryArgs {
                        vault_context: without_knowledge(),
                        mode: "daily-consolidation".to_owned(),
                        task: None,
                    },
                    &case.events,
                ),
            )
            .await;
            let mut artifact = json!({
                "schema_version": 1, "manifest_sha256": manifest_sha256,
                "date": case.date, "events_sha256": digest(&serde_json::to_vec(&case.events)?),
                "status": "failed", "error": null, "elapsed_ms": started.elapsed().as_millis(),
                "source_facts": null
            });
            match result {
                Ok(Ok(ledger)) => {
                    artifact["source_facts"] = serde_json::to_value(&ledger)?;
                    artifact["status"] = json!("succeeded");
                }
                Ok(Err(error)) => {
                    artifact["error"] = json!(
                        llm::daily_model_failure_code(&error).unwrap_or("source_extraction_failed")
                    )
                }
                Err(_) => {
                    artifact["status"] = json!("timed_out");
                    artifact["error"] =
                        json!("Source extraction exceeded configured total timeout");
                }
            }
            write_new_json(&extraction_path, &artifact)?;
            println!(
                "evaluation date={} stage=source_extraction status={} elapsed_ms={}",
                case.date, artifact["status"], artifact["elapsed_ms"]
            );
            artifact
        };
        let extraction_sha256 = value_digest(&extraction)?;
        let ledger = if extraction["status"] == "succeeded" {
            Some(serde_json::from_value(extraction["source_facts"].clone())?)
        } else {
            None
        };
        let order = if index == 0 {
            [false, true]
        } else {
            [true, false]
        };
        for with_knowledge in order {
            let arm = if with_knowledge {
                "withKnowledge"
            } else {
                "withoutKnowledge"
            };
            let path = output.join(format!("{}-{arm}.json", case.date));
            if path.exists() {
                let prior = read_json(&path)?;
                ensure!(
                    prior["manifest_sha256"] == manifest_sha256
                        && prior["date"] == case.date
                        && prior["with_knowledge"] == with_knowledge
                        && prior["extraction_sha256"] == extraction_sha256,
                    "existing arm does not match evaluation"
                );
                ensure!(
                    matches!(
                        prior["status"].as_str(),
                        Some("succeeded" | "failed" | "timed_out")
                    ),
                    "incomplete existing arm"
                );
                println!("evaluation date={} arm={arm} resumed=1", case.date);
                continue;
            }
            let args = llm::SuggestMarkdownSummaryArgs {
                vault_context: if with_knowledge {
                    manifest["cases"][index]["knowledge"]["vault_context"].clone()
                } else { without_knowledge() },
                mode: "daily-consolidation".to_owned(),
                task: Some("Create a concise, evidence-backed daily engineering record. Preserve distinct outcomes, decisions, trade-offs, validation, blockers, and follow-up.".to_owned()),
            };
            let started = Instant::now();
            let extraction_elapsed_ms = extraction["elapsed_ms"]
                .as_u64()
                .context("source extraction timing missing")?;
            // Charge the same shared extraction time to each arm, preserving
            // the previous total per-arm deadline rather than extending it.
            let remaining_ms = config
                .timeout_seconds()
                .saturating_mul(1000)
                .saturating_sub(extraction_elapsed_ms);
            let result = if let Some(ledger) = &ledger {
                tokio::time::timeout(std::time::Duration::from_millis(remaining_ms),
                    llm::compose_prepared_daily_summary(&config, args.clone(), case.events.clone(), ledger, |done, total, fallback| {
                        println!("evaluation date={} arm={arm} stage=composition completed={done} total={total} fallback={fallback}", case.date);
                        Ok(())
                    })).await
            } else {
                Ok(Err("source_extraction_failed".to_owned()))
            };
            let mut artifact = json!({
                "schema_version": 1, "manifest_sha256": manifest_sha256,
                "date": case.date, "with_knowledge": with_knowledge,
                "snapshot_id": case.snapshot_id, "elapsed_ms": started.elapsed().as_millis(),
                "stage": if ledger.is_some() { "composition" } else { "source_extraction" },
                "extraction_sha256": extraction_sha256,
                "extraction_elapsed_ms": extraction_elapsed_ms,
                "total_charged_elapsed_ms": extraction_elapsed_ms + started.elapsed().as_millis() as u64,
                "status": if extraction["status"] == "timed_out" { "timed_out" } else { "failed" }, "error": null,
                "parser_accepted": false, "persistable": false,
                "structured_draft": null, "daily_revision_content": null, "preview_markdown": null
            });
            match result {
                Ok(Ok(proposal)) => {
                    if let Some(draft) = proposal.structured_draft {
                        artifact["parser_accepted"] = json!(true);
                        artifact["structured_draft"] = serde_json::to_value(&draft)?;
                        let content = llm::daily_revision_content(draft, Vec::new(), &args);
                        artifact["preview_markdown"] =
                            json!(llm::render_daily_revision_preview(&content, &[], &[]));
                        artifact["daily_revision_content"] = serde_json::to_value(&content)?;
                        let context_payload = with_knowledge
                            .then_some(&manifest["cases"][index]["knowledge"]["snapshot_payload"]);
                        if validate_candidate_for_save(&content, &case.events, context_payload)
                            .is_ok()
                        {
                            artifact["persistable"] = json!(true);
                            artifact["status"] = json!("succeeded");
                        } else {
                            artifact["stage"] = json!("revision_validation");
                            artifact["error"] = json!("revision_contract_invalid");
                        }
                    } else {
                        artifact["error"] = json!("Generator omitted structured content");
                    }
                }
                // Fixed diagnostics never retain response text, endpoint details, or evidence prose.
                Ok(Err(error)) => {
                    artifact["error"] = if ledger.is_none() {
                        extraction["error"].clone()
                    } else {
                        json!(
                            llm::daily_model_failure_code(&error)
                                .unwrap_or("generation_or_provider_failure")
                        )
                    };
                }
                Err(_) => {
                    artifact["status"] = json!("timed_out");
                    artifact["error"] = json!("Evaluation arm exceeded configured total timeout");
                }
            }
            write_new_json(&path, &artifact)?;
            println!(
                "evaluation date={} arm={arm} elapsed_ms={} completed=1",
                case.date, artifact["elapsed_ms"]
            );
        }
    }
    Ok(())
}

/// Frozen source-mode comparison. Runs the production engine and save validators
/// without a model, database, note resolver or Markdown writer.
#[tokio::test]
#[ignore = "requires private frozen October 13–14 evidence and Knowledge; no inference"]
async fn evaluate_source_based_real_days() -> Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let input_bytes = read_bounded(Path::new(&std::env::var("LOG_INBOX_EVALUATION_INPUT")?))?;
    let input: EvaluationInput = serde_json::from_slice(&input_bytes)?;
    let frozen = read_json(Path::new(&std::env::var(
        "LOG_INBOX_EVALUATION_FROZEN_MANIFEST",
    )?))?;
    validate_frozen_cases(&frozen, &input)?;
    ensure!(
        frozen["identity"]["fixture_sha256"] == digest(&input_bytes),
        "frozen fixture changed"
    );
    ensure!(
        input.cases.len() == 2,
        "exactly two original dates required"
    );
    for (case, (date, count)) in input
        .cases
        .iter()
        .zip([("2026-10-13", 32), ("2026-10-14", 44)])
    {
        ensure!(
            case.date == date && case.events.len() == count,
            "original date/evidence count required"
        );
    }
    let requested = std::env::var("LOG_INBOX_EVALUATION_OUTPUT")?;
    let requested = Path::new(&requested);
    if !requested.exists() {
        fs::DirBuilder::new().mode(0o700).create(requested)?;
    }
    ensure!(
        fs::symlink_metadata(requested)?.file_type().is_dir(),
        "real output directory required"
    );
    let output = fs::canonicalize(requested)?;
    ensure!(
        !output.starts_with(fs::canonicalize(&input.workspace_root)?)
            && !output.starts_with(fs::canonicalize(std::env::current_dir()?)?),
        "private output outside vault and checkout required"
    );
    ensure!(
        fs::metadata(&output)?.permissions().mode() & 0o077 == 0,
        "private output permissions required"
    );
    let manifest = json!({
        "identity": {
            "schema_version": 1,
            "pipeline": "source-based-exact-reports-with-complete-history",
            "preparation_mode": "source_based", "model_used": false,
            "fixture_sha256": digest(&input_bytes),
            "contract_digest": llm::daily_generation_contract_digest(),
            "frozen_manifest_sha256": value_digest(&frozen)?,
            "knowledge_scope": "reviewed navigation only; frozen excerpts unused",
            "order": [
                {"date":"2026-10-13","with_knowledge":false},
                {"date":"2026-10-13","with_knowledge":true},
                {"date":"2026-10-14","with_knowledge":true},
                {"date":"2026-10-14","with_knowledge":false}
            ]
        },
        "cases": frozen["cases"]
    });
    write_new_json(&output.join("manifest.json"), &manifest)?;
    let manifest_sha256 = value_digest(&manifest)?;
    let mut succeeded = 0;
    for (index, case) in input.cases.iter().enumerate() {
        let started = Instant::now();
        let ledger = llm::prepare_source_based_ledger(&case.events).map_err(anyhow::Error::msg)?;
        llm::validate_daily_sources(&ledger, &case.events).map_err(anyhow::Error::msg)?;
        let extraction = json!({"date":case.date,"manifest_sha256":manifest_sha256,
            "status":"succeeded","elapsed_ms":started.elapsed().as_millis(),
            "events_sha256":digest(&serde_json::to_vec(&case.events)?),"source_facts":ledger});
        write_new_json(
            &output.join(format!("{}-source-facts.json", case.date)),
            &extraction,
        )?;
        let extraction_sha256 = value_digest(&extraction)?;
        let order = if index == 0 {
            [false, true]
        } else {
            [true, false]
        };
        for with_knowledge in order {
            let started = Instant::now();
            let arm = if with_knowledge {
                "withKnowledge"
            } else {
                "withoutKnowledge"
            };
            let mut vault_context = if with_knowledge {
                frozen["cases"][index]["knowledge"]["vault_context"].clone()
            } else {
                without_knowledge()
            };
            let mut payload = if with_knowledge {
                frozen["cases"][index]["knowledge"]["snapshot_payload"].clone()
            } else {
                json!({})
            };
            if with_knowledge {
                knowledge::normalize_source_based_resolution(
                    &mut vault_context,
                    &mut payload,
                    &case.events,
                )
                .map_err(anyhow::Error::msg)?;
            }
            let mut args = llm::SuggestMarkdownSummaryArgs {
                vault_context,
                mode: "daily-consolidation".into(),
                task: None,
            };
            llm::normalize_source_based_context(&mut args, &case.events)
                .map_err(anyhow::Error::msg)?;
            let result = llm::compose_source_based_prepared_summary(
                args.clone(),
                case.events.clone(),
                &ledger,
                |_, _, _| Ok(()),
            )
            .await;
            let mut artifact = json!({
                "schema_version":1,"manifest_sha256":manifest_sha256,
                "date":case.date,"with_knowledge":with_knowledge,"model_used":false,
                "preparation_mode":"source_based","snapshot_id":case.snapshot_id,
                "extraction_sha256":extraction_sha256,
                "elapsed_ms":started.elapsed().as_millis(),"status":"failed",
                "parser_accepted":false,"persistable":false,"error":null,
                "structured_draft":null,"daily_revision_content":null,"preview_markdown":null,
                "normalized_context_payload":if with_knowledge { payload.clone() } else { Value::Null }
            });
            match result {
                Ok(proposal) => {
                    if let Some(draft) = proposal.structured_draft {
                        artifact["parser_accepted"] = json!(true);
                        artifact["structured_draft"] = serde_json::to_value(&draft)?;
                        let content = llm::daily_revision_content(draft, Vec::new(), &args);
                        artifact["preview_markdown"] =
                            json!(llm::render_daily_revision_preview(&content, &[], &[]));
                        artifact["daily_revision_content"] = serde_json::to_value(&content)?;
                        match validate_candidate_for_save(
                            &content,
                            &case.events,
                            with_knowledge.then_some(&payload),
                        ) {
                            Ok(()) => {
                                artifact["persistable"] = json!(true);
                                artifact["status"] = json!("succeeded");
                                succeeded += 1;
                            }
                            Err(error) => artifact["error"] = json!(error.to_string()),
                        }
                    } else {
                        artifact["error"] = json!("structured content missing");
                    }
                }
                Err(error) => artifact["error"] = json!(error),
            }
            write_new_json(&output.join(format!("{}-{arm}.json", case.date)), &artifact)?;
            println!(
                "source evaluation date={} arm={arm} status={} elapsed_ms={}",
                case.date, artifact["status"], artifact["elapsed_ms"]
            );
        }
    }
    ensure!(
        succeeded == 4,
        "only {succeeded}/4 candidates passed both production parser and save contract"
    );
    Ok(())
}

#[test]
#[ignore = "requires private October 13–14 fixtures; performs no model calls"]
fn preflight_real_day_source_cards() -> Result<()> {
    let path = std::env::var("LOG_INBOX_EVALUATION_INPUT").context("set private fixture path")?;
    let input: EvaluationInput = serde_json::from_slice(&read_bounded(Path::new(&path))?)?;
    let frozen = std::env::var("LOG_INBOX_EVALUATION_FROZEN_MANIFEST")
        .ok()
        .map(|path| read_json(Path::new(&path)))
        .transpose()?;
    if let Some(manifest) = &frozen {
        validate_frozen_cases(manifest, &input)?;
    }
    let mut layouts = Vec::new();
    for (index, case) in input.cases.iter().enumerate() {
        llm::validate_source_extractability(&case.events)
            .map_err(|error| anyhow::anyhow!("{}: {error}", case.date))?;
        let mut contexts = vec![without_knowledge()];
        if let Some(manifest) = &frozen {
            contexts.push(manifest["cases"][index]["knowledge"]["vault_context"].clone());
        }
        for (context_index, vault_context) in contexts.into_iter().enumerate() {
            let args = llm::SuggestMarkdownSummaryArgs {
                vault_context,
                mode: "daily-consolidation".to_owned(),
                task: None,
            };
            let groups = llm::validate_prepared_source_layout(&case.events, &args)
                .map_err(|error| anyhow::anyhow!("{} layout: {error}", case.date))?;
            layouts.push(json!({
                "date": case.date, "with_knowledge": context_index != 0, "groups": groups
            }));
        }
        println!(
            "source card and mandatory-layout preflight date={} events={} passed",
            case.date,
            case.events.len()
        );
    }
    if let Ok(output) = std::env::var("LOG_INBOX_EVALUATION_PREFLIGHT_OUTPUT") {
        let requested = Path::new(&output);
        let parent = fs::canonicalize(requested.parent().context("preflight output parent")?)?;
        let path = parent.join(requested.file_name().context("preflight output file")?);
        ensure!(
            !path.starts_with(fs::canonicalize(&input.workspace_root)?)
                && !path.starts_with(fs::canonicalize(std::env::current_dir()?)?),
            "preflight output must be outside the workspace and checkout"
        );
        write_new_json(&path, &json!({"layouts": layouts}))?;
    }
    Ok(())
}

/// Validate a private human oracle against the unchanged production choice
/// parser and save contract. No inference, workspace reads, database or writer.
#[test]
#[ignore = "requires private frozen evidence and exact catalog selections; no inference"]
fn validate_source_selection_oracle() -> Result<()> {
    use std::{collections::BTreeMap, os::unix::fs::PermissionsExt};
    let input_bytes = read_bounded(Path::new(&std::env::var("LOG_INBOX_EVALUATION_INPUT")?))?;
    let input: EvaluationInput = serde_json::from_slice(&input_bytes)?;
    let frozen = read_json(Path::new(&std::env::var(
        "LOG_INBOX_EVALUATION_FROZEN_MANIFEST",
    )?))?;
    validate_frozen_cases(&frozen, &input)?;
    ensure!(
        frozen["identity"]["fixture_sha256"] == digest(&input_bytes),
        "frozen fixture changed"
    );
    let selections_path = std::env::var("LOG_INBOX_SELECTION_ORACLE")?;
    let selections_path = Path::new(&selections_path);
    ensure!(
        fs::symlink_metadata(selections_path)?.file_type().is_file()
            && fs::metadata(selections_path)?.permissions().mode() & 0o077 == 0,
        "private regular selection file required"
    );
    let oracle = read_json(selections_path)?;
    let dates = oracle["dates"]
        .as_array()
        .context("oracle dates required")?;
    ensure!(
        dates.len() == input.cases.len(),
        "exact original dates required"
    );
    let output = capability_output(&input)?;
    let card_refs = std::env::var("LOG_INBOX_SELECTION_ORACLE_CARD_REFS").as_deref() == Ok("1");
    let flat_refs = std::env::var("LOG_INBOX_SELECTION_ORACLE_FLAT_REFS").as_deref() == Ok("1");
    let manifest = json!({"fixture_sha256":digest(&input_bytes),
        "frozen_manifest_sha256":value_digest(&frozen)?,
        "oracle_sha256":value_digest(&oracle)?,
        "contract_digest":llm::daily_generation_contract_digest(),
        "model_used":false,"card_refs":card_refs,"flat_refs":flat_refs,"pipeline":"exact-source-choice-oracle"});
    write_new_json(&output.join("oracle-manifest.json"), &manifest)?;
    let mut passed = 0;
    for (case_index, (case, selected)) in input.cases.iter().zip(dates).enumerate() {
        ensure!(selected["date"] == case.date, "oracle date/order mismatch");
        let mut groups = BTreeMap::new();
        for group in selected["groups"]
            .as_array()
            .context("oracle groups required")?
        {
            let id = group["group_id"]
                .as_str()
                .context("oracle group ID required")?;
            let selection = if flat_refs {
                let fields: BTreeMap<String, Vec<Vec<usize>>> =
                    serde_json::from_value(group["selection_fact_refs"].clone())?;
                json!({"selected":fields.into_values().flatten().flatten().collect::<Vec<_>>()})
            } else {
                group[if card_refs {
                    "selection_fact_refs"
                } else {
                    "selection"
                }]
                .clone()
            };
            ensure!(
                groups.insert(id.to_owned(), selection).is_none(),
                "duplicate oracle group"
            );
        }
        let ledger = llm::prepare_source_based_ledger(&case.events).map_err(anyhow::Error::msg)?;
        for with_knowledge in [false, true] {
            let mut context = if with_knowledge {
                frozen["cases"][case_index]["knowledge"]["vault_context"].clone()
            } else {
                without_knowledge()
            };
            let mut payload = if with_knowledge {
                frozen["cases"][case_index]["knowledge"]["snapshot_payload"].clone()
            } else {
                json!({})
            };
            if with_knowledge {
                knowledge::normalize_source_based_resolution(
                    &mut context,
                    &mut payload,
                    &case.events,
                )
                .map_err(anyhow::Error::msg)?;
            }
            let mut args = llm::SuggestMarkdownSummaryArgs {
                mode: "daily-consolidation".into(),
                task: None,
                vault_context: context,
            };
            llm::normalize_source_based_context(&mut args, &case.events)
                .map_err(anyhow::Error::msg)?;
            let validate = if flat_refs {
                llm::validate_source_based_flat_selection
            } else if card_refs {
                llm::validate_source_based_card_selection
            } else {
                llm::validate_source_based_selection
            };
            let draft = validate(args.clone(), &case.events, &ledger, &groups)
                .map_err(anyhow::Error::msg)?;
            let content = llm::daily_revision_content(draft.clone(), Vec::new(), &args);
            validate_candidate_for_save(
                &content,
                &case.events,
                with_knowledge.then_some(&payload),
            )?;
            let arm = if with_knowledge {
                "withKnowledge"
            } else {
                "withoutKnowledge"
            };
            write_new_json(
                &output.join(format!("{}-{arm}-oracle.json", case.date)),
                &json!({
                    "date":case.date,"with_knowledge":with_knowledge,"status":"succeeded",
                    "parser_accepted":true,"persistable":true,"model_used":false,
                    "manifest_sha256":value_digest(&manifest)?,"source_ledger_sha256":value_digest(&serde_json::to_value(&ledger)?)?,
                    "structured_draft":draft,"daily_revision_content":content,
                    "preview_markdown":llm::render_daily_revision_preview(&content,&[],&[])
                }),
            )?;
            passed += 1;
            println!(
                "source selection oracle date={} arm={arm} persistable=true",
                case.date
            );
        }
    }
    ensure!(
        passed == input.cases.len() * 2,
        "all original context arms required"
    );
    Ok(())
}

fn generic_probe_events(events: &[StoredLogEvent]) -> Vec<StoredLogEvent> {
    [
        (
            "Completed investigation.",
            json!({"tests":"29 checks passed"}),
        ),
        (
            "Implemented duplicate callback suppression while preserving the first response.",
            json!({}),
        ),
        (
            "Expired signing key is a probable cause; fresh login remains unverified.",
            json!({"follow_up":"Fresh login and live acceptance remain pending."}),
        ),
    ]
    .into_iter()
    .enumerate()
    .map(|(index, (message, metadata))| {
        let mut event = events[index].clone();
        event.id = format!("generic-probe-{index}");
        event.message = message.into();
        event.metadata = serde_json::from_value(metadata).unwrap();
        event.metadata.insert("repo".into(), json!("generic-probe"));
        event
            .metadata
            .insert("task_id".into(), json!("selection-capability"));
        event
    })
    .collect()
}

/// Freeze and run one explicit local selection-only capability trial.
#[tokio::test]
#[ignore = "explicit single local-model selection-only capability trial"]
async fn probe_source_card_selection() -> Result<()> {
    use std::collections::BTreeMap;
    let config = llm::LlmConfig::from_env().context("explicit local model required")?;
    let input_bytes = read_bounded(Path::new(&std::env::var("LOG_INBOX_EVALUATION_INPUT")?))?;
    let mut input: EvaluationInput = serde_json::from_slice(&input_bytes)?;
    let frozen = read_json(Path::new(&std::env::var(
        "LOG_INBOX_EVALUATION_FROZEN_MANIFEST",
    )?))?;
    validate_frozen_cases(&frozen, &input)?;
    ensure!(
        frozen["identity"]["fixture_sha256"] == digest(&input_bytes),
        "frozen fixture changed"
    );
    let generic = std::env::var("LOG_INBOX_SELECTION_PROBE_STAGE").as_deref() != Ok("real");
    let mut diagnostic_proof = Value::Null;
    if generic {
        let case = &mut input.cases[0];
        case.events = generic_probe_events(&case.events);
        input.cases.truncate(1);
    } else if let Ok(manifest_path) = std::env::var("LOG_INBOX_SELECTION_PROBE_DIAGNOSTIC_MANIFEST")
    {
        let manifest = read_json(Path::new(&manifest_path))?;
        let response = read_json(Path::new(&std::env::var(
            "LOG_INBOX_SELECTION_PROBE_DIAGNOSTIC_RESPONSE",
        )?))?;
        let decision = read_json(Path::new(&std::env::var(
            "LOG_INBOX_SELECTION_PROBE_DIAGNOSTIC_DECISION",
        )?))?;
        diagnostic_proof = validate_diagnostic_generic_proof(
            &config,
            &input.cases[0].events,
            &digest(&input_bytes),
            &manifest,
            &response,
            &decision,
        )?;
    } else {
        let proof = read_json(Path::new(&std::env::var(
            "LOG_INBOX_SELECTION_PROBE_GENERIC_PROOF",
        )?))?;
        ensure!(
            proof["generic"] == true
                && proof["status"] == "succeeded"
                && proof["protocol"] == "flat-source-card-selection-v2"
                && proof["model"] == config.model
                && proof["generic_gate"] == "substantive-qualified-no-noise-v1"
                && proof["selection_policy_sha256"] == llm::source_card_probe_policy_digest(),
            "successful generic proof required"
        );
    }
    let output = capability_output(&input)?;
    ensure!(
        llm::knowledge_text_stays_local(&config) && config.api_key.is_none(),
        "private local no-key endpoint required"
    );
    let endpoint = reqwest::Url::parse(&config.base_url)?;
    ensure!(
        endpoint.username().is_empty()
            && endpoint.password().is_none()
            && endpoint.query().is_none()
            && endpoint.fragment().is_none(),
        "plain local endpoint required"
    );
    let args = llm::SuggestMarkdownSummaryArgs {
        mode: "daily-consolidation".into(),
        task: None,
        vault_context: without_knowledge(),
    };
    let mut prepared = Vec::new();
    for case in &input.cases {
        let ledger = llm::prepare_source_based_ledger(&case.events).map_err(anyhow::Error::msg)?;
        let requests = llm::source_card_probe_requests(&config, &args, &case.events, &ledger)
            .map_err(anyhow::Error::msg)?;
        prepared.push((ledger, requests));
    }
    write_new_json(
        &output.join("probe-manifest.json"),
        &json!({"diagnostic_only":!diagnostic_proof.is_null(),"generic_diagnostic_proof":diagnostic_proof,"generic":generic,"generic_gate":"substantive-qualified-no-noise-v1","model":config.model,"protocol":"flat-source-card-selection-v2","selection_policy_sha256":llm::source_card_probe_policy_digest(),"timeout_seconds":config.timeout_seconds(),"request_profile":config.profile,
        "model_change_from":frozen["identity"]["model"],"fixture_sha256":digest(&input_bytes),
        "frozen_manifest_sha256":value_digest(&frozen)?,"contract_digest":llm::daily_generation_contract_digest(),
        "budget":{"facts_per_group":8,"highlight_text_characters_per_group":1600,"cards_per_bundle":3,"characters_per_bundle":320,"bundles_per_field":4},
        "paired_plan_shared":true,"knowledge_fact_prompt":false,
        "requests":prepared.iter().map(|(_,request)|request).collect::<Vec<_>>() }),
    )?;
    let mut failed_dates = 0;
    for ((case_index, case), (ledger, requests)) in input.cases.iter().enumerate().zip(prepared) {
        let day_started = Instant::now();
        let mut selections = BTreeMap::new();
        let result: Result<()> = async {
        for (index, request) in requests
            .as_array()
            .context("requests required")?
            .iter()
            .enumerate()
        {
            let started = Instant::now();
            let model_used = request["model_used"] != false;
            let response = if model_used {
                llm::execute_source_card_probe_request(&config, &request["request"]).await
            } else {
                Ok(request["selection"].to_string())
            };
            write_new_json(
                &output.join(format!("{}-group-{index}-response.json", case.date)),
                &json!({"response":response.as_ref().ok(),"error":response.as_ref().err(),"elapsed_ms":started.elapsed().as_millis(),"model_used":model_used}),
            )?;
            let selection = serde_json::from_str(&response.map_err(anyhow::Error::msg)?)?;
            selections.insert(
                request["group_id"].as_str().context("group ID")?.to_owned(),
                selection,
            );
        }
        let draft = llm::validate_source_based_flat_selection(
            args.clone(),
            &case.events,
            &ledger,
            &selections,
        )
        .map_err(anyhow::Error::msg)?;
        llm::validate_source_card_probe_budget(&draft).map_err(anyhow::Error::msg)?;
        if generic {
            let highlights = draft
                .workstreams
                .iter()
                .flat_map(|work| {
                    [
                        &work.outcome,
                        &work.decision,
                        &work.trade_off,
                        &work.validation,
                        &work.blocker,
                        &work.follow_up,
                    ]
                    .into_iter()
                    .flatten()
                })
                .map(|fact| fact.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            validate_generic_probe_highlights(&highlights)?;
        }
        for with_knowledge in if generic {
            vec![false]
        } else {
            if case_index == 0 {vec![false,true]} else {vec![true,false]}
        } {
            let mut arm_args = args.clone();
            let mut payload = json!({});
            if with_knowledge {
                let index = input
                    .cases
                    .iter()
                    .position(|item| item.date == case.date)
                    .unwrap();
                arm_args.vault_context =
                    frozen["cases"][index]["knowledge"]["vault_context"].clone();
                payload = frozen["cases"][index]["knowledge"]["snapshot_payload"].clone();
                knowledge::normalize_source_based_resolution(
                    &mut arm_args.vault_context,
                    &mut payload,
                    &case.events,
                )
                .map_err(anyhow::Error::msg)?;
            }
            llm::normalize_source_based_context(&mut arm_args, &case.events)
                .map_err(anyhow::Error::msg)?;
            let content = llm::daily_revision_content(draft.clone(), Vec::new(), &arm_args);
            validate_candidate_for_save(
                &content,
                &case.events,
                with_knowledge.then_some(&payload),
            )?;
            write_new_json(
                &output.join(format!(
                    "{}-{}-probe.json",
                    case.date,
                    if with_knowledge {
                        "withKnowledge"
                    } else {
                        "withoutKnowledge"
                    }
                )),
                &json!({"diagnostic_only":!diagnostic_proof.is_null(),"strict_generic_gate":if diagnostic_proof.is_null(){"passed"}else{"failed"},"elapsed_since_day_start_ms":day_started.elapsed().as_millis(),"status":"succeeded","generic":generic,"generic_gate":"substantive-qualified-no-noise-v1","protocol":"flat-source-card-selection-v2","selection_policy_sha256":llm::source_card_probe_policy_digest(),"model":config.model,"date":case.date,"with_knowledge":with_knowledge,"selections":selections,"structured_draft":draft,"daily_revision_content":content,"preview_markdown":llm::render_daily_revision_preview(&content,&[],&[])}),
            )?;
        }
        Ok(())
        }.await;
        if result.is_err() {
            failed_dates += 1;
        }
        write_new_json(
            &output.join(format!("{}-day-result.json", case.date)),
            &json!({
                "date":case.date,"status":if result.is_ok(){"succeeded"}else{"failed"},
                "error":result.as_ref().err().map(ToString::to_string),"elapsed_ms":day_started.elapsed().as_millis(),
            "diagnostic_only":!diagnostic_proof.is_null(),"strict_generic_gate":if !diagnostic_proof.is_null() || (generic && result.is_err()){"failed"}else{"passed"},
                "selections":selections,"model":config.model
            }),
        )?;
    }
    ensure!(
        failed_dates == 0,
        "{failed_dates} evaluation dates failed; immutable diagnostics retained"
    );
    Ok(())
}

fn validate_generic_probe_positives(highlights: &str) -> Result<()> {
    ensure!(
        [
            "Implemented duplicate callback suppression",
            "probable cause",
            "unverified",
            "pending"
        ]
        .iter()
        .all(|required| highlights.contains(required)),
        "generic substantive behavior/probable/unverified/pending selection gate failed"
    );
    Ok(())
}

fn validate_generic_probe_highlights(highlights: &str) -> Result<()> {
    validate_generic_probe_positives(highlights)?;
    ensure!(
        !highlights.contains("Completed investigation") && !highlights.contains("29 checks passed"),
        "generic substantive behavior/probable/unverified/pending/no-noise selection gate failed"
    );
    Ok(())
}

fn validate_diagnostic_generic_proof(
    config: &llm::LlmConfig,
    original_events: &[StoredLogEvent],
    fixture_sha256: &str,
    manifest: &Value,
    response: &Value,
    decision: &Value,
) -> Result<Value> {
    ensure!(
        manifest["generic"] == true
            && manifest["model"] == config.model
            && manifest["protocol"] == "flat-source-card-selection-v2"
            && manifest["generic_gate"] == "substantive-qualified-no-noise-v1"
            && manifest["selection_policy_sha256"] == llm::source_card_probe_policy_digest()
            && manifest["fixture_sha256"] == fixture_sha256,
        "diagnostic proof model/protocol/policy/fixture mismatch"
    );
    ensure!(
        response["error"].is_null() && response["model_used"] == true,
        "actual generic model response required"
    );
    let selection: Value = serde_json::from_str(
        response["response"]
            .as_str()
            .context("generic response missing")?,
    )?;
    ensure!(
        decision["decision"]
            == "Permit one original-frozen real-day diagnostic after larger-model boilerplate-only generic failure."
            && decision["observed_generic_result"]["model"] == config.model
            && decision["observed_generic_result"]["selected"] == selection["selected"]
            && decision["observed_generic_result"]["elapsed_ms"] == response["elapsed_ms"],
        "diagnostic decision does not match observed generic response"
    );
    let events = generic_probe_events(original_events);
    let args = llm::SuggestMarkdownSummaryArgs {
        mode: "daily-consolidation".into(),
        task: None,
        vault_context: without_knowledge(),
    };
    let ledger = llm::prepare_source_based_ledger(&events).map_err(anyhow::Error::msg)?;
    let expected = llm::source_card_probe_requests(config, &args, &events, &ledger)
        .map_err(anyhow::Error::msg)?;
    ensure!(
        manifest["requests"]
            .as_array()
            .is_some_and(|cases| cases.len() == 1)
            && manifest["requests"][0]
                .as_array()
                .is_some_and(|groups| groups.len() == 1)
            && expected.as_array().is_some_and(|groups| groups.len() == 1)
            && manifest["requests"][0][0]["request"] == expected[0]["request"],
        "diagnostic generic request differs from unchanged policy/sample"
    );
    let id = expected[0]["group_id"]
        .as_str()
        .context("generic group missing")?
        .to_owned();
    let draft = llm::validate_source_based_flat_selection(
        args.clone(),
        &events,
        &ledger,
        &std::collections::BTreeMap::from([(id, selection.clone())]),
    )
    .map_err(anyhow::Error::msg)?;
    let content = llm::daily_revision_content(draft, Vec::new(), &args);
    validate_candidate_for_save(&content, &events, None)?;
    let highlights = content
        .workstreams
        .iter()
        .flat_map(|work| {
            [
                &work.outcome,
                &work.decision,
                &work.trade_off,
                &work.validation,
                &work.blocker,
                &work.follow_up,
            ]
            .into_iter()
            .flatten()
        })
        .map(|fact| fact.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    validate_generic_probe_positives(&highlights)?;
    ensure!(
        validate_generic_probe_highlights(&highlights).is_err(),
        "diagnostic exception requires strict noise failure"
    );
    Ok(
        json!({"diagnostic_only":true,"strict_generic_gate":"failed","substantive_positives":"passed",
        "manifest_sha256":value_digest(manifest)?,"response_sha256":value_digest(response)?,"decision_sha256":value_digest(decision)?,
        "selected":selection["selected"],"model":config.model,"selection_policy_sha256":llm::source_card_probe_policy_digest()}),
    )
}

#[test]
fn generic_probe_gate_requires_qualifications_and_rejects_admin_and_count_noise() -> Result<()> {
    let accepted = "Implemented duplicate callback suppression. Expired signing key is a probable cause; fresh login remains unverified. Fresh login and live acceptance remain pending.";
    validate_generic_probe_highlights(accepted)?;
    for required in [
        "Implemented duplicate callback suppression",
        "probable cause",
        "unverified",
        "pending",
    ] {
        ensure!(validate_generic_probe_highlights(&accepted.replace(required, "")).is_err());
    }
    for noise in ["Completed investigation.", "29 checks passed"] {
        validate_generic_probe_positives(&format!("{accepted} {noise}"))?;
        ensure!(validate_generic_probe_highlights(&format!("{accepted} {noise}")).is_err());
    }
    Ok(())
}

#[test]
#[ignore = "requires private frozen failed generic response; validates only, no inference"]
fn audit_failed_generic_diagnostic_proof() -> Result<()> {
    let config = llm::LlmConfig::from_env().context("explicit model required")?;
    let input_bytes = read_bounded(Path::new(&std::env::var("LOG_INBOX_EVALUATION_INPUT")?))?;
    let input: EvaluationInput = serde_json::from_slice(&input_bytes)?;
    let frozen = read_json(Path::new(&std::env::var(
        "LOG_INBOX_EVALUATION_FROZEN_MANIFEST",
    )?))?;
    validate_frozen_cases(&frozen, &input)?;
    ensure!(
        frozen["identity"]["fixture_sha256"] == digest(&input_bytes),
        "frozen fixture changed"
    );
    let manifest = read_json(Path::new(&std::env::var(
        "LOG_INBOX_SELECTION_PROBE_DIAGNOSTIC_MANIFEST",
    )?))?;
    let response = read_json(Path::new(&std::env::var(
        "LOG_INBOX_SELECTION_PROBE_DIAGNOSTIC_RESPONSE",
    )?))?;
    let decision = read_json(Path::new(&std::env::var(
        "LOG_INBOX_SELECTION_PROBE_DIAGNOSTIC_DECISION",
    )?))?;
    let check = |manifest: &Value, response: &Value, decision: &Value| {
        validate_diagnostic_generic_proof(
            &config,
            &input.cases[0].events,
            &digest(&input_bytes),
            manifest,
            response,
            decision,
        )
    };
    let proof = check(&manifest, &response, &decision)?;
    ensure!(proof["strict_generic_gate"] == "failed" && proof["substantive_positives"] == "passed");
    for key in [
        "model",
        "protocol",
        "selection_policy_sha256",
        "fixture_sha256",
    ] {
        let mut changed = manifest.clone();
        changed[key] = json!("tampered");
        ensure!(check(&changed, &response, &decision).is_err());
    }
    let mut altered_request = manifest.clone();
    altered_request["requests"][0][0]["request"]["messages"][1]["content"] =
        json!("different policy");
    ensure!(check(&altered_request, &response, &decision).is_err());
    let mut missing = response.clone();
    missing["response"] = json!("{\"selected\":[0,2,4]}");
    let mut amended_decision = decision.clone();
    amended_decision["observed_generic_result"]["selected"] = json!([0, 2, 4]);
    ensure!(check(&manifest, &missing, &amended_decision).is_err());
    write_new_json(
        &capability_output(&input)?.join("diagnostic-proof.json"),
        &proof,
    )?;
    Ok(())
}

#[cfg(unix)]
fn capability_output(input: &EvaluationInput) -> Result<std::path::PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let requested = std::env::var("LOG_INBOX_CAPABILITY_OUTPUT")?;
    let path = Path::new(&requested);
    ensure!(
        fs::symlink_metadata(path)?.file_type().is_dir(),
        "real output directory required"
    );
    let output = fs::canonicalize(path)?;
    ensure!(
        !output.starts_with(fs::canonicalize(&input.workspace_root)?)
            && !output.starts_with(fs::canonicalize(std::env::current_dir()?)?)
            && fs::metadata(&output)?.permissions().mode() & 0o077 == 0,
        "private output outside workspace and checkout required"
    );
    Ok(output)
}

fn capability_window_ids(item: &Value) -> Result<(Vec<String>, Vec<String>)> {
    let ids: Vec<String> = serde_json::from_value(item["card_ids"].clone())?;
    ensure!(
        (2..=24).contains(&ids.len()),
        "bounded capability sample required"
    );
    match std::env::var("LOG_INBOX_CAPABILITY_STAGE")
        .as_deref()
        .unwrap_or("single")
    {
        "single" => Ok((ids, Vec::new())),
        "prefix" => Ok((ids[..ids.len() / 2].to_vec(), Vec::new())),
        "continuation" => Ok((ids[..ids.len() / 2].to_vec(), ids[ids.len() / 2..].to_vec())),
        _ => anyhow::bail!("unknown capability stage"),
    }
}

fn capability_prefix_content(id: &str) -> Result<String> {
    let prefix = std::env::var("LOG_INBOX_CAPABILITY_PREFIX_OUTPUT")?;
    let response = read_json(&Path::new(&prefix).join(format!("{id}-response.json")))?;
    ensure!(
        response["choices"][0]["finish_reason"] == "stop",
        "prefix did not finish normally"
    );
    Ok(response["choices"][0]["message"]["content"]
        .as_str()
        .context("prefix content required")?
        .to_owned())
}

#[tokio::test]
#[ignore = "requires private failed-preparation fixture; performs no model calls"]
async fn export_first_daily_topic_request() -> Result<()> {
    let input: EvaluationInput = serde_json::from_slice(&read_bounded(Path::new(
        &std::env::var("LOG_INBOX_EVALUATION_INPUT")?,
    ))?)?;
    let output = capability_output(&input)?;
    let config = llm::LlmConfig::from_env().context("explicit local model required")?;
    ensure!(
        llm::knowledge_text_stays_local(&config),
        "local model required"
    );
    let case = input.cases.first().context("evaluation case required")?;
    let exported = llm::export_first_daily_topic_request(&config, &case.events)
        .await
        .map_err(anyhow::Error::msg)?;
    write_new_json(&output.join("first-daily-topic-request.json"), &exported)?;
    println!("first daily topic request exported date={}", case.date);
    Ok(())
}

#[tokio::test]
#[ignore = "requires private frozen capability sample"]
async fn export_semantic_capability_requests() -> Result<()> {
    let input: EvaluationInput = serde_json::from_slice(&read_bounded(Path::new(
        &std::env::var("LOG_INBOX_EVALUATION_INPUT")?,
    ))?)?;
    let sample = read_json(Path::new(&std::env::var("LOG_INBOX_CAPABILITY_INPUT")?))?;
    let output = capability_output(&input)?;
    let config = llm::LlmConfig::from_env().context("explicit local model required")?;
    ensure!(
        llm::knowledge_text_stays_local(&config),
        "local model required"
    );
    for item in sample["samples"].as_array().context("samples required")? {
        let id = item["id"].as_str().context("sample ID required")?;
        ensure!(
            id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "invalid sample ID"
        );
        let case = input
            .cases
            .iter()
            .find(|case| item["date"] == case.date)
            .context("unknown sample date")?;
        let (ids, next) = capability_window_ids(item)?;
        let exported = if next.is_empty() {
            llm::export_topic_window_request(&config, &case.events, &ids).await
        } else {
            llm::export_topic_continuation_request(
                &config,
                &case.events,
                &ids,
                &capability_prefix_content(id)?,
                &next,
            )
            .await
        }
        .map_err(anyhow::Error::msg)?;
        write_new_json(&output.join(format!("{id}-request.json")), &exported)?;
        println!(
            "capability request exported sample={id} cards={}",
            ids.len()
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires private frozen capability responses"]
async fn validate_semantic_capability_responses() -> Result<()> {
    let input: EvaluationInput = serde_json::from_slice(&read_bounded(Path::new(
        &std::env::var("LOG_INBOX_EVALUATION_INPUT")?,
    ))?)?;
    let sample = read_json(Path::new(&std::env::var("LOG_INBOX_CAPABILITY_INPUT")?))?;
    let output = capability_output(&input)?;
    for item in sample["samples"].as_array().context("samples required")? {
        let id = item["id"].as_str().context("sample ID required")?;
        ensure!(
            id.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
            "invalid sample ID"
        );
        let case = input
            .cases
            .iter()
            .find(|case| item["date"] == case.date)
            .context("unknown sample date")?;
        let (ids, next) = capability_window_ids(item)?;
        let response = read_json(&output.join(format!("{id}-response.json")))?;
        let choice = &response["choices"][0];
        ensure!(
            choice["finish_reason"] == "stop",
            "response did not finish normally"
        );
        let content = choice["message"]["content"]
            .as_str()
            .context("content required")?;
        let result = if next.is_empty() {
            llm::validate_topic_window_response(&case.events, &ids, content).await
        } else {
            llm::validate_topic_continuation_response(
                &case.events,
                &ids,
                &capability_prefix_content(id)?,
                &next,
                content,
            )
            .await
        };
        let validated = match result {
            Ok(topics) => json!({"structurally_valid":true,"proposed":topics}),
            Err(error) => json!({"structurally_valid":false,"error":error}),
        };
        write_new_json(&output.join(format!("{id}-validation.json")), &validated)?;
        println!(
            "capability response sample={id} structurally_valid={}",
            validated["structurally_valid"]
        );
    }
    Ok(())
}

#[tokio::test]
#[ignore = "requires private current-contract source ledgers for request inspection"]
async fn inspect_current_selection_requests() -> Result<()> {
    let input: EvaluationInput = serde_json::from_slice(&read_bounded(Path::new(
        &std::env::var("LOG_INBOX_EVALUATION_INPUT")?,
    ))?)?;
    let manifest = read_json(Path::new(&std::env::var(
        "LOG_INBOX_EVALUATION_FROZEN_MANIFEST",
    )?))?;
    validate_frozen_cases(&manifest, &input)?;
    let output = capability_output(&input)?;
    let source_directory = std::env::var("LOG_INBOX_CAPABILITY_SOURCE_LEDGERS")?;
    let config = llm::LlmConfig::from_env().context("explicit local model required")?;
    for (case_index, case) in input.cases.iter().enumerate() {
        let prior = read_json(
            &Path::new(&source_directory).join(format!("{}-source-facts.json", case.date)),
        )?;
        // Current-only inspection: never upgrade a historical interpretation by
        // stamping its contract or inventing the newly required subject labels.
        ensure!(
            prior["status"] == "succeeded",
            "successful source ledger required"
        );
        ensure!(
            prior["source_facts"]["contract"] == llm::DAILY_GENERATION_CONTRACT,
            "current-contract source ledger required; historical comparisons remain frozen artifacts"
        );
        let ledger = serde_json::from_value(prior["source_facts"].clone())?;
        let groups =
            manifest["cases"][case_index]["knowledge"]["snapshot_payload"]["workstream_evidence"]
                .as_object()
                .context("frozen workstream evidence required")?;
        for (group_index, (_, event_ids)) in groups.iter().enumerate() {
            let ids: Vec<String> = serde_json::from_value(event_ids.clone())?;
            let events = case
                .events
                .iter()
                .filter(|event| ids.contains(&event.id))
                .cloned()
                .collect::<Vec<_>>();
            let args = llm::SuggestMarkdownSummaryArgs {
                mode: "daily-consolidation".into(),
                task: None,
                vault_context: without_knowledge(),
            };
            let inspection =
                llm::inspect_composition_request(&config, &args, &events, &case.events, &ledger)
                    .map_err(anyhow::Error::msg)?;
            write_new_json(
                &output.join(format!("{}-group-{group_index}-inspection.json", case.date)),
                &inspection,
            )?;
        }
    }
    Ok(())
}

#[test]
fn artifact_publication_never_overwrites() -> Result<()> {
    let directory =
        std::env::temp_dir().join(format!("log-inbox-evaluation-{}", uuid::Uuid::new_v4()));
    fs::create_dir(&directory)?;
    let path = directory.join("arm.json");
    write_new_json(&path, &json!({"original": true}))?;
    let duplicate = write_new_json(&path, &json!({"original": false}));
    let original = read_json(&path)?;
    fs::remove_dir_all(&directory)?;
    ensure!(
        duplicate.is_err() && original == json!({"original": true}),
        "artifact overwritten"
    );
    Ok(())
}

#[test]
fn shared_source_extraction_is_bound_to_manifest_evidence_and_terminal_state() -> Result<()> {
    let case: EvaluationCase = serde_json::from_value(json!({
        "date": "2026-10-13", "events": [], "snapshot_id": "snapshot",
        "baseline_revision": null
    }))?;
    let artifact = json!({
        "manifest_sha256": "manifest", "date": case.date,
        "events_sha256": digest(&serde_json::to_vec(&case.events)?),
        "status": "succeeded", "source_facts": {"facts": []}
    });
    validate_extraction_artifact(&artifact, "manifest", &case)?;
    for (field, value) in [
        ("manifest_sha256", json!("other")),
        ("date", json!("2026-10-14")),
        ("events_sha256", json!("changed")),
        ("status", json!("running")),
        ("source_facts", Value::Null),
    ] {
        let mut tampered = artifact.clone();
        tampered[field] = value;
        ensure!(validate_extraction_artifact(&tampered, "manifest", &case).is_err());
    }
    let mut failed = artifact;
    failed["status"] = json!("failed");
    ensure!(validate_extraction_artifact(&failed, "manifest", &case).is_err());
    failed["source_facts"] = Value::Null;
    validate_extraction_artifact(&failed, "manifest", &case)?;
    Ok(())
}

#[test]
fn evaluation_rejects_parser_only_content_and_unauthorized_links() -> Result<()> {
    let event = |id: &str| -> Result<StoredLogEvent> {
        Ok(serde_json::from_value(json!({
            "id":id,"received_at":"2026-10-13T12:00:00Z",
            "timestamp":"2026-10-13T12:00:00Z","source":"evaluation",
            "level":"info","message":"Reported work.","metadata":{},
            "fingerprint":null,"truncated":false,"reviewed":false
        }))?)
    };
    let events = vec![event("a")?, event("b")?];
    let mut value = json!({"schema_version":1,"manual_entry_ids":[],
        "workstreams":[{"id":"work","title":"Work","evidence_event_ids":["a","b"],
            "outcome":[{"text":"Reported outcome.","evidence_event_ids":["a"]}]}]});
    let content = serde_json::from_value(value.clone())?;
    ensure!(validate_candidate_for_save(&content, &events, None).is_err());
    value["workstreams"][0]["validation"] = json!([
        {"text":"Reported check.","evidence_event_ids":["b"]}
    ]);
    let content = serde_json::from_value(value.clone())?;
    validate_candidate_for_save(&content, &events, None)?;
    value["workstreams"][0]["canonical_links"] = json!(["[[Product]]"]);
    let content = serde_json::from_value(value)?;
    ensure!(validate_candidate_for_save(&content, &events, None).is_err());
    validate_candidate_for_save(
        &content,
        &events,
        Some(&json!({
            "workstream_links":{"work":["[[Product]]"]},
            "workstream_evidence":{"work":["a","b"]}
        })),
    )?;
    Ok(())
}

/// Recheck earlier immutable experiment outputs using the production save checks.
/// Emits only bounded status/count diagnostics; never calls a provider or reads notes.
#[test]
#[ignore = "requires private frozen evaluation artifacts"]
fn audit_frozen_candidate_persistability() -> Result<()> {
    let input: EvaluationInput = serde_json::from_slice(&read_bounded(Path::new(
        &std::env::var("LOG_INBOX_EVALUATION_INPUT")?,
    ))?)?;
    let manifest = read_json(Path::new(&std::env::var(
        "LOG_INBOX_EVALUATION_FROZEN_MANIFEST",
    )?))?;
    validate_frozen_cases(&manifest, &input)?;
    let directory = fs::canonicalize(std::env::var("LOG_INBOX_EVALUATION_AUDIT_DIR")?)?;
    for (index, case) in input.cases.iter().enumerate() {
        for with_knowledge in [false, true] {
            let arm = if with_knowledge {
                "withKnowledge"
            } else {
                "withoutKnowledge"
            };
            let artifact = read_json(&directory.join(format!("{}-{arm}.json", case.date)))?;
            let content = serde_json::from_value(artifact["daily_revision_content"].clone())?;
            let payload = with_knowledge
                .then_some(&manifest["cases"][index]["knowledge"]["snapshot_payload"]);
            let persistable = validate_candidate_for_save(&content, &case.events, payload).is_ok();
            println!(
                "{}",
                json!({"date":case.date,"with_knowledge":with_knowledge,
                "prior_status":artifact["status"],"persistable":persistable})
            );
        }
    }
    Ok(())
}

#[test]
fn frozen_replay_checks_fixture_evidence_and_knowledge_without_reading_notes() -> Result<()> {
    let fixture = json!({
        "workspace_root": "/does-not-exist/evaluation-vault",
        "collections": [], "mappings": [],
        "cases": [{"date": "2026-10-13", "snapshot_id": "snapshot-original",
            "baseline_revision": {"id": "baseline-original"},
            "events": [{"id": "event-original", "received_at": "2026-10-13T12:00:00Z",
                "timestamp": "2026-10-13T12:00:00Z", "source": "evaluation", "level": "info",
                "message": "fixture evidence", "metadata": {}, "fingerprint": null,
                "truncated": false, "reviewed": false}]}]
    });
    let bytes = serde_json::to_vec(&fixture)?;
    let input: EvaluationInput = serde_json::from_slice(&bytes)?;
    let identity = json!({"fixture_sha256": digest(&bytes), "model": "local-model",
        "request_profile": "max_tokens", "timeout_seconds": 1200, "contract_digest": "new-contract",
        "configuration_digest": "new-local-proxy-port"});
    let knowledge = json!({"vault_context": {"candidate_notes": []},
        "snapshot_payload": {"excerpts": [{"text": "frozen background"}]}});
    let case = &input.cases[0];
    let mut manifest = json!({"identity": identity, "cases": [{
        "date": case.date, "snapshot_id": case.snapshot_id,
        "event_ids": ["event-original"], "events_sha256": digest(&serde_json::to_vec(&case.events)?),
        "baseline_revision": case.baseline_revision, "knowledge_sha256": value_digest(&knowledge)?,
        "knowledge": knowledge}]});
    manifest["identity"]["contract_digest"] = json!("old-contract");
    manifest["identity"]["configuration_digest"] = json!("old-local-proxy-port");
    validate_replay(&manifest, &input, &identity)?;
    for (pointer, replacement) in [
        ("/identity/fixture_sha256", json!("different-fixture")),
        ("/identity/model", json!("different-model")),
        ("/identity/request_profile", json!("different-profile")),
        ("/identity/timeout_seconds", json!(30)),
        ("/cases/0/date", json!("2026-10-14")),
        ("/cases/0/snapshot_id", json!("different-snapshot")),
        ("/cases/0/event_ids/0", json!("different-event")),
        ("/cases/0/events_sha256", json!("different-evidence")),
        ("/cases/0/baseline_revision/id", json!("different-baseline")),
        (
            "/cases/0/knowledge/vault_context/candidate_notes",
            json!(["tampered-note"]),
        ),
        (
            "/cases/0/knowledge/snapshot_payload/excerpts/0/text",
            json!("tampered-background"),
        ),
    ] {
        let mut changed = manifest.clone();
        *changed.pointer_mut(pointer).context("test field")? = replacement;
        ensure!(
            validate_replay(&changed, &input, &identity).is_err(),
            "tampered replay accepted: {pointer}"
        );
    }
    let mut alternate = identity.clone();
    alternate["model"] = json!("alternate-local-model");
    ensure!(
        validate_replay(&manifest, &input, &alternate).is_err(),
        "implicit model change accepted"
    );
    alternate["model_change_from"] = identity["model"].clone();
    validate_replay(&manifest, &input, &alternate)?;
    alternate["model_change_from"] = json!("wrong-baseline");
    ensure!(
        validate_replay(&manifest, &input, &alternate).is_err(),
        "wrong model baseline accepted"
    );
    alternate["model_change_from"] = identity["model"].clone();
    let mut changed = manifest.clone();
    changed["cases"][0]["events_sha256"] = json!("different-evidence");
    ensure!(
        validate_replay(&changed, &input, &alternate).is_err(),
        "model change bypassed evidence check"
    );
    Ok(())
}
