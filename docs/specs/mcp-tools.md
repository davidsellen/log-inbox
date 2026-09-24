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

## Future read-only context interface

The next MCP expansion, if the M4 pilot justifies it, is a separate read-only connection named `log-inbox-context`. It is intentionally distinct from this collector endpoint: `log_activity` credentials remain write-only, and context credentials cannot ingest events or mutate Daily.

The endpoint is disabled unless a dedicated context-read credential is configured in the dashboard service. It reuses the reviewed workspace binding and enabled reference folders. Agents may search eligible note titles, aliases and relative paths and read bounded excerpts with source digests; every request rechecks path containment, exclusions, file eligibility and workspace identity. Returned Markdown is untrusted reference data, not executable instructions.

The interface must not provide full-vault browsing, arbitrary file reads, generated Daily blocks, recursive link following, automatic generation, Markdown writes, or knowledge-note promotion. Ambiguous identity matches remain ambiguous. Markdown remains authoritative; AI-generated knowledge requires a later proposal/review/apply contract.

Required evidence before enabling it: one real product/two-module vertical slice, separate-credential tests, path and exclusion tests, bounded UTF-8 excerpts, changed/deleted-note handling, concurrent ingestion/generation checks, and measured usefulness versus setup effort.

## Future write boundary

M5 may add structured, bounded generation and inspection tools for explicitly tested clients. A future Markdown write tool must additionally require:

- a revocable `vault:write` scope distinct from read/generation scopes;
- the exact immutable revision ID and content hash already approved in the dashboard;
- the frozen destination and expected managed-block/file hashes;
- the same journaled writer, conflict handling, and recovery protocol used by dashboard Apply.

A token or tool invocation alone is not evidence of human review. No compatibility alias will restore the removed `stage_markdown_summary` or `apply_markdown_proposal` behavior.
