# Daily Engineering Knowledge Roadmap

Status: Daily generation failure repaired and deployed locally; tested local models failed factual-summary acceptance. October 22 has a source-reviewed editorial draft; automatic usefulness and agent recall remain unproven. Updated 2026-10-28. Initial baseline inspected: `ae46de99fd0e80fa4d70ef92a953d076bcf04057`.

This roadmap defines the target direction, delivered foundation, and remaining delivery order. The [README](../README.md) and runtime specs describe the active Daily release; milestone checkboxes record implemented and pending work.

## Human summaries and ordinary-activity KB suggestions — 2026-10-23

Canonical task record for the owner's report that recent Daily output is a dump
and ordinary work is not feeding the KB. Starting clean HEAD
`c6deb798e4c041cc7dc677da7eac99907a91baa8` on inherited branch
`codex/accepted-work-kb-capture`. Endpoint: local implementation and validation;
publication and deployment are not part of this request.

Diagnosis: both preparation modes select original report text; the earlier human
recap delivery explicitly excluded automatic recap authorship. KB intake filters
for special `knowledge_submission` payloads, so ordinary lifecycle reports and
`canonical_note_candidates` never become candidates. Existing KB promotion can
update existing eligible notes but cannot create pages.

Plan accepted in chat: generate a concise cited paragraph and up to five
Done/Pending topics in explicit Semantic preparation, retaining source history,
manual notes, evidence exclusions and owner Apply. Daily initially selects this
mode with an eligible saved default model and existing external consent; explicit
source/retry choices take precedence. Keep model-free source reports available
without a model; scheduled/API defaults remain source-based. Add reviewable KB
suggestions from ordinary original
activity without requiring producer payload changes, invented product attribution,
or lifecycle-based implementation/verification claims. Capture freezes exact
evidence; canonical updates retain separate owner review and confirmation.

Factory routing: Sol/high bounded KB diagnosis, separate Sol/medium summary and KB
implementation workers, coordinator integration/documentation, independent Sol/high
review. Ponytail lite: reuse the recap schema, ingestion, candidate store and KB
writer; no new provider, dependency, service or automatic Markdown write.

| Contract/scenario | Evidence | State |
| --- | --- | --- |
| Semantic prose, bounded citations, qualifications and failure behavior | Five focused semantic regressions; core recap persistence validation | Passed deterministic checks |
| Source mode remains model-free; manual prose and historical drafts preserved | Full workspace suite; both-browser generation/recap coverage; v22 expired-source manual amendment regression | Passed |
| Ordinary original activity suggested without special payload or Daily | Core/API and both-browser capture regressions | Passed |
| Exact capture, retries, stale/expired evidence, workspace and owner scopes | Core/API regressions, captured-source pre-limit filtering and truncated/oversized guards | Passed |
| No inferred product or implementation/verification authority; reviewed existing-note writes | Reported-claim and owner-attribution tests; existing promotion/recovery suite | Passed |
| Complete task diff, required formatting/Clippy/Rust/browser checks | Independent complete review and focused follow-ups; 314 Rust passed/13 ignored; all 256 browser scenarios covered across full/focused runs; formatting and Clippy | Passed |

Implemented locally, without deployment, publication, canonical-note writes or
changes to existing saved Daily revisions. The complete independent task review
used base/HEAD/merge-base `c6deb798` in this checkout and inherited branch. It found
and fixed missing v22 legacy compatibility for safe manual amendments after source
expiry. Follow-up review removed illustrative recap prose that could be echoed as
a purported fact. No remaining actionable code findings were reported.

Validation: the serial workspace run passed 314 Rust tests with 13 ignored. An
earlier run under concurrent compilation/browser load missed an inherited 50 ms
mock-request deadline; the unchanged test passed in the full serial run. The full
two-engine browser run passed 254 scenarios and exposed two failures in the new
oversized-report test's route fixture. Correcting that fixture was followed by all
10 KB checks passing across Chromium/Firefox, covering the remaining scenarios.
Rust/client formatting, Clippy with warnings denied and diff whitespace checks
passed. Final prompt changes are additionally checked by the focused semantic suite.
Tests use disposable stores and mocked browser responses. A new release Compose
smoke and deployed owner workflow were not run for this change.

Local model evaluation uses three synthetic source reports and the production
recap prompt/schema on `qwen3:4b-instruct-2507-q4_K_M`, without owner data or note
writes. It exposed both-null topic statuses, fixed with strict schema alternatives,
and an unsupported blanket negative about verification. Explicit scope-preservation
instructions did not eliminate that error on the smaller model; its structurally
valid recap remains unsuitable as independently verified output. The source/history and candidate validators do
not prove semantic entailment. Private synthetic request/result material and Rust
results are retained in ignored `tmp/evaluations/semantic-recap-20261023/`.
The final smaller-model sample took 73.09 seconds. A comparison with the installed
`gemma3:12b-8k` took 209.92 seconds for this recap stage and passed structural,
text-bound and actual-citation checks. Manual inspection found a concise paragraph
and two Done/Pending topics preserving local Chromium validation, outstanding
Firefox/customer acceptance and the KB review gate, without the smaller model's
blanket negative. It still put citation IDs into prose despite the prompt, and
one pending bullet also restated the existing claims-preservation behavior. These
are editing/quality limitations; this small synthetic trial is not a real-day or
whole-generation performance acceptance result. No saved model choice changed.
Real-day quality, practical whole-generation latency and owner usefulness acceptance
remain unproven; this task does not mark the earlier usefulness or recall pilots
passed. Scheduled preparation still produces source reports, and KB suggestions
still require owner review and an existing eligible destination note.

### Local deployment for manual verification — 2026-10-23

The owner subsequently authorized deployment and verification. Built the current
reviewed working tree on HEAD `c6deb798` and recreated only the existing collector
and Daily containers in Compose project `log-inbox`, with the existing provider
override, environment, ports and mounts. Ollama and saved model choices were left
unchanged. No commit, push or PR update was requested. The 53-file runtime source
manifest identifies the deployed uncommitted source exactly; its digest is
`ffef097deccedb746702eb55b153ec3dc707f9a2e6849cf219daea1d0b3cba7c`.

Before stopping the app writers, confirmed no active generation or pending Daily
Apply recovery. Saved rollback image tags, copied quiesced app data, produced an
SQLite online-backup copy from that snapshot and passed its integrity check.
Independent deployment-script review corrected stale-copy retry behavior,
consistent post-deployment SQLite snapshots and pre-start Apply-recovery guards.
Verification snapshots pause both app writers only during copying and guarantee
unpause. Recovery material and the private receipt are in ignored
`tmp/deployments/recap-kb-20261023/`; preserve post-deployment evidence, new
`reported` candidates and unfinished operations before any rollback.

Live checks passed both health endpoints, authenticated API reads, unauthenticated
KB rejection, exact served client/source equality and running/built image identity.
Schema remains 29; all 1,139 existing event IDs, all 12 protected review/configuration
tables and all 76 Markdown file hashes were preserved. The KB suggestions endpoint
returns 100 suggestions with truncation disclosed; the existing candidate remains.
Saved default is still `qwen2.5:1.5b`. Its real-day summary quality has not been
accepted by this deployment verification. No existing draft was regenerated and no
owner candidate was captured, approved or written.

The exact-source disposable release Compose smoke also passed Daily review/Apply,
KB capture/review/promotion, stale-target rejection, preservation and restart with
idempotent confirmation. It used separate synthetic data, workspace and ports.
Live Chromium/Firefox checks passed: Human summary initially selected, deployed
mode controls visible, all 100 activity suggestions loaded, zero page errors and
zero unexpected mutating requests. The temporary verification session was revoked
and its private cookie file removed. Runtime source hashes remain unchanged.

Manual verification starts at `http://127.0.0.1:8788/`: choose Human summary
(Semantic) for the day to review, or open `/?view=knowledge` and Activity
suggestions. Review all generated prose and KB claims before canonical confirmation.

### Branch publication authorization — 2026-10-23

After the local deployment, the owner requested committing and pushing the changes
and plans to check again Monday, October 26. This is publication authorization,
not acceptance of generated-summary usefulness. Publish on the existing
`codex/accepted-work-kb-capture` branch; its previous PR #4 is already merged.
No new PR, merge, model-setting change or scheduled follow-up is requested.
The runtime still matches all 53 files in the verified deployment manifest;
this publication includes the implementation, regression tests, contracts and
deployment evidence above. GitHub CI for the new commit is separate from the
completed local validation.

## Failed human-summary generation — 2026-10-28

Canonical follow-up to the owner's screenshot and request to make summaries work.
Starting HEAD `dab8940278e1222f6571dcad326d07600aa756c4`, existing branch
`codex/accepted-work-kb-capture`. Preserve the unrelated preexisting M5 period-API
planning addition. Previous changes were pushed, not merged; the running local
build already contained them. The owner previously authorized push, deployment
and verification. No merge or new PR is included.

**Diagnosis.** The failing October 22 day has 36 events. Saved local
`qwen2.5:1.5b` failed before composing with `daily topic partition exceeds its
subject limit`. A prerequisite model partition exceeded its twelve-subject window.
Earlier mocks and three-event recap-only trials missed this real production path.

**Plan and delivery.** Reuse deterministic source preparation, retain complete
reported history and reviewed aliases, and ask the model only for a bounded cited
recap. Keep source mode, manual notes, exclusions, immutable revision checks and
explicit Apply. Errors remain visible rather than substituting a report dump.
Factory routing used a bounded Sol/medium worker, independent Sol/high review and
coordinator-owned replay, deployment and documentation. Ponytail lite: remove
unnecessary model prerequisites; add no provider, dependency or automatic write.

The final `daily-structured-v26-focused-recap` contract prepares workstreams in
Rust. The writer receives all exact cards from each workstream's latest material
report, then only already-selected older source clauses labeled as history.
Complete history remains available for review. Request-local integer citations map
strictly to original snapshot IDs; unoffered references are rejected. Citations
must cover the two newest material anchors. These are relevance/source-bound
checks, not proof of factual entailment or automatic supersession. Manual prose
and Knowledge titles, links and excerpts are excluded from writer input.

Model-only limits are 500 characters for the paragraph, one to three topics,
240 characters per status and six references per statement. Duplicate normalized
topics and identical Done/Pending pairs fail. Public/manual/historical limits and
safe older-contract amendments remain supported. Local compatibility probes allow
up to 300 seconds per profile, other providers 30 seconds, always capped by the
configured request limit; at most two profiles run sequentially. The overall
1,200-second generation deadline is unchanged.

| Contract/scenario | Evidence | State |
| --- | --- | --- |
| More than twelve source subjects cannot block writing | 36-source production-entry regression with recap-only model | Passed |
| Retained history, aliases, exclusions and focused source ownership | Semantic/source, focused anchor, offered-reference and reviewed-alias regressions | Passed |
| Failure safety, source mode, manual notes and historical amendments | 327 workspace tests passed / 14 opt-in ignored; 14 Chromium/Firefox summary browser scenarios passed | Passed |
| Exact request retains review history while reducing writer input | 105 history cards retained; 44 writer cards from 14 sources; prompt bytes reduced 42.3% | Passed bounds; no speed claim |
| Independent complete diff and focused follow-up review | No actionable code or documentation findings; reviewed against starting HEAD | Passed; semantic entailment remains a human check |
| Local deployment and preservation | Verified backup, exact images/configuration/assets, health, 12 protected tables and 77 Markdown hashes | Passed |
| Actual October 22 automatic factual scope | Final v26 Gemma trial, independent original-source review | Failed; default unchanged |
| Corrected October 22 editorial draft | Independently reviewed manual recap saved through normal candidate API | Passed source review; no Apply |

**Actual model evidence.** Reaching the writer alone did not solve quality.
The saved 1.5B model first exhausted its token budget after 553.65 seconds;
a compact-reference run completed in 422.52 seconds but repeated sentences and
topics and misclassified status. Larger 4B live attempts completed in 576.57 and
439.29 seconds, but still repeated resolved blockers or made broad unsupported
scope claims. A full-input 12B Gemma run completed in 1,042.73 seconds; independent
review rejected overstatement of webhook completion and stale architecture/CI
claims. These runs created review drafts only; none was applied. They motivated
the final focused reported view rather than treating schema success as acceptance.

The final v26 deployment source manifest is
`abb0dbb08118a56f506f5b1f4669524711d8b6e3cc503824d3f57bd15fb8d7db`.
It recreates only the existing collector and Daily services; Ollama is unchanged.
The quiesced backup passed SQLite integrity checks; schema remains 29. All 1,069
nonexpired events survived. Five older events expired at startup under the
unchanged 30-day policy and remain in the verified backup. All 12 protected
review/configuration tables and all 77 Markdown hashes are unchanged. Rust/client
formatting, Clippy with warnings denied and whitespace checks passed. Sandboxed
mock-listener/browser restrictions were resolved by authorized unsandboxed reruns.

The final deployed compatibility probe passed for installed local
`gemma3:12b-8k`. The normal real-day API generated a structured recap in 586.58
seconds, preserving all 77 Markdown hashes, but **failed factual acceptance**.
It marked already-passed email/TLS verification pending, omitted the cited webhook
completion gates and called the completed read-only investigation ongoing.
Independent Sol/high original-source review and a bounded Astra/low architecture
escalation confirmed that the decisive qualifications were already supplied and
cited: this is model interpretation failure, not absent evidence. Stop prompt
retries rather than treating citation membership as entailment. The saved default
remains `qwen2.5:1.5b`; no tested model is accepted for automatic summary quality.

For immediate review value, the coordinator authored a corrected paragraph and
three topics from original sources 35, 33 and 24. Independent factual review
narrowed browser verification to the isolated view, distinguished a proposed
storage approach from implementation and removed historical requirements whose
current status is unknown. The normal candidate API saved this as a new
`structured_edit` revision, preserving the model output in history. Its recap
exactly matches the reviewed proposal and all 77 Markdown hashes remain unchanged.
This is editorial assistance, not successful automatic synthesis or owner Apply.
Live Chromium and Firefox both display the exact reviewed paragraph and three
topics, with no page errors or unexpected mutating browser requests.

The owner selected a stronger provider and configured Azure OpenAI credentials
and activity-sharing consent through Generation settings. The connection initially
used the resource root and returned not found. Correcting only its path on the
same approved host to `/openai/v1`, preserving the stored key and authorized
consent, enabled discovery. The first candidate `gpt-6.1-sol` returned model not
found; a discovered catalog ID does not establish an Azure deployment. The owner
was asked for the resource's deployment name before further capability trials.
No credential or private endpoint was printed or committed. A next candidate must pass original-source
review on this frozen day and a second held-out real day before default selection
or a usefulness claim; practical latency also remains an acceptance gate.
The broader recall pilot remains separate. All live attempts preserve explicit
owner Apply. Private backups, receipts and replay
artifacts are retained under ignored `tmp/deployments/focused-recap-20261028/`
and `tmp/evaluations/summary-failure-20261028/`; earlier trial receipts remain
in their dated deployment directories.

## Product Promise

Turn selected engineering activity into a concise, evidence-linked daily record: what changed, why, what was validated, and what remains. Let the owner review and correct it before writing to existing Markdown notes.

Keep Log Inbox a single-owner, self-hosted HTTP collector, dashboard, scheduler, MCP interface, and reviewed Markdown writer. It is not a vault manager, Notion clone, generic agent-memory platform, or developer productivity tracker.

- **Daily is the product:** date navigation, structured drafts, evidence review, omissions, preview, and Apply.
- **Reference context is optional shared context:** human-maintained Markdown product/module notes can ground Daily drafts and, later, approved agent reads. It is not a prerequisite for a useful draft and is not an autonomous memory store.
- **Settings configures the service:** workspace, dates, automation, model connection, retention, security, and integration.

