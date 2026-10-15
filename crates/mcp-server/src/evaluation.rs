//! Opt-in, read-only real-day evaluation. No Store, API, or Markdown writer is used.

use crate::{knowledge, llm};
use anyhow::{Context, Result, ensure};
use log_inbox_core::{
    models::{ContextMapping, KnowledgeCollection, StoredLogEvent},
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
        ensure!(
            !identity[field].is_null() && manifest["identity"][field] == identity[field],
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
        "pipeline": "deterministic-per-source-cards-then-source-slot-selection",
        "extraction_reuse": "one validated ledger per date shared by both Knowledge arms",
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
        // Extraction is independent of Knowledge. Freeze it once so each pair
        // composes exactly the same source facts, not separate model samples.
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
                llm::extract_daily_sources(&config, &case.events),
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
                "structured_draft": null, "daily_revision_content": null, "preview_markdown": null
            });
            match result {
                Ok(Ok(proposal)) => {
                    if let Some(draft) = proposal.structured_draft {
                        artifact["structured_draft"] = serde_json::to_value(&draft)?;
                        let content = llm::daily_revision_content(draft, Vec::new(), &args);
                        artifact["preview_markdown"] =
                            json!(llm::render_daily_revision_preview(&content, &[], &[]));
                        artifact["daily_revision_content"] = serde_json::to_value(content)?;
                        artifact["status"] = json!("succeeded");
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
    for (index, case) in input.cases.iter().enumerate() {
        llm::validate_source_extractability(&case.events)
            .map_err(|error| anyhow::anyhow!("{}: {error}", case.date))?;
        let mut contexts = vec![without_knowledge()];
        if let Some(manifest) = &frozen {
            contexts.push(manifest["cases"][index]["knowledge"]["vault_context"].clone());
        }
        for vault_context in contexts {
            let args = llm::SuggestMarkdownSummaryArgs {
                vault_context,
                mode: "daily-consolidation".to_owned(),
                task: None,
            };
            llm::validate_prepared_source_layout(&case.events, &args)
                .map_err(|error| anyhow::anyhow!("{} layout: {error}", case.date))?;
        }
        println!(
            "source card and mandatory-layout preflight date={} events={} passed",
            case.date,
            case.events.len()
        );
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
    Ok(())
}
