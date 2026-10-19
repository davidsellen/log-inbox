# Accepted-work Knowledge capture

Status: implemented and validated locally; evidence is tracked in the
[roadmap task record](../roadmap.md#accepted-work-kb-capture--2026-10-19).

## Scope and authority

Capture original redacted producer submissions independently of Daily generation.
No model, Daily day or generated Daily prose is needed. Markdown remains canonical.
The owner captures, accepts or rejects an exact candidate, then separately previews
and confirms a canonical note update. Reported acceptance is a claim until reviewed;
owner acceptance for KB inclusion is not independent proof of implementation,
verification, merge or deployment. Claims never form an inferred status ladder.
Agent recall, automatic promotion, remote deployment and PR publication are outside
this delivery. The earlier Daily/model usefulness gate remains unmet; this direct
source/review flow does not depend on that generator. It does not establish useful
agent recall or authorize its expansion.

## Submission version 1

Place `knowledge_submission` in event metadata through the unchanged HTTP or MCP
ingest interface. Required fields are `schema_version: 1`, `submission_key`,
`plan_id`, `plan_revision`, confirmed `product`, nonempty `modules`, `title`,
`summary`, and one to 32 `claims`. Each claim contains `kind` and `text`.
Kinds are `proposed`, `accepted`, `implemented`, `verified`, `merged`, `deployed`.
A verified claim additionally requires `verification_scope`. Accepted claims may
include `reported_acceptance: {source, actor, at}` with an ISO8601 UTC timestamp.
This supplied attribution remains producer-reported; the dashboard records its own
fixed owner actor separately.

Optional arrays: `decisions`, `constraints`, `questions`, `commits`, `pull_requests`,
and `artifacts: [{path, sha256}]`. Artifacts are references with lowercase SHA-256,
not fetched/verified documents. An optional `supersedes` names a previous candidate.
Unknown fields, control characters, empty required values and invalid typed fields
are rejected at capture. The submission is at most 64 KiB after ingestion redaction;
summary is at most 4000 bytes, claim text 2000, titles 300, module names 200.
The core validator defines the remaining per-array/string limits. See the
[producer example](../agent-integration.md#submitting-accepted-work-to-product-kb).
Malformed submissions remain ordinary retained events; capture creates no partial
candidate.

## Immutable candidates and corrections

The active reviewed workspace is resolved by the server. Optional producer
`metadata.workspace_id` must match it; metadata never selects workspace authority.
Each original source event binds to one workspace. Materialization deduplicates
`(workspace, producer source, submission key)` over the redacted JSON payload
SHA-256, preserving explicit optional-field presence. Event UUID/receipt time are
not part of this retry key. Different content under an existing key conflicts.
Candidate hashes separately bind workspace, producer, original event ID/digest and
payload digest. Original event fields are used, never Daily metadata overlays.

A correction has a new key and `supersedes` naming the current candidate for the
same workspace, producer, product and plan. Implicit forks conflict. Candidates and
owner dispositions are immutable. A replacement immediately makes its predecessor
ineligible for new review/promotion, even before replacement review. Historical
acceptance stays visible and is never inherited. Reject a mistaken submission and
submit a corrected revision; this first UI supports accept/reject rather than an
inline text editor. No rejection or replacement automatically rewrites Markdown.
Previously promoted text can therefore remain historical/stale until the separate
correction is reviewed and confirmed.

Raw-event expiry preserves the candidate's original source ID/digest and redacted
payload. The UI discloses unavailable evidence; fresh approval and writing are
blocked. Restoring evidence requires the original matching event envelope, not
reconstructing proof from a summary. Candidates/reviews and KB journals are retained
as explicit audit records in this first version; existing raw/Daily retention does
not delete them. Unfinished journal bytes must not be removed. There is no new
automatic KB audit-pruning policy.

## Canonical update

Choose an existing Markdown note in an enabled Reference collection. Path and
workspace capability checks reject missing, excluded, protected, outside-workspace
or symlink targets. Preview freezes the owner-approved candidate hash, approval
digest, collection/workspace configuration, destination, exact old/new KB blocks,
original/new full-file hashes and exact replacement bytes in a separate KB journal.
It writes no Markdown. Its stable preview hash identifies an idempotent operation.

Confirmation requires `knowledge:read` plus `vault:write`, dashboard cookie, CSRF
and allowed Host/Origin. Capture requires `logs:read` plus `review:write`; review
requires `knowledge:read` plus `review:write`. Ingestion credentials grant none of
these. All reads are workspace-scoped. Daily Apply never approves a KB write.

Each producer/plan has one stable `log-inbox:knowledge:<id>` block in its chosen
note. Different plans accumulate in separate blocks; an explicitly reviewed
superseding revision replaces that plan's block. A plan cannot silently move to a
second canonical note after writing starts. The resulting note stays within the 1 MiB per-note and 32 MiB aggregate Reference catalog limits; overlapping collections count each source path once. The shared atomic writer preserves
frontmatter, human prose, Daily blocks, permissions and line endings. Submitted
text is escaped as plain Markdown text; it cannot inject HTML, links or markers.

Both workflows serialize through the same Apply lock and refuse to write over a
pending write from the other workflow. Full-file optimistic hashes detect a changed
target; no stale preview overwrites it. As with Daily, an unrelated editor can race
between the last check and rename; this is not a filesystem-wide editor lock.

Journal states are `prepared`, `writing`, `finalized`, `conflict`, `cancelled`.
After a timeout or restart, inspect the saved operation and retry its exact
confirmation. Recovery uses frozen bytes. If rename already completed, verify full
file/block hashes, sync the parent and finalize the historical approval even if
source evidence has since expired. Otherwise fresh eligibility and exact original
file hashes still apply. An exact durable temporary file can finish the same write;
unrecognized temporary bytes require manual reconciliation. KB recovery is explicit
in Reference notes; it does not write automatically at startup.

Discard a prepared preview, or cancel an unwritten operation after verifying its
original file and removing only its exact temporary bytes. Already-renamed writes
must finish recovery; changed or missing files requiring reconciliation are never
silently restored. Conflicts can be discarded after inspection and replaced by a
fresh preview. The UI retains saved operations so failures are actionable.

## API

All paths begin `/api/v2/knowledge`:

| Method/path | Request / result |
| --- | --- |
| `GET /submissions` | Latest 100 matching retained original events; SQL filters before bounding, workspace eligibility and truncation disclosed |
| `GET /candidates` | Latest 100 candidates and current source/supersession flags |
| `POST /candidates` | `{source_event_id}` → `{candidate}` |
| `GET /candidates/{id}` | `{candidate}` |
| `POST /candidates/{id}/review` | `{expected_candidate_hash, disposition: accept|reject, reason}` → `{review}` |
| `POST /candidates/{id}/promotion/preview` | `{candidate_hash, destination_path}` → exact operation preview |
| `GET /promotions` | Up to 100 operation metadata records, unfinished first; no recovery file bytes |
| `GET /promotions/{id}` | Exact operation preview and state; no recovery file bytes |
| `POST /promotions/{id}/confirm` | `{preview_hash}` → resulting operation |
| `DELETE /promotions/{id}` | Safe discard/cancellation; returns `{state: cancelled}` |

No producer endpoint approves, reviews, chooses an authoritative destination, or
writes Markdown. There is no extra MCP tool, service or dependency.

## Upgrade and rollback

Additive transactional migrations 28/29 add immutable candidate/review tables and
KB operation journals. Existing workspace IDs, events, Daily revisions and existing
Markdown are preserved; reopening is idempotent. Back up the live SQLite database
with its online backup facility and verify integrity before upgrading. Retain the
matching old app/database backup. Rollback must preserve/export events received
since that backup and every unfinished KB operation before restoring; never replace
a live database with an old copy and silently lose new evidence. Canonical files
already changed by explicit confirmation are not reverted by database rollback;
reconcile them using the frozen original bytes and current file hashes under owner
review. Local tests use disposable workspaces/databases. A separately requested local
deployment on 2026-10-19 upgraded the owner installation from schema 27 to 29 after
a verified backup and preserved existing events, review/configuration state and
tracked Markdown hashes; see the roadmap deployment record.
