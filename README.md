# Log Inbox

Spec-first project for collecting logs from hosts, VMs, and local devices into a durable inbox that can feed curated summaries into a Markdown vault through agent-readable tools.

## Goal

Provide a small local logging system with two separate responsibilities:

- Producers send logs to an ordinary ingest API.
- An agent reads curated log slices through MCP tools and writes human summaries to notes only when useful.

MCP is not the ingest protocol. It is the agent-facing read/search/ack interface. In this local setup, the main managed output is a Markdown vault that may be read with Obsidian.

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
    "source": "examplewin/iis",
    "level": "error",
    "message": "Request failed",
    "metadata": { "app": "ExampleOne", "status": 500 }
  }'
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

To drop proposals into an Obsidian-visible folder, set the host folder in `.env`:

```env
LOG_INBOX_PROPOSAL_HOST_DIR=/absolute/path/to/your/vault/00 Inbox/Log Inbox/pending
LOG_INBOX_PROPOSAL_DIR=/vault-inbox
```

On Linux, set `LOG_INBOX_HOST_UID` and `LOG_INBOX_HOST_GID` to the owner of that vault folder (usually the output of `id -u` and `id -g`). The defaults are `1000:1000`. The MCP service uses the host user namespace so bind-mounted files retain that ownership while the process itself remains unprivileged.

Then call `stage_markdown_summary` with the same arguments as `suggest_markdown_summary`. Each call creates a distinct, complete Markdown file with `status: pending`; it does not append to the daily note or mark events reviewed. This keeps concurrent writers isolated. A later consolidator reviews `pending`, updates canonical notes with a content-change check, moves handled files to `processed`, and only then calls `mark_reviewed`.

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
