# MCP Interface

The collector exposes an authenticated Streamable HTTP MCP endpoint at `/mcp`. It is a narrow agent-ingestion interface; the existing [ingest API](ingest-api.md) remains the default for services, scripts, and non-MCP producers.

## Authentication

MCP requests use the same `Authorization: Bearer <key>` header and `LOG_INBOX_API_KEYS` validation as HTTP ingestion. Loopback binding is the deployment default. Authentication failures occur before MCP dispatch.

## Tool

### `log_activity`

Store one activity event using the same validation, redaction, and persistence path as `POST /v1/logs`.

Input fields are `source`, `message`, optional `level`, optional ISO 8601 `timestamp`, optional object `metadata`, and optional `fingerprint`. Output contains the stable event `id`, `status: "stored"`, and `truncated`.

The tool is additive, non-destructive, non-idempotent, and closed-world. Agents should send one start event and one terminal event for meaningful work, reusing `task_id` and `session_id` in metadata and incrementing `sequence`.

## Deliberate boundary

The removed legacy MCP surface included log browsing, review mutation, proposal staging, and Markdown Apply operations. None are restored. Review, generation, and Apply remain behind the authenticated Daily dashboard and its reviewed-revision contract.

## Future interface

M5 may add structured, bounded generation and inspection tools for explicitly tested clients. A future Markdown write tool must additionally require:

- a revocable `vault:write` scope distinct from read/generation scopes;
- the exact immutable revision ID and content hash already approved in the dashboard;
- the frozen destination and expected managed-block/file hashes;
- the same journaled writer, conflict handling, and recovery protocol used by dashboard Apply.

A token or tool invocation alone is not evidence of human review. No compatibility alias will restore the removed `stage_markdown_summary` or `apply_markdown_proposal` behavior.
