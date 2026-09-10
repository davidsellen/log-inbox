# Refocused Dashboard API

Status: active Daily interface.

All routes use exact configured Host/Origin boundaries. Error responses have `{ "error": "..." }`.

The server exposes only the authenticated v2 Daily/settings/migration routes, health endpoint, and static root assets. Removed browser-vault, proposal/consolidation, and `/mcp` routes have no runtime aliases.

## Authentication

### `POST /api/v2/auth/login`

Body: `{ "owner_secret": "..." }`. Requires an allowed Host and Origin. Returns an `HttpOnly`, `SameSite=Strict` session cookie plus an in-memory CSRF token. The cookie is `Secure` when the validated login request uses an HTTPS origin, which keeps explicitly allowed HTTP and HTTPS deployments independent.

### `GET /api/v2/auth/session`

Requires the session cookie and `logs:read`. Returns the granted scopes and absolute expiry.

### `POST /api/v2/auth/logout`

Requires the session cookie, an allowed Host/Origin, and the matching `X-CSRF-Token`. Revokes the server-side session and expires the cookie.

## Workspace settings

The host or Compose configuration mounts one Markdown workspace at `LOG_INBOX_WORKSPACE_DIR` (default `/workspace`). A browser cannot mount a directory from the device on which it happens to be open; the service inspects the server-side mount without writing marker files and derives a replacement-sensitive binding from its canonical identity.

### `GET /api/v2/settings/workspace`

Requires `logs:read`. Returns the configured server path, the active stable profile when present, and whether its reviewed binding still matches the mounted directory.

### `POST /api/v2/settings/workspace/preview`

Requires `settings:write` and CSRF. Validates an IANA timezone, relative daily root, supported date-token pattern, optional existing Markdown template, and link style. It rejects traversal, protected editor/Git locations, symlinks, and non-Markdown targets. The response contains the normalized settings, an example resolved destination, and a digest binding the reviewed values to the current mount. It saves nothing.

### `PUT /api/v2/settings/workspace`

Requires `settings:write`, CSRF, and the exact preview digest. It activates the first workspace profile or updates the active profile in place using its expected ID and timestamp, preserving the stable workspace ID. Existing days retain their frozen timezone and destination. A replaced mount or stale settings editor returns `409 Conflict` and must be reviewed again.

## Daily read model

### `GET /api/v2/daily/{YYYY-MM-DD}`

Requires the session cookie and `logs:read`. The URL contains a calendar date, never browser-computed UTC bounds. The server returns:

- active workspace ID and frozen IANA timezone;
- server-resolved start/end instants, including DST-short or DST-long days;
- frozen destination for an existing day, otherwise a non-mutating path preview;
- bounded automated evidence and truncation state;
- trusted owner-authored manual entries stored outside ingest;
- frozen day state, current immutable proposal revision and its evidence snapshot when present;
- server-derived candidate freshness, separate new/expired evidence counts, and exact active late-evidence deferrals, so late evidence is never presented as part of an older current draft and raw retention is not mistaken for a new arrival;
- deterministic `preview_markdown` rendered from the structured current revision, trusted manual entries, and evidence dispositions.

The endpoint never creates a day, snapshot, revision, folder, or Markdown file. Missing workspace review or a replaced mount is `409 Conflict`; malformed dates are `400 Bad Request`.

Snapshot evidence carries an `available` flag. Expired raw evidence remains represented by its immutable ID, digest, and review decision; it makes `evidence_complete` false but does not by itself make the candidate stale. Only a live automated event absent from the snapshot, or a changed trusted manual-entry set, requires regeneration.

### `GET /api/v2/daily/overview`

Requires the session cookie and `logs:read`. It returns profile-local `today`, the IANA timezone and server time, summary counts, and at most 1–31 recent meaningful day records. Today is always present; untouched empty past dates are omitted. The bounded discovery window is returned as `window_start` and is derived from the larger of configured catch-up and raw-retention days, capped at 90 calendar days.

Each day exposes independent generation, review, freshness, scheduling, new-evidence, and expired-evidence facts plus one display status. Counts are exact and do not load event messages. Apply state is considered only when it belongs to the exact current revision. A past day is missed only when it has unhandled automated evidence; manual-only days are shown as notes to review. Browser navigation uses the returned profile-local Today rather than the device calendar.

## Manual daily entries

### `POST /api/v2/daily/{YYYY-MM-DD}/manual`

Requires `review:write`, the session cookie, an allowed Host/Origin, and the matching `X-CSRF-Token`. Body:

