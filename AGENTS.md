# Codex Instructions

## Log Inbox Activity Logging

- At the start of meaningful work in this repository, try to send one compact activity log event to the local Log Inbox collector.
- During longer work, prefer logging durable milestones only: plan chosen, implementation started, validation result, blocker, commit, push, deployment, or handoff-worthy summary.
- Do not log every command, file read, intermediate thought, or raw command output.
- If the collector is unavailable, do not block the task. Continue normally and mention the missed log only if it matters for handoff.
- Keep log messages concise and structured. The best default is a short summary of the activity performed or plan executed.

Use this shape:

```bash
curl -sS "${LOG_INBOX_URL:-http://127.0.0.1:8787}/v1/logs" \
  -H "Authorization: Bearer ${LOG_INBOX_API_KEY:-dev-local-key}" \
  -H "Content-Type: application/json" \
  -d '{
    "source": "codex/log-inbox",
    "level": "info",
    "message": "Started task: <short user goal>",
    "metadata": {
      "repo": "log-inbox",
      "agent": "codex",
      "activity": "start"
    }
  }'
```

For milestone or completion events, use the same endpoint with `metadata.activity` set to values such as `plan`, `validation`, `commit`, `push`, `blocked`, or `summary`.
