# Log Inbox

Spec-first project for collecting logs from hosts, VMs, and local devices into a durable inbox that can feed curated summaries into a Markdown vault through agent-readable tools.

## Goal

Provide a small local logging system with two separate responsibilities:

- Producers send logs to an ordinary ingest API.
- An agent reads curated log slices through MCP tools and writes human summaries to notes only when useful.

MCP is not the ingest protocol. It is the agent-facing read/search/ack interface. The managed output is a directory of ordinary Markdown files that can be read by any notes app, editor, static-site generator, or agent.

## Default Deployment

The system should run with Docker Compose:

- `collector`: HTTP ingest API for hosts, VMs, scripts, and services.
- `store`: persistent SQLite or JSONL-backed volume owned by the collector.
- `mcp`: MCP server exposing log review tools to an agent.
- `ollama`: private local inference service; its API is not published to the host.
- `ollama-pull`: one-shot initializer that ensures the configured model is available.

See [Docker deployment](docs/specs/docker.md).

## First Iteration Usage

Build and run the local stack:

```bash
cp .env.example .env
docker compose up --build
```

Send one log event:

```bash
curl -sS http://127.0.0.1:8787/v1/logs \
  -H "Authorization: Bearer dev-local-key" \
  -H "Content-Type: application/json" \
  -d '{
    "source": "windows/iis",
    "level": "error",
    "message": "Request failed",
    "metadata": { "app": "customer-portal", "status": 500 }
  }'
```

Accepted events are stored completely after secret redaction. Messages up to 1 MiB and metadata up to 512 KiB are accepted; larger values are rejected instead of silently truncated. Split large activity into ordered events with shared context:

```json
{
  "source": "codex/windows",
  "message": "Ran the targeted test suite",
  "metadata": {
    "task_id": "task_123",
    "session_id": "codex_456",
    "sequence": 3,
    "event_type": "test",
    "repo": "log-inbox",
    "branch": "main",
    "canonical_note": "Log Inbox"
  }
}
```

Call the MCP-style tools endpoint:

```bash
curl -sS http://127.0.0.1:8788/mcp \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 1,
    "method": "tools/call",
    "params": {
      "name": "search_logs",
      "arguments": { "query": "Request failed", "limit": 10 }
    }
  }'
```

### Local LLM Consolidation

The Compose stack runs Ollama locally and pulls the text-only `granite3.3:2b` model on first start. Ollama is reachable only by other Compose services at `http://ollama:11434`; prompts and log summaries are not sent to a hosted model API. Model downloads are retained in the `ollama-data` volume.

`suggest_markdown_summary` calls Ollama through its OpenAI-compatible chat-completions endpoint. The tool returns a proposed Markdown summary with `requires_review: true`; it does not write to the vault directly.

The first `docker compose up --build` may take several minutes while the model downloads. Check readiness and installed models with:

```bash
docker compose ps
docker compose logs ollama-pull
```

To select another locally installed Ollama model, change:

```env
LOG_INBOX_LLM_MODEL=granite3.3:2b
```

Call the summary proposal tool with selected event IDs:

```bash
curl -sS http://127.0.0.1:8788/mcp \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 2,
    "method": "tools/call",
    "params": {
      "name": "suggest_markdown_summary",
      "arguments": {
        "event_ids": ["evt_123"],
        "vault_context": {
          "daily_note": "Daily log Sep 3",
          "candidate_notes": ["Log Inbox"]
        },
        "task": "Summarize these logs for a daily-note entry."
      }
    }
  }'
```

To drop proposals into a Markdown vault or watched folder, set the host folder in `.env`:

```env
LOG_INBOX_PROPOSAL_HOST_DIR=/absolute/path/to/your/vault/00 Inbox/Log Inbox/pending
LOG_INBOX_PROPOSAL_DIR=/vault-inbox
```

Product names and note targets are user-owned configuration. Point `LOG_INBOX_VAULT_CONTEXT_HOST_FILE` at a JSON file anywhere on the host, including a products folder inside the vault:

```json
{
  "daily_note_format": "Work log %Y-%m-%d",
  "products": [
    {
      "note": "Customer Portal",
      "aliases": ["customer-portal", "portal-api", "windows/iis"]
    }
  ]
}
```

The worker reloads this file for each group and matches aliases against event `source` plus `product`, `repo`, `app`, and `service` metadata. It may also use an explicit `canonical_note` supplied by the producer. With no match, it emits no product link instead of inventing one.

On Linux, set `LOG_INBOX_HOST_UID` and `LOG_INBOX_HOST_GID` to the owner of that vault folder (usually the output of `id -u` and `id -g`). The defaults are `1000:1000`. The MCP service uses the host user namespace so bind-mounted files retain that ownership while the process itself remains unprivileged.

Then call `stage_markdown_summary` with the same arguments as `suggest_markdown_summary`. Each call creates a distinct, complete Markdown file with `status: pending`; it does not append to the daily note or mark events reviewed. This keeps concurrent writers isolated. A later consolidator reviews `pending`, updates canonical notes with a content-change check, moves handled files to `processed`, and only then calls `mark_reviewed`.

Compose also enables automatic staging every 30 seconds. Unreviewed events remain quiet for 30 seconds before they are grouped by `task_id`, then `session_id`, then `fingerprint`; events without one of those identifiers are staged separately. The worker drains retained unstaged events in bounded batches. Successfully staged event IDs are recorded in SQLite so they are not proposed repeatedly. Set `LOG_INBOX_AUTO_STAGE_INTERVAL_SECONDS=0` to disable the worker.

The LLM receives a bounded projection (16 KiB message and 8 KiB metadata per event), but the complete accepted, redacted event remains queryable in SQLite. The context fields shown above remain in that projection even when other metadata is too large.

## Specs

- [Product brief](docs/specs/product-brief.md)
- [Architecture](docs/specs/architecture.md)
- [Ingest API](docs/specs/ingest-api.md)
- [MCP tools](docs/specs/mcp-tools.md)
- [LLM consolidation workflow](docs/specs/llm-consolidation.md)
- [Storage model](docs/specs/storage.md)
- [Docker deployment](docs/specs/docker.md)
- [Security](docs/specs/security.md)
- [Vault writing policy](docs/specs/vault-policy.md)

## License

Log Inbox is open-source software licensed under the [MIT License](LICENSE).
