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
  "source": "windows/iis",
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

### suggest_markdown_summary

Use an LLM-backed workflow to propose a concise Markdown summary for a bounded log group. The tool may be implemented by the MCP server or by an agent that calls the MCP read tools first.

Input:

```json
{
  "event_ids": ["evt_123", "evt_124"],
  "vault_context": {
    "candidate_notes": ["Customer Portal", "Forms"],
    "daily_note": "Daily log Aug 31"
  },
  "mode": "daily-note"
}
```

### stage_markdown_summary

Generate the same reviewable proposal and atomically create a unique Markdown file in `LOG_INBOX_PROPOSAL_DIR`. This operation never edits a daily or product note and does not mark evidence events reviewed.

Input is the same as `suggest_markdown_summary`. Output includes the stable proposal ID, created path, timestamp, and `pending` status.

Successful staging also records each evidence event in `proposal_state`. The automatic worker uses the same operation after a quiet period and will not repeatedly stage recorded events.

Generated proposal content:

```json
{
  "target_note": "Daily log Aug 31",
  "canonical_links": ["[[Customer Portal]]"],
  "markdown": "- Confirmed IIS export failures came from missing route metadata in the host response.",
  "evidence_event_ids": ["evt_123", "evt_124"],
  "requires_review": true
}
```

### apply_markdown_proposal

Apply one explicitly reviewed pending proposal to its target daily note.

Input:

```json
{
  "proposal_id": "proposal_123"
}
```

The target is restricted to a plain Markdown filename in `LOG_INBOX_DAILY_NOTES_DIR`. The tool atomically replaces the daily note, embeds a proposal marker for retry safety, marks its evidence events reviewed in SQLite, and then deletes the consumed pending proposal. A failed acknowledgement leaves the proposal pending.

Output:

```json
{
  "proposal_id": "proposal_123",
  "daily_path": "/vault-daily/Daily log Aug 31.md",
  "evidence_event_ids": ["evt_123", "evt_124"],
  "proposal_removed": true,
  "status": "applied"
}
```

## Output Rules

- Return stable event IDs.
- Return timestamps in ISO 8601 UTC.
- Default to bounded results.
- Include bounded-result metadata when a query or model projection was limited.
- Never return secrets that the collector already marked redacted.
- LLM-generated Markdown must be marked as proposed output until reviewed or explicitly applied.
