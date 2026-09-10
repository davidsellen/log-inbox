# Log Inbox

Local-first software for turning selected engineering activity into reviewed daily records in an existing Markdown workspace.

## Goal

Log Inbox is a single-owner, self-hosted pipeline:

- Hosts, scripts, and coding agents send selected activity to the HTTP ingest API.
- Log Inbox keeps the redacted evidence in SQLite and prepares one structured candidate per day.
- You add manual work, review or edit the candidate, inspect the exact Markdown change, and explicitly Apply it.

Daily review is the product. Knowledge context is optional. Log Inbox is not a vault manager, a Notion clone, or an automatic recorder of everything you do.

## Current status

The authenticated Daily workflow, stable workspace profile, immutable revisions, recent-day catch-up, scheduled candidate preparation, exact managed-block preview, journaled Apply/recovery, and reviewed legacy-data cutover are implemented. The [roadmap](docs/roadmap.md) tracks the remaining retention, optional context, and integration work.

The browser never mounts or browses a local folder. The service receives one existing Markdown workspace as a host mount; Settings only defines the daily-note convention inside it. Log Inbox writes nothing until **Confirm Apply**, and then changes only its owned managed block.

## Run with Docker Compose

1. Copy the environment template and generate a private owner secret of at least 20 bytes:

   ```bash
   cp .env.example .env
   openssl rand -base64 32
   ```

2. Set `LOG_INBOX_OWNER_SECRET` to that value and set `LOG_INBOX_WORKSPACE_HOST_DIR` to your existing Markdown workspace.

3. Start the stack:

   ```bash
   docker compose up --build
   ```

4. Open `http://127.0.0.1:8788/`, sign in, and review the timezone, Daily folder, filename pattern, and optional template. Previewing or saving Settings does not create a Markdown file.

The stack includes a private Ollama service and pulls `granite3.3:2b` by default. Its API is not published to the host. A first start can take several minutes while the model downloads.

## Send activity

Send one bounded event to the collector:

```bash
curl -sS http://127.0.0.1:8787/v1/logs \
  -H "Authorization: Bearer $LOG_INBOX_API_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "source": "codex/workstation",
    "level": "info",
    "message": "Completed the reviewed Daily Apply workflow.",
    "metadata": {
      "task_id": "daily-apply",
      "session_id": "agent-session",
      "sequence": 2,
      "event_type": "complete",
      "status": "completed",
      "repo": "log-inbox",
      "branch": "main",
      "modules": ["daily", "writer"],
      "tests": ["cargo test --workspace"]
    }
  }'
```

Messages up to 1 MiB and metadata up to 512 KiB are accepted after secret redaction; larger payloads are rejected rather than silently truncated. Send conclusions and validation, not source code, full diffs, secrets, or noisy command output. A copy-ready agent policy and fuller examples are in [Agent activity reporting](docs/agent-integration.md).

## Daily workflow

1. Choose the calendar date you want to review. The server resolves its exact interval using the saved timezone; changing the selector loads only that day.
2. Add work that did not come from an agent under **My notes**. Those words remain separate and are never rewritten by the model.
3. Generate the day candidate. Automated evidence is grouped into human-readable workstreams with outcomes, decisions, trade-offs, validation, blockers, and follow-ups when supported.
4. Correct the structured fields and include or omit evidence as needed. Each save creates an immutable revision in SQLite and still does not touch Markdown.
5. Select **Review Apply** to see the current and approved managed blocks plus the exact frozen destination.
6. Select **Confirm Apply** to write. Existing frontmatter and all content outside the Log Inbox block are preserved. Interrupted or conflicting writes remain visible and retryable.

If an older date was missed, use the recent-days rail or select it directly and run the same flow. Optional scheduling prepares previous-day and catch-up candidates for review; it never auto-Applies Markdown. An explicitly saved retention policy is owned by the Daily service and never by the collector.

## Upgrading older installations

For an installation that used pending proposal or context files, map those sources only for the migration run:

```bash
docker compose -f docker-compose.yml -f docker-compose.migrate.yml up --build
```

The override accepts the three host paths documented in `.env.migration.example`; the ordinary deployment has only the app-data volume and one Markdown workspace mount. After saving workspace settings, Settings shows **Bring forward older Log Inbox data** when legacy state exists. Review the report before completing it. Cutover:

- creates and verifies a timestamped SQLite backup outside the Markdown workspace;
- imports valid link mappings, ignored identities, and old manual logs into workspace-scoped records;
- preserves legacy settings and every recognized proposal as exact bytes in app storage, including malformed proposals;
- removes obsolete preferences only in the same database transaction as their preservation;
- deletes only valid pending proposal files whose bytes still match the reviewed SHA-256 hash;
- never rewrites historical Markdown notes, guesses unresolved mappings, or deletes changed/unrecognized files.

The operation is journaled and idempotent. If interrupted, opening Settings exposes the same operation for safe completion.
After it completes, restart with ordinary `docker compose up --build`; the migration mounts are no longer needed.

## Configuration

The essential settings are:

- `LOG_INBOX_API_KEYS`: comma-separated ingest credentials.
- `LOG_INBOX_OWNER_SECRET`: private dashboard credential, at least 20 bytes.
- `LOG_INBOX_WORKSPACE_HOST_DIR`: host path of the existing Markdown workspace.
- `LOG_INBOX_WORKSPACE_DIR`: container mount path, normally `/workspace`.
- `LOG_INBOX_ALLOWED_HOSTS` and `LOG_INBOX_ALLOWED_ORIGINS`: exact dashboard request boundaries.
- `LOG_INBOX_LLM_BASE_URL`, `LOG_INBOX_LLM_MODEL`, and optional `LOG_INBOX_LLM_API_KEY`: model connection used for draft generation.

The collector and dashboard bind to loopback by default. Put them behind an authenticated secure route before allowing nonlocal access.

## Specs

- [Roadmap and delivery milestones](docs/roadmap.md)
- [Product brief](docs/specs/product-brief.md)
- [Architecture](docs/specs/architecture.md)
- [Ingest API](docs/specs/ingest-api.md)
- [Refocused API](docs/specs/refocus-api.md)
- [Daily domain](docs/specs/daily-domain.md)
- [Storage model](docs/specs/storage.md)
- [Docker deployment](docs/specs/docker.md)
- [Security and threat model](docs/specs/threat-model.md)
- [Vault writing policy](docs/specs/vault-policy.md)
- [Agent activity reporting](docs/agent-integration.md)

## License

Log Inbox is open-source software licensed under the [MIT License](LICENSE).
