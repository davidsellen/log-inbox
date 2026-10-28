# Daily Preparation

Preparation turns one server-resolved day of bounded automated evidence into a structured review candidate. The server API defaults to source-based preparation and requires no model. Daily initially selects Human summary (Semantic) when an eligible saved default model is available; explicit selection and retry choices take precedence. Neither mode chooses the destination, writes Markdown, resolves review decisions, or rewrites owner-authored notes.

## Active flow

1. The server resolves the selected calendar day and reads its eligible events.
2. Rust groups events using namespaced repository, work-item, pull-request, task, session, and source identities. Numeric/string/URL and consistent object references share identity normalization with Knowledge. A task may inherit its single unambiguous external identity. Otherwise an exact feature branch can associate tasks without explicit identities; integration branches and conflicting or malformed identities cannot authorize that merge.
3. The service freezes event IDs and content digests in an immutable evidence snapshot.
4. When Knowledge collections or mappings are enabled, the server resolves reviewed mappings and unique exact title, alias, or typed-reference matches. Conflicting matches produce no link. Repository-wide mappings may authorize a link but cannot merge distinct work items; only reviewed work-item/PR relationships may alias groups.
5. Rust deterministically extracts all eligible bounded fact cards from each automated source separately, without Knowledge. Rust validates each card and assigns source-owned fact IDs. In default source-based mode, Rust retains each conservative source identity bucket and selects at most four bounded source-report bundles per field, prioritizing distinct reported tasks/events and their main reports, without inference or a configured provider. Semantic mode uses the same deterministic source retention and highlight selection, preserving reviewed group aliases. It does not ask the model to partition cards or select highlight IDs. Source highlights retain source-owned prose and citations. Only semantic mode sends activity to a provider; nonlocal endpoints require connection-specific consent. Only verified local semantic endpoints may receive bounded canonical-note excerpts.
6. Rust renders validated display projections with source times, derives citations only from visible supporting clauses, retains every original card as typed reported history, and validates group membership, complete evidence coverage, factual evidence IDs, canonical links, and text limits using the persisted revision contract. Malformed extraction, stale-only selection and invalid composition fail visibly.
7. Semantic preparation makes one writing request using a focused reported view: all cards from each workstream's latest material report (or its latest context report when none is material), without older selected clauses. It sends activity identities without manual prose, Knowledge titles, canonical links or excerpts. It synthesizes a short recap paragraph and up to three Done/Pending topics, mapping request-local citations back to actual snapshot event IDs. The parser rejects malformed, oversized or unknown-citation output; invalid recap generation fails visibly rather than returning a report dump. These editorial statements are model interpretations, not independently verified facts; task completion must not imply acceptance, verification, merge, deployment or release. A valid result becomes an immutable proposal revision bound to its evidence and optional Knowledge snapshots in SQLite. Semantic model errors never silently switch the selected mode.
8. The server renders the reviewed structure deterministically for preview. Only explicit dashboard Apply invokes the Markdown writer.

Source-based and manual-only days need no model. Semantic mixed days send only automated evidence to the provider and keep manual prose verbatim in a separate section. The separately selected activity-record path remains available; source preparation is a structured, cited proposal rather than a free-form Markdown fallback.

## Structured output

Each supplied workstream may contain evidence-backed Outcome, Decision, Trade-off, Validation, Blocker, and Follow-up facts plus open questions. Every factual item names evidence from that workstream. Unknown fields, invented or missing evidence, cross-group evidence, placeholder titles, missing fact text/citations, and empty factual workstreams are rejected.

