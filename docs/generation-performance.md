# Daily generation performance

Daily exposes the current attempt, elapsed time and the configured total timeout. Server logs record model request time and prompt byte counts without recording prompts, note contents or credentials. A fast HTTP acceptance response is not a completed draft.

Daily deterministically prepares source-scoped fact cards, then makes one composition request per existing evidence group. Each request remains subject to the hard input limit; groups and server-owned evidence identities are preserved. Invalid output fails without a fallback or transport retry. All work shares the existing attempt deadline. Progress counts validated groups, not generated tokens. Historical batching experiments below describe earlier contracts.

Reference snapshots are validated before inference. A mapped note outside the selected usable collections fails explicitly; Daily offers a one-attempt retry without reference notes rather than reading unselected files. This does not change reference settings.

## Repeatable local-model measurement

The normal test suite never contacts a real model. The ignored benchmark accepts a private JSON array containing one to three representative days, ordered small, medium and large. Each item has `events` (the existing `StoredLogEvent` JSON shape) and `vault_context` (the bounded context projection actually supplied to generation). Keep this file outside source control and exclude secrets. The benchmark produces no revisions or Markdown and prints only counts, durations and validation outcomes.

Run with an explicitly configured local model:

```sh
LOG_INBOX_BENCHMARK_INPUT=/tmp/private-daily-benchmark.json \
LOG_INBOX_LLM_BASE_URL=http://127.0.0.1:11434/v1 \
LOG_INBOX_LLM_MODEL=granite3.3:2b \
cargo test -p log-inbox-mcp-server benchmark_representative_days -- --ignored --nocapture
```

For each day it compares activity-only and supplied context, with two consecutive passes. Every pass validates the structured output and has the configured total timeout. A failed or timed-out result remains a result, not a fast successful sample. A first pass is **not automatically cold**: note the model's actual load state separately. To measure cold loading, use an isolated provider started without a loaded model; do not unload a model serving active Daily work.

Empty context fixtures skip the duplicate context run. Set `LOG_INBOX_BENCHMARK_VARIANT=baseline` to measure the historical free-prose single-group strategy on the same fixture and model. Omit it for the current source-card/selection pipeline. These are different generation contracts, not a batching-only speed comparison. The benchmark reports a legacy raw-evidence prompt-size diagnostic; it is not the sum of actual v9 request sizes. Progress prints completed/total groups. Keep the model idle from other workloads during comparisons and do not run both variants concurrently.

Compare output usefulness as well as elapsed time. Context is worthwhile only if it improves factual usefulness or reduces editing. Do not lower evidence coverage or claim a speed improvement from a faster invalid output. Keep the existing timeout until measured representative results justify a change.

Real-day quality evaluation and the ten-active-day pilot require actual use. Synthetic workflow tests establish reliability, not summary quality or real-model speed.

## October 13–14 paired quality evaluation

The ignored `evaluation::evaluate_real_days` test runs the production Knowledge
resolver, generation/validation and reviewed Markdown renderer without opening a
Store, creating a Daily revision or invoking Apply. It requires a private fixture
containing the exact applied evidence snapshots, reviewed collection/mapping
definitions and the existing workspace root. No private fixture belongs in source
control. The two days are ordered October 13 then 14; each contains at most 500
complete events. Manual prose remains outside this automated-evidence comparison.

Set `LOG_INBOX_EVALUATION_INPUT` to that JSON fixture,
`LOG_INBOX_EVALUATION_OUTPUT` to a new private directory outside the workspace and
checkout, and explicitly configure a local model through the normal LLM environment
variables. Run:

```sh
cargo test -p log-inbox-mcp-server evaluation::evaluate_real_days -- --exact --ignored --nocapture
```

The manifest freezes input/model/contract identities and Knowledge before inference.
October 13 runs activity-only then Knowledge; October 14 reverses that order. Each
arm has the normal configured timeout and records its own terminal result, including
failures, elapsed time and successful structured content/Markdown. Artifacts are
private, complete files are published without overwrite, and the same invocation
can resume missing arms only against the matching frozen manifest. Use a fresh
directory for repeated measurements. Record the code and immutable model digests
alongside the manifest; a model tag alone is not an immutable weight identity.

For a changed generation contract, set
`LOG_INBOX_EVALUATION_FROZEN_MANIFEST` to the original private manifest and use a
fresh output directory. Replay verifies the fixture digest, exact dates/snapshot
IDs/event IDs/event digests/baseline, original Knowledge digest, model/profile and
timeout, then copies the frozen cases without inspecting or resolving source notes.
It records the original manifest digest/identity alongside the new contract and
configuration. A changed temporary local proxy address is permitted; verify the
same local model weights separately. Ordinary resume still requires the exact new
manifest identity and never overwrites a terminal arm.

