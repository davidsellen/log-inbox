# Codex Instructions

## Log Inbox Activity Reporting

- At the start of meaningful work in this repository, try to send one compact activity event to the Log Inbox collector and one terminal event when the work completes, fails, or becomes blocked.
- During longer work, prefer logging durable milestones only: plan chosen, implementation started, validation result, blocker, commit, push, deployment, or handoff-worthy summary.
- Do not log every command, file read, intermediate thought, or raw command output.
- If the collector is unavailable, do not block the task. Continue normally and mention the missed log only if it matters for handoff.
- Read the API key only from `LOG_INBOX_API_KEY`. Never put its value in this file, source control, messages, or metadata; skip reporting when it is unset.
- Reuse a stable `task_id` and `session_id`, increment `sequence`, and identify this machine with `LOG_INBOX_HOST_ID` or its hostname.
- Make the terminal message and metadata daily-note-ready: include the outcome, important decision or diagnosis, validation, blocker or follow-up, and durable links.
- Do not read, create, or append an Obsidian daily note for work logging. Send the material to Log Inbox; its consolidation workflow owns Markdown generation and review.
- Keep log messages concise and structured. Never send secrets, source contents, full diffs, personal data, or large command output.

Use this shape:

```bash
if [ -n "${LOG_INBOX_API_KEY:-}" ]; then
  curl -sS --max-time 5 "${LOG_INBOX_URL:-http://127.0.0.1:8787}/v1/logs" \
    -H "Authorization: Bearer $LOG_INBOX_API_KEY" \
    -H "Content-Type: application/json" \
    -d '{
      "source": "codex/<host-id>",
      "level": "info",
      "message": "Started task: <short user goal>",
      "metadata": {
        "agent": "codex",
        "host": "<host-id>",
        "repo": "log-inbox",
        "branch": "<branch-name>",
        "task_id": "<stable-task-id>",
        "session_id": "<session-id>",
        "sequence": 1,
        "event_type": "start",
        "status": "running"
      }
    }'
fi
```

Build real requests with `jq -n` rather than manually escaping dynamic JSON. For completion events, set `event_type=complete` and `status=succeeded`; use matching terminal values for blocked or failed work.