Generated factual text and citations retain `{text, evidence_event_ids}`. Server
rendered bundles also carry optional `source_clauses` with each exact clause and
its source event, preserving review exclusions through preview, Copy and Apply.
Model responses cannot supply this metadata. Stored facts without clause metadata
remain readable; mixed-source legacy facts are hidden as a whole if any cited
source is excluded, since their clauses cannot safely be separated. Invalid or
stale clause maps are rejected when saving owner edits. The editor clears a map
only when its fact text is explicitly changed; unchanged and title-only saves
preserve the original text/map, including browser-normalized Windows line endings.
The readable Daily projection applies the same clause filtering as server-rendered
Copy and Apply. Both modes share deterministic source extraction and the same validated composition catalog. Both modes choose bounded exact source reports in Rust. Semantic mode then writes an editorial recap from the focused reported view, while retaining the complete original ledger in review history. The model sees request-local integer source references instead of repeatedly emitting full event IDs. The model writes each claim before its supporting references; the stored/public fact still uses `text` and `evidence_event_ids`. The prompt reconciles each subject chronologically, replaces earlier blockers only when a later report explicitly resolves them, and treats read-only scope qualifications as qualifications rather than invented pending work. The two newest material anchors require citation coverage across the recap. This is a bounded relevance check, not proof of factual entailment or automatic supersession. Strict parsing rejects unknown, duplicate, unoffered or out-of-range references and maps valid references back to the exact original snapshot IDs before normal recap validation and storage. The request shares common source-group identity metadata once and preserves differing per-report identity values and lifecycle context. Original report text is retained, and temporal order remains explicit. The model-only response budget is a paragraph of at most 500 characters, one to three topics, status text of at most 240 characters and up to six supporting references per statement. Existing public/manual/historical recap limits remain unchanged. New model output with repeated normalized topic titles, identical Done/Pending text pairs or fact text ending in a comma, colon or semicolon is rejected; this narrow repetition check does not establish factual entailment and does not change historical/manual recap acceptance. Workstream source citations remain derived exclusively from server-owned facts. Existing stored
revisions and historical string-compatible readers retain their format; no
migration or automatic rewrite is performed. The generation contract is
`daily-structured-v27-latest-report-recap`, invalidating ordinary cache reuse from older
contracts. Final output retains four facts per field and 320 characters per fact.
Oversized selected bundles fail rather than being clipped.
Required visible source clauses may be impossible to fit within these inherited
limits, even when a less grounded free-text summary previously fit. Such a group
fails closed; this contract preserves the evidence checks, not a guarantee that
every formerly representable group remains generatable.

Extraction processes the bounded message and allowed metadata of exactly one
event, including explicit `metadata.outcome`, original typed decisions, checks,
blockers and follow-ups. Rust
retains all eligible cards from complete short source values or complete original
sentence/semicolon clauses of longer strings. A bare newline cannot split a
clause. Only outer whitespace is normalized; internal whitespace, including CRLF,
and negation remain exact source content. Clipped or unrepresentable clauses are excluded; if required material has
no eligible representation, preparation fails. No model request prunes this ledger.
Eligible whole typed containers are retained as one exact JSON value, without also
emitting their descendants. This nonoverlapping extraction frontier keeps all
statuses, counts, names and qualifications together. Oversized or otherwise
ineligible containers still descend through original pointers; scalar extraction
is unchanged. No deduplication occurs across source pointers or events.
Fact IDs map to field-scoped source pointers and exact quotations. Rust verifies
the pointer, quotation, field compatibility, source identity and required fields;
reloaded ledgers are checked again against original evidence. Not-run, pending and
probable qualifications remain source data; an extracted validation fact does not
establish that checks passed. Knowledge cannot create source facts.

### Default source-based mode

Source identity buckets are review aids, not inferred semantic subjects. The UI
labels the mode **Source-based preparation · review grouping** and **No model
used**. Repository/work-item/PR/task/feature-branch and conservative ambiguous
identity handling reuse the existing grouping rules. Each raw identity bucket
remains separate in both Knowledge arms; reviewed note aliases cannot merge it.
Server-encoded branch keys retain their stable identity but display the original
UTF-8 branch name in headings; malformed/control-character names use a safe
generic branch label.

