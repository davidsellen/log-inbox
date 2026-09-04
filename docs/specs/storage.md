# Storage Model

## Default

Use SQLite for the first implementation unless a simpler JSONL-only prototype is needed.

SQLite gives enough structure for filtering, review state, retention cleanup, and source counts without operating a separate database service.

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

The first version should keep logs for `LOG_INBOX_RETENTION_DAYS`, defaulting to 14 days. Reviewed state may be kept longer if it only stores IDs and note references.

## Proposal State

`proposal_state` records each event included in a staged proposal. This is separate from `review_state`: staging prevents duplicate automatic proposals, while review means a human or agent has applied or otherwise handled the proposal.

Accepted redacted content is stored completely. Ingestion rejects values above the API limits rather than accepting partial evidence. LLM prompt projections have smaller independent limits and never overwrite stored content.

## Vault Link Rules

`vault_link_rules` stores user-owned selectors and canonical note IDs. Selectors are encoded as structured JSON so one rule can combine multiple fields while note names and folder conventions remain outside the application schema.