Freeze the [quality rubric](pilot.md#daily-quality-regression-sample) before reading
model output. Score facts against source events, preserve the latest explicitly
supported state, and distinguish generation rejection from a published false claim.
A failed arm is unusable, not an accurate empty summary. Mask arm labels for an
independent review where practical; canonical links can still reveal context use.
One run per arm is preliminary and does not establish repeatability or the owner's
usefulness judgment. Load/warm-cache effects also prevent treating elapsed-time
differences as a controlled speed benchmark.

### Observed result — 2026-10-15

The real-day trial used the exact applied October 13/14 snapshots (32/44 available
events), current v5 code, and the saved local default `qwen2.5:1.5b` (model ID
`65ec06548149`, weight SHA-256
`183715c435899236895da3869489cc30ac241476b4971a20285b1a462818a5b4`).
Knowledge definitions and current note excerpts were frozen once. No production
records, settings or Markdown were changed.

| Day | Activity-only time to rejection | Knowledge time to rejection | Outcome |
|---|---|---|---|
| October 13 | 128.8 s | 103.8 s | Both `model_lifecycle_coverage`; only first 16-event group reached |
| October 14 | 179.3 s | 138.0 s | Both `model_lifecycle_coverage`; 19 events across two groups reached |

Zero of four arms produced a complete usable candidate. All six provider requests
returned HTTP 200, valid JSON and normal `stop` finish reasons. The evaluation
test passed because it recorded every terminal outcome; that is not a generation
quality pass. Counterbalanced order does not eliminate model-loading/cache effects,
and these are rejection times, not time to useful output.

Independent label-masked review preferred the activity-only October 13 diagnostic
for recoverable source content: four substantive fragments versus none with
Knowledge. It still required substantial rewriting, contained two strings cut at
the 320-character field limit, one wrong event citation and placeholder validation.
The Knowledge diagnostic contained only illustrative prompt text. October 14
outputs were identical, with five placeholders per arm and no substantive facts.
One single-event workstream even structurally passed with `Supported fact` as its
only outcome, exposing a semantic/template-echo gap beyond coverage checks.

October 14 had two ambiguous groups without excerpts; its failing larger group
therefore did not meaningfully test reference grounding. The later diagnosis and
completed implementation groups were not reached. Unreached work is not counted
as a model omission, and this trial cannot establish preservation of their pending
states. One run per arm establishes no repeatability or owner usefulness result.

The conclusion is **no demonstrated Knowledge benefit under this configuration**.
Keep the real-model usefulness gate unmet. Before KB promotion/agent-recall
expansion, remove template echoes, preserve typed evidence/state with less factual
reconstruction burden, test representative 16–18-event groups, and repeat the
frozen comparison (with any model change as a separately identified experiment).
Do not weaken coverage or raise token limits merely to accept these outputs.
Private artifacts, source references and the independent assessment are retained
under the ignored `tmp/evaluations/oct13-14-20261015/` directory on the evaluation
host; no raw evidence or note excerpts are committed.

### Source-claim correction and frozen rerun

The owner requested a production fix followed by the same comparison. Contract v6
removes factual examples, rejects generated template echoes, moves typed producer
claims into field-indexed source cards, and exposes every required field/event-ID
set using the validator's predicates. Original outcome/context metadata and empty
or false values remain available; populated typed values are moved, not duplicated.
No evidence check is relaxed, no missing model facts or citations are supplied by
the server, and no activity-record fallback is introduced. This changes the model
task from discovering field requirements inside metadata to summarizing explicitly
associated source claims; semantic entailment still requires inspection.

The v6 trial replays the original private manifest with the same 32/44 events,
Knowledge excerpts, model weights, profile, timeout, order and scoring rubric. New
outputs are separate from the initial v5 artifacts. The unchanged input/context
digests were verified before replay; no current vault resolution occurred.

| Day | Activity only | With Knowledge |
|---|---|---|
| October 13 | Rejected `model_output_truncated`, 390.2 s | Structurally accepted, 755.4 s; failed source review |
| October 14 | Rejected `model_content_invalid`, 14.1 s | Rejected `model_content_invalid`, 43.9 s |

Observed known placeholders fell from 13 in the original reached provider output
to zero. Structural completion rose from zero to one of four arms, but **usable
daily summaries remained zero of four**. Independent, non-blind source review of
the completed October 13 Knowledge candidate found two supported narrative
bullets and seven misleading bullets out of nine: early implementation-pending
claims superseded by later implementation, deployment and validation events.
All six BankID fields repeated one sentence. None of the six applicable rubric
items was fully covered; two were partially covered. Broad citation lists passed
membership/coverage while failing entailment. This is not a factual improvement.

October 13 activity-only stopped at fonts; October 14 both arms stopped at the
first one-event review-start group with empty arrays, leaving 43 events unreached.
No full-day paired usefulness comparison or later-work accuracy claim is possible.
The first fonts prompts grew from 6,162/7,018 tokens without/with Knowledge to
7,466/8,322; typed source cards reduce field discovery, not input size. Times are
terminal-outcome latency, not a controlled speed benchmark; compilation/smoke
overlapped part of inference. Single runs do not isolate each change or establish
repeatability.

All six requests returned HTTP 200. Only the activity-only October 13 response hit
the output-token limit; the others stopped normally. Keep the real-model
usefulness gate unmet and do not promote these outputs into Knowledge or agent
memory. Next evaluate smaller linked lifecycle units or per-source extraction
before final prose, retaining all evidence gates; any model change is a separate
experiment. Private output, provenance and independent assessment are retained
under ignored `tmp/evaluations/oct13-14-v6-20261015/` on the evaluation host.

### Per-source extraction and selection-only composition

The owner-requested correction separates source extraction from composition.
V9 deterministically retains all eligible cards from each source in isolation
without Knowledge or model pruning. The server validates exact source boundaries,
assigns fact IDs and retains source timestamps. Composition selects one fact per
source/field slot from that ledger; it cannot
write factual prose or arbitrary citation lists. Final source-time framing and
citations come only from visible selected clauses. Required source-field coverage,
newest-field inclusion and all existing final output checks remain enforced.

The new frozen trial uses the original October 13–14 evidence, model, Knowledge
and rubric. Extraction is frozen once per date and revalidated against source
digests, then shared by both Knowledge arms. Extraction duration counts against
each arm's existing total timeout; separate stage times make this protocol change
explicit. Exact provenance does not establish complete qualifications, and output
that cannot fit retained limits fails.

The v7 implementation passed 230 Rust tests (three ignored), Clippy, formatting,
independent incremental review and isolated workflow/restart checks. Its frozen
trial made four provider requests, all HTTP 200, valid JSON and normal stop. Both
dates failed at their second source: October 13 after 33.304 s, October 14 after
23.965 s. The model paired quotations with wrong claims, altered literal text or
clipped it. Source checks rejected these outputs. Both Knowledge pairs were
blocked before composition; they are not four independent model failures. No
summary accuracy, latest-state preservation or Knowledge benefit can be measured.
Private artifacts remain under `tmp/evaluations/oct13-14-v7-20261015/`.

A separately identified v8 correction replaces generated quotations with
selection of server-owned eligible clause IDs. Source-local extraction and final
composition remain separate, but neither model stage copies or creates factual
text. Existing checks and final limits remain; v7 outcomes are immutable. V8
implementation passed 233 workspace tests (four ignored), Clippy, formatting,
independent review and isolated workflow/restart checks. The no-model preflight
passed all 32/44 actual sources. A wrapped-negation regression prevents bare
newlines becoming clause boundaries. The frozen trial produced zero usable
summaries in four arms. October 13 extracted 32 sources/67 facts in 369.498 s;
composition failed in both arms (660.547 s total without Knowledge; 445.840 s
with Knowledge). October 14 extraction failed on duplicated IDs at source 40
after 440.353 s, blocking both arms before composition. All 75 HTTP responses
were normal-stop valid JSON. Independent source review found missing essential
font validation and accepted decisions in the extracted ledger. This provides no
accepted paired summary or evidence of a Knowledge benefit.

V9 is a separately implemented correction: deterministic retention of all eligible
source cards before single-group model selection through source/field-specific
slots. Compact temporal rendering and deterministic packing retain the same
output limits and fail-closed checks. Its fresh frozen trial is reported separately; no v8 artifact or source ledger is
reused as a v9 result. Source preparation completed without model calls for all
76 events (197/266 cards, 48/67 ms). Independent non-blind review found essential
source material for all twelve rubric topics, with ineligible long/source-note
qualifications disclosed. Availability does not establish composed accuracy.
The frozen v9 comparison completed: 0/4 accepted or usable daily candidates.
All six HTTP responses were normal-stop valid JSON with valid source-slot IDs;
no runtime/protocol/rubric changes occurred. Both October13 arms select identical
facts requiring seven outcome bullets. Both October14 arms pass the first small
group then select identical PR9713 material requiring ten outcome/five follow-up
bullets. Overflow fails at the unchanged four-facts/field limit. Later groups are
unreached, not scored omissions. Total charged seconds (off/on): October13
122.984/143.344; October14 190.201/154.970. Single CPU samples do not support a
performance comparison. Source retention and source-bound citations improved;
complete-day accuracy and Knowledge benefit remain unestablished. The remaining
limitation is useful selection within the output budget.

Final integration: 240 Rust tests passed, four ignored; Clippy, Rust/client
formatting, real-source layout preflight and disposable workflow/restart passed.
Browser suite had216 passes and four overlapping-artifact cleanup errors; those
four passed in an isolated rerun. Changed-map/line-ending regressions passed in
both engines. Independent incremental review found and verified server review,
client projection and owner-edit fixes, with no open findings. Private full
protocol/provenance/outputs/report: `tmp/evaluations/oct13-14-v9-20261015/`.
No production Apply, KB/agent-memory promotion, publication or deployment.

### Publication follow-up

Full PR review found and corrected padded short typed-source rejection and legacy
ADO/PR title/alias, exact-mapping and ignored-identity normalization regressions.
Only source-value outer whitespace is normalized; internal whitespace/negation
and source digests remain intact. The frozen v9 real-model trial above predates
these publication fixes and is preserved unchanged; its negative result is not a
real-model measurement of the later publication revision. Required-source layout
preflight and regression checks are rerun on the publication code. The next
Product KB delivery is queued in the canonical roadmap; it is not implemented.