Current local delivery: [accepted-work KB capture](#accepted-work-kb-capture--2026-10-19), with explicit owner review and canonical confirmation. The earlier [bounded-selection/KB plan](#next-product-kb-ingestion-delivery) remains historical; generated-summary quality and a measured agent-recall pilot are still pending.

### Current delivery: make Daily understandable

The explicit **Use activity record** recovery path provides a model-independent, owner-requested record after failure. It preserves original activity in collapsed groups, labels the output as not AI-summarized, and reuses reviewed Apply. This is a completion path, not evidence that AI latency or quality has passed the personal-pilot gate.

Prioritize reliable generation and direct actions on Daily before expanding Knowledge. Persist attempts, recover interrupted work, show actual stage/elapsed time and actionable failure, and allow exact-attempt cancellation. Keep one active operation and no hidden manual backlog. Accepted generation runs independently of browser navigation; the UI follows server state.

Make the current revision readable before editing, offer Copy summary, and preserve exact reviewed Markdown writes. Move optional Reference notes into Settings while retaining existing configuration and Knowledge deep links. Context setup must demonstrate a benefit to the daily draft rather than become another inbox to maintain.

Use bounded intake receipt aggregates and content-free timing diagnostics. The opt-in [generation benchmark](generation-performance.md) separates real-model measurement from deterministic workflow tests. Real-day speed/quality comparisons and the ten-day pilot remain validation work; implementation alone does not satisfy those gates.

The service operates on a workspace available to its Docker host. Signing in from another device does not make that device's local folder available. Synchronization, if needed, remains an explicitly configured external responsibility.

## Recall and navigation delivery

### Daily quality and shared knowledge deliveries — 2026-10-15

Owner-requested delivery plan, following inspection of 865 retained events, the
October 13–14 generated revisions and applied notes, and selected canonical notes.
The database copy passed its integrity check. This section is the task record;
planned behavior below is not a statement that it has shipped.

Initial local scope: improve Daily first, then accept reviewed plan/implementation knowledge and
prove bounded agent recall. Keep Markdown canonical, immutable evidence/revisions,
explicit review, and the ingestion-only credential boundary. No automatic knowledge
promotion, deployment, or PR publication is included. Starting revision:
`bee5e40c82198ccafdf3397763d7b1a2f25b1b9b`. Local branch:
`codex/daily-log-knowledge-deliveries` (the runtime requires this prefix; remote
collision inspection was unavailable because the local SSH configuration fails its
ownership check). Publication and merge were subsequently authorized on 2026-10-15; the remote target was verified as `main` at the same starting revision.

1. **Daily identity and summary quality — implemented and validated locally.** Normalize work-item/PR
   identities across scalar and structured producer formats. Use the same resolver
   for generation and Knowledge; associate a lifecycle with its unambiguous task
   identity and use exact feature branches conservatively when explicit identities
   are absent. Keep different work items and repositories distinct. Give the model
   useful bounded branch/repository context. Ground new facts in particular events,
   distinguish plans from implementation and verification, and keep reference-note
   background out of reported daily accomplishments. Improve the actual reviewed
   renderer, not a generation-only rendering path.
   Acceptance: representative numeric/string/object IDs group consistently; branch
   changes do not split one unambiguous task; common branches and conflicting IDs
   do not combine unrelated work; unknown/cross-group fact citations fail; pending
   acceptance stays pending; preview, Copy and Apply use the same reviewed content.
   Existing saved revisions remain readable and are never rewritten automatically.
2. **Real-day evaluation — executed; usefulness gate unmet.** Use the October 13–14 evidence
   as regression cases, including Forms fonts, PR9417 diagnosis and PR9713 follow-up.
   Compare opt-in drafts with/without Knowledge using the existing frozen comparison
   mechanism. Measure unsupported claims, lost limitations, duplicate bullets,
   grouping accuracy, editing effort, latency and model compatibility. Acceptance:
   no background-only accomplishments or plan-to-implementation upgrades in the
   inspected sample, preserved actionable gaps, and an owner judgment of usefulness.
   Deterministic tests cannot substitute for this real-model/human gate.
3. **Reviewed knowledge capture — pending, depends on 1 and 2.** Add a separate
   bounded, revisioned knowledge-candidate contract for accepted plans, decisions,
   implementation results and operational lessons. Retain plan ID/revision,
   product/module, acceptance actor/source, evidence/artifact references, commit/PR,
   verification scope and supersession. Distinguish proposed, accepted, implemented,
   verified, merged and deployed; these are separate facts, not an inferred ladder.
   Producer-reported human acceptance remains a claim until reviewed. Store candidates
   without writing Markdown. Present an exact canonical-note change for accept/edit/
   reject; promotion requires its own approved revision and journaled write.
   Acceptance: idempotent retry, immutable updates, no plan-only implementation
   claims, no rejected/superseded content promoted, safe stale-target conflicts and
   crash recovery. Daily Apply grants no knowledge-write authority. Update storage,
   domain, API, agent integration and writer-policy specs with that implementation.
4. **Cited agent recall pilot — pending, depends on 3.** Pilot Sweet Forms and two
   modules with separately authorized read-only context tools. Return bounded
   approved content, paths, source digests/revisions, verification scope, unresolved
   questions and supersession. Recheck collection eligibility on each read and
   disclose expired/missing evidence. Keep generated Daily prose out of independent
   proof. Acceptance: an agent can retrieve current decisions/constraints, report new
   work and propose a knowledge correction without autonomous promotion; rejected,
   excluded, stale or superseded knowledge cannot masquerade as current truth.
   Measure useful retrieval, correction rate and maintenance burden before expanding.

Delivery 1 uses a bounded Sol implementation worker and independent Sol review;
the coordinator owns integration and this record. Subsequent candidate persistence,
authorization and writer units require Sol ownership and independent review too.

| Scenario / contract | Evidence at delivery revision | State |
|---|---|---|
| Historical ID formats and lifecycle/branch grouping | Mixed-format/conflict/task/branch regressions; full-day keys frozen across batches and individual retries | Passed |
| Knowledge/generation group agreement; conflicting identities | Shared resolver; exact mappings, broad mappings and reviewed aliases covered by Knowledge tests | Passed |
| Per-fact evidence and context/plan boundaries | Strict source-set/rejection/required-field/prompt tests pass; semantic entailment remains a real-model gate | Structural checks passed |
| Historical revisions and reviewed render parity | Actual revision-preview heading/omission tests; v2/v4 expired-source manual amendments; existing Apply and browser contracts | Passed |
| Repository checks | 214 Rust tests passed, 2 explicit benchmarks ignored; Rust format/Clippy and client format passed; 214 Chromium/Firefox browser tests passed; isolated Compose smoke passed generation, Apply and restart recovery | Passed |
| Complete local task diff | Independent Sol review of all nine files against starting HEAD/base/merge-base; reviewed diff SHA-256 `cccc1ad3b822607212931c8fc824691f01ae23563256c9e4861f4e384c7e9074`; two findings fixed and re-reviewed, no remaining findings | Passed |
| Real-model usefulness and owner evaluation | October 13/14 exact snapshots, qwen2.5:1.5b, frozen current Knowledge, four counterbalanced arms: 0/4 usable; independent masked diagnostic review; owner judgment/repeatability separate | Benefit not demonstrated |

Stage ledger: plan and Delivery 1 implementation complete; independent review
passed; integrated tests and isolated Compose smoke passed.
Initial browser execution required local process permission; the permitted run
passed. Compose initially could not read the existing Docker buildx configuration;
the retry uses an empty temporary Docker configuration without modifying owner
configuration. The isolated test generated/applied disposable Markdown and verified
restart recovery; its temporary containers, volumes and workspace were cleaned up.
Full-snapshot grouping is frozen before batching so contradictory
identities cannot disappear during a subset retry. Generation contract v5 keeps
safe manual amendments to expired v2/v4 revisions while invalidating ordinary
generation reuse. No schema migration or new dependencies were introduced. No
production deployment, production Markdown regeneration or production Apply was
performed. Real-day evaluation was subsequently executed with the new ignored,
read-only harness: all four arms failed coverage, with template echoes and no
complete usable summary. Activity-only retained more recoverable content on
October 13; October 14 reached outputs were identical. The [observed results](generation-performance.md#observed-result--2026-10-15)
separate rejected diagnostics from valid daily summaries and document the two
unreached October 14 groups. No deployment is justified by these model results.
Remove template echoes and reduce factual-reconstruction burden, then repeat the
frozen comparison before treating the usefulness gate as passed. Deliveries 3–4
remain gated; valid citations are not proof of semantic truth.

### Grounding correction and frozen rerun — 2026-10-15

Owner authorized fixing the observed template copying and factual reconstruction
burden, then repeating the same October 13–14 comparison. This follow-up preserves
the existing uncommitted delivery work and all evidence gates. Endpoint: integrated
local fix, independent review, frozen four-arm rerun and source-based assessment;
no deployment, Daily replacement, Apply or PR publication.

- Remove factual response examples; the provider already receives the JSON schema.
  Reject known template echoes only on new model output, preserving historical and
  manual content.
- Project field-indexed producer claims with original JSON values, including not-run
  and pending qualifications, plus source-scoped lifecycle/task context. Publish exact
  required event-ID sets from the same predicates used by coverage validation.
  Keep whole-snapshot grouping, per-fact IDs and required field coverage unchanged;
  do not auto-append IDs, fill model omissions or create an activity-record fallback.
- Bump the generation contract to v6 and preserve expired-v5 manual amendments
  without permitting old-contract ordinary reuse.
- Replay the original private manifest after verifying fixture/evidence/context
  digests. Keep the same qwen model, snapshots, note excerpts, rubric and order.
  Contract changes require fresh outputs; no current-note resolution in replay.
- Run focused projection/rejection/replay regressions, repository checks and an
  independent diff review before assessing the new real-model outputs. Score
  usefulness and source entailment independently from structural success and retain
  failures as results. Any further prompt/model iteration is a separate experiment.

Stage ledger: plan, integrated implementation and focused independent diff review
complete. Workspace checks passed (220 Rust tests, three ignored; formatting,
Clippy and client checks), as did isolated generation/review/Apply/restart recovery.
The frozen four-arm rerun and independent non-blind source assessment are complete.
Known placeholders disappeared, but usable summaries remained zero of four. The
October 13 Knowledge arm structurally passed with seven misleading state bullets
out of nine; activity-only truncated. October 14 both arms returned empty content
and were rejected at the first group. No demonstrated factual usefulness or
Knowledge benefit; keep KB promotion/agent-memory expansion gated. See
`docs/generation-performance.md` for results and limitations.

| Changed contract | Scenario and evidence | Status |
|---|---|---|
| Template rejection | Exact generated echoes rejected; legitimate similar wording and historical/manual readers preserved | Passed regression tests |
| Source projection | Typed values and qualifiers retained; terse-message outcome/context metadata preserved without duplicating moved claims | Passed regression tests |
| Evidence gates | Unknown, cross-group and wrong-field citations still rejected; all required field/event sets retained | Passed regression tests |
| Frozen replay | Original evidence/context verified without reading current vault notes; tampered manifests and artifact overwrite rejected | Passed deterministic tests |
| Lifecycle compatibility | Expired v5 manual amendments allowed; ordinary v5 reuse rejected | Passed regression tests |
| Useful summaries | Same October 13–14 evidence/model/Knowledge, separately stored four-arm trial and source review | Failed usefulness: 0/4 usable; 1/4 structurally accepted |

### Per-source extraction and constrained composition — 2026-10-15

Owner authorized addressing stale-state and blanket-citation failures, extracting
facts per source before composition and repeating October 13–14. Local endpoint:
integrated implementation, independent review, frozen rerun and source assessment;
no Apply, deployment, KB promotion or publication. Preserve prior uncommitted work
on `codex/daily-log-knowledge-deliveries`, baseline commit
`bee5e40c82198ccafdf3397763d7b1a2f25b1b9b`.

1. Deterministically extract all eligible bounded source-scoped fact cards from
   one event at a time, without Knowledge, model pruning or other source events.
   Validate provenance, complete source boundaries and required fields; assign
   fact IDs on the server. Assess wider source qualifications separately.
2. Give the composer only the validated chronological ledger and authorized
   background. Accept fact selections rather than new factual prose or arbitrary
   event-ID lists. Render source clauses and timestamps deterministically, so every
   citation has visible supporting material and earlier states remain historical.
3. Retain group, snapshot, lifecycle/required-source coverage, placeholder and
   output-size checks. Missing extraction/selection, unknown or wrong-field facts,
   stale-only selection and oversized output fail; no clipping or evidence filling.
4. Bump the contract and retain safe expired-v6/v7/v8 manual amendments. Exercise
   preparation and composition in compatibility/isolated checks; independently review the
   complete task diff before freezing it for evaluation.
5. Replay the original events/model/Knowledge/rubric in fresh private artifacts.
   Reuse one verified extracted ledger per date across the Knowledge pair to
   isolate composition context. Record stage failures and source-based accuracy;
   do not score unreached work or mistake generation completion for usefulness.

Stage ledger: plan recorded; Sol/high bounded planning complete; Sol/medium owns
the generation pipeline, coordinator owns replay integration and documentation.
Implementation and independent incremental review complete. Workspace checks
passed: 230 Rust tests, three ignored, Clippy, formatting and whitespace checks.
The disposable generation/review/Apply/restart workflow passed with exact source
content and per-fact citation assertions. Final schema guards require the sole
available fact field without excluding optional alternatives. The frozen v7
real-model trial completed: both dates failed at their second source extraction
because the model copied text against the wrong claim IDs or clipped quotations.
All four composition arms were blocked by these two shared failures. No summary
accuracy or Knowledge comparison is possible; no within-trial changes occurred.
Retained source quotes establish provenance, not
semantic completeness; selective omission and lost qualifications require the
independent real-day assessment.

| Contract/scenario | Evidence | Status |
|---|---|---|
| Isolated extraction and typed required fields | V9 deterministic all-eligible cards, exact ledger rebuild and required-field rejection; no source model calls | Passed |
| Negation and bounded input | Indivisible scalar, complete original-source clauses, clipped-prefix regression | Passed |
| Frozen ledger integrity | Quote, pointer, source digest and timestamp tamper regressions | Passed |
| Selective citations and historical state | Exact visible selected clauses, latest-field inclusion, strict source slots; packed-source owner exclusions through readable Daily/Copy/Apply/reopen | Passed |
| Dense required coverage and retained bounds | Nine-source regression, exact BankID FFD packing, all 76 real-source minimum layouts in both contexts; overflow fails without dropping/clipping | Passed |
| Pipeline lifecycle | Composition cancellation/timeout/concurrent edits/model capture; deterministic source preparation yields cooperatively and has no model fallback | Passed |
| Provider compatibility and legacy amendments | Both request profiles; expired v2/v4/v5/v6/v7/v8 manual edits; mapped fact edits clear only changed ownership, unchanged CRLF/CR preserved | Passed |
| Shared evaluation | Manifest/event-bound immutable ledgers, both arms charged preparation time, verified frozen hashes | Passed; all six v9 model responses source-slot-valid |
| Useful/accurate summaries | Original frozen October 13–14 evidence/model/Knowledge/rubric | V9 failed: 0/4 usable; source availability improved, final accuracy/Knowledge benefit unestablished |

Bounded v8 correction: retain source-local extraction but replace copied quote
text with selection of server-owned eligible complete-clause IDs. The server
constructs original-boundary-validated clause cards before inference; the model
selects IDs only, then the unchanged composer selects ledger fact bundles. No
stage may write factual prose or arbitrary citations. Preserve all source, latest
field, required coverage and final-size checks; fail if required source material
cannot be represented. Repeat checks/review and run a separately identified
frozen trial, preserving v7 artifacts. This removes the measured text-copying
burden without accepting mutated quotations. Implementation and independent
review complete. V8 workspace: 233 tests passed, four ignored; the private-data
preflight was invoked separately and passed all 32/44 sources without inference.
Clippy, formatting, whitespace and isolated workflow/restart checks passed.
Review found and fixed a bare-newline negation inversion; only actual sentence
or semicolon boundaries split long source strings. The frozen v8 trial completed:
0/4 usable summaries. October 13 extracted 32 sources/67 facts, then both
composition arms failed invalid selections. October 14 extraction failed on
repeated claim IDs at source 40, blocking both arms. All 75 requests returned
HTTP 200 and normal-stop JSON; transport success did not establish useful output.
Independent source review also found essential font validation and accepted
trade-offs pruned before composition. No Knowledge benefit was established.

V9 initial implementation plan (delivered; final outcome below): retain all eligible per-source cards,
then make one model call per group with source-specific, field-specific choices.
Every source slot is explicit; required and newest-field sources cannot be null.
The server renders selected clauses, derives only visible citations and packs
without dropping or clipping material into the retained four-by-320 bounds.
Compact historical time labels and deterministic first-fit-decreasing packing
address measured dense-field capacity. No source-model pruning, copied factual
prose, generic ID arrays or cross-group choices remain in the proposed contract.
Repeat integration checks and independent review, then run a separately frozen
v9 comparison using fresh ledgers and the unchanged evidence/model/Knowledge/rubric.
Keep negative v7/v8 trials and distinguish ledger coverage from summary quality.
Initial v9 checks passed 234 Rust tests (four ignored), Clippy and actual-source
minimum-layout preflight in both contexts. Independent review then found packed
multi-source facts could retain excluded-source text during review/Apply; this
blocks freezing until explicit per-clause provenance survives storage/rendering.
The smoke also exposed a fixture choosing a root tests value while asserting a
child label. Those server/workflow corrections passed: 239 Rust tests (four
ignored), actual-source layout preflight and isolated workflow/restart. Follow-up
review identified two client consumers needing the same contract: readable Daily
must filter exact clauses, and explicit owner edits must clear stale clause maps
only on changed facts. Complete that bounded client integration and browser/API
regressions before freezing the model trial. Client integration is now complete;
final incremental review has no open findings. Both engines passed the changed
paths and line-ending preservation. Broad browser run: 216 passed, four artifact
cleanup collisions passed in an isolated rerun. Clippy, formatting and client style
checks passed. Final integrated workspace: 240 Rust tests passed, four ignored;
Clippy, formatting, client style, actual-source preflight and isolated workflow
passed. Independent review has no open findings in the task's incremental scope.

The frozen v9 trial completed with no runtime/protocol/rubric changes: **0/4
accepted or usable Daily summaries**. Both ledgers completed (197/266 cards,
48/67 ms); independent non-blind review found essential material available for
all twelve rubric topics. All six model responses used valid source slots and
normal-stop JSON. Both October 13 arms selected identical content requiring
seven outcome bullets; both October 14 arms passed the initial small group then
selected identical PR9713 content requiring ten outcome/five follow-up bullets.
The retained four-bullet checks rejected each arm. Later groups are unreached,
not scored omissions. Total charged seconds off/on: October13 122.984/143.344;
October14 190.201/154.970. This improves source retention and citation mechanics,
not demonstrated final accuracy or Knowledge usefulness. No KB/agent-memory
promotion or deployment. Remaining gate: informative selection within the output
budget, evaluated in a separately frozen experiment; do not relax checks to get a
pass. Private evidence/report: `tmp/evaluations/oct13-14-v9-20261015/`.

Publication follow-up: owner authorized push and merge of these fixes plus the
queued plan. Full independent review covered all17 intended paths against `main`
at `bee5e40`, including the evaluation harness; padded typed-source and legacy
Knowledge matching/ignored-identity regressions were fixed. Publication validation:
244 Rust tests passed, four ignored; Clippy, Rust/client formatting and all76
actual-source layout checks passed. Private trial artifacts remain ignored; no
new real-model quality claim is made. Remote CI and merge outcome are recorded in
the associated pull request and lifecycle report.

### Next Product KB ingestion delivery

**PR delivery — authorized 2026-10-19.** Published the five local delivery
commits through [PR #3](https://github.com/davidsellen/log-inbox/pull/3), targeting
`main` at `34bbcc6`. Full integrated independent review at `bdca681` found an
untouched recap could trigger the navigation discard guard because server and
browser JSON object keys have different insertion orders. The fix compares recap
values in schema-key order while preserving citation/topic array order and nulls.
The regression reproduces the false guard before the fix and checks untouched
navigation plus genuine unsaved edits afterward; ten focused Chromium/Firefox
cases and client formatting/syntax checks passed. Retain existing local deployment evidence separately
from this client follow-up; GitHub CI and follow-up review determine PR readiness.
Merge is outside this request. Automatic concise generation and Product KB
ingestion remain deferred under the existing usefulness gates.

**Recap Knowledge links — authorized 2026-10-19 (fixed, deployed and applied).**
The owner reports missing Forms and Log Inbox links after the short recap was
applied. Starting clean `b01b86c` on the same local branch. The recap renderer
returns before rendering existing reviewed canonical links. A separate live
diagnosis found the saved Forms mapping matches product `Forms`, while newer
reports use `Sweet Forms`; the existing Forms note is available and reviewed.
Factory Sol/medium owns the bounded renderer/UI/browser regressions; root owns
this record, alias correction, local preservation/deployment and requested note
repair. Independent review follows; no Knowledge-note content is changed.

1. Restore a compact Related line from reviewed links on source workstreams
   owning visible cited recap facts. Deduplicate; exclude uncited/omitted reports
   and malformed links, without inferring new associations or factual claims.
2. Add the explicit Sweet Forms product alias to the existing Forms note using
   the normal Knowledge mapping API. Refresh references through normal source
   preparation and reattach the unchanged authored recap with its same citations.
3. Test link rendering/exclusions in Markdown and both browsers; review and run
   required checks. Preserve source history, prose and note surroundings.
4. Back up, deploy locally and use normal reviewed Apply to restore the links in
   the October16 note. Automatic recap writing and Product KB ingestion stay deferred.

Complete independent review of all six task paths against `b01b86c` found no
findings (diff SHA256 `5cf9ce455e8456afcf7ebfa432b1d1120bf779dc56c9a6e2d2aef3b2180f5aca`).
Validation:289 Rust tests passed/13 ignored,244 Chromium/Firefox tests passed,
format/client formatting/Clippy and release Compose Apply/restart passed. Tests
cover deduplication, invalid/uncited links, pending-only references, whole-prose
exclusions and literal text rendering. Earlier worker Chromium launches lacked
sandbox permissions; the permitted complete run passed.

The existing reviewed Forms target was reused for the explicit `Sweet Forms`
product alias. Normal source preparation refreshed its frozen references, then
the exact prior authored recap was reattached as revision5
`revision_68b9fc389e7f498eb5cd365cce014701`. All49 event IDs, manual notes,
recap text/citations and source fields/history are unchanged except canonical
links. The resulting1394-character preview contains one compact Related line
with Log Inbox and Sweet Forms, each once. No product paths are hardcoded in
the application, and no Knowledge-note contents were modified.

Both local built images are healthy; existing drafts, note metadata, environment
and mounts were preserved before the requested repair. A consistent39-table
backup passes quick_check; rollback executables, the original note, alias and
refresh records are in ignored `tmp/deployments/recap-links-20261019/`.
Normal fresh-preview Apply finalized the repaired note as
`apply_aa40e2141ada491b5f8aa6ee2d9ed00349089b4d3781d68a96ef1b9ed498be4c`.
Exact whole-file and block checks verify both links, unchanged recap prose,
one managed block and preservation of surrounding note bytes/line ending.

**Daily reapply — authorized 2026-10-19 (fixed, deployed and reapplied).**
The owner applied the previous note and cannot apply the newer recap. Starting
clean `7248879` on the same local branch. Live diagnosis: October16 revision3 is
`in_review`, but a previous finalized Apply makes the browser label it applied
and hide Review & save. The day review status describes the current revision;
the latest operation remains relevant for recovery, not proof a newer draft was
written. Factory Sol/medium owns the bounded UI/browser fix; root owns this
record, API verification and preserving local deployment. Independent review follows.

1. Correct current-revision status and expose normal Review & save for new
   drafts plus Review & reapply for an already-applied current draft.
2. Preserve pending-operation recovery, dismissal, authentication, fresh preview,
   exact approval hashes, conflict checks and explicit Apply semantics.
3. Cover stale finalized history and reviewed repeat Apply in both browsers;
   verify existing backend Apply contracts and local preservation.
4. Deploy locally, obtain a fresh preview and reapply the current October16 recap
   as requested, preserving note content outside the managed block and backups.

| Scenario | Evidence | State |
| --- | --- | --- |
| Old Apply/new draft | Live revision3 remained in_review despite old finalized history; corrected status/action verified in both browsers | Passed |
| Applied current draft | Review & reapply opens normal preview; no write before confirmation; fresh backend reapproval leaves one unchanged block | Passed |
| Apply safety | Pending/dismissed/auth guards; backend stale-file rejection and owner-content protection | Passed |
| Local delivery | Full independent review, regression checks, verified backup, healthy local build and requested recap Apply | Passed |

Only UI applied-state projection changed; backend production approval, writer,
recovery and idempotency contracts are unchanged. Complete independent review of
all five task paths against `7248879` found no findings (diff SHA256
`aa5248f26330f1f236f98d5d47b7fa5f69143ad935eb394dae2b1e46c945df46`).
Validation:288 Rust tests passed/13 ignored,242 Chromium/Firefox tests passed,
format/client formatting/Clippy passed, and release Compose passed source Apply,
new recap Apply and restart. Initial worker Chromium launches were blocked by
sandbox permissions; the permitted complete run passed. A new backend scenario
checks a fresh preview and approval of an already-applied revision, then retains
the existing stale-source/file conflict assertions.

Local deployment preserves existing drafts, note metadata, environment and mounts;
both services run matching built images and pass health checks. A consistent
39-table database backup passes quick_check; old executables, original October16
note and receipts remain in ignored `tmp/deployments/daily-reapply-20261019/`.
The explicitly requested recap revision3 was applied through a fresh normal
approval, finalized as
`apply_6ed9c4ad9d82f48ab4081152e134ddb5447883df86c5887fcb121ad0e5edeb0d`.
Its revision/manual notes are unchanged; exactly one current managed block is in
the Markdown file, and bytes outside it plus its existing closing newline are
preserved. A post-write fresh preview succeeds and contains the current block.
The local verification initially omitted that closing newline when constructing
its expected bytes; correcting the check to the existing writer contract matched
both the exact approved whole-file hash and unchanged surrounding content.
No direct filesystem edit bypassed Apply. The deployed script exposes the normal
review/reapply actions; future owner writes still require confirmation.

**Human Daily recap — authorized 2026-10-19 (implemented and locally deployed).**
The owner requests a short human paragraph and a few topics showing Done versus
Pending. Reuse the clean local branch `codex/source-highlight-coverage`, starting
at `9573c44`; root owns this record, UI, integration and local deployment.
Factory Sol/high planning recommends a separate editorial recap, authored by the
owner or an authorized writing agent, using existing immutable candidate edits.
Sol/medium owns bounded backend implementation; independent Sol/high review
follows. Ponytail lite limits the delivery to one paragraph and one Done/Pending
bullet per topic, with at most five topics. Automatic synthesis is not claimed.

1. Add optional evidence-linked recap prose without changing source facts or
   history. Validate snapshot citations, bounds and explicit structured-edit origin.
2. Show and export the short recap; collapse the detailed reports in the app.
   Existing drafts without recaps retain their current presentation. Source
   exclusions hide entire dependent prose, never partial paraphrases.
3. Support normal edit/save/cancel and revision conflict protection. Author an
   actual October16 draft from retained evidence, including unresolved gates;
   preserve the prior revision and note, and do not Apply it automatically.
4. Independently review, run required checks, back up app data and deploy locally.
   Product KB ingestion and automatic narrative generation remain separate work.

| Scenario | Evidence | State |
| --- | --- | --- |
| Concise editorial recap | Independently checked October16 paragraph and three Done/Pending topics; new draft saved | Passed editorial example; not automatic synthesis |
| Citation/revision safety | Snapshot ownership, whole-prose exclusions, legacy hashes, immutable revisions and stale-save rejection; all49 sources, nine groups and206 history cards preserved | Passed |
| Editing/export | Chromium/Firefox save/cancel/reload/navigation/Copy; real release recap edit, reviewed Apply and restart | Passed |
| Integrated delivery | Independent complete-task review, Rust/browser/Compose checks, verified backup and healthy local images | Passed; live signed-in visual check pending |

The optional recap uses the normal authenticated candidate edit and immutable
revision lifecycle. It exports one short paragraph plus topic Done/Pending
bullets; supporting activity, source history and original open questions remain
collapsed in the app. Models do not author recaps in this delivery. Citations
identify supporting reports rather than verifying an editorial interpretation.
Owners or explicitly authorized writing agents can add/edit/remove the recap.

Independent review corrected two P2 issues: source-picker ordering could make
an untouched recap appear dirty, and existing open questions disappeared from
the supporting view. Regressions now cover reversed citations, navigation guards
and collapsed questions. No remaining code findings were reported over the full
14-path task diff, including the new asset. Final checks:288 Rust tests passed,
13 opt-in tests ignored;238 browser tests passed, plus the final two-browser
open-question assertion; format/client formatting/Clippy passed. Release Compose
verified original source Apply, recap-only Apply, preservation of owner content,
unchanged source/history and recap persistence after restart. The first smoke
attempt could not read desktop Buildx state; isolated Docker configuration passed.

The actual October16 recap was authored from all49 retained event messages and
follow-ups. Independent review checked all seven prose units against seven
distinct cited source events, narrowed the mobile-rejection claim to the tested
manual-check path, and found no remaining factual issue. The draft explicitly
keeps automatic-summary quality/KB work, PR9713 human/provider acceptance,
broader browser checks and unconfirmed PR9828 remote CI separate from completed
implementation and passed PR9713 CI. It is editorial prose, not a new automatic
summary-quality evaluation or independent verification of the original reports.

Normal compare-and-swap saved revision3
`revision_c069bc50b9ef4797a2cf7e415f55e9ec` from revision2
`revision_3603c98f015b41b085f53e30ab7d5d4d`. The rendered managed preview shrank
from15710 to1305 characters (about92%); the short Markdown is180 whitespace
words. Content excluding the recap, all nine workstreams,206 history cards,
the evidence snapshot and manual notes are unchanged. The existing Markdown file
hash is unchanged: no production Apply occurred. Private source, proposal,
independent assessment and save evidence are outside the checkout under
`/tmp/log-inbox-human-recap-evaluation-20261019/`.

Both local services run matching built images and pass health/owner API checks.
Before/after checks preserve October13–16 and19 notes/revisions and runtime
bindings. Today's evidence grew from one to two concurrent arrivals; all backed-up
IDs remain available. Offline primary backup passes quick_check with39 tables;
rollback executables and receipts are in ignored
`tmp/deployments/human-recap-20261019/`. An isolated browser reached the sign-in
page; automatic approval review rejected creating a temporary plaintext credential
copy, so that copy was not made. Authenticated API and Chromium/Firefox tests
verified the delivered state; the live signed-in visual check remains unverified.
Automatic recap writing and Product KB ingestion remain deferred.


**Concise source summaries — authorized 2026-10-16 (implementation/capability validation).**
The owner requests shorter summaries with the remaining outcomes and pending
work visible, unchanged evidence checks, the original October13–14 paired
Knowledge evaluation and local deployment before Product KB ingestion.
Reuse `codex/source-highlight-coverage` at clean `06cc1b0`; the branch remains
local. Root owns this task record, specifications, frozen evaluation and deployment.

1. Prove which original rubric outcomes/qualifications fit the existing exact-card
   and display bounds. The v21 task-head selector improved coverage to6/12 while
   lengthening both drafts; a third positional ranking tweak is not sufficient.
2. Use a bounded factory Astra/low architecture escalation after Sol/high
   diagnosis, then a bounded Sol implementation of the chosen information-selection
   contract. Keep quote ownership, qualifications, whole source history, existing
   revision protections and optional Knowledge. Do not infer closure, hard-code
   fixture identities or make a new model a prerequisite for source preparation.
3. Compare the unchanged four arms against v21 coverage6/12 and6272/7810
   Markdown characters, also retaining the original shorter4368/6075 baseline.
   Target all twelve original rubric items with no unsupported claims and shorter
   output; disclose any infeasibility/remaining omissions instead of weakening checks.
4. Independently review actual code and outputs, run required checks, back up app
   data and deploy locally. Preserve existing notes/drafts; no production Apply.

| Stage/scenario | Required evidence | State |
| --- | --- | --- |
| Representability and architecture | Human reference12/12; exact-card automatic packing/parser/save in four arms; bounded architecture review | Passed feasibility only |
| Concise outcomes and pending work | Producer explicit outcomes covered; strict generic noise gate failed; frozen real-day diagnostic | Historical usefulness not demonstrated |
| Evidence and revision safety | Ownership/history/qualification, protected outcome reduction, visible zero-card failure, expiry/cache and strict parser/save regressions | Passed |
| Integrated review/checks | Complete task review plus focused fixes; Rust286 passed/13 ignored, browser230 passed, format/Clippy/release Compose passed | Passed; actual output quality separate |
| Local preservation/deployment | Offline39-table backup verified; both matching images healthy; owner/browser checks and four-day preservation | Passed |

**Delivery result: partial implementation deployed; historical usefulness gate
failed.** Both real-day selection plans timed out on their first nonempty model
request at180003ms each (360.18seconds total). October14's empty source group
used no model. Zero candidates were produced for the four planned arms; strict
parsing/save was never reached for those failed dates. Their factual accuracy,
rubric coverage and summary length are therefore not assessable, and this run
cannot establish Knowledge benefit. The failed generic boilerplate gate remains
failed. No model-backed preparation mode, prompt/model sweep, repaired output or
historical replacement was promoted.

| Frozen comparison | October13 | October14 | Knowledge result |
| --- | --- | --- | --- |
| Existing source default, before and after explicit-outcome change |6272 Markdown characters;27 highlights |7810 characters;32 highlights | Facts/content identical apart from navigation; no demonstrated factual/editing benefit |
| New selection-only diagnostic |180003ms timeout; neither arm has a candidate |180003ms timeout; neither arm has a candidate | Not measurable without candidates |
| Private human reference, not automatic output |3507 characters;15 highlights |3705 characters;14 highlights |12/12 original items representable; all four parser/save arms pass |

The unchanged default still fully highlights6/12 original rubric items; explicit
outcomes improve future producer inputs, which the historical fixture does not
contain. Its four before/after content and preview artifacts are byte-identical.
The reference proves that useful complete summaries fit the bounds, not that the
automatic selector can find them. Next delivery remains selection usefulness and
latency over exact cards, or explicit owner selection/review; accepted-plan Product
KB ingestion remains deferred until that daily-summary gate is demonstrated.
Final local checks:286 Rust tests passed/13 opt-in tests ignored,230 browser
tests passed, formatting/Clippy passed, qualified-outcome release Compose smoke
passed through reviewed Apply/restart. The separate opt-in real-model diagnostic
failed as reported. Tests initially unable to bind fixture sockets in the sandbox
passed with permitted loopback execution; no evidence or assertion was weakened.

Pretrial hypothesis, original inputs and unchanged rubric are frozen privately in
`/tmp/log-inbox-concise-evaluation-20261016/`. KB ingestion remains gated.

Architecture/feasibility evidence: Sol diagnosis and bounded Astra/low review
rejected another positional/keyword ranking tweak. Source-only inference may
select exact cards independently of semantic grouping; source preparation must
remain provider-free unless the owner explicitly selects a model-backed option.
A private human reference exposed catalog locality: distant same-field facts fit
four×320 but useful pairings were absent from the bounded precomputed catalog.
The evaluation-only helper therefore packs selected existing singleton cards
on demand (at most three cards per bundle) and submits server-owned choices to
the unchanged parser/save checks. Quotes, fields, historical labels, citations,
complete ledger and final bounds remain unchanged. The oracle is private test
data, never a production ranking, prompt, lookup or repaired model response.

Independent reference review initially found11/12: the candidate omitted a scoped
passed check alongside the interrupted full-host build. A separate corrected
reference, retaining the first artifact, passed all four parser/save arms and all
12 original rubric items. Its activity-only Markdown is3507/3705 characters,
15/14 bullets, with58/58 dated bullet claims supported across all four arms.
History remains147/191 cards. Each group fits eight facts and1600 highlight-text
characters (largest1581); four facts per field,320 characters per fact and exact
ownership remain enforced. This proves representability, not automatic selection.
Actual model usefulness was not demonstrated in the trials below. The first small local
selection probe (qwen3:4b-instruct-2507-q4_K_M, already installed) returned the
needed source cards but incorrectly assigned outcome cards to Decision/Blocker;
the unchanged field validator rejected it. The protocol asked the model to repeat
known field ownership and propose packing, creating avoidable responsibilities.
One evidence-based evaluation-only protocol revision now requests a flat list of
known card refs; Rust derives fields and bounded packing from original cards.
Original failed artifacts remain immutable. No repaired model output or production
mode change was promoted. Private local endpoint transport initially required permitted loopback
execution; the permitted response failure is distinct from that infrastructure gap.

Flat-reference v2 initially passed the tiny sample (selected concrete behavior and
probable/unverified/pending cards, omitted generic headline/count). Exposing the
existing mandatory fields and exact Earlier/Latest labels changed the 4B choice:
it omitted the probable-cause card and failed the semantic gate. No real-day
output or runtime mode is promoted from that failed capability result. A second
bounded Astra decision authorized one larger local instruction-model escalation,
`gemma3:12b` from the official Ollama library, with the corrected flat policy and
unchanged evidence checks. Resource inspection found10GiB available RAM plus7.6GB
of idle task-model memory subject to normal expiry, and32GiB free disk. Existing
model/default settings were preserved. Its frozen choice/stopping policy is in
private `larger-model-decision.json`; the subsequent diagnostic exception below
was recorded before real-day calls. No further model trial followed those failures.

The larger model's small response retained all substantive behavior and
probable/unverified/pending qualifications, but also the generic completion
headline: strict boilerplate omission failed. Actual response `[0,2,3,4]`,62558ms;
no unknown facts, field reassignment, count fragment or source/budget violation.
That failed noise result remains failed. A focused Astra decision permitted one
read-only real-day diagnostic against the original rubric/length targets; this
explicitly changes the stopping policy, not the original result, model, prompt,
evidence or rubric. No further model/prompt trial follows a failed real-day
result. Private `real-diagnostic-decision.json` records the pre-run conditions.
An experimental opt-in strategy required shorter summaries on both dates and
improved complete coverage without losing previous full items. Independent
review also had to find no material pending/qualification loss or unsupported
closure, with passing strict save/history checks and acceptable whole-day latency.
Those promotion conditions were not met.
Provider-free defaults and the KB gate remain unchanged.

Deterministic producer improvement implemented in this delivery:
optional `metadata.outcome` is now admitted as exact source-owned outcome data and
prioritized before that event's generic message. No original cards/history are
pruned; qualified message-only sources remain supported. Agent reporting, ingest
and MCP schema guidance describe short self-contained outcomes, separate checks
and pending work, and no inferred verification or Knowledge approval. No owner
metadata-correction authority or saved recommendation preferences were expanded.
Contract `daily-structured-v22-source-outcomes` retains v21 safe historical expiry
amendments. Focused source/forgery/provider-free and full legacy partial-expiry
checks pass. Both frozen dates contain zero such new fields; before/after facts
and topics match apart from the contract, and all four existing source-mode drafts,
contents and previews are exactly identical. This is a future-input improvement,
not a repair of historical prose or a passed automatic-usefulness gate.

Independent complete-task review and focused follow-up resolved one P2: protected
metadata reduction could omit an explicit outcome alongside large ancillary
values. Outcomes now survive that reduction, and an explicit nonempty outcome
with no eligible card fails visibly. Mixed typed containers require at least one
eligible descendant, not every leaf. Regression coverage and the qualified-outcome
release Compose Apply/restart check pass. Follow-up review found no remaining
code findings; reviewed nine-path diff SHA256
`bca4b00e68c6d0a0581c728f7b725d5992fa6878eecf1f573dba1c85772f68ed`
against starting HEAD/base/merge-base `06cc1b0`. The actual diagnostic failed as
reported above; passing code review does not establish summary usefulness.

The deterministic producer change is deployed locally. Both built images match
the running services; health and owner
authentication pass, with October13–16 notes/revisions/evidence, environment and
mounts preserved. Verified offline primary backup: quick_check `ok`,39 tables;
old executable binaries are retained in ignored
`tmp/deployments/concise-source-20261016/`. Browser verification shows the existing
review draft preserved, Source-based default, no model used and nothing written.
No historical draft was regenerated or applied, and no model-backed mode has
been promoted.

**Source highlight coverage — authorized 2026-10-16 (implemented and locally verified).**
Improve important outcome and pending-work visibility before Product KB ingestion.
Starting clean `main`: `34bbcc6690f063ce9f1345cf1a8e708c7b696214`;
branch `codex/source-highlight-coverage` uses the runtime-required prefix.
Root owns this record, specifications, evaluation and local deployment. Factory
Sol/high planning and independent review, with a bounded Sol/medium engine worker;
Ponytail lite applies within scope. The requested endpoint is local verification.

1. Replace newest-event slot monopolization with a deterministic task/event
   priority queue, using original source order and preferring main reports and
   existing scope-bearing validation parents. Pack only existing bounded catalog
   singleton/pair/triple choices; retain singleton fallback. This diversifies
   reported tasks/events and prefers complete source reports.
   Preserve exact quotes, qualifications, strict evidence ownership, conservative
   grouping, complete history, four facts per field and existing text bounds.
   Never infer that a later completion closes an independent earlier gate.
2. Keep the original October13–14 evidence, Knowledge and twelve-item rubric
   frozen. Compare new four-arm results to the prior 4/12 fully highlighted
   baseline, independently checking source support, pending states, repetition
   and editing burden. No fixture-specific production ranking or output repair.
3. Run focused regressions and required integrated checks, independently review
   the actual task diff, then back up app data and deploy locally. Verify health,
   preserved notes/revisions and the source preparation flow without Apply.

| Stage/scenario | Required evidence | State |
| --- | --- | --- |
| Selection and bounds | Generic task/event diversity, main-message/source-order, scoped validation, exact pending clauses, deterministic packing regressions | Passed |
| Evidence/history/legacy safety | Strict parser/save, ownership, expiry and cache regressions; four-arm save validation; full history identical to baseline | Passed |
| Real-day usefulness | Independent original-rubric comparison:6/12 versus4/12 full;118/118 dated bullets supported | Partial; substantial editing remains |
| Integrated review and checks | Independent complete code diff;278 Rust tests,230 browser tests, fmt/Clippy/client format, release Compose smoke | Passed |
| Local preservation/deployment | Offline backup integrity, health/login, preserved October13–15 revisions/notes/evidence; new October16 source candidate | Passed; no Apply |

Frozen trial1 completed with the original32/44 events and unchanged rubric.
All four candidates pass production parser and save validation. Each date shares
one source ledger across Knowledge arms; no model or production DB insert is used.

| Date | Baseline fully highlighted rubric items | New fully highlighted items | New highlights / retained history per arm |
| --- | --- | --- | --- |
| October13 | 2/6 | 3/6 | 27 / 147 |
| October14 | 2/6 | 3/6 | 32 / 191 |

Independent assessment finds age-validation delivery/limitations and signer-return
aggregate-PDF behavior now complete in highlights, increasing total coverage from
4/12 to6/12. All118 dated highlighted bullet claims (166 source clauses across
four arms) are supported in the inspected evidence; support does not imply
currentness or completeness. All original history is preserved. Remaining six
rubric items are partial: font delivery/live-PDF qualifications/decision details,
PR9417 interrupted full build and policy gates, the expired-key probable diagnosis,
and the separate unimplemented OIDC story still need source-history inspection.

The drafts remain useful audit/review starting records requiring substantial
editing. Fewer bullets do not mean more concise output: Markdown grows from4368
to6272 characters on October13 and6075 to7810 on October14 (activity-only).
Historical review instructions still create stale clutter; no closure is inferred.
Knowledge changes navigation only; paired factual content/history is identical,
with zero source excerpts and no demonstrated factual/editing benefit. Composition
in this single trial took136–196ms per arm; this is neither full API latency nor a
performance benchmark. The bounded selector is a modest coverage improvement,
not a passed human-usefulness or Product KB gate.

Private immutable trial inputs/rubric and pretrial hypothesis:
`/tmp/log-inbox-highlight-evaluation-20261016/freeze.json`; actual four arms and
independent assessment in `trial-1/`. All source-pointer quotes (147/191 per date)
were checked against original events. No fixture/rubric changes or candidate
repair occurred. New contract `daily-structured-v21-source-highlights` invalidates
ordinary older-candidate reuse; v20 retains the safe historical expiry amendment
path. There is no schema change or automatic revision rewrite.

Stage ledger: factory Sol/high plan, bounded Sol/medium implementation and
independent Sol/high actual-code/output reviews complete. Independent complete
code review covers `llm.rs` and `main.rs` against base/merge-base `34bbcc6`,
code-diff SHA256 `974ea9aded5b945d80fa1a5fdab910b1f9050e3fe7ee59e90d932f2b11c6a77e`:
no findings. Reviewer loopback-fixture permission gaps were covered by the
coordinator's permitted full workspace run:278 passed,10 explicitly ignored.
Rust format/all-target Clippy, client format and Chromium/Firefox230 tests passed.
Release Compose smoke passed preparation with its fake provider stopped,
reviewed Apply, owner-content preservation, restart and navigation/history checks
in disposable storage. Final production build passed; no remote CI was run for
this local delivery.

Local verification: [Daily October16](http://127.0.0.1:8788/?date=2026-10-16)
shows a ready source-based draft with no model:40 uniquely owned events,9 source
groups,150 history cards, exact v21 provenance and unchanged manual notes/Apply
state. No existing October13–15 revision or note was regenerated. Both services
are healthy and running the exact built images; data/workspace/master-key mounts
and environment are unchanged. Offline primary SQLite backup passes quick_check
with39 tables; existing archived backups and rollback binaries were retained in
ignored `tmp/deployments/source-highlights-20261016/`. An initial backup inventory
assertion included archived databases; explicit primary-database verification
subsequently passed against the same offline copy. Runtime and release checks
are recorded privately in `/tmp/log-inbox-highlight-validation-20261016/`.
Knowledge remains optional; Product KB ingestion and agent recall stay gated.

**Source-based preparation — authorized 2026-10-16 (implemented and locally verified).**
The owner accepted the revised recovery strategy and explicitly requested the
software factory, source-based preparation with optional semantic grouping,
unchanged evidence checks, October13–14 comparisons with/without Knowledge, and
local verification deployment. This supersedes the earlier requirement that a
semantic capability pass must precede every usable preparation. It does not
establish semantic accuracy or authorize Product KB ingestion.

Reuse the existing `codex/bounded-daily-selection` branch, based on `af5036b`,
including the reviewed uncommitted v18 corrections. Root coordinates and owns
this task record, specifications, evaluation and deployment. Sol/medium workers
own the preparation engine and API/UI integration; Sol/high independently reviews
correctness and actual output quality. Apply factory and Ponytail lite within
scope. Keep existing historical revisions, manual notes and source evidence.

1. Implement a deterministic default source-based path: conservative existing
   source identity buckets; bounded chronological highlights selected from exact
   source cards; complete cited reported history; explicit source-based labels.
   Both extraction and composition must work without a model. Knowledge provides
   reviewed headings/navigation/background, never historical evidence or inferred
   resolution. Keep ambiguous identities separate. Preserve raw source grouping
   across both Knowledge arms.
2. Add an explicit semantic option through the existing generation lifecycle.
   Preserve strict semantic/model validation and fail visibly when it cannot
   generate a candidate. Capture the chosen mode in provenance and freshness;
   preserve cancellation, concurrent-edit protection, expiry and scheduling.
   No silent switch or claims that source buckets are semantic subjects.
3. Run appropriate regression/build/browser/Compose checks, independently assess
   the four frozen October13–14 outputs against the original currentness/outcome
   rubric and source evidence, then deploy locally with app-data preservation and
   rollback evidence. Report actual usefulness, omissions and Knowledge effect,
   including neutral/negative findings. Nothing auto-Applies Markdown.

| Contract/scenario | Required evidence | Stage |
| --- | --- | --- |
| Default source preparation without inference | No-provider engine/API tests, provider-stopped Compose smoke and live October15 candidate | Pass |
| Explicit semantic option | Mode/model capture and strict failure tests; no silent fallback | Pass; model capability still limited |
| Exact facts, qualifications, ownership and history | Tamper/exclusion/Copy/Apply regressions; frozen four-arm save validation | Pass |
| Mode changes, legacy revisions and edited/expired drafts | Cache/provenance/concurrency and actual browser/API expiry-amendment regressions | Pass |
| Stable source groups, optional Knowledge | Same frozen evidence/ledger in all four arms; reviewed links only | Pass; no demonstrated factual benefit |
| Useful/current readable output | Independent original12-item rubric:4/12 fully highlighted; all retained in history | Partial: substantial editing required |
| Local build and deployment | Rust/browser checks, release Compose smoke, offline backup, health/login and live source preparation | Pass locally |


Frozen source-mode evaluation (v19 engine, original32/44 events and original
Knowledge snapshot; no provider calls):

| Date | Activity-only | Knowledge | Source support of highlighted dated reports | Complete checklist items in highlights |
| --- | --- | --- | --- | --- |
| October13 | Parser + save contract pass;50ms | Parser + save contract pass;52ms | 30/30 in each arm | 2/6 in each arm |
| October14 | Parser + save contract pass;61ms | Parser + save contract pass;67ms | 34/34 in each arm | 2/6 in each arm |

These timings cover composition in one read-only trial, not full API/SQLite/UI
latency or a repeated performance benchmark. All four candidates pass both the
production parser and production save contract, versus none in the v11 baseline.
No production revisions were inserted by the harness. Original evidence and the
pretrial12-item rubric were unchanged. Each date shares one exact deterministic
ledger across both arms; frozen Knowledge is normalized to reviewed navigation
without re-reading notes or using excerpts.

Independent output assessment found128/128 highlighted bullets supported as
explicitly dated source reports, with no partially supported/unsupported bullet
claims. This does not establish current-state completeness: only4/12 distinct
rubric items are fully captured in highlights, six are partial and two have their
core result only in history. No rubric item is entirely absent from retained
history (147/191 cards per date). Important implementation outcomes, acceptance
qualifications and separate planned work still require manual promotion from
history. Zero exact duplicate strings does not eliminate repeated meaning.

Both dates are usable auditable review starting records, but need substantial
editing to become concise complete Daily summaries. Knowledge changes canonical
navigation links only; factual fields and history are identical across each pair.
There is no demonstrated factual/editing benefit and some broad product links
may distract from the actual module. Keep Knowledge optional and Product KB
ingestion gated. A future bounded task/event-diversity selection experiment may
improve highlight coverage, but is not implemented or counted as success here.
Private frozen artifacts/independent assessment:
`/tmp/log-inbox-source-evaluation-20261016/trial-1/`; original negative trials
remain untouched. Independent code review is separate from this output review.


Final contract `daily-structured-v20-source-headings` additionally displays exact
UTF-8 branch names instead of encoded identity keys. Group IDs and source facts
are unchanged; malformed/control-character display names use a safe generic label.
A fresh four-arm trial2 passed parser/save validation; every candidate content and
rendered preview is byte-identical to independently assessed trial1. The original
assessment therefore applies to the final October13–14 outputs. A live October15
regeneration additionally proved that only headings changed from the initial v19
source candidate; all factual content/history and the manual entry are unchanged.

Correctness review covered the complete task diff against `af5036b` (base/HEAD/
merge-base), followed by focused expiry, documentation and heading corrections.
The final follow-up diff hash was
`9a037a0097378d02aecf17f2369469d15a09265963e4a6910286ae513a57da10`;
no remaining findings. This is separate from the output-quality limitation above.
The final branch decoder uses the Clippy-recommended fixed array chunks under
an even-length guard; it passed focused heading and legacy-expiry tests and
all-target Clippy. No additional semantic-model sweep was performed.

Validation: full workspace276 tests passed (10 ignored), Rust/client formatting
and all-target Clippy passed. The initial browser suite passed220/228; two request
expectations needed explicit new mode fields (four cases across both browsers),
and four Chromium cleanup errors came from overlapping Playwright artifact
folders. All affected cases passed an isolated12-case rerun. The final fully and
partially expired amendment cases passed4/4 across Chromium/Firefox, including the
new partial-expiry case. Existing cases plus that addition cover230 scenarios.
The isolated release-container smoke passed with the fake provider stopped before
preparation, then validated exact cited facts/history, Apply, restart, navigation
and retained owner content in disposable data. Final heading-only follow-ups also
passed focused LLM92 tests (1 ignored), expiry and display checks; the final release
image was rebuilt from the lint-corrected source.

Local deployment: both ordinary and provider-secret Compose files were retained.
Offline app-data backups passed SQLite quick_check across39 tables; previous
binaries are retained. Both service images match the final build, both health
endpoints and owner login pass, and workspace/data/secret mounts and environment
are unchanged. Existing October13–14 notes/revisions/evidence reads are unchanged.
Live October15 preparation succeeded with no model:122 events,28 raw source
groups,476 history cards, attached unchanged manual entry and no Markdown Apply.
Final local revision: `revision_23d1c4414f904633aaaccfe8ec0ad053`.
Receipt: ignored `tmp/deployments/source-preparation-final-20261016/receipt.json`;
screenshot: `/tmp/log-inbox-source-validation-20261016/daily-ready-final.jpg`.
Source preparation is operationally recovered; summary completeness and Product KB
ingestion remain gated. No remote publication, merge or branch deletion is claimed.


Publication follow-up — 2026-10-16: after the operational recovery and remaining
highlight/Knowledge limitations were disclosed, the owner explicitly requested
push, PR and merge. Publish the verified source-preparation delivery through
existing PR2, with its description rewritten around the final default/optional
modes and observed limitations. This authorizes merging the reviewed operational
recovery after CI; it does not establish concise-summary completeness or remove
the Product KB ingestion gate. Retain the prior full-PR review and subsequent
source/expiry/heading/documentation reviews, reconcile coverage against the actual
`main` target, verify final-head CI and merge without bypassing repository checks.
Remote branch cleanup remains authorized after confirmed merge. The local app
already runs the verified source code; publication does not auto-Apply Markdown.


**Next authorized delivery — 2026-10-16 (v12–v15, semantic acceptance failed).** Run a frozen,
source-grounded semantic grouping capability check before the full experiment.
Compact selection prompts without removing source facts or weakening ownership,
coverage, history, exclusions or output bounds. Prove the small sample against
independently defined semantic requirements, then rerun October13–14 with and
without Knowledge using unchanged original evidence. Reuse the prior branch and
retain every negative trial. Coordinator owns evaluation and documentation; bounded
Sol implementation and independent assessment/review cover the changes.

Authorized endpoint now includes the build pipeline, deployment to the existing
local stack, commit and PR merge into main, and deletion of the merged remote
source branch. Publication and merge depend on verified behavior and required
checks; a schema-only or synthetic pass does not close semantic acceptance.
Product KB ingestion remains gated on useful, accurate Daily output. Stages:
capability reference frozen (47 original cards, 28 hard semantic pairs plus cluster
coherence). The unchanged-reference probes are retained separately: v12 qwen2.5:1.5b
and granite3.3:2b passed structure but failed semantics; Qwen3 4B passed two of
three windows. Original scalar source context (v13) still left three authentication
pair errors. Meaningful exact subject labels in a per-card map (v14) exceeded the
12-subject limit in all three windows. The final bounded v15 probe asks for joint
subject clusters; strict coverage, exact labels, deterministic canonical IDs and
original anchors remain mandatory. All three final responses pass production structure/evidence validation. Fonts
and deletion pass their semantic checks; authentication fails two must-link pairs
connecting a separate implementation recommendation, its created story and later
retained scope. It also mixes that scope with unrelated publication work. All 15
cannot-link pairs pass, and 11 of 13 must-link pairs pass (26/28 overall).
Independent assessment confirms the remaining errors. Two of three windows pass,
the same aggregate as the earlier 4B probe; reduced pair errors alone do not prove
useful Daily summaries. Subject labels remain untrusted metadata, including a
stale approval-blocked label; they are excluded from factual Daily text.
No continuation or full real-day model inference is eligible after this failure.

Compact selection is implemented: the matched 696-bundle/106-card font request
shrunk 189,522 to 36,670 bytes (80.65%), preserving exact source/catalog membership.
This proves request reduction, not improved summaries or a Knowledge benefit.
V15 focused LLM checks: 88 passed, one ignored. Fresh full Rust suite: 269 passed,
eight ignored; formatting, all-target Clippy and client formatting passed. The
isolated Compose build and Daily Apply/restart smoke passed. All 224
Chromium/Firefox browser checks and the release build passed. Initial sandbox-only
browser/socket failures were
reproduced as permission errors and rerun with local-process/socket access.
Independent complete review plus v15 follow-up found no remaining code defects;
that does not establish semantic acceptance. Implementation commit `9cb3752` is
pushed in [draft PR2](https://github.com/davidsellen/log-inbox/pull/2). Remote CI
was not yet reported at publication. Local deployment, full-day
comparison, merge and remote source-branch deletion are blocked by semantic
acceptance; Product KB ingestion remains gated. No model-output repair, weakened
checks or selective retries were used. Private evidence is preserved under
`tmp/evaluations/oct13-14-v12-20261016/` through `oct13-14-v15-20261016/`.

CI follow-up — 2026-10-16: run101 at `8d38e32` passed browser and Compose jobs
but failed Rust1.99.1 Clippy on an unnecessary borrowed closure in the read-only
history comparison; Rust tests were skipped after that lint failure. Local checks
used Rust1.98.1, explaining the missed newer lint. Concurrent remote commit
`bac50a6` removed the unnecessary borrow without changing comparison or validation
behavior; that identical fix was preserved during integration. Local formatting,
workspace Clippy and all86 core tests pass. Remote
[CI run103](https://github.com/davidsellen/log-inbox/actions/runs/38889038721)
passed on `bac50a6`. Semantic acceptance and release gates remain unchanged.

Local deployment — 2026-10-16: after the failed semantic gate was disclosed,
the owner explicitly requested deployment. Deployed runtime `481d40d` to the
existing loopback stack with both ordinary and provider-key Compose files.
An offline app-data backup passed SQLite integrity checking (39 tables); the
shared data volume, workspace binding and read-only provider-key mount are
preserved. Both service health endpoints, owner login, served Daily asset hash
and authenticated October13/14 reads passed (32/44 evidence records and existing
revisions). The browser sign-in page renders. No new generation or Apply was
performed. Old Docker image layers were unavailable; exact running binaries and
the offline database were retained privately for rollback. Local deployment does
not close semantic acceptance, authorize KB ingestion or mark PR2 ready to merge.

Preparation recovery — 2026-10-16 (changes implemented; real-model recovery failed): the owner reported the
October15 attempt ending13:04:48 with `model_output_truncated`. Its captured
selection was environment Qwen2.5:1.5b; the first topic request consumed exactly
2,048 completion tokens after a3,252-token prompt. No current revision existed;
manual notes and122 evidence events are retained. A subsequent owner retry was
canceled, not silently replaced. Deterministic preflight finds637 source cards in
28 workstreams and43 topic windows before composition, so runtime viability is
also an acceptance requirement. The old cluster schema allows288 ID entries while
valid evidence coverage permits only24 once each. The exact first production request was preserved and replayed once privately: its
3,252 prompt tokens match the failed attempt; it again stopped at2,048 output
tokens. It emitted208 ID entries from only five distinct facts (one repeated54
times), not a nearly complete valid partition. V16 fixed slots terminated with271 tokens
but the old model generated duplicate labels; the installed4B model also returned
inconsistent label/index references. V17 removes free-label generation entirely:
one integer per card selects an existing source quotation as representative. The
chosen quote persists separately from earliest-member canonical identity. Keep
strict rejection, original ownership, the global12-topic cap, continuation and
mandatory semantic topic/field coverage. The exact first window now finishes in
41 seconds/93 tokens but1.5B makes24 self-rooted topics and is still rejected.
The installed4B model and a newer local Qwen3.5:0.8b also returned the same
24-self-root partition. Disabling the newer model's presence penalty made no
difference; no model default or provider sampling profile was changed. No day
recovery is claimed. V18 additionally retains each eligible whole typed metadata
container once rather than also emitting its descendants. Oversized containers
still descend; exact pointers, complete values and all existing evidence checks
remain. This is an input-redundancy correction, not proof of semantic capability.
No deterministic task-based substitute or activity-record fallback is authorized
as proof of summary quality. Verify full-window/profile/parser regressions and actual request termination;
require a demonstrably valid semantic partition before a day-level retry. Retry October15 without Apply or
manual-note replacement. Do not claim recovery until the complete candidate can
be saved and inspected. Coordinator owns this record; bounded workers own protocol
and fixtures, with independent review before deployment/publication.

V18 measured preflight:122 events,28 workstreams,476 cards and37 topic windows
(previously637 cards/43 windows). The original Qwen2.5:1.5b first-window replay
stopped normally after93 completion tokens in71 seconds, but again returned24
self-rooted topics and failed the unchanged12-topic limit. The input reduction
therefore does not recover preparation. V17/V18 code has independent correctness
review with no findings. Final workspace regressions passed268 tests (9 ignored),
with formatting and all-target Clippy passing; the earlier v17 isolated Compose
Apply/restart smoke passed, but v18 was not deployed. A read-only live check found
manual entries and the current revision unchanged. These checks do not satisfy
model accuracy or operational acceptance. Keep changes pending and the existing v15 local runtime unchanged;
no new candidate, note Apply, deployment or merge is claimed.

Remaining delivery prerequisite: demonstrate a model/runtime that passes the frozen
semantic must-link/must-separate sample and continuation within the preparation
budget, then run complete October15 preparation without Apply and inspect the
saved candidate. If capable inference is unavailable, a separately designed,
explicitly reviewed source-to-subject map is the next product approach; automatic
identity grouping, pairwise inference without measured accuracy, and a larger
token limit are not accepted substitutes. October13–14 Knowledge comparisons,
Product KB ingestion and merge remain gated on useful, accurate summaries.


Local verification deployment — 2026-10-16: after the failed recovery was
explicitly disclosed, the owner requested deployment for verification. Built and
started the reviewed v18 working tree (uncommitted, based on `af5036b`) with the
ordinary and provider-secret Compose files. An offline app-data backup passed
SQLite quick_check across39 tables; previous binaries are retained for rollback.
Both running service images match the rebuilt tags and both health endpoints
return ok. Owner login and October13–15 note/revision/evidence reads passed;
workspace, app-data volume, provider-secret mount and environment are unchanged.
Receipt: ignored `tmp/deployments/verification-20261016T115828Z/receipt.json`, with
per-file build hashes and image IDs. No preparation retry or Apply was performed;
semantic recovery, publication and merge remain unverified.


Owner verification — 2026-10-16 14:02:50 Europe/Stockholm: deployed v18
preparation failed after62 seconds on environment Qwen2.5:1.5b with
`daily topic partition exceeds its subject limit`, before composition (0/0
groups). The configuration digest is
`6e6744fd4aae659a638f8827e3fa5ddc5808766d16b95551e180c04d4e8963f9`.
A controlled replay changed only `response_format` from JSON schema to JSON
object on the captured v18 first window: same3,585 prompt tokens, normal stop,
93 completion tokens and the same24 identity assignments. This does not support
changing schema enforcement as a recovery. The12-topic bound is also connected to
four highlights per field and bundles containing at most three source facts;
raising it is not an evidence-backed repair. The owner was asked to choose
between explicitly reviewed source-based grouping for local operation and keeping
semantic grouping mandatory pending a capable model. No code, model settings,
evidence validators or deployed services were changed in this diagnosis.

| Acceptance scenario | Evidence at v15 working tree | Result |
| --- | --- | --- |
| Same source facts, precise ownership and strict rejection | Three actual responses validated by production parser; Rust/Compose regressions | Pass |
| Smaller selection request with unchanged catalog | Frozen matched request: 189,522 → 36,670 bytes | Pass |
| Coherent grouping across all original sample subjects | Independent 47-card assessment; two authentication must-links fail | Fail |
| Cross-window exact subject reuse | Deterministic half-split protocol frozen; no inference after failed prerequisite | Not run |
| Useful summaries with/without Knowledge, October13–14 | Original evidence and prior failed trials preserved; prerequisite fails | Not run |
| Build and integration | 269 Rust, 224 browser, fmt/Clippy/client, release, isolated Compose/restart | Pass |
| Existing local stack | Explicit owner deployment; health/authentication/binding/32+44 evidence reads verified | Pass locally; semantic limitation remains |
| Merge and branch cleanup | Draft PR and unmerged source branch retained | Blocked by semantic acceptance |

Next delivery must change the grouping approach rather than repeat another blind
prompt/model sweep. Bound the proposal to an auditable source-linked subject map
with explicit human correction, then re-evaluate continuation and the complete
original outcome/stale-follow-up rubric before claiming agent memory or KB
readiness. The final response representation experiment is closed as a failure.


**Active follow-up — 2026-10-16, units1–2 (v11 trial complete; quality gate failed).** Owner authorized fixing
missing outcomes, stale follow-ups and human readability, preserving evidence
checks, then rerunning October13–14 with and without Knowledge. Reuse the existing
local branch and preserve the completed v10 trial. Coordinator owns documentation,
evaluation and integration; bounded Sol planning/implementation and independent
review cover the nontrivial change. No publication or KB runtime expansion.

Acceptance: retain material accomplishments/decisions and unresolved qualifications
from the original twelve-topic rubric; distinguish obsolete instructions from
remaining work using explicit evidence; remove metadata syntax and generic lifecycle
noise from the human summary. Retain verified original cards, precise clause/source
ownership, immutable review and exclusions, strict output limits and visible failure
for invalid output. Assess full candidates and editing effort independently before
claiming usefulness. One frozen ledger/date and unchanged original model/evidence/
Knowledge/rubric support a new, separately recorded four-arm trial.

Current stage: focused Sol planning and source acceptance review complete, followed
by bounded Astra/low architecture review after the failed heuristic rounds.
Experimental implementation is complete: source-only topic assignments and non-null material
highlights replace source-slot ranking; optional typed reported history retains all
original cards separately from the concise summary. History is app-only audit,
read-only in edits, not an inferred resolution or raw Markdown dump. The six main
fields retain four facts ×320 characters. Two Sol workers own generation and
persistence/client integration; coordinator owns evaluation and documentation.
The implementation must address source restrictions, optional nulls and historical
action presentation together; another prompt-only or lexical-ranking adjustment
does not establish completion. The frozen trial is complete: all four arms failed content validation; zero complete
parser-accepted, persistable or usable candidates. Units3–6 remain planned and gated.

Evaluation correction: the v10 harness counted parser acceptance without running
the Store's complete-support contract. All four artifact contents omit fact
support for original snapshot events (16 on October13, 20 on October14), so its
reported 4/4 complete results are parser-only, not persistable Daily successes.
Preserve the original experiment and record this correction; v11 must pass the
shared production evidence/link validator and report parser acceptance,
persistability and independently assessed usefulness separately.

V11 validation: 261 Rust tests passed (5 ignored), 224 browser cases passed,
Clippy/formatting/diff checks and the original 76-event extraction/bounds preflight
passed. Isolated Compose generation, explicit Apply and restart passed with raw
history/source ownership and owner content preserved. Complete independent review
found timestamp-only pruning of earlier independent gates; fixed, regression-tested
and independently re-reviewed without findings. The final smoke assertion change
was reviewed separately. Runtime remained frozen throughout the 1,536.76-second trial.

Actual result: all four arms failed `composition/model_content_invalid`. Source
ledgers remain identical to v10 (197/266 cards); the model chose source-owned IDs
but missed mandatory topic/field coverage. On October13 both versions covered only
four of twelve required outcome topics. On October14 both versions chose the same
source-owned IDs, covering only one of ten required outcome and follow-up topics
and missing required validation coverage. No complete final summary exists to score
for readability or currentness; unreached groups are not omissions. Knowledge
provided no demonstrated usable improvement. Preserve the negative trial and the
private report at `tmp/evaluations/oct13-14-v11-20261016/report.md`; one non-blind
sample per arm does not establish causality or repeatability.

Next delivery before another full comparison: freeze a bounded source-only semantic
capability check and independently assess topic cohesion against original evidence.
The tested 1.5B model/prompt assigned topics by position even in a small prompt;
schema acceptance did not establish meaningful grouping. A stronger suitable model
is a candidate, not a verified fix. Separately compact the composition menu (short
IDs, source text once, obligations first) and measure full request/schema/context
feasibility. The observed 696-bundle catalog is a concrete burden; provider token
counts do not prove truncation. Retain all facts and strict validation. An automatic
coverage-filling step would conceal bad topics and is not an accepted solution.
Product KB runtime ingestion remains gated; this experimental local diff is not a
release or proof that missing outcomes and stale actions are solved end to end.

The preceding v10 delivery implemented bounded selection,
validation and a fresh October13–14 comparison. Units3–6 remain planned. The
preceding comparison remains 0/4 usable; source
availability and citation correctness do not satisfy the usefulness gate. Coordinator
owns this record; use bounded Sol implementation and independent Sol review for each
unit, with durable storage, authorization and recovery assigned to Sol.

- [ ] **1. Constrain useful selection to the output budget — v11 implemented experimentally; usefulness acceptance failed.** Keep the complete
  verified source ledger and typed, read-only history. Prepare bounded source-only
  topics, then select up to four source-owned bundle IDs per factual field. Each
  bundle renders at most 320 characters, keeping all eligible singleton cards
  available and permitting bounded same-field combinations. Mandatory topic/field
  coverage cannot disappear through null or be satisfied by history alone. One
  topic can use several facts; remove source-slot lexical ranking and per-topic
  caps that hid material outcomes. Project typed checks for humans, retain raw cards
  separately, and distinguish earlier instructions from latest explicit scope.
  Preserve precise source clauses, snapshot coverage, review exclusions and strict
  limits. Invalid IDs, overlapping selections, missing coverage and overflow fail
  visibly; no clipping, filling, fabricated citations or activity-record fallback.
  Acceptance: the actual October13/PR9713 failures fit with useful completed work,
  every mandatory source and relevant pending/not-run/scope qualifications retained.
- [ ] **2. Prove usefulness on frozen evidence and obtain owner review.** Independently
  review before freezing a fresh experiment. Reuse the original October13–14 events,
  model weights, reviewed Knowledge, twelve-topic rubric and counterbalanced order;
  share each date's verified ledger across arms and retain all negative trials.
  Require complete candidates, no unsupported accomplishments or lost material
  qualifications, useful distinct outcomes and an owner assessment of editing effort.
  Record accepted/usable arms, omissions, repetition, latency and Knowledge benefit
  separately. Unreached work is not an omission; structural success is insufficient.
  If this fails, stop KB runtime expansion and record a separately frozen follow-up.
  Next follow-up after v10: preserve material outcomes/decisions before ancillary
  source coverage, prevent optional nulls from removing the core accomplishment,
  distinguish historical instructions from remaining actions using explicit source
  evidence, and format selected checks for humans. Retain original clauses and
  review/citation ownership; task completion alone cannot resolve a defect or gate.
  Compare a separately frozen four-arm trial against the same rubric before KB work.
- [ ] **3. Specify accepted-plan capture independently of Daily prose.** Contract
  design may run alongside units1–2; runtime capture/promotion stays gated by their
  reviewed outcome. Capture directly from original redacted events and reviewed
  plan/artifact revisions, with product/module, submission key/digest, plan ID/revision,
  reported acceptance actor/source/time, decisions, constraints, questions, evidence
  digests, PR/commit and supersession. Keep accepted, implemented, verified, merged
  and deployed as separate claims. Agent-reported human acceptance requires owner
  review; Daily generation, terminal task status and Daily Apply grant no KB approval.
  Initially reuse ingestion-only transport; materialization must deduplicate because
  `log_activity` itself is not idempotent. No new service or dependencies.
- [ ] **4. Add immutable candidates and explicit owner review.** After the gate,
  persist workspace-scoped candidate revisions, source bindings and owner dispositions.
  Exact key/digest retries return the same candidate; conflicting payloads conflict,
  and corrections create revisions. Accept/edit/reject binds the exact revision/hash.
  Reject malformed/cross-workspace input without partial materialization. Stale reviews,
  rejected/superseded content and expired evidence cannot inherit approval. Ingestion
  and editing write no Markdown. Direct plan capture must work without Daily generation;
  producers cannot approve their submissions or upgrade claim authority.
- [ ] **5. Promote through a separate reviewed canonical write.** Start with one
  owner-selected existing product/module note in an enabled collection and a distinct
  owned KB block. Reuse the inspected workspace and journaled writer; preserve human
  content and Daily blocks. Freeze candidate/approval hashes, target eligibility and
  expected file/block hashes in exact preview. Require separate KB confirmation under
  dashboard authentication/CSRF; Daily Apply cannot authorize it. Verify stale targets,
  concurrent edits, protected paths/symlinks, disk failure, crash/restart and idempotent
  recovery. Define retention, backup, first-upgrade verification and rollback without
  losing newly received evidence or unfinished recovery data.
- [ ] **6. Pilot approved cited agent recall.** After promotion is verified, pilot one
  product and two modules through separately authorized read-only context tools.
  Return bounded approved content, canonical paths, current digests/revisions, reviewed
  provenance, verification scope, questions and supersession. Recheck eligibility and
  freshness per read. Excluded, stale, rejected or superseded material cannot appear
  as current approved knowledge. Keep collector credentials ingestion-only; agents
  report/propose corrections without promotion authority. Measure useful retrieval,
  corrections and maintenance effort before expanding.

Implementation boundaries: unit1 uses `llm.rs`/`generation.rs`/`evaluation.rs` and
provider compatibility; units3–4 use core models/store/context storage plus the
existing ingestion boundary; unit5 uses `main.rs`/`knowledge.rs`/`daily_writer.rs`
and the native review client. Update the existing domain, storage, API, MCP,
writer-policy, threat-model and agent-integration specs when those contracts ship.
Do not introduce a parallel writer or use generated Daily text as independent proof.

| Follow-up scenario | Required evidence | State |
|---|---|---|
| Dense optional starts, required-only overflow, qualification elsewhere in source | Actual failing groups, exact cost/layout regressions, independent source review | V11 mechanical history/bounds/exclusion checks passed; real-model topic selection failed |
| Useful bounded Daily and optional Knowledge | Fresh frozen four-arm output, twelve-topic assessment, owner judgment | V10: four parser acceptances, zero persistable/useful; v11: zero parser acceptances, zero persistable/useful |
| Accepted plan versus implementation/verification/merge/deployment | Positive/negative original-source fixtures; no inferred state ladder | Pending |
| Duplicate/conflicting submission, stale review, rejection/supersession/expiry | Immutable store/API regressions and recovery checks | Pending |
| Producer authority, cross-workspace scope and CSRF | Negative authorization tests; no producer self-approval | Pending |
| Exact canonical preview, preservation, concurrent edits, crash/rollback | Browser/API, writer fault injection and disposable restart/upgrade checks | Pending |
| Approved recall, exclusions/freshness and credential separation | One-product/two-module integration pilot and retrieval assessment | Pending |

Current stage: scoped Sol/high design and independent source-layout assessment
complete; allocation is implemented and independently reviewed locally. Useful layouts exist for the dense groups within the inherited limits;
a generic two-card same-source choice is needed to retain a completion and a
remaining gate from one event. Sol/medium owns the selection code and a separate
worker owns integration fixtures; coordinator owns this record, evaluation and
report. Base `93774590b51032e6274040577879ca1f6bb7eca7`, branch
`codex/bounded-daily-selection` (required runtime prefix).

V10 plan (completed but ineffective): keep extraction and all evidence/review guards; precompute feasible source
choices and reserved-bin rendering, exercise actual-source layouts and unchanged
workflow contracts, independently review the complete local diff, then freeze a
separate v10 experiment. Reuse the original events, model weights, Knowledge and
rubric in fresh private artifacts; do not tune during the four-arm trial. Assess
full accepted text, unsupported claims, material omissions, repeated starts,
readability and editing burden separately from schema success and Knowledge effect.
Deterministic quality ranking and conservative allocation are heuristics; they do
not prove semantic completeness or optimal packing. Owner usefulness review remains
a separate gate. Preserve earlier negative trials.

Preflight found generic completion text and unqualified parent facts could crowd
out concrete local delivery and scope limitations. The focused diagnosis changes
the allocation order to meaningful mandatory coverage, compact optional concrete
work, complementary qualifications, then enrichment. Actual offered choices must
be assessed before inference; a hand-constructed feasible layout is insufficient.
Independent review also found synchronous planning could delay cancellation.
Whole-day admission now bounds cards/pair work before grouping, precomputes card
properties and reuses plans. The same private large-input reproduction rejects in
0.83 seconds rather than 16.07 seconds; focused independent review found that issue
resolved. The final actual-source preflight restores publication/remaining-gate,
interrupted-build, expired-key/probable-cause, implementation and local-delivery
material. Independent review still finds unavailable font push/direct-PDF/button
caveats, signing aggregate-PDF behavior and missing-ID-token checks. Freeze this
coherent fix for the requested comparison with those limitations recorded; do not
claim complete topic availability or tune during the trial. Final Rust workspace
checks passed 254 tests (four ignored); browser checks passed 222 cases. Clippy,
formatting and isolated Compose review/Apply/restart checks passed.

The frozen v10 trial completed all four parser arms with all 14 responses source-choice
valid and within four facts/field ×320 characters, versus zero parser successes in v9.
These did not pass persistence; the correction above supersedes the former
complete-candidate interpretation without modifying its frozen trial artifacts.
The 197/266 original cards and all frozen input/runtime hashes are unchanged.
October13 off/on took 302.242/333.476 seconds; October14 off/on took
432.023/463.629 seconds, including shared preparation. These are single-run
observations, not performance benchmarks. Independent source review finds no
invented clauses, but historical instructions remain misleading as current actions,
important outcomes/decisions are omitted, and raw metadata requires substantial
rewriting. Knowledge adds navigation and a browser check on October13; October14
without Knowledge retains more PR9713 outcomes. No useful-summary advantage is
established. Private report and exact previews: `tmp/evaluations/oct13-14-v10-20261016/`.
Units1–2 remain open for semantic usefulness and owner review. Product KB runtime
ingestion, promotion and agent recall remain gated; no publication or deployment
was performed in this delivery.

Run affected regressions, repository formatting/Clippy/workspace/client/browser
checks and isolated Compose smoke; run real-model evaluation separately. Endpoint
now: local implementation, independent review and fresh quality report for units1–2.
Future ingestion, canonical promotion, recall, publication and deployment require
their own authorized delivery stages.

Keep Daily quiet: one date toolbar and Search history, without a recent-day list or permanent sidebar.

- [x] Calendar previous/next, Today, date picker and browser history without a page refresh. Removed the recent-day strip after owner feedback; retained-history search remains the path for finding past work.
- [x] Full-page Settings with General, Markdown destination and Preparation & retention sections; independent saves, retained edits across sections, and return to the same Daily context. Optional diagnostics stay collapsed instead of expanding a configuration popup.
- [x] Single-column Daily with one date toolbar, brand navigation, grouped draft/recovery/save controls and compact manual notes. The date heading and grouped controls replace the day strip. Settings retains changed-value saves and optional maintenance disclosures.
- [x] Removed the recent-day count control and scrolling JavaScript. Existing backend preferences remain API-compatible; no retention or scheduling changes.
- [x] Model-independent, submitted literal-text search across retained manual notes, activity and readable current drafts. Search is not restricted by the overview lookback. Results show date, source and excerpt, open the exact item, and preserve the query/results when returning.
- [x] Bounded results and retention disclosure; no persisted content index, Markdown crawl, metadata search or superseded-revision search. Search queries stay out of the browser address bar and persistent browser storage.
- [ ] Complete a real-user walkthrough: find an older PR, inspect two matches, return to Today during generation, and record observed confusion. Expert and simulated-user reviews are not substitutes for this evidence.
- [ ] Verify search latency alongside real deployment ingestion/generation. The opt-in synthetic `history_search_timing_with_concurrent_writer` test covers 50,000 events and 180 frozen dates, including concurrent ingestion; it does not prove production performance.

Next, only after retrieval proves useful:

- [ ] **Ask your log**, experimental and disabled by default in Settings. When enabled, expose Ask inside search, not as another permanent tab. Answers must cite accessible source items, disclose missing/expired history, remain read-only, and leave ordinary search usable when the model fails. Review provider consent, source-data boundaries, latency and answer grounding before shipping. Do not expose a nonfunctional flag before the feature exists.
- [ ] **Saved daily Markdown search** for longer-term recall, limited to the configured Daily scope, with safe indexing, external-edit detection and deletion/exclusion invalidation. No general vault browser.

Navigation/search completion does not satisfy Daily generation reliability or the ten-active-day pilot gate.

## Scope and Defaults

- One active workspace and one active LLM connection per instance.
- Generic folder-based Markdown; no editor-opening integration or required folder names.
- Relative Markdown links as the portable baseline, with detected/configurable wikilink style and explicit override.
- One day aggregate and one current proposal candidate per day, with immutable revisions; no per-task proposal worker after cutover.
- Today opens by default. Show date navigation and selected-day status; use history search to find retained past work. Do not count empty calendar days as missed work.
- Manual entries remain separate under My notes; disclose redaction/normalization and never silently rewrite authored prose with the model.
- Render Outcome, Decision, Trade-off, Validation, Blocker, and Follow-up only when supported. Keep useful work-item, PR, commit, and validation references in Markdown; keep raw metadata in app data.
- Prefer one canonical subject in a heading and optional categorized Related links. Unmapped subjects may remain plain text.
- Review-first writes only. Never automatically Apply or automatically replace an edited draft.
- Automatic generation defaults to the previous day at 00:15 in the configured IANA timezone, once the scheduling milestone is complete.
- Raw automated evidence defaults to 30 days from receipt. Catch-up is independently configurable within the available evidence; completeness, not age alone, governs regeneration.
- Explicit Save per ordinary settings section. Review and Apply for approved folder creation or destination changes; show effective values and saved/unsaved state.
- Keep app proposals, indexes, jobs, and secrets out of the Markdown workspace.

## Existing Behavior to Preserve or Correct

The baseline already includes repository/work-item or PR grouping, manual/automated separation, durable daily jobs and evidence snapshots, cancellation/restart recovery, canonical-link validation, and temporary-file replacement. Preserve these where they match the new contract; do not count them as new implementation work without inspecting their gaps.

Correct the split mounted read/write paths, the disconnected Structure destinations, browser-specific routing, global mappings/ignored identities, path-derived workspace identity, and per-task staging overlap. Replace valid-JSON/schema-invalid daily fallback with a visible failure; invalid JSON already fails in the inspected implementation. Existing fallback tests must change with the contract.

Source anchors: [grouping and model output](../crates/mcp-server/src/llm.rs), [storage](../crates/core/src/store.rs), [Daily writer](../crates/mcp-server/src/daily_writer.rs), [HTTP routing](../crates/mcp-server/src/main.rs), and [Compose](../docker-compose.yml).

## Contracts to Settle Before Implementation

### Day, Evidence, and Proposal Revisions

| Record | Required meaning |
|---|---|
| Workspace profile | Stable generated ID, independently of mount path/browser state; reviewed workspace binding |
| Day | Workspace plus local date, retaining its effective timezone/configuration and resolved destination |
| Evidence snapshot | Immutable eligible event set/content digests for a generation attempt |
| Proposal revision | Structured workstreams and edits against a specific snapshot; one current candidate |
| Review decision | Attributed inclusion/omission for identified evidence, not an accidental consequence of editing Markdown |
| Context snapshot | Selected excerpts, source revisions/digests, collections, and retrieval reasons |
| Apply record | Operation ID, exact approved revision/hash, target, expected block revision, and recovery state |

- Keep generation status, review resolution, and new-evidence status separate.
- Every eligible event is represented, covered as duplicate/superseded evidence, explicitly omitted, or still unresolved. Never silently acknowledge dropped events.
- Validate factual-field evidence IDs against the snapshot. Traceability does not independently verify an agent's reported outcome.
- Omit resolves only that evidence version; later events do not inherit the decision automatically. Undo/reopen must disclose evidence expiry.
- Repository/work-item identities must be namespaced. Avoid basename/punctuation collisions and project/repository-wide fallback that combines unrelated tasks.
- Prefer repository plus work item, then PR, then task/session. Merge PR/work-item aliases only through an explicit relationship.
- Deduplicate lifecycle status at field level; retain unique earlier decisions and validations absent from a terminal message.
- Preserve the ingestion envelope. Evolve optional producer metadata additively and support producer-scoped retry identity; do not permit producer metadata to impersonate trusted manual authorship.
- Advanced Markdown is an explicit detached override. Preserve evidence coverage; require confirmation to discard it or return to structured regeneration. Do not promise lossless arbitrary Markdown round-tripping.
- Model/schema failure remains failed, with Retry and Create manual draft. Manual-only days need no model. Mixed-day failures must not resolve automated evidence.

### Dates, Scheduling, and Late Events

- Accept a calendar date from the browser; compute `[start of local date, start of next local date)` on the server. Never add a fixed 24 hours.
- Persist event and receipt time separately. Use event time for attribution and receipt time for raw retention.
- Specify ambiguous/nonexistent local-time handling for both day boundaries and schedule times.
- Freeze historical timezone/destination interpretation. Settings changes must not silently retarget or rebucket applied days.
- Quarantine implausibly future-dated events from automated generation without silently changing their timestamps.
- Reconcile due days on startup and periodic wake-up; use database claims, uniqueness, backoff, and bounded catch-up.
- Mark late arrivals Update available; preserve edited candidates until explicit regeneration. Stale/cancelled attempts cannot replace a newer revision.
- Dismiss resolves the current day version, quiets its reminders, and preserves evidence until expiry. New evidence and reopening remain explicit.

### Retention and Reopening

| Data | Target policy |
|---|---|
| Automated raw events | 30 days from receipt by default, configurable, with visible expiry |
| Unhandled manual notes and user-edited drafts | Retain until handled or explicitly deleted; not disposable telemetry |
| Handled proposal bodies | Retain through the supported revision window, then expire under the configured policy |
| Detailed audit metadata | 30 days by default |
| Minimal Apply ownership/idempotency metadata | Keep hashes, target and operation identity as long as required for safe writes; disclose the exception to detailed-audit expiry |
| Indexed excerpts | Rebuildable; promptly invalidate excluded/deleted source content |
| Rollback/recovery copies | Separately disclosed retention; migration backup defaults to 30 days |

Complete-day regeneration requires complete evidence coverage. After expiry, do not replace an existing day's block using only surviving or newly arrived events. Offer a clearly labeled amendment/manual reconciliation, or refuse regeneration. Reopening surviving content is not recovery of deleted evidence. Coordinate active data, FTS, diagnostics, and backup cleanup; do not promise universal forensic erasure.

### Markdown and Apply

- Mount one workspace at one container path; keep SQLite/app data outside it. The collector never needs workspace write access.
- Use a documented Obsidian-style date-token subset with literal escapes, locale and preview; reject unsupported formats rather than claim full Moment compatibility.
- Expand templates without executing scripts/plugins. Define target-date versus recorded creation-time semantics and freeze expansion across retries.
- Preserve frontmatter, line endings and user-owned content outside the managed block. Create a missing note from the reviewed template or a minimal fallback.
- Use stable workspace/day block identity independent of generation jobs. Missing/duplicated/changed markers require reconciliation; markers in fenced code do not confer ownership.
- Show the exact target and block diff before Apply. Approve date-derived folders under a configured root/pattern; unexpected destinations require review.
- Never move, rename, or delete user notes. Only create approved folders/notes and update the managed block.
- Enforce path containment, symlink-escape prevention, regular-file checks, protected editor/Git paths, and reviewed workspace binding. Use directory-relative/capability-based access rather than check-then-open path validation alone.
- Journal the exact approved revision and recovery material in app data before mutation. Lock per target, reread and validate, write/sync a same-directory temporary file, replace, sync the parent directory, then finalize evidence outcomes.
- On restart, reconcile the file's intended block hash with the Apply journal; finalize an already completed write without duplicating it.
- Atomic replacement and optimistic checks do not guarantee no lost updates against an independently writing editor. Document this limit and test supported storage/editor combinations; an absolute guarantee requires cooperation or a different output model.

### Security and Model Boundaries

- Require dashboard authentication, session expiry/revocation, CSRF, mutation authorization, and host/origin validation before enabling the refocused writable deployment. Keep loopback host publishing as the default; nonlocal access requires a documented secure route.
- One-time login challenges need an owner-only renewal/recovery command. Keep login secrets out of URI logs, referrers, exports and browser storage.
- Separate ingestion, log-read, knowledge-read, generation, review-state mutation and `vault:write` privileges. Dismiss/omit are mutations, not read-only actions.
- Initially expose Apply only through the authenticated dashboard. Later MCP Apply must require the exact approved revision/destination as well as a separate revocable write scope; a token alone does not prove human review.
- Pin supported MCP protocol/client combinations. Private static-token support is not a claim of universal remote-MCP OAuth compatibility.
- Treat retrieved notes and events as untrusted data. The model cannot choose destinations, authorize links, resolve evidence, or Apply. Sanitize preview HTML/URLs and avoid implicit remote-image loads.
- Make redaction metadata-key-aware and value-aware; redact diagnostic/model errors too. Do not promise arbitrary logs are secret-free.
- Consent must cover logs and excerpts sent to nonlocal LLMs and data returned to remote MCP clients. A local configured LLM does not make every integration local.
- Configure named OpenAI-compatible connections with one saved default and captured per-attempt choices, plus capability/schema testing. Use synthetic connection tests, endpoint/redirect validation, and renewed consent for changed providers or data categories.
- Encrypt stored model credentials using reviewed authenticated encryption and a separately mounted master key; never return stored credentials to the browser. Specify key backup, loss and rotation. Docker secret injection is not host encryption.
- Bound total prompt size, events, documents, chunks, response size, retries and concurrency. Test real-model usefulness separately from deterministic workflow correctness.

## Delivery Milestones

Checkboxes distinguish delivered behavior from evidence that still requires real use. Do not expose an insecure intermediate runtime or enable both old and new writers against the same workspace merely to advance a milestone.

### M0 — Freeze Contracts and Protect the Baseline

M0 is delivered as three behavior-preserving review slices: **M0a Contracts**, **M0b Characterization**, and **M0c Storage and security foundation**. Authentication and workspace identity must land before M1 exposes manual-entry, omission, dismissal, editing, or other new mutation endpoints.

- [x] **M0a:** Specify the day/evidence/revision schema, state transitions, ownership markers and Apply recovery protocol in the [Daily domain contract](specs/daily-domain.md).
- [x] Define the [single-owner deployment threat model](specs/threat-model.md), scopes, workspace binding, date semantics and retention exceptions.
- [x] **M0b:** Add representative golden event/day fixtures and characterize useful existing behavior.
- [x] Add formatting/Clippy checks and a Chromium/Firefox browser test harness; record actual results instead of assuming the current suite passes.
- [x] **M0c:** Add versioned migrations, consistent backup/restore verification, an idempotent migration journal, stable workspace identity, and the authentication/authorization foundation.
- [x] Expose authenticated agent ingestion through one bounded MCP `log_activity` tool without restoring legacy read, review, proposal, or Apply tools.

Exit gate: contracts and fixtures are reviewable, the backup can be restored, and no new unauthenticated writable exposure has been introduced.

### M1 — Trustworthy Daily Preview

- [x] Develop the replacement behind a disabled feature gate. Its preview may read legacy evidence, but it cannot mutate review state or Markdown until M0c is complete.
- [x] Implement server-authoritative date navigation and one current candidate with immutable snapshots/revisions.
- [x] Tighten grouping/deduplication and preserve unique decision/validation evidence.
- [x] Implement strict structured generation, adaptive fields, evidence coverage and explicit failures without raw fallback.
- [x] Keep manual entries separate; add structured editing, scoped omission/reopen, references and final preview.
- [x] Preserve edited drafts and require explicit confirmation before regeneration replaces them. Advanced Markdown remains unshipped rather than adding a detached editing mode prematurely.
- [x] Provide a useful draft with zero Knowledge collections or canonical mappings.

Exit gate: representative days can be reviewed accurately without Knowledge setup; malformed output or missing evidence cannot become an applyable automated draft.

### M2 — Safe Apply and Coordinated Cutover

- [x] Enforce the M0c authentication, CSRF and scoped sensitive-access foundation for every new mutation and write path; refocused mode excludes legacy routes and workers rather than exposing a parallel bypass.
- [x] Resolve all reads/writes from one stable workspace profile and reviewed daily convention/template.
- [x] Implement block diffs, conflict reconciliation, path safety, idempotency, recovery copies and Apply journaling.
- [x] Implement the migration protocol below, with dry-run reporting and source-preserving failure behavior.
- [x] Replace Structure destinations with the scoped context/mapping foundation; do not require a rich retrieval UI yet.
- [x] Remove browser-vault/browser-Apply APIs, IndexedDB code, per-task staging, obsolete semantic-structure APIs, separate proposal/daily/context/index mounts, and superseded configuration.
- [x] Revise runtime docs/specs/examples together; remove obsolete styles and routes without keeping legacy runtime aliases.
- [x] Make dashboard Apply the sole initial write interface; remove or disable legacy MCP Apply until its replacement is separately gated.
- [x] Exercise Compose startup, authentication, generation, reviewed Apply, owner-content preservation, and restart persistence against a disposable workspace and deterministic provider.

Exit gate: recovery, idempotency, preserved-content, authorization and containment tests pass on the declared supported storage; Compose/docs describe only the new contract. This is the first usable refocused release.

### M3 — Daily Habit, Catch-up, and Expiry

- [x] Add previous-day generation at 00:15, editable time/timezone, restart catch-up and bounded background work.
- [x] Show selected-day status, late-event updates and evidence-expiry warnings. Daily keeps its quiet date-focused layout. An optional [compact Dashboard](specs/dashboard.md#compact-dashboard) shows preparation state, recent review links and bounded model timing samples without treating historical pending rows as queued work.
- [x] Preserve edited candidates on new evidence; implement dismiss/reopen and retention-safe revisions/amendments.
- [x] Coordinate cleanup across raw data, handled bodies, indexes, audit data and recovery copies.
- [ ] Run a ten-active-day personal pilot using the [pilot guide](pilot.md); record correctness, review burden and operational problems.

Exit gate: the pilot needs no database repair or manual proposal-folder cleanup, and no loss/authorization blocker remains.

### M4 — Optional Knowledge Context

M4 remains an evidence-gated context milestone, not a commitment to build a general knowledge-management product. The intended shared-brain loop is: a human maintains a product or module note; an approved agent reads bounded, cited context; the agent reports work through normal activity ingestion; Daily summarizes the reported work while treating the note as background, never as proof that work happened. Markdown remains the source of truth.

- [x] Add named source collections: label, purpose, selected existing/reviewed-new roots, and exclusions.
- [x] Keep folder search confined to collection selection and note search to templates/mappings/context; no general vault browser.
- [x] Group identifier aliases by canonical note; surface curated unresolved identities and hide transient technical noise in diagnostics.
- [x] Resolve saved mappings and unique exact title/alias/reference matches before bounded text retrieval. Text retrieval does not create canonical links.
- [x] Explain unavailable review states, including saved links outside enabled source collections, and guide recovery through bounded collection setup.
- [ ] Add incremental local SQLite FTS only where pilot cases justify it; refresh at startup, periodically, when stale before generation, and via Rescan.
- [x] Show Context used with excerpts/revisions and per-workstream adjustment.
  - [x] Freeze and disclose exact bounded excerpts for local models; keep nonlocal models link-only, invalidate removed/excluded sources, and exclude generated blocks from self-reinforcing evidence.
  - [x] Let the owner exclude an excerpt per workstream and regenerate into a new immutable revision without changing its canonical link.
- [ ] Evaluate drafts with versus without context. Keep Knowledge optional.
  - [x] Add an opt-in blind paired-comparison workflow with identical frozen inputs, immutable shadow drafts, categorical owner feedback, and atomic selected-arm promotion.
- [ ] Run representative real-model comparisons using the [pilot guide](pilot.md) and review usefulness, editing burden, latency, and configuration effort before accepting the result.
- [ ] Prove one vertical slice with one real product and two modules: note maintenance, exact/ambiguous identity resolution, bounded agent read, activity report, and grounded Daily review.
- [ ] Add read-only context MCP only after the vertical slice passes. Use a separate credential and dashboard-side workspace boundary; keep collector `log_activity` write-only and unchanged.
- [ ] Expose note titles, relative paths, match reasons, bounded excerpts and source digests only. Recheck enabled-folder eligibility on every read; never expose arbitrary files, full-vault search, generated Daily blocks, or write tools.
- [ ] Measure retrieval usefulness, setup/maintenance effort, correction rate, latency and prompt cost. A neutral or negative result keeps Knowledge optional and stops expansion.

Exit gate: representative cases show improved factual usefulness or reduced editing with manageable configuration effort.

### M5 — Broader Integration, Only When Justified

- [x] Add UI-managed local/custom model presets, connection/capability testing, encrypted credentials and explicit activity-egress consent. Saved defaults and one-attempt Daily choices are documented in [generation models](generation-models.md). Reference excerpts remain local; broader cloud-excerpt consent and guided preferences remain gated.
- [ ] Add versioned configuration import/export without secrets or Markdown content; review destination changes after import.
- [ ] Add the optional first-run host setup helper for workspace validation, identity/security generation and Compose startup; no ongoing workflow features.
- [ ] Add structured MCP generation/inspection interfaces for tested clients only if the M4 read-only context slice proves useful; separately gate approved-revision Apply with revocable `vault:write`.
- [ ] Run a small external pilot before expanding distribution or making demand claims.

Exit gate: every additional outbound-data or write path passes its own security, compatibility and recovery acceptance tests. If a remote model is needed before M5, bring its complete privacy/security requirements forward, not only its endpoint setting.

## Migration Protocol

1. Quiesce old workers and record source configuration plus migration identity. Do not run old and new writers together.
2. Make a consistent timestamped rollback backup in app storage, retained 30 days by default; account for SQLite WAL and verify restoration.
3. Inventory proposal files, hashes, mappings, ignored identities, destinations and parse errors. Treat detected folders as suggestions, not unquestioned intent.
4. Produce a dry-run report with workspace identity and destination previews. Empty/conflicting destinations require review.
5. Import valid scoped records transactionally with source identities/digests for idempotency. Preserve unparseable bytes for manual review; fatal validation failures stop cutover without deleting source data.
6. Commit the new schema/configuration before source cleanup. Remove obsolete preferences as part of the committed transition, not before validated import.
7. Remove only successfully imported app-owned proposal files whose hashes still match; journal cleanup per item. Remove only empty application-created pending directories. Never delete modified/unrecognized files or user notes.
8. Flag historical days only for optional repair proposals; do not change already applied notes automatically.
9. Verify postconditions and recovery/rollback, including how to preserve events received after migration. Do not imply a database rollback can undo filesystem deletion or safely discard new events.

No runtime legacy fallback or compatibility aliases remain after cutover. Interrupted cleanup is a journaled migration state, not permission to resume obsolete writers.

## Acceptance Matrix

| Area | Required coverage |
|---|---|
| Time | Profile-local selection; browser timezone mismatch; 23/25-hour days; ambiguous/nonexistent times; midnight; late/future delivery; historical timezone changes |
| Evidence | Duplicate lifecycle events; producer retries; repo/identifier collisions; earlier decisions retained; invented/omitted IDs; manual-author spoofing; conflicting claims |
| Drafts | Manual/automated separation; adaptive fields; useful references; omission/undo; late data after omission; edited draft/override preservation; stale generation completion |
| LLM | Unavailable/refusing/invalid/schema-invalid/truncated output; capability mismatch; total-input bounds; failures never silently resolve evidence |
| Retrieval | Exact resolution order; ambiguous aliases; exclusions/deletion; search without auto-linking; context adjustment; background context not historical proof |
| Markdown | Templates and path preview; literal/date tokens; year boundary; Unicode; relative links; duplicate titles; BOM/CRLF/frontmatter; missing/duplicated/fenced markers |
| Apply | Correct target only; unrelated edits preserved; managed-block conflicts; two writers; symlink/traversal/non-regular targets; disk full; crash before/after rename and DB finalization |
| Security | Sessions/CSRF/origin/host checks; token expiry/revocation; scopes and approved revisions; safe previews; remote LLM/MCP egress; endpoint redirects; diagnostic redaction |
| Retention | Expiry during generation; incomplete-evidence regeneration refused; manual draft preservation; dismiss/reopen after expiry; coordinated FTS/backup cleanup |
| Migration | Global rules/ignores; empty destinations; browser preferences; pending files; live WAL; unparseable/changed files; interrupted cleanup; rollback with newly received events |
| UI and deployment | Firefox/Chromium navigation, keyboard use, settings persistence, login, editing and error visibility; temporary-workspace Compose test; obsolete artifacts removed |

Keep deterministic fake-model workflow tests separate from real-model quality evaluation. Preserve existing passing Rust behavior only where it matches the new contract; replace tests that enforce obsolete behavior. Check formatting and Clippy in CI; do not assume either is available or passing until verified.

## Pilot and Scope Gates

Use ten active engineering days, comparing some drafts with a small Git/manual-notes baseline. These are initial product hypotheses, not industry benchmarks:

- Zero lost user-owned content, unauthorized writes, silent draft overwrites or silent evidence loss. Any occurrence blocks expansion.
- Measure active review time; initially aim for a median below three minutes and inspect outliers.
- Track unsupported/corrected factual fields and capture of important outcomes, decisions and validations, including work without commits.
- Test later recall and resuming unfinished work; polished prose alone is not success.
- Measure setup, model latency, mapping and maintenance effort. Simplify if context administration exceeds the benefit.
- Do not add general knowledge-note writing or commercial hosting until the daily workflow proves useful. Demand and willingness to pay remain unverified.

## Explicitly Deferred

- Product-note mutations, durable decision records, and feature recaps until each has a complete proposal/review/apply lifecycle.
- General note creation/editing tools, vault trees, reorganization, graph UI, template languages, and automatic “everything needs a home” queues.
- Multiple active workspaces/models, teams, public multi-tenant hosting, and guaranteed compatibility with every Markdown extension.
- Broad autonomous MCP writes or automatic Apply.
- An autonomous knowledge writer, automatic note creation, or activity-to-knowledge promotion without a separate proposal/review/apply contract.
- New brokers/microservices as a prerequisite for modularity. Split Rust modules and UI responsibilities first.

## Design References

These sources support design constraints, not claims that the roadmap is implemented. External documentation was consulted for the 2026-09-09 direction review; pin implementation versions and revalidate compatibility at delivery.

- [MDN: showDirectoryPicker](https://developer.mozilla.org/en-US/docs/Web/API/Window/showDirectoryPicker) — limited browser availability and secure-context requirements.
- [Docker bind mounts](https://docs.docker.com/engine/storage/bind-mounts/) and [Compose secrets](https://docs.docker.com/compose/how-tos/use-secrets/) — host access and mounted-secret boundaries.
- [Obsidian daily notes](https://obsidian.md/help/plugins/daily-notes), [templates](https://obsidian.md/help/plugins/templates), [Moment formatting](https://momentjs.com/docs/#/displaying/format/), [Zettlr settings](https://docs.zettlr.com/en/reference/settings.html), [Foam link references](https://docs.foam.md/features/link-reference-definitions/), and [SilverBullet Markdown](https://silverbullet.md/Markdown) — explicit compatibility surfaces.
- [Chrono local-time mappings](https://docs.rs/chrono/latest/chrono/offset/enum.LocalResult.html) — ambiguous and nonexistent times.
- [SQLite FTS5](https://sqlite.org/fts5.html), [WAL](https://sqlite.org/wal.html), and [backup API](https://sqlite.org/backup.html) — indexing, deletion, same-host storage, and consistent backup.
- [Linux openat2](https://man7.org/linux/man-pages/man2/openat2.2.html), [rename](https://man7.org/linux/man-pages/man2/rename.2.html), and [fsync](https://man7.org/linux/man-pages/man2/fsync.2.html) — containment, atomicity and durability are distinct guarantees.
- [Ollama structured outputs](https://docs.ollama.com/capabilities/structured-outputs) — schema capability still requires application validation and quality evaluation.
- OWASP [CSRF](https://cheatsheetseries.owasp.org/cheatsheets/Cross-Site_Request_Forgery_Prevention_Cheat_Sheet.html), [sessions](https://cheatsheetseries.owasp.org/cheatsheets/Session_Management_Cheat_Sheet.html), [SSRF](https://cheatsheetseries.owasp.org/cheatsheets/Server_Side_Request_Forgery_Prevention_Cheat_Sheet.html), and [prompt injection](https://cheatsheetseries.owasp.org/cheatsheets/LLM_Prompt_Injection_Prevention_Cheat_Sheet.html) — layered request, model and egress defenses.
- MCP [Streamable HTTP](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http) and [authorization](https://modelcontextprotocol.io/specification/2026-07-28/basic/authorization), revision 2026-07-28 — explicit protocol/security compatibility choices.
- Basic Memory [lifecycle capture](https://docs.basicmemory.com/integrations/harness-capture) and [Cloud review suggestions](https://docs.basicmemory.com/whats-new/comments-and-suggestions), plus [Foam MCP](https://docs.foam.md/tools/cli/mcp/) — adjacent documented capabilities. Differentiate through the useful daily engineering review workflow, not Markdown/MCP alone.

## Accepted-work KB capture — 2026-10-19

Canonical task record for the owner's request to plan and fix feeding accepted work
into Product KB. Starting HEAD/base `bb5a111aa60357940f1ba214e2328a3f667d9291`;
branch `codex/accepted-work-kb-capture` (runtime-required prefix). Endpoint: local
implementation and validation; no deployment or PR publication requested.

Decision: implement direct original-source capture independently of Daily prose and
model usefulness. The owner selected Product KB ingestion as the next work. Earlier
Daily usefulness failures remain recorded; this scoped flow replaces the runtime
coupling for accepted-work capture/review/promotion, without treating that gate as
passed or enabling agent recall. Detailed domain, payload, API and recovery contract:
[Accepted-work Knowledge capture](specs/knowledge-capture.md).

Plan and acceptance:

1. Receive a bounded structured submission through existing redacted ingestion;
   retain independent plan/acceptance/implementation/verification/merge/deployment claims.
2. Materialize immutable workspace-scoped candidates and exact owner dispositions.
   Exact retries deduplicate, conflicts/forks fail, corrections supersede explicitly,
   missing evidence cannot inherit approval. No Daily generation or Markdown write.
3. Preview and confirm one existing eligible canonical note through distinct KB
   blocks and the shared atomic writer. Freeze authority and exact bytes in an
   additive KB journal; retain Daily and human content and provide restart recovery.
4. Expose native Reference notes capture/review/preview/confirm/discard controls;
   validate storage, authorization, stale edits, expiry, recovery and browser flow.
5. Independent complete task-diff review, required repository checks and disposable
   runtime validation. Coordinator owns this task record and integration.

Initial implementation stage: plan, implementation, complete independent review, integrated Rust/browser validation and exact-final-source disposable Compose smoke passed. Commit, deployment and publication were separately authorized afterward; their records follow.
Factory routing: bounded Sol/medium core and app workers; independent Sol/high
reviewer; current chat coordinates. Ponytail lite applied for this implementation
stage: reuse ingest, existing scopes/native DOM and atomic writer; no new dependency,
service, model generation or recall interface.

| Contract/scenario | Evidence | State |
| --- | --- | --- |
| Original-source capture without Daily; independent claims | Core/API fixtures including plan-only completed task | Passed |
| Exact/concurrent retries, conflicting keys, explicit supersession, workspace scope | Core regression tests and immutable SQL constraints | Passed |
| Expired/mutated source blocks fresh authority | Core digest/expiry tests | Passed |
| Upgrade from populated v26/v27 and reopen; prior data retained | Core upgrade fixtures at schema29 | Passed |
| Owner scope, bearer-only rejection, Host/Origin/CSRF, immutable review | Capture API integration regression | Passed |
| Native review and exact confirmation/discard controls | Focused Chromium/Firefox and reference regressions | Passed |
| Preserve human/Daily blocks, stale target/collection, crash before/after rename | Promotion integration and first live Compose/restart smoke | Passed |
| Complete task diff and documentation consistency | Independent Sol/high full review against starting HEAD, tracked/untracked paths; focused followups retained | Passed; no remaining findings |
| Required formatting, Clippy, workspace tests and browser tests | 307 Rust passed/13 ignored; 250 Chromium/Firefox passed; Rust/client format and Clippy passed | Passed |
| Exact final source runtime | Release Compose HTTP capture/review/preview/confirm/stale409/restart/idempotence/no extra Daily generation; 13 runtime/source hashes identical before/after | Passed |

Known scope: inline candidate editing uses a new corrected producer revision; KB
audit records have no new automatic pruning policy. Source-backed canonical text
may remain stale until a reviewed superseding update is explicitly confirmed. Useful
agent recall remains a separate measured pilot. No owner vault note is changed by
this development task; test writes use disposable workspaces.

Review fixes: expose safe preview discard/cancellation, serialize one plan across canonical destinations, align review scopes, preserve all unfinished Daily write ownership and check it before a new journal is inserted, and reject updates exceeding per-note/aggregate Reference budgets. Regressions cover these paths. Full review uses checkout/branch above, HEAD/base/merge-base `bb5a111aa60357940f1ba214e2328a3f667d9291`; followups reviewed affected code/tests/docs after each fix. Actual disk-full/fsync fault injection for KB and arbitrary concurrent external-editor races remain unverified; the inherited atomic writer, synthesized staged/renamed recovery and live restart are tested.

Final validation: `cargo test --workspace -- --test-threads=4` passed 307 tests with 13 ignored; `cargo clippy --workspace --all-targets -- -D warnings`, Rust formatting, client formatting, both browser engines (250 tests), shell syntax and documentation-link/diff checks passed. Initial sandbox runs could not start local provider mocks/browsers; permitted runs completed. One original 5-second generation test timed out under concurrent Docker/browser load; the final reduced-parallelism full suite passed without changing that test or its deadline. Final live smoke used only disposable synthetic data and matched the reviewed runtime files before and after its release build/run.


### Local deployment — 2026-10-19

The owner subsequently requested a commit and deployment to the existing local
installation. Feature commit `f6408269a36bf45e36ff29ebbdc72fabd2bcae0b` on
`codex/accepted-work-kb-capture` contains the reviewed/tested runtime. Built and
started only the existing collector and Daily services using the same Compose
project, provider override, environment, ports and mounts; the model service was
left running. Both services report healthy at the existing loopback addresses.

Before upgrade, stopped both app writers and copied their app data, then created
and integrity-checked a consistent SQLite backup. Previous images are retained as
local rollback tags. Recovery material and the deployment receipt are private,
ignored files under `tmp/deployments/kb-capture-f640826`; do not restore the old
schema snapshot over newly received evidence without reconciliation.

Live verification: schema 27 → 29; all 1000 backed-up event IDs retained; nine existing
protected configuration/Daily/review tables have identical row hashes; all 32
tracked Daily/canonical-note hashes unchanged. Running image IDs match the new
builds; environment, provider/workspace mounts and port bindings are unchanged.
Authenticated submission/candidate/promotion GET endpoints returned 200, and the
served KB client matches the committed asset. The live browser reaches normal
owner sign-in; authenticated page interaction is left for the owner. The previous
250 browser tests and exact-source release Compose workflow remain the UI and
write/restart evidence. No owner candidate is accepted and no canonical note is
written as part of deployment.

Try the flow at `http://127.0.0.1:8788/?view=knowledge`: sign in and open Product KB
submissions. The deployment's structured terminal report supplies real source
material for capture and review, with implementation, verification and local
deployment retained as separate reported claims. KB acceptance and canonical
confirmation remain the owner's actions. No remote publication was requested.


### KB flow PR publication — 2026-10-19

After trying capture, review, preview and the completed canonical update, the owner
requested [PR #4](https://github.com/davidsellen/log-inbox/pull/4). The PR targets
`main`; its 26-path scope retains the reviewed KB implementation and local
deployment documentation. The target at publication is `30e636b`, with merge-base
`bb5a111`; their trees are identical, so the previous complete implementation
review remains applicable. Publication review found no remaining actionable issues.
The PR tracks current CI and readiness; local validation and deployment evidence
remain recorded above. No merge is authorized by this publication request.

UX follow-up: the owner finds the flow acceptable for this delivery and wants the
experience improved. Refine that scope in follow-up work; this PR does not add a
redesign or treat the first UI as the final experience.
