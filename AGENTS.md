# Codex Instructions

## Log Inbox Activity Reporting

- For meaningful work, use the `log-inbox` MCP server's `log_activity` tool once when work starts and once when it completes, fails, or becomes blocked.
- Do not send Log Inbox activity through shell commands, `curl`, or direct HTTP. If the MCP tool is unavailable, continue the primary task and mention the missed report briefly; do not request elevation or use an HTTP fallback.
- During longer work, prefer logging durable milestones only: plan chosen, implementation started, validation result, blocker, commit, push, deployment, or handoff-worthy summary.
- Do not log every command, file read, intermediate thought, or raw command output.
- If the MCP server is unavailable, do not block the task. Continue normally and mention the missed log only if it matters for handoff.
- Reuse a stable `task_id` and `session_id`, increment `sequence`, and identify this machine with `LOG_INBOX_HOST_ID` or its hostname.
- Make the terminal message and metadata daily-note-ready: include the outcome, important decision or diagnosis, validation, blocker or follow-up, and durable links.
- Do not read, create, or append an Obsidian daily note for work logging. Send the material to Log Inbox; its consolidation workflow owns Markdown generation and review.
- Keep log messages concise and structured. Never send secrets, source contents, full diffs, personal data, or large command output.

Pass the same event fields used by the ingest API directly to `log_activity`: `source`, `level`, `message`, and structured `metadata`. For completion events, set `event_type=complete` and `status=succeeded`; use matching terminal values for blocked or failed work.
