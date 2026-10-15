# Daily Model Generation

The model turns one server-resolved day of bounded automated evidence into a structured review candidate. It never chooses the destination, writes Markdown, resolves review decisions, or rewrites owner-authored notes.

## Active flow

1. The server resolves the selected calendar day and reads its eligible events.
2. Rust groups events using namespaced repository, work-item, pull-request, task, session, and source identities. Numeric/string/URL and consistent object references share identity normalization with Knowledge. A task may inherit its single unambiguous external identity. Otherwise an exact feature branch can associate tasks without explicit identities; integration branches and conflicting or malformed identities cannot authorize that merge.
3. The service freezes event IDs and content digests in an immutable evidence snapshot.
4. When Knowledge collections or mappings are enabled, the server resolves reviewed mappings and unique exact title, alias, or typed-reference matches. Conflicting matches produce no link. Repository-wide mappings may authorize a link but cannot merge distinct work items; only reviewed work-item/PR relationships may alias groups.
5. Rust deterministically extracts all eligible bounded fact cards from each automated source separately, without Knowledge. Rust validates each card and assigns source-owned fact IDs. The configured OpenAI-compatible endpoint composes one workstream per request from this chronological ledger and server-authorized background, selecting source-specific facts rather than generating factual prose or citation lists. When the endpoint is verifiably local (`localhost`, a loopback address, or the Compose `ollama` service), background includes bounded opening excerpts from the already-resolved canonical notes. Nonlocal endpoints require connection-specific consent and receive link metadata without reference-note excerpts.
6. Rust renders the selected clauses with source times, derives citations only from their visible supporting clauses, and validates group membership, evidence coverage, factual evidence IDs, canonical links, and text limits. Malformed extraction, stale-only selection and invalid composition fail visibly.
7. A valid result becomes an immutable proposal revision bound to its evidence and optional Knowledge snapshots in SQLite. Invalid, incomplete, or unavailable model output is a visible failure.
8. The server renders the reviewed structure deterministically for preview. Only explicit dashboard Apply invokes the Markdown writer.

Manual-only days need no model. Mixed days send only automated evidence to the provider and keep manual prose verbatim in a separate section. There is no raw-log or free-form Markdown fallback.

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
Copy and Apply. Generation now has two constrained stages: isolated source extraction and
selection-only composition. Neither stage may invent event IDs; final citations
are derived exclusively from selected server-owned source facts. Existing stored
revisions and historical string-compatible readers retain their format; no
migration or automatic rewrite is performed. The generation contract is
`daily-structured-v9-source-slots`, invalidating ordinary cache reuse from older
contracts. Final output retains four facts per field and 320 characters per fact.
Oversized selected bundles fail rather than being clipped.
Required visible source clauses may be impossible to fit within these inherited
limits, even when a less grounded free-text summary previously fit. Such a group
fails closed; this contract preserves the evidence checks, not a guarantee that
every formerly representable group remains generatable.

Extraction processes the bounded message and allowed metadata of exactly one
event, including original typed decisions, checks, blockers and follow-ups. Rust
retains all eligible cards from complete short source values or complete original
sentence/semicolon clauses of longer strings. A bare newline cannot split a
clause. Only outer whitespace is normalized; internal whitespace, including CRLF,
and negation remain exact source content. Clipped or unrepresentable clauses are excluded; if required material has
no eligible representation, preparation fails. No model request prunes this ledger.
Fact IDs map to field-scoped source pointers and exact quotations. Rust verifies
the pointer, quotation, field compatibility, source identity and required fields;
reloaded ledgers are checked again against original evidence. Not-run, pending and
probable qualifications remain source data; an extracted validation fact does not
establish that checks passed. Knowledge cannot create source facts.

Composition sees only validated source facts and permitted background, not raw
messages or metadata. Each of six fields maps every available source slot to one
source/field-specific fact ID or permitted null. Required and newest-field slots
must select a fact. Unknown keys, IDs, wrong fields and free prose fail. Each cited
source contributes a visible selected clause with a server-owned time, making
earlier plans historical rather than unqualified current state. Rust packs every
selected clause by descending length into the existing bounds, then orders clauses
chronologically within each bundle; no selected material is dropped. Each packed source clause retains its explicit event association through stored
review content, so omit/duplicate/superseded decisions remove only that source's
clauses. This bounded packing heuristic does not guarantee optimal packing. Same-UTC-date groups carry
the reported date in the title and compact UTC times per clause; multiday clauses
include the full date. Root typed field labels are redundant with field headings;
descendant path labels and commit-message labels remain visible. Selecting older facts while omitting the newest source
for that field is rejected. This is not last-event-wins: earlier required evidence
and unrelated qualifications remain, and a later task completion does not prove
an application defect was resolved.