Rust creates one nonsemantic bucket assignment containing every eligible card,
rebuilds and validates the complete ledger against original events, then ranks
eligible cards per field. The priority queue first covers distinct exact task
identities (session or event identity when absent), then remaining events, then
remaining clauses. These identities prioritize reports within a source bucket;
they do not merge buckets or establish semantic subjects. Main outcome messages
and existing scope-bearing validation values precede ancillary leaves. Explicit
`metadata.outcome` reports precede generic message headlines within the same event;
message and ancillary cards remain in the ledger/history. Original source order
is preserved rather than lexical fact-ID order. Explicit outcomes remain dated
producer claims: they do not establish verification, acceptance, closure or KB
promotion authority. See [agent reporting](../agent-integration.md) for concise,
self-contained producer guidance.

A nonempty explicit outcome must yield at least one eligible bounded card at
`/metadata/outcome` or an eligible descendant; otherwise preparation fails
visibly rather than falling back to the generic message. Typed containers use
the existing descendant extraction rules. This requires one eligible card, not
that every oversized or otherwise ineligible leaf can be represented. Optional
omission remains supported, and protected metadata reduction retains outcomes
when large ancillary values are removed.

Selection uses only existing singleton/pair/triple catalog choices, with at most
four choices per field and 320 characters per choice. Packing is anchored to the
next queued card and favors diverse report heads; extra low-priority leaves do
not earn a coverage bonus. Singleton fallback preserves representability.
Selected reports are rendered chronologically. All original eligible reports
remain in typed reported history with precise source clauses. This bounded,
deterministic heuristic does not guarantee semantic completeness or surface every
qualification. An earlier blocker or next-step report remains explicitly historical;
a later report never proves closure of an independent gate.
Existing ownership, lifecycle, field coverage, citation and size checks still
apply. Oversized/unrepresentable required source material fails visibly.

Source-mode Knowledge contributes reviewed navigation only. Resolve without note
excerpts, normalize note associations and evidence maps to raw group identities,
then hash/persist that exact context. It supplies no historical facts or inferred
product acceptance. Source preparation makes no provider request, requires no
provider consent and does not count as a model-performance sample.

The generate request may select `preparation_mode: source_based|semantic`;
omission selects source-based. Capture the mode in attempt/revision provenance
and context freshness so changing modes cannot silently reuse the other mode's
candidate. Historical revisions without mode retain semantic provenance. Retry,
reference adjustment and comparison preserve the relevant captured mode; scheduled
preparation defaults to source-based. The existing edited-draft, expired-evidence,
cancellation, concurrent-write and explicit Apply protections remain in force.

### Optional semantic mode

The production path validates the complete source ledger, builds retained
workstreams deterministically and requests only the cited editorial recap. A
reviewed alias may combine source buckets; the server uses one ownership bucket
for that resolved group without inferring a semantic topic or changing source
identity. No model partition or catalog-selection request is made. The model
receives complete latest material report bundles and identities, without older
selected clauses, manual prose or reference excerpts. Earlier distinct work and
qualifications remain in review history; their omission from writer input does
not mean they were superseded or completed. A short recap therefore emphasizes
latest reported outcomes rather than covering every activity from the day. Knowledge can contribute authorized navigation, but cannot improve the
recap through note excerpts in this version. Comparison arms remain separate
experiments; their main prose uses the same activity-only writer.

Progress is reported before writing and completes only after the recap validates.
A failed or oversized response fails the attempt without changing the existing
draft or substituting source reports. Existing bounds, citation validation,
review exclusions and explicit Apply still apply. The opt-in real-day replay
uses the public production entry point and writes private test artifacts only.

### Historical topic-partition experiments (test-only)

The following representative/selection protocol is retained in experimental test
and frozen-evaluation helpers. It is not a production summary prerequisite.

