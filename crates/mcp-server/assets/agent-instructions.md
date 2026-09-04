## Activity reporting

- For meaningful work, send an HTTP event when work starts and one when it completes, fails, or becomes blocked.
- Read the endpoint from `LOG_INBOX_URL`, falling back to `{{INGEST_URL}}`, and read the API key only from `LOG_INBOX_API_KEY` at execution time.
- Never paste an API key into `AGENTS.md`, source control, messages, or metadata. Skip reporting without blocking the primary task when `LOG_INBOX_API_KEY` is unset.
- Use PowerShell `Invoke-RestMethod`; build a hashtable and serialize it with `ConvertTo-Json -Depth 8 -Compress`. Do not hand-escape JSON for `curl.exe`.
- Before the start event, inspect the repository basename, current branch, starting commit, and relevant working-tree paths.
- Reuse one stable `task_id` and `session_id`; increment integer `sequence` for every event.
- Start with `event_type=start` and `status=running`. Finish with `event_type=complete|blocked|failed` and the matching status.
- Use agent `{{AGENT_NAME}}`, source prefix `{{SOURCE_PREFIX}}`, and default host `{{DEFAULT_HOST}}` when the machine does not provide a better hostname.
- Put structured facts in metadata: `agent`, `host`, `repo`, `branch`, known base/target branches, confirmed `product`, `modules`, bounded `changed_paths`, commit, tests, work item, pull request, and canonical note candidates.
- Derive changed paths from working-tree, staged, and committed changes since the captured starting commit. Convert paths into short logical module names.
- Confirm product names against product navigation when available. Do not guess for a multi-product repository; omit product and preserve repo/module facts when uncertain.
- Send progress events only for durable diagnoses, decisions, deployments, validation results, or blockers. Do not report every command or file.
- Make the terminal event's message and metadata sufficient for later daily consolidation: state the outcome, important decision or diagnosis, validation, blocker or follow-up, and durable links without dumping raw logs.
- Do not read, create, or append an Obsidian daily note for work logging. Send the daily-note-ready material to Log Inbox; its consolidation workflow owns Markdown generation and review.
- Never send secrets, source contents, full diffs, personal data, or large command output. Reporting failure must not block the primary task.

PowerShell request shape:

```powershell
$ingestUrl = if ([string]::IsNullOrWhiteSpace($env:LOG_INBOX_URL)) { "{{INGEST_URL}}" } else { $env:LOG_INBOX_URL.TrimEnd("/") }
$apiKey = $env:LOG_INBOX_API_KEY
if ([string]::IsNullOrWhiteSpace($apiKey)) {
    Write-Warning "LOG_INBOX_API_KEY is not set; skipping activity reporting"
    return
}

$body = @{
    source = "{{SOURCE_PREFIX}}/$($env:COMPUTERNAME.ToLower())"
    level = "info"
    message = "<concise intent or outcome>"
    metadata = @{
        task_id = "<stable-task-id>"
        session_id = "<session-id>"
        sequence = 1
        event_type = "start"
        status = "running"
        agent = "{{AGENT_NAME}}"
        host = $env:COMPUTERNAME
        repo = "<repository-name>"
        branch = "<branch-name>"
        modules = @("<module>")
    }
} | ConvertTo-Json -Depth 8 -Compress

Invoke-RestMethod -Method Post -Uri "$ingestUrl/v1/logs" `
    -Headers @{ Authorization = "Bearer $apiKey" } `
    -ContentType "application/json" -Body $body -TimeoutSec 5
```