Each group retains mandatory field/source coverage derived from the existing
lifecycle predicates. A nonempty array alone is insufficient: every required
source must have a selected fact in that field. The server does not append missing
facts or citations, silently mark older evidence superseded, or downgrade a failed
summary to an activity record.

The response schema remains provider-supplied, with no factual response examples
in the daily prompt. New generated facts that exactly echo known template text
are rejected even when their citations are valid. Historical stored revisions and
manual prose retain their previous reading/rendering behavior. Contract-only v7/v8
changes still permit safe manual amendments after source expiry; ordinary older-contract
generation-cache reuse remains invalid.

Required factual arrays cover explicit decisions (including `decisions` metadata),
validation, blockers and follow-up, with the corresponding source events cited.
The prompt separates planned/accepted work from implementation, verification,
merge and deployment. Completed task status does not establish defect resolution;
pending, probable, blocked and not-run qualifications must survive summarization.
Knowledge excerpts explain background and terminology, never what happened today.
Exact quotations establish provenance, not completeness: selecting a narrow quote
can still omit a qualification elsewhere in a source. Newest-field inclusion also
does not establish topic-level supersession. The real-model pilot checks those
boundaries independently of schema and coverage tests.

The reviewed Markdown renderer groups workstreams with the same authorized
canonical links under one heading while retaining separate workstream titles and
evidence decisions. It uses the same heading grouping as generated Markdown;
manual-note sections preserve the additional heading level. Review, Copy and
Apply continue to use the server-rendered reviewed revision.

The prompt lists exact mandatory source slots per field using the validator’s lifecycle predicates. Each request covers exactly one group; the schema restricts each slot to that source and field. Compatibility tests exercise synthetic decision and validation facts, rather than only a single generic event.

Model-controlled text is escaped. Active links come only from server-owned validated context. Provider requests, responses, retries, concurrency, and total input are bounded independently from raw evidence retention. Fixed safe errors distinguish truncation, schema, lifecycle coverage, group identity, factual content, and response-envelope failures; diagnostics contain normalized finish reasons and numeric token counts without provider response text. Optional malformed telemetry does not invalidate an otherwise usable response.

Knowledge remains optional; a useful Daily candidate does not depend on it. Exact resolution happens before any text is selected, so retrieval never creates a canonical link. The resolver freezes a compact resolution digest, used-note content digests, selected excerpts, workstream associations, matching reasons, group aliases, and link authorization instead of retaining the complete catalog. Each excerpt is capped at 4 KiB, the snapshot at 32 excerpts and 64 KiB of excerpt text. Excerpts are treated as untrusted background data and cannot override the task, schema, evidence rules, links, or destination.

## Knowledge source safety

Before Knowledge context is enabled, Markdown sources are read only through the inspected workspace capability and within the reviewed collection bounds. Frontmatter parsing accepts bounded titles, aliases, and allowlisted typed references; malformed metadata excludes the note from context instead of being treated as trusted configuration. Titles fall back from frontmatter to the first top-level heading and then the filename.

Log Inbox removes every structurally valid Daily managed block from a source before deriving usable content or a context digest. Marker examples inside fenced code remain ordinary text. Missing, nested, mismatched, or out-of-order markers—and unterminated fences—fail closed so generated summaries can never reinforce themselves as source evidence.

## Selected generation configuration

The dashboard resolves a tested provider/model from an explicit one-attempt selection, saved workspace default, or initial environment connection. Attempts capture endpoint, credentials, model and request profile once; running generation, reference selection and both comparison arms use that capture. Model changes invalidate generation reuse even when evidence is unchanged, with existing edit and expired-evidence protection retained. Revisions record the actual nonsecret generation configuration atomically.

Connection tests reuse the production builder and validator with synthetic evidence, bounded to 30 seconds per request profile. Two profiles are supported: `max_tokens` plus temperature, and `max_completion_tokens` without temperature. Failed discovery is not a generation fallback; model IDs can be tested manually. The environment's original model remains legacy-compatible; additional models require a successful test.

External connections require endpoint-bound consent for activity and authorized link metadata, including when used for scheduled preparation. Manual-note prose is never sent, and source-note excerpts remain restricted to loopback/Compose Ollama. Endpoint policy validates and pins resolved addresses, requires HTTPS outside local hosts, disables proxies and rejects redirects. Credentials are encrypted only when UI-managed and are absent from public projections and diagnostics.