```json
{
  "text": "Recorded the deployment trade-off.",
  "references": ["https://example.test/decisions/42"]
}
```

The server resolves the active workspace and calendar date, freezes the day destination if this is its first durable activity, and stores the owner-authored prose separately from automated ingest. Text is never sent through the model. References are bounded absolute HTTP(S) URLs. The route does not create or edit Markdown.

## Generate a candidate

### `POST /api/v2/daily/{YYYY-MM-DD}/generate`

Requires `draft:generate`, the session cookie, an allowed Host/Origin, and the matching `X-CSRF-Token`. The optional body is `{ "replace_edited": false }`. The server serializes generation per owner process, freezes the day, rejects a truncated evidence set, creates an immutable evidence snapshot, and stores a schema-validated immutable proposal revision. An identical evidence/manual snapshot returns the existing current revision instead of calling the model again. New manual entries are attached to the existing structured revision without discarding its edits or calling the model.

When new automated evidence exists and the current revision contains structured edits, the server returns `409 Conflict` unless `replace_edited` is explicitly true. The Daily UI asks for confirmation before sending that authorization. A successful replacement remains a new immutable `regenerated` revision; the edited revision is retained in history.

If source evidence from the current snapshot has expired, generation preserves that complete revision and refuses to replace it from a partial event set. Owner-authored manual notes can still be attached without a model call. The owner may explicitly leave each newly arrived event for later; active deferrals are excluded from regeneration and Apply freshness checks only for that exact revision and event digest.

Automated evidence always uses the strict structured model contract, even if an ingest producer claims `entry_kind=manual`. Manual-only days create a reviewable revision without an LLM. Model absence, oversized input, provider failure, invalid JSON, schema mismatch, missing evidence, or unsafe grouping returns `422 Unprocessable Entity`; raw log text is never substituted as a candidate. Generation changes SQLite review state only. It does not create a folder or Markdown file.

## Review evidence

### `PUT /api/v2/daily/{YYYY-MM-DD}/evidence/{event_id}`

Requires `review:write` and CSRF. Body names the exact `expected_revision_id`, a disposition (`include`, `omit`, `duplicate_of`, or `superseded_by`), an optional related event ID, and an optional bounded reason. Duplicate/superseded decisions require a different event from the same snapshot; include/omit decisions cannot name one. A stale revision ID returns `409 Conflict`.

### `DELETE /api/v2/daily/{YYYY-MM-DD}/evidence/{event_id}`

Requires `review:write` and CSRF. Body contains the exact `expected_revision_id`. It reopens the evidence by clearing its disposition and related decision fields. Both review routes return the complete ordered snapshot decision list. They do not rewrite the immutable candidate revision or Markdown.

### `POST|DELETE /api/v2/daily/{YYYY-MM-DD}/late-evidence/{event_id}`

Requires `review:write` and CSRF. POST explicitly leaves one live event that arrived after the named current revision for later; DELETE reopens that exact deferral. Snapshot evidence cannot be deferred through this route. A deferral never deletes or marks the event reviewed, never changes candidate content, and applies only while its bound revision remains current. Replacing the revision automatically closes its deferrals.

### `POST /api/v2/daily/{YYYY-MM-DD}/dismiss`

Requires `review:write` and CSRF. Body contains the exact `expected_revision_id`. It records a dismissal bound to that immutable revision and content hash, removes the day from review reminders, and writes or deletes no Markdown or evidence. Applied revisions cannot be dismissed. New evidence remains independently visible as Update available.

### `DELETE /api/v2/daily/{YYYY-MM-DD}/dismiss`

Requires `review:write` and CSRF. Body contains the exact current dismissed revision ID. It reopens only that dismissal and reports whether any snapshot evidence has expired. Reopening does not reconstruct expired raw evidence.

## Edit the structured candidate

### `PUT /api/v2/daily/{YYYY-MM-DD}/candidate`

Requires `review:write` and CSRF. Body contains the exact `expected_revision_id` and a complete `DailyRevisionContent` object. The storage boundary validates schema version, manual-entry ownership, unique workstreams, canonical-link shape, complete snapshot coverage, and evidence on every factual item. A successful edit creates a new immutable `structured_edit` revision; compare-and-swap prevents a stale editor from replacing a newer candidate. Arbitrary Markdown is not accepted by this route.

The Daily read model renders the final preview deterministically. Owner-authored manual prose remains verbatim and separate. Model-authored titles, facts, and questions are escaped as plain Markdown text; only validated server-owned canonical links are active. Facts supported solely by omitted, duplicate, or superseded evidence are absent from the preview.
