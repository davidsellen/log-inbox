# Daily Engineering Knowledge Roadmap

Status: usable refocused release; personal-pilot validation active. Updated 2026-09-24. Initial baseline inspected: `ae46de99fd0e80fa4d70ef92a953d076bcf04057`.

This roadmap defines the target direction, delivered foundation, and remaining delivery order. The [README](../README.md) and runtime specs describe the active Daily release; milestone checkboxes record implemented and pending work.

## Product Promise

Turn selected engineering activity into a concise, evidence-linked daily record: what changed, why, what was validated, and what remains. Let the owner review and correct it before writing to existing Markdown notes.

Keep Log Inbox a single-owner, self-hosted HTTP collector, dashboard, scheduler, MCP interface, and reviewed Markdown writer. It is not a vault manager, Notion clone, generic agent-memory platform, or developer productivity tracker.

- **Daily is the product:** date navigation, structured drafts, evidence review, omissions, preview, and Apply.
- **Reference context is optional shared context:** human-maintained Markdown product/module notes can ground Daily drafts and, later, approved agent reads. It is not a prerequisite for a useful draft and is not an autonomous memory store.
- **Settings configures the service:** workspace, dates, automation, model connection, retention, security, and integration.

### Current delivery: make Daily understandable

The explicit **Use activity record** recovery path provides a model-independent, owner-requested record after failure. It preserves original activity in collapsed groups, labels the output as not AI-summarized, and reuses reviewed Apply. This is a completion path, not evidence that AI latency or quality has passed the personal-pilot gate.

Prioritize reliable generation and direct actions on Daily before expanding Knowledge. Persist attempts, recover interrupted work, show actual stage/elapsed time and actionable failure, and allow exact-attempt cancellation. Keep one active operation and no hidden manual backlog. Accepted generation runs independently of browser navigation; the UI follows server state.

Make the current revision readable before editing, offer Copy summary, and preserve exact reviewed Markdown writes. Move optional Reference notes into Settings while retaining existing configuration and Knowledge deep links. Context setup must demonstrate a benefit to the daily draft rather than become another inbox to maintain.

Use bounded intake receipt aggregates and content-free timing diagnostics. The opt-in [generation benchmark](generation-performance.md) separates real-model measurement from deterministic workflow tests. Real-day speed/quality comparisons and the ten-day pilot remain validation work; implementation alone does not satisfy those gates.

The service operates on a workspace available to its Docker host. Signing in from another device does not make that device's local folder available. Synchronization, if needed, remains an explicitly configured external responsibility.

## Recall and navigation delivery

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
- Configure one active OpenAI-compatible connection with capability/schema testing. Use synthetic connection tests, endpoint/redirect validation, and renewed consent for changed providers or data categories.
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
- [x] Show selected-day status, late-event updates and evidence-expiry warnings. Recent-list counts were removed from the dashboard after owner feedback.
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

- [ ] Add UI-managed local/custom model presets, connection/capability testing, encrypted credentials, explicit egress consent and bounded guided preferences.
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
