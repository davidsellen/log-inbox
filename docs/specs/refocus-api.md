# Refocused Dashboard API

Status: feature-gated M1 interface. Set `LOG_INBOX_REFOCUS_ENABLED=1` only for development until the coordinated writer cutover is complete.

All routes use exact configured Host/Origin boundaries. Error responses have `{ "error": "..." }`.

## Authentication

### `POST /api/v2/auth/login`

Body: `{ "owner_secret": "..." }`. Requires an allowed Host and Origin. Returns an `HttpOnly`, `SameSite=Strict` session cookie plus an in-memory CSRF token. The cookie is `Secure` for HTTPS configured origins.

### `GET /api/v2/auth/session`

Requires the session cookie and `logs:read`. Returns the granted scopes and absolute expiry.

### `POST /api/v2/auth/logout`

Requires the session cookie, an allowed Host/Origin, and the matching `X-CSRF-Token`. Revokes the server-side session and expires the cookie.

## Daily read model

### `GET /api/v2/daily/{YYYY-MM-DD}`

Requires the session cookie and `logs:read`. The URL contains a calendar date, never browser-computed UTC bounds. The server returns:

- active workspace ID and frozen IANA timezone;
- server-resolved start/end instants, including DST-short or DST-long days;
- frozen destination for an existing day, otherwise a non-mutating path preview;
- bounded automated evidence and truncation state;
- trusted owner-authored manual entries stored outside ingest;
- frozen day state, current immutable proposal revision and its evidence snapshot when present;
- server-derived candidate freshness, so late evidence is never presented as part of an older current draft.

The endpoint never creates a day, snapshot, revision, folder, or Markdown file. Missing workspace review is `409 Conflict`; malformed dates are `400 Bad Request`.

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

Requires `draft:generate`, the session cookie, an allowed Host/Origin, and the matching `X-CSRF-Token`. The server serializes generation per owner process, freezes the day, rejects a truncated evidence set, creates an immutable evidence snapshot, and stores a schema-validated immutable proposal revision. An identical evidence/manual snapshot returns the existing current revision instead of calling the model again.

Automated evidence always uses the strict structured model contract, even if an ingest producer claims `entry_kind=manual`. Manual-only days create a reviewable revision without an LLM. Model absence, oversized input, provider failure, invalid JSON, schema mismatch, missing evidence, or unsafe grouping returns `422 Unprocessable Entity`; raw log text is never substituted as a candidate. Generation changes SQLite review state only. It does not create a folder or Markdown file.
