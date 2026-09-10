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

Raw events expire from `received_at`, while snapshot IDs, digests, ordering, and review decisions referenced by a retained revision survive with their live reference cleared. Imported manual entries also retain provenance after their original raw event expires. Expired sessions, terminal schedule runs, reopened dismissal records, superseded unreferenced proposal revisions, their orphan snapshots, and validated imported artifact copies use the separately configured audit window. Finalized Apply operations keep their target, hashes, and operation identity, but their rollback bytes/path and temporary filename are scrubbed after the recovery window. The same recovery window removes only exact completed-cutover backups named in the migration journal. Active dismissals, current revisions, manual entries, unparseable migration artifacts, and unfinished Apply recovery material are preserved.

## Proposal State

`proposal_state` records each event included in a staged proposal. This is separate from `review_state`: staging prevents duplicate automatic proposals, while review means a human or agent has applied or otherwise handled the proposal.

Accepted redacted content is stored completely. Ingestion rejects values above the API limits rather than accepting partial evidence. LLM prompt projections have smaller independent limits and never overwrite stored content.

## Knowledge Collections and Mappings

`knowledge_collections` stores workspace-scoped, reviewed include roots and exclusions with a semantic revision digest. Definitions never contain Markdown content and never create, move, or delete workspace files.

`context_mappings` stores workspace-scoped user-owned selectors and normalized canonical Markdown paths. All selectors in one mapping must match one evidence event. Exact mappings are preferred; legacy `contains` selectors remain explicit and deterministic. A missing, protected, ambiguous, or otherwise invalid reviewed target authorizes no fallback link. Mapping a repository or product can authorize a link, but only reviewed work-item or pull-request identities can merge otherwise distinct workstreams.

`ignored_context_identities` preserves reviewed migration state for the later curated-diagnostics workflow. It is not yet interpreted as a retrieval instruction.

`context_snapshots` stores an immutable, size-bounded resolution record for one workspace day. It contains semantic collection/mapping revisions, a compact catalog-resolution digest, only the canonical notes actually resolved, resolution reasons, group aliases, and exact per-workstream link/evidence authorization. It does not retain the full note catalog or note bodies. `proposal_context_snapshots` immutably binds one proposal revision to one context snapshot; stale unreferenced snapshots expire with audit retention.

Legacy `vault_link_rules`, semantic destination preferences, and catalog records are migration inputs only and have no runtime API or writer authority.
