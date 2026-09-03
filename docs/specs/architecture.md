# Architecture

## Shape

```text
host / VM / device / script
        |
        | HTTP JSON, JSONL batch, or file forwarder
        v
collector container
        |
        | append-only durable storage
        v
SQLite or JSONL volume
        |
        | bounded read/search tools
        v
MCP server
        |
        | agent tool calls and optional LLM consolidation
        v
reviewed Markdown summaries
```

## Responsibilities

### Collector

- Accept log events from trusted local producers.
- Normalize timestamps, source names, levels, message text, and metadata.
- Assign stable event IDs.
- Persist events before responding success.
- Avoid summarization or vault writes.

### Store

- Keep raw events for short retention.
- Support source/time/level/query filters.
- Track reviewed or exported state.
- Be easy to back up or delete.

### MCP Server

- Expose narrow tools for an agent.
- Return bounded results with stable IDs.
- Support search and time-window review.
- Mark reviewed events after the agent has handled them.
- Serve the local proposal reader and agent-preference dashboard on the same private port.

### Local Dashboard

- Read pending proposal files through structured server responses.
- Apply one reviewed proposal through the same serialized path used by MCP.
- Store non-secret display and instruction preferences in SQLite.
- Generate copy-ready agent instructions without persisting the API key.
- Build a bounded whole-day consolidation proposal and return the stored result for preview.
- Persist consolidation jobs and frozen evidence snapshots so work survives refresh and service restart.
- Support cooperative cancellation while the worker awaits the LLM.
- Consume explicitly superseded proposals only after the consolidated proposal is applied.

### Agent

- Pull logs on demand.
- Correlate log windows with the active task.
- Update product notes or daily notes only with durable conclusions.

### LLM Consolidation

- Runs after logs are grouped and bounded by source/time/correlation.
- May run automatically after a configurable inactivity period.
- Produces proposed summaries, canonical note targets, and Markdown patches.
- Writes immutable pending files into a vault-mounted inbox and records staged evidence IDs.
- Must cite event IDs, source windows, and external links used for the conclusion.
- Should be review-first by default; automatic vault writes are only safe for low-risk local notes with strict policies.
