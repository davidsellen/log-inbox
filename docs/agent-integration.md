# Agent Activity Reporting

An agent should send a small lifecycle of structured events, not one context-free summary. The message explains what happened to a person; metadata preserves the facts needed to group, link, verify, and consolidate it later.

## Recommended Lifecycle

Send at least two events for meaningful work:

1. `start`: after inspecting repository context and before edits or execution.
2. `complete`, `blocked`, or `failed`: after final validation, including the actual outcome.

Send `progress` only for durable milestones, decisions, deployments, or blockers. Reuse one `task_id` and `session_id`, and increment `sequence`. This lets the worker consolidate the whole activity after the quiet period instead of producing unrelated notes.

## Metadata Contract

| Field | Purpose |
|---|---|
| `task_id` | Stable ID for the user request or work item; primary grouping key. |
| `session_id` | One agent run or continuation; fallback grouping key. |
| `sequence` | Integer ordering within the task. |
| `event_type` | `start`, `progress`, `decision`, `validation`, `complete`, `blocked`, or `failed`. |
| `status` | Current result such as `running`, `succeeded`, `blocked`, or `failed`. |
| `agent` and `host` | Sender identity and machine context. |
| `repo` or `project` | Repository basename as observed from Git or the workspace. |
| `branch` | Current branch; add `base_branch` and `target_branch` when known. |
| `product` | Confirmed canonical product name. Omit when uncertain. |
| `modules` | Short logical module names derived from changed paths. |
| `changed_paths` | Bounded list of important paths, not a patch or full file contents. |
| `commit` | Resulting commit hash when one was created. |
| `tests` or `validation` | Commands or checks and their outcome. |
| `work_item` or `pull_request` | Durable external identifiers or named URLs. |

The product resolver compares normalized repository identifiers with wiki-link targets in the configured product navigation. For example, `CustomerPortal`, `customer-portal`, and `Customer Portal` can match the same existing note. A repository may contain several products, so changed modules and an explicitly confirmed `product` take precedence over guessing from the repository name.

Do not send secrets, access tokens, complete diffs, source files, personal data, or huge command output. Reference an artifact path and digest when detailed evidence must remain elsewhere.

## Start Event

```json
{
  "source": "codex/windows-workstation",
  "level": "info",
  "message": "Started investigating chat navigation failures and inspected the repository state.",
  "metadata": {
    "task_id": "work-item-56693",
    "session_id": "codex-20260903-01",
    "sequence": 1,
    "event_type": "start",
    "status": "running",
    "agent": "codex",
    "host": "windows-workstation",
    "repo": "application-suite",
    "branch": "feature/56693-chat-navigation",
    "modules": ["chat", "host-navigation"]
  }
}
```

## Completion Event

```json
{
  "source": "codex/windows-workstation",
  "level": "info",
  "message": "Fixed route validation, confirmed navigation behavior, and completed the requested change.",
  "metadata": {
    "task_id": "work-item-56693",
    "session_id": "codex-20260903-01",
    "sequence": 3,
    "event_type": "complete",
    "status": "succeeded",
    "agent": "codex",
    "host": "windows-workstation",
    "repo": "application-suite",
    "branch": "feature/56693-chat-navigation",
    "base_branch": "dev",
    "product": "Customer Portal",
    "modules": ["chat", "host-navigation"],
    "changed_paths": ["frontend/chat", "backend/navigation"],
    "commit": "abc1234",
    "tests": ["navigation tests: passed", "frontend build: passed"]
  }
}
```

## Codex Instructions

Add this adapted section to the agent's global or repository `AGENTS.md`. Keep the URL and key in environment variables rather than committing credentials.

```md
## Activity reporting

- For meaningful work, send an HTTP log event when work starts and one when it completes, fails, or becomes blocked.
- Before the start event, inspect the repository basename, current branch, and relevant working-tree paths. Derive short module names from the paths involved.
- Reuse one stable `task_id` and `session_id`; increment integer `sequence` for each event.
- Use `event_type=start` and `status=running` initially. Finish with `event_type=complete|blocked|failed` and the matching status.
- Write a concise human message stating the intent or outcome. Put structured facts in metadata: `agent`, `host`, `repo`, `branch`, known base/target branches, confirmed `product`, `modules`, bounded `changed_paths`, commit, tests, work item, pull request, and canonical note candidates.
- Confirm product names against the configured product navigation when available. Do not guess a product for a multi-product repository; omit it and preserve repo/module facts when uncertain.
- Make the terminal event daily-note-ready: include the outcome, important decision or diagnosis, validation, blocker or follow-up, and durable links without dumping raw logs.
- Do not read, create, or append an Obsidian daily note for work logging. Send the material to Log Inbox; its consolidation workflow owns Markdown generation and review.
- Never send secrets, tokens, source contents, full diffs, personal data, or large command output. Reporting failure must not corrupt or replace the primary task result.
- POST JSON to `$LOG_INBOX_URL/v1/logs` using `Authorization: Bearer $LOG_INBOX_API_KEY`.
```

The agent can issue the POST with `curl` on Unix-like shells or `Invoke-RestMethod` in PowerShell. Generate JSON with the platform's JSON serializer when values come from Git commands; avoid hand-built quoting. Bound reporting requests to a few seconds so an unavailable inbox cannot stall the primary task.
