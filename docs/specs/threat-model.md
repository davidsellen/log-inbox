# Single-owner Threat Model

Status: M0 contract for the refocused runtime.

## Deployment Boundary

Log Inbox is a single-owner HTTP service. The supported default publishes collector and dashboard ports only on loopback and mounts one host Markdown workspace into the writer container. SQLite, credentials, proposal revisions, indexes, backups, and Apply journals remain outside that workspace.

Nonlocal access is unsupported unless the owner places the service behind an authenticated TLS tunnel or private reverse proxy and explicitly configures trusted hosts and origins. Log Inbox does not treat network location, Docker isolation, an unguessable URL, or a browser's same-origin policy as authentication.

## Protected Assets

- Raw and redacted engineering events, including metadata and source windows.
- Manual notes and user-edited proposal revisions.
- Existing Markdown and the authority to modify its managed block.
- Workspace structure, canonical mappings, exclusions, and model context.
- Ingestion, session, MCP, model-provider, and workspace-write credentials.
- Migration backups, recovery copies, Apply journals, and audit records.

## Principals and Trust

| Principal | Trust and permitted role |
|---|---|
| Owner browser | Human review surface after an authenticated session and CSRF validation |
| Producer | May ingest bounded events with its ingestion credential; cannot claim manual authorship |
| Configured model | Untrusted processor of explicitly selected redacted evidence/context |
| MCP client | Receives only its granted scopes; tokens are independently revocable |
| Markdown editor/sync | Independent writer; never trusted to preserve Log Inbox markers atomically |
| Workspace contents | Untrusted input, including Markdown, links, templates, filenames, and symlinks |

## Scope Matrix

| Scope | Allows | Does not allow |
|---|---|---|
| `logs:ingest` | Submit bounded automated evidence | Read logs, create manual notes, or impersonate the owner |
| `logs:read` | Search retained redacted evidence | Read Knowledge or mutate review state |
| `knowledge:read` | Read configured source excerpts | Change collections/mappings or write Markdown |
| `draft:generate` | Request generation against an immutable snapshot | Resolve evidence or Apply |
| `review:write` | Create manual entries and edit/include/omit/dismiss/reopen drafts | Write Markdown |
| `settings:write` | Change reviewed ordinary settings | Reveal stored secrets or bypass destination review |
| `vault:write` | Execute an already reviewed exact Apply operation | Select a destination or approve a revision |

The initial dashboard owner session may receive all dashboard scopes, but every handler still checks its required scope. Initial MCP support excludes `vault:write`.

## Browser Session Contract

- Login uses a secret in a request body, never a query string. Only a slow verifier or keyed digest is stored.
- On success, rotate to an opaque, high-entropy session ID in an `HttpOnly`, `SameSite=Strict` cookie. Add `Secure` whenever the configured public origin is HTTPS.
- Store only a digest of the session ID. Sessions have idle and absolute expiry and can be revoked individually or globally.
- Return a separate per-session CSRF token to authenticated HTML/JSON; never store it in persistent browser storage. Require it on every state-changing request.
- Reject unsafe requests with an absent/mismatched CSRF token, disallowed `Origin`, disallowed effective host, expired session, or missing scope.
- Regenerate the session and CSRF tokens at login. Logout revokes the server-side session and expires the cookie.
- Owner recovery is an explicit local command or secret rotation followed by session revocation; there is no email/cloud recovery dependency.

## Request and Content Defenses

- Validate effective host against configured values. Trust forwarded headers only from explicitly configured proxies.
- Permit configured origins exactly; do not reflect arbitrary origins or use wildcard credentialed CORS.
- Sanitize rendered model/Markdown output and URLs. Never load remote images implicitly in preview.
- Treat prompts, logs, and knowledge excerpts as data. They cannot grant scopes, change destinations, or instruct Apply.
- Enforce bounded request, stored-event, prompt, excerpt, response, retry, and concurrency limits.
- Redact secrets from ingestion, model errors, HTTP diagnostics, and audit details. Never return stored provider credentials.

## Workspace and Apply Defenses

- Bind the active generated workspace ID to a reviewed host identity and detect replacement before reads or writes.
- Resolve paths relative to an opened workspace capability; reject traversal, symlink escape, non-regular targets, and protected editor/Git paths.
- Require the exact approved proposal revision, destination, expected block revision, and `vault:write` scope.
- Journal recovery data before mutation. A crash or concurrent edit produces recovery/reconciliation, never silent overwrite or duplicate append.

## Retention Exceptions

Automated raw evidence and detailed audit records default to 30 days. Unhandled manual content and edited drafts remain until handled or explicitly deleted. Minimal operation IDs, target paths, block hashes, and ownership markers remain as long as necessary to prevent duplicate or unsafe writes. Backups and recovery copies have separately disclosed expiry and are not stored in the Markdown workspace.

## Acceptance Threats

Tests must cover session fixation, expiry/revocation, missing or cross-session CSRF, hostile Origin/Host, scope escalation, producer manual-author spoofing, unsafe preview URLs, prompt-injected destination changes, symlink/path escape, changed managed blocks, and replayed Apply requests. Any unauthorized write, secret disclosure, or silent loss of user-owned Markdown blocks release.