Topic preparation sees verified source cards and event identities, without Knowledge.
Chronological windows contain at most 24 cards and at most 12 server-owned topic
anchors. The representative protocol requires `{assignments:[integer,...]}` with exactly one entry per
current card, in its displayed order. References select existing topic representatives
or current source cards; no subject prose or source IDs are generated. A newly chosen
representative must select itself. Unknown references, chains, cycles, invalid roots,
noninteger/negative values, wrong counts, duplicate root properties and extra fields
fail. The server resolves references to original cards, retains complete coverage,
assigns chronological topic IDs and preserves the earliest member as canonical anchor.
At most twelve topics may exist across the whole workstream.

`representative_fact_id` is optional for legacy decoding and mandatory for the new
contract; it must name an owned member of that topic. Its original quote is retained
for later windows even when it differs from the earliest canonical anchor. Thus
canonical identity does not silently replace the model's selected semantic example.
The legacy `proposed_subject` field remains decodable but new partitions do not
produce it. A representative remains an untrusted grouping interpretation, never
product identity, resolution, approval or a new factual claim. The frozen semantic evaluation harness shares the prepared assignment across its
Knowledge arms, revalidated against original source cards and groups. The existing
interactive semantic comparison prepares each arm separately; shared semantic
partitions there remain unverified. Source-mode comparisons always share one
exact deterministic source ledger.
An all-one partition may be structurally valid; coherence and material distinctions
still require independent assessment.

Production, compatibility and request-capture/continuation helpers share a2,048-token
output budget. The compact canonical reply contains at most24 small integers and
has no free-form labels or repeated source-ID membership arrays. A length finish
still fails closed: no partial JSON, assignment repair, source dropping or silent
fallback. Semantic quality and whole-day latency remain separate requirements.


V13 restores bounded original source context to topic preparation. A deduplicated
table covers window owners, canonical anchor owners and representative owners;
representative records include the
exact owning event ID. Only original scalar task_id, session_id, branch, event_type
and status values are included. Identity strings are at most 256 characters,
lifecycle/status strings at most 64, and the table at most 32 KiB. Absent or
non-scalar fields are omitted; oversized permitted values/table fail preparation
without clipping identities or pruning facts. Shared identities are contextual
clues, not required merges; different identities do not establish separate topics.
No lifecycle/status value proves defect resolution, verification or acceptance.
Exact source quotations, owner validation and coverage remain mandatory.

The topic prompt asks for coherent subjects across lifecycle and factual fields,
not one topic per card, position, source event or field. The number of topic IDs
is an upper bound, not a requested count. This instruction still requires an
independent semantic capability check; structural validation cannot prove cohesion.

V12 compacts each composition request without removing facts or choices. Short
scoped bundle IDs map only on the server to original source-owned cards. The
request begins its evidence section with mandatory topic/field obligations, then
uses positional records: subjects `[topic, anchor_fact]`, sources `[source, time]`,
facts `[fact, topic, field, source, human_display]`, and bundle choices
`[bundle, field, fact_ids, rendered_characters]`. Field codes are outcome=0,
decision=1, trade_off=2, validation=3, blocker=4 and follow_up=5. Source display
text appears once. The exact HTTP request, including schema, is measurable through
private opt-in inspection; a smaller prompt does not establish sufficient context
or semantic performance. Topic anchors and original source facts remain unchanged.

Composition receives the complete prepared topics and permitted background.
Material outcome, decision and trade-off topics require non-null choices; repeated
start-only narration is retained in history rather than invented accomplishments.
Each field returns an array of at most four server-owned bundle IDs. A bundle
contains one to three same-field cards, may cover several proposed topics, and is
pre-rendered to at most 320 characters with precise source ownership. Every eligible
singleton remains offered; bounded pair and chronological triple candidates cannot
crowd singletons out or require an exhaustive cubic search. Thus one coherent topic
can use all four facts without a per-topic character cap. The union of selected
cards must cover every mandatory topic/field obligation: an outcome cannot satisfy
a required follow-up or gate obligation. Unknown or wrong-field IDs, duplicate
bundles, overlapping selected cards, missing required highlights, free prose and
overflow fail; accepted facts are never filled, clipped, substituted or dropped.
Conservative allocation can reject a feasible arrangement and does not guarantee
semantic completeness. Whole-day input/work, prompt, response, time and cancellation
bounds remain enforced.

