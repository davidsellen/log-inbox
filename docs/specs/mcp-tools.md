# MCP Tools

The MCP server is the agent-facing interface. It reads from the durable log store; it does not receive logs directly from devices.

## Tools

### list_sources

Return known sources and recent event counts.

Input:

```json
{
  "since": "2026-08-31T00:00:00Z"
}
```

### read_recent_logs

Return a bounded recent window.

Input:

```json
{
  "source": "examplewin/iis",
  "since": "2026-08-31T00:00:00Z",
  "level": "error",
  "limit": 100
}
```

### search_logs

Search message and selected metadata fields.

Input:

```json
{
  "query": "500 export",
  "since": "2026-08-31T00:00:00Z",
  "limit": 100
}
```

### get_log_window

Return logs around a specific event ID.

Input:

```json
{
  "event_id": "evt_123",
  "before": "5m",
  "after": "2m",
  "limit": 200
}
```

### mark_reviewed

Mark events or a query result as reviewed after the agent has handled them.

Input:

```json
{
  "event_ids": ["evt_123"],
  "note": "Summarized in Daily log Aug 31"
}
```

## Output Rules

- Return stable event IDs.
- Return timestamps in ISO 8601 UTC.
- Default to bounded results.
- Include truncation metadata when output was limited.
- Never return secrets that the collector already marked redacted.
