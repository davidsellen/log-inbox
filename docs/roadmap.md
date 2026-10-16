# Daily Engineering Knowledge Roadmap

Status: reviewed Daily workflow implemented; generated-summary usefulness and Product KB ingestion remain gated. Updated 2026-10-15. Initial baseline inspected: `ae46de99fd0e80fa4d70ef92a953d076bcf04057`.

This roadmap defines the target direction, delivered foundation, and remaining delivery order. The [README](../README.md) and runtime specs describe the active Daily release; milestone checkboxes record implemented and pending work.

## Product Promise

Turn selected engineering activity into a concise, evidence-linked daily record: what changed, why, what was validated, and what remains. Let the owner review and correct it before writing to existing Markdown notes.

Keep Log Inbox a single-owner, self-hosted HTTP collector, dashboard, scheduler, MCP interface, and reviewed Markdown writer. It is not a vault manager, Notion clone, generic agent-memory platform, or developer productivity tracker.

- **Daily is the product:** date navigation, structured drafts, evidence review, omissions, preview, and Apply.
- **Reference context is optional shared context:** human-maintained Markdown product/module notes can ground Daily drafts and, later, approved agent reads. It is not a prerequisite for a useful draft and is not an autonomous memory store.
- **Settings configures the service:** workspace, dates, automation, model connection, retention, security, and integration.

Next queued delivery: [bounded selection and reviewed Product KB ingestion](#next-product-kb-ingestion-delivery). The plan is recorded below; KB ingestion is not implemented.

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
that does not establish semantic acceptance. Draft publication is pending. Local deployment, full-day
comparison, merge and remote source-branch deletion are blocked by semantic
acceptance; Product KB ingestion remains gated. No model-output repair, weakened
checks or selective retries were used. Private evidence is preserved under
`tmp/evaluations/oct13-14-v12-20261016/` through `oct13-14-v15-20261016/`.

| Acceptance scenario | Evidence at v15 working tree | Result |
| --- | --- | --- |
| Same source facts, precise ownership and strict rejection | Three actual responses validated by production parser; Rust/Compose regressions | Pass |
| Smaller selection request with unchanged catalog | Frozen matched request: 189,522 → 36,670 bytes | Pass |
| Coherent grouping across all original sample subjects | Independent 47-card assessment; two authentication must-links fail | Fail |
| Cross-window exact subject reuse | Deterministic half-split protocol frozen; no inference after failed prerequisite | Not run |
| Useful summaries with/without Knowledge, October13–14 | Original evidence and prior failed trials preserved; prerequisite fails | Not run |
| Build and integration | 269 Rust, 224 browser, fmt/Clippy/client, release, isolated Compose/restart | Pass |
| Existing local stack, merge and branch cleanup | Release blocked; keep running stack and unmerged source branch | Blocked |

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
