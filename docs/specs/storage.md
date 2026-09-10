# Storage Model

## Default

Use SQLite for the first implementation unless a simpler JSONL-only prototype is needed.

SQLite gives enough structure for filtering, review state, retention cleanup, and source counts without operating a separate database service.

Schema changes are recorded in `schema_migrations` and applied transactionally in version order. Reopening a database is idempotent. Cross-cutover operations use a separate `migration_journal`; adding the table does not by itself authorize or perform a product migration.

Before a cutover, the service creates a new destination with SQLite's online backup API so the source WAL is included consistently. A backup is accepted only after `PRAGMA integrity_check`, schema-version verification, and event-count comparison. Existing backup files are never overwritten.

## Tables

### log_events

| Column | Purpose |
|---|---|
| `id` | Stable event ID |
| `received_at` | Collector receive time |
| `timestamp` | Producer event time |
| `source` | Stable producer/source name |
| `level` | Trace, debug, info, warn, error, fatal, or unknown |
| `message` | Main log text |
| `metadata_json` | Structured producer metadata |
| `fingerprint` | Optional duplicate/correlation key |
| `truncated` | Whether content from a legacy event was truncated |

### review_state

| Column | Purpose |
|---|---|
| `event_id` | Reviewed event |
| `reviewed_at` | Time reviewed |
| `reviewed_by` | Tool or user identifier |
| `note` | Short note or vault reference |

## Retention

Retention is a workspace policy stored with Daily automation settings. Defaults are displayed but cleanup does not begin until the owner explicitly saves the policy. The Daily service runs bounded maintenance at startup and at most hourly; the collector only ingests and never deletes on startup.

Raw events expire from `received_at`, while immutable snapshot IDs, digests, ordering, and review decisions survive with their live reference cleared. Expired sessions, terminal schedule runs, and reopened dismissal records use the separately configured audit window. Active dismissals, manual entries, user-edited candidates, migration artifacts, and unhandled content are not removed by this maintenance pass.

## Proposal State

`proposal_state` records each event included in a staged proposal. This is separate from `review_state`: staging prevents duplicate automatic proposals, while review means a human or agent has applied or otherwise handled the proposal.

Accepted redacted content is stored completely. Ingestion rejects values above the API limits rather than accepting partial evidence. LLM prompt projections have smaller independent limits and never overwrite stored content.

## Vault Link Rules

`vault_link_rules` stores user-owned selectors and canonical note IDs. Selectors are encoded as structured JSON so one rule can combine multiple fields while note names and folder conventions remain outside the application schema.

Vault-scoped semantic destinations are stored together as versioned JSON preferences keyed by stable vault ID. They contain role, user-owned base path, constrained path template, write mode, and enabled state. Complete structures are validated and replaced atomically. Catalog revisions include note and folder state to protect saves from stale selections; revisions are not used as vault identity. Imported and exported templates contain configuration only and never Markdown contents.
