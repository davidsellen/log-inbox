# MCP Interface

The current Daily release does not expose an MCP route or MCP write tools. Producers send activity through the authenticated [ingest API](ingest-api.md), and the owner reviews, edits, and applies a day through the authenticated dashboard.

This is intentional: the removed legacy MCP surface included proposal staging and Apply operations that did not satisfy the approved-revision and workspace-capability contract.

## Planned interface

M5 may add structured, bounded generation and inspection tools for explicitly tested clients. A future Markdown write tool must additionally require:

- a revocable `vault:write` scope distinct from read/generation scopes;
- the exact immutable revision ID and content hash already approved in the dashboard;
- the frozen destination and expected managed-block/file hashes;
- the same journaled writer, conflict handling, and recovery protocol used by dashboard Apply.

A token or tool invocation alone is not evidence of human review. No compatibility alias will restore the removed `stage_markdown_summary` or `apply_markdown_proposal` behavior.
