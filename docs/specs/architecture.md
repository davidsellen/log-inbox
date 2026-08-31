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

### Agent

- Pull logs on demand.
- Correlate log windows with the active task.
- Update product notes or daily notes only with durable conclusions.

### LLM Consolidation

- Runs after logs are grouped and bounded by source/time/correlation.
- Produces proposed summaries, canonical note targets, and Markdown patches.
- Must cite event IDs, source windows, and external links used for the conclusion.
- Should be review-first by default; automatic vault writes are only safe for low-risk local notes with strict policies.
