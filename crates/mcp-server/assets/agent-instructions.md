## Activity reporting

- For meaningful work, send an HTTP event when work starts and one when it completes, fails, or becomes blocked.
- Endpoint: `{{INGEST_URL}}/v1/logs` with `Authorization: Bearer {{API_KEY}}`.
- Use PowerShell `Invoke-RestMethod`; build a hashtable and serialize it with `ConvertTo-Json -Depth 8 -Compress`. Do not hand-escape JSON for `curl.exe`.
- Before the start event, inspect the repository basename, current branch, starting commit, and relevant working-tree paths.
- Reuse one stable `task_id` and `session_id`; increment integer `sequence` for every event.
- Start with `event_type=start` and `status=running`. Finish with `event_type=complete|blocked|failed` and the matching status.
- Use agent `{{AGENT_NAME}}`, source prefix `{{SOURCE_PREFIX}}`, and default host `{{DEFAULT_HOST}}` when the machine does not provide a better hostname.
- Put structured facts in metadata: `agent`, `host`, `repo`, `branch`, known base/target branches, confirmed `product`, `modules`, bounded `changed_paths`, commit, tests, work item, and pull request.
- Derive changed paths from working-tree, staged, and committed changes since the captured starting commit. Convert paths into short logical module names.
- Confirm product names against product navigation when available. Do not guess for a multi-product repository; omit product and preserve repo/module facts when uncertain.
- Send progress events only for durable diagnoses, decisions, deployments, validation results, or blockers. Do not report every command or file.
- Never send secrets, source contents, full diffs, personal data, or large command output. Reporting failure must not block the primary task.

PowerShell request shape:

```powershell
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

Invoke-RestMethod -Method Post -Uri "{{INGEST_URL}}/v1/logs" `
    -Headers @{ Authorization = "Bearer {{API_KEY}}" } `
    -ContentType "application/json" -Body $body
```