Known typed metadata is projected deterministically as labels and original values
with fixed punctuation, rather than JSON syntax. Projection adds no model-written
claims or test-pass inference. The exact original card remains in reported history;
unknown structures retain their original source representation. Projected character
costs are checked before offering a choice. A multi-card choice groups all visible
fragments from one event into one source clause; excluding that event removes its
entire projection without borrowing another event's citation.

Every verified source card is retained in optional typed `reported_history`, with
its original field and one bounded source-owned fact. Duplicate identical display
cards may share one retained report. Saved revision evidence coverage is validated
over highlights plus history; history does not manufacture main-summary citations.
It is read-only in structured edits and absent from legacy revisions by default.
The browser exposes it collapsed as dated source reports, explicitly distinct from
current instructions. It is not the owner-requested `activity` recovery record.
Copy, Markdown preview and Apply export concise highlights, not the raw history;
the original reports remain inspectable in Log Inbox. Evidence review exclusions
apply equally to highlights and the history view.

Semantic preparation or an explicit owner/authorized-agent `recap` edit replaces
exported highlights with a short editorial paragraph and Done/Pending topics.
This is a separate interpretation with snapshot citations, not exact source quotes.
Existing source highlights
and history remain unchanged and inspectable in collapsed supporting activity.
Whole dependent recap paragraphs/bullets disappear on source exclusion. See the
[Daily recap contract](daily-domain.md#workstream-and-proposal-revision) for bounds
and review requirements; no automatic factual verification or KB approval is implied.

Earlier follow-ups and blockers are historical reports, not automatically current
actions. A latest explicit report may supply a scoped next-step highlight; older
instructions remain selectable as explicitly earlier reports and are retained in source history. A newer timestamp never removes an independent unresolved gate from the offered choices.
Related later evidence never grants the model authority to mark a source resolved,
omitted or superseded. Partial completion cannot remove provider proof, browser/
live acceptance, probable causality, successful-login or release-policy boundaries.
Task completion alone establishes none of those state transitions.

Main fields retain four facts and 320 characters per fact. The complete typed
history is separately bounded at 1,024 cards per workstream and 512 KiB serialized
workstream content when history exists; legacy activity/summary limits do not change.
Start-only work may be represented only by dated history, with no fabricated outcome.
This is an explicit supported representation, not fallback after an invalid model
response. Snapshot completeness, source maps, canonical-link authorization and
immutable review/Apply behavior remain mandatory.

The response schema remains provider-supplied, with no factual response examples
in the daily prompt. New generated facts that exactly echo known template text
are rejected even when their citations are valid. Historical stored revisions and
manual prose retain their previous reading/rendering behavior. Contract-only v7/v8/v9/v10/v11/v12/v13
changes still permit safe manual amendments after source expiry; ordinary older-contract
generation-cache reuse remains invalid.

Typed coverage retains explicit decisions (including `decisions` metadata),
validation, blockers and follow-up in highlights or their original-field history,
with the corresponding source events cited.
The prompt separates planned/accepted work from implementation, verification,
merge and deployment. Completed task status does not establish defect resolution;
pending, probable, blocked and not-run qualifications must survive summarization.
Knowledge excerpts explain background and terminology, never what happened today.
Exact quotations establish provenance, not completeness: selecting a narrow quote
can still omit a qualification elsewhere in a source. Newest-field inclusion also
does not establish topic-level supersession. The real-model pilot checks those
boundaries independently of schema and coverage tests.

Legacy reviewed Markdown groups workstreams with the same authorized canonical
links under one heading. For v11 and later workstreams with retained source history, the
renderer uses the workstream title and displays authorized canonical links as
Related navigation, avoiding an inferred product-ownership heading. Evidence
decisions remain separate; manual-note sections preserve the additional heading
level. Review, Copy and Apply use the server-rendered reviewed revision.

Each composition request from v12 onward covers one group and lists mandatory topic/field
obligations. Its six schema fields are arrays of authorized bundle IDs; a bundle
may combine same-field cards from several sources while retaining exact per-clause
ownership. These replace legacy per-source selection slots. Compatibility tests
exercise synthetic decision and validation facts, rather than only a generic event.

Model-controlled text is escaped. Active links come only from server-owned validated context. Provider requests, responses, retries, concurrency, and total input are bounded independently from raw evidence retention. Fixed safe errors distinguish truncation, schema, lifecycle coverage, group identity, factual content, and response-envelope failures; diagnostics contain normalized finish reasons and numeric token counts without provider response text. Optional malformed telemetry does not invalidate an otherwise usable response.

Knowledge remains optional; a useful Daily candidate does not depend on it. Exact resolution happens before any text is selected, so retrieval never creates a canonical link. The resolver freezes a compact resolution digest, used-note content digests, selected excerpts, workstream associations, matching reasons, group aliases, and link authorization instead of retaining the complete catalog. Each excerpt is capped at 4 KiB, the snapshot at 32 excerpts and 64 KiB of excerpt text. Excerpts are treated as untrusted background data and cannot override the task, schema, evidence rules, links, or destination.

## Knowledge source safety

Before Knowledge context is enabled, Markdown sources are read only through the inspected workspace capability and within the reviewed collection bounds. Frontmatter parsing accepts bounded titles, aliases, and allowlisted typed references; malformed metadata excludes the note from context instead of being treated as trusted configuration. Titles fall back from frontmatter to the first top-level heading and then the filename.

Log Inbox removes every structurally valid Daily managed block from a source before deriving usable content or a context digest. Marker examples inside fenced code remain ordinary text. Missing, nested, mismatched, or out-of-order markers—and unterminated fences—fail closed so generated summaries can never reinforce themselves as source evidence.

## Selected generation configuration

The dashboard resolves a tested provider/model from an explicit one-attempt selection, saved workspace default, or initial environment connection. Attempts capture endpoint, credentials, model and request profile once; running generation, reference selection and both comparison arms use that capture. Model changes invalidate generation reuse even when evidence is unchanged, with existing edit and expired-evidence protection retained. Revisions record the actual nonsecret generation configuration atomically.

Connection tests reuse the production builder and validator with synthetic evidence, bounded per request profile to 300 seconds for verified local endpoints or 30 seconds for other providers, and never longer than the configured request timeout. Two profiles are supported: `max_tokens` plus temperature, and `max_completion_tokens` without temperature. Failed discovery is not a generation fallback; model IDs can be tested manually. The environment's original model remains legacy-compatible; additional models require a successful test.

External connections require endpoint-bound consent for activity and authorized link metadata, including when used for scheduled preparation. Manual-note prose is never sent, and source-note excerpts remain restricted to loopback/Compose Ollama. Endpoint policy validates and pins resolved addresses, requires HTTPS outside local hosts, disables proxies and rejects redirects. Credentials are encrypted only when UI-managed and are absent from public projections and diagnostics.

### v11 evaluation status (2026-10-16)

The experimental implementation passes mechanical ownership/history/bounds checks,
but the unchanged October 13–14 four-arm model trial produced no complete accepted
candidates. Proposed topics were not reliably semantic; selected source-owned IDs
missed mandatory coverage. This contract is not a verified useful-summary release.
Keep Product KB ingestion gated. Require a separately frozen source-topic capability
check and a compact composition representation before another full experiment; do
not weaken evidence validation or infer useful current state from retained history.
