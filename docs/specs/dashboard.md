# Local Dashboard

The MCP service exposes a small local interface for reviewing Markdown proposals and configuring agent-facing behavior. It does not require a separate frontend service.

## Queue

- List every Markdown proposal currently present in the configured pending folder.
- Show the proposed summary, target note, confidence, timestamp, evidence count, provider, and canonical links.
- Expand a proposal to read the exact Markdown that will be applied.
- Edit proposal Markdown in the preview, save it with optimistic concurrency, or revert unsaved changes before applying.
- Applying a proposal appends it to its daily note, marks its evidence reviewed, and removes the pending file.
- Discarding a proposal marks its evidence reviewed with a discarded outcome, removes the pending file, and leaves the original logs in SQLite.

## Manual Work Logs

- A global **Add log** dialog captures one required work description, optional existing vault notes, optional work-item and pull-request references, and a local timestamp.
- The dashboard validates selected notes against the current vault catalog and stores the entry through the same redacting event store without exposing an API key to the browser.
- Manual entries do not create standalone automatic proposals. Whole-day consolidation renders them verbatim under **My notes** and sends only automated activity to the LLM.
- Mixed reports place **My notes** before **Automated activity** while retaining every event ID for review, apply, and reconsolidation.

## Daily Consolidation

1. The browser sends the selected local day's UTC start and end instants plus the expected daily-note filename.
2. The service reads all stored events in that half-open time range, including already staged or reviewed evidence.
3. The request fails rather than silently omitting evidence when the day exceeds the 500-event model boundary.
4. The service freezes the event IDs in a durable SQLite job and immediately returns its `pending`, `running`, `completed`, `failed`, `cancel_requested`, or `cancelled` state.
5. One background worker claims pending jobs. A restart returns interrupted jobs to the queue, and repeated requests for the same event snapshot resolve to the same job.
6. The local or configured remote LLM merges lifecycle updates, removes duplicates, and groups distinct workstreams into readable Markdown.
7. The service atomically stages one consolidated proposal and records its ID on the completed job.
8. The browser polls durable state, survives refresh, permits cancellation, and opens the stored result with Apply, Keep in queue, and Discard actions. Previewing never writes the daily note.
9. Applying the consolidated proposal removes older pending proposals fully covered by its evidence set. SQLite retains the original events, job, and review records.

## Preferences

- Persist non-secret preferences in SQLite as key/value rows.
- Generate copy-ready `AGENTS.md` activity-reporting instructions from the saved agent preferences.
- Tell agents to submit daily-note-ready terminal events through ingestion rather than directly writing Markdown notes.
- Store the complete user-editable daily-consolidation prompt in SQLite and use it to shape organization, tone, and detail.
- Migrate the legacy `consolidation_instructions` preference by appending it to the stronger default when no `daily_consolidation_prompt` has been saved.
- Fixed evidence, redaction, canonical-link, and output-schema rules take precedence over user prompt text.
- Never accept, return, persist, or copy the ingest API key. Generated instructions reference `LOG_INBOX_API_KEY` at execution time.

## Vault Linking

- Scan the configured Markdown vault read-only and derive note groups, aliases, tags, and references without assuming folder or product names.
- Compare all retained event identifiers with the catalog, including scalar and array metadata.
- Persist user-approved exact or prefix rules in SQLite; optional conditions make a rule specific to a branch, work item, module, or other supported field.
- Treat model output as a suggestion only. The resolver accepts only notes present in the current catalog.
- Browser mode uses an explicitly selected local directory, persists its handle only in browser storage, and synchronizes a derived catalog for background consolidation.
- **Browse vault** presents the discovered folder tree and can add a developer starter or a selected Markdown template folder. Setup is additive; existing files are never intentionally opened for writing.
- Browser-mode proposal application prepares content server-side, verifies the source and result hashes in the browser, and acknowledges evidence only after the write is verified.

## HTTP API

- `GET /api/dashboard` returns preferences, generated instructions, and pending proposals.
- `GET /api/logs/manual/options` returns selectable vault notes and recent choices; `POST /api/logs/manual` records a manual entry for daily consolidation.
- `PUT /api/preferences` validates and persists non-secret preferences.
- `GET /api/linking` returns the current catalog, observed identifiers, suggestions, and rules.
- `POST /api/linking/scan` refreshes the read-only catalog view.
- `POST`, `PUT`, and `DELETE /api/linking/rules` manage persisted mappings.
- `PUT /api/linking/ignored` hides an unresolved identifier; `DELETE /api/linking/ignored/{id}` restores it. Ignoring is reversible and never deletes events or vault notes.
- `GET /api/vault/connection` reports browser, mounted, or unconfigured mode; `PUT` and `DELETE /api/vault/browser/catalog` synchronize or disconnect a browser-selected vault.
- `POST /api/proposals/{id}/browser-apply/prepare` renders an idempotent daily-note result; `POST /api/proposals/{id}/browser-apply/acknowledge` consumes it after browser verification.
- `POST /api/consolidations/daily` idempotently queues a durable daily consolidation job.
- `GET /api/consolidations/{job_id}` returns authoritative durable job state.
- `POST /api/consolidations/{job_id}/cancel` requests cancellation of pending or running work.
- `POST /api/proposals/{proposal_id}/apply` applies and consumes a proposal.
- `PUT /api/proposals/{proposal_id}` atomically updates only the editable Markdown body using an expected content revision.
- `POST /api/proposals/{proposal_id}/discard` rejects and consumes a proposal.
- `POST /api/proposals/{proposal_id}/regenerate` queues a durable replacement for a stale daily consolidation proposal.

## Acceptance Criteria

- Concurrent apply operations cannot interleave daily-note writes.
- A failed LLM call does not remove existing proposals or alter a daily note.
- Refreshing or repeating a request cannot create duplicate work for the same event snapshot.
- Cancellation state is durable, and cancelling a running job aborts its in-flight model future before staging.
- An API key is never accepted by the dashboard, returned by the server, copied into generated instructions, or written to SQLite.
- Desktop and mobile layouts have no horizontal overflow.
- All dynamic proposal content is inserted as text, not executable HTML.
