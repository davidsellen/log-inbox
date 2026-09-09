# Log Inbox

Spec-first project for turning selected engineering activity into reviewed daily records in an existing Markdown workspace.

## Goal

The product direction is a single-owner, self-hosted daily engineering journal pipeline:

- Producers send selected activity to the ordinary HTTP ingest API.
- Log Inbox collects evidence, generates daily drafts, and supports explicit review before applying summaries to Markdown notes.

MCP is an agent-facing interface, not the ingest protocol. Daily review is the product; Knowledge provides optional context; Settings configures the service. Log Inbox is not a vault manager or a Notion replacement.

## Roadmap and Implementation Status

See the [Daily Engineering Knowledge roadmap](docs/roadmap.md) for the planned direction, phased milestones, safety contracts, migration gates, and pilot criteria.

The refocus is planned, not implemented by this documentation update. The usage instructions below describe the pre-refocus runtime, including browser-connected vaults, separate mounts, and per-task proposal staging. Those workflows remain documented until the coordinated cutover; the roadmap governs future development. Do not assume the planned dashboard authentication or new write-safety guarantees are available in the current runtime.

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

For coding agents, use a correlated start and terminal event with repository, branch, changed-module, commit, and validation metadata. A copy-ready `AGENTS.md` policy and complete payload examples are in [Agent activity reporting](docs/agent-integration.md).

### Local Dashboard

Open the MCP service root in a browser:

```text
http://127.0.0.1:8788/
```

The dashboard provides quick manual work logging, a Markdown proposal reader, Apply and Discard actions, a durable and cancellable LLM-backed daily consolidation preview, persisted non-secret agent and consolidation preferences, and generated Windows-ready `AGENTS.md` instructions. Manual entries can reuse existing vault-note links and are preserved verbatim under **My notes**, separately from model-summarized automated activity. Refreshing the page restores consolidation status from SQLite. Generated instructions read the API key from `LOG_INBOX_API_KEY` at execution time; the key is never entered into or copied by the dashboard. See the [dashboard specification](docs/specs/dashboard.md) for the review and cleanup lifecycle.

When the stack is hosted behind a VM address, use that address with port `8788`, for example `http://10.0.2.2:8788/`.

### Local LLM Consolidation

The Compose stack runs Ollama locally and pulls the text-only `granite3.3:2b` model on first start. Ollama is reachable only by other Compose services at `http://ollama:11434`; prompts and log summaries are not sent to a hosted model API. Model downloads are retained in the `ollama-data` volume. The dashboard stores the editable daily-consolidation prompt in SQLite; fixed evidence, redaction, canonical-link, and output-schema guardrails remain part of the application.

Local inference can be slow on CPU. Active model requests default to a 300-second timeout, configurable with `LOG_INBOX_LLM_REQUEST_TIMEOUT_SECONDS`; requests waiting behind another model call do not consume that timeout.

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

Set the URL shown in generated agent instructions independently from the collector's internal Compose address:

```env
LOG_INBOX_PUBLIC_INGEST_URL=http://127.0.0.1:8787
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
          "daily_note": "Configured daily note",
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

To discover existing product notes directly from a Markdown navigation page, bind it read-only:

```env
LOG_INBOX_PRODUCT_INDEX_HOST_FILE=/absolute/path/to/vault/Products.md
LOG_INBOX_PRODUCT_INDEX_FILE=/config/product-navigation.md
```

Wiki-link targets such as `[[Customer Portal]]` and `[[Billing#Operations|Billing ops]]` become allowed canonical note candidates. Exact `product`, `repo`, `app`, `service`, or source values can select them. Configured alias mappings are also checked against this navigation when it is present, preventing stale aliases from creating links to removed products.

For the dashboard's complete, user-owned note catalog, mount the vault read-only:

```env
LOG_INBOX_VAULT_HOST_DIR=/absolute/path/to/vault
LOG_INBOX_VAULT_DIR=/vault
LOG_INBOX_VAULT_EXCLUDE_PREFIXES=00 Inbox,01 Work Log,.obsidian
```

Open **Knowledge** to edit the vault's semantic structure, review unresolved names, browse saved links by canonical note, and restore names you previously ignored. Start from detected folders, an unnumbered developer baseline, a blank structure, or an imported Log Inbox structure template. Every suggestion remains an unsaved draft until its exact destinations and missing folders are reviewed; numeric prefixes have no application meaning. The proposal inbox and editor-owned folders are protected from selection. Identifier-to-note links remain separate from output destinations and may match `source`, `repo`, `project`, `product`, `app`, `service`, `module`, `work_item`, or `branch`.

Chrome and Edge users may choose an existing or empty vault from **Knowledge → Settings**. The browser retains the directory permission and uses a stable vault-scoped ID so destination choices cannot leak between vaults. Structure setup shows folders only through a search-first picker; Markdown files remain available only in note-linking workflows. Applying a structure may create approved missing directories but never moves, renames, copies, overwrites, or deletes Markdown files. The mounted configuration remains read-only and available for unattended and headless operation.

On Linux, pre-create the proposal and daily-note host directories, then set `LOG_INBOX_HOST_UID` and `LOG_INBOX_HOST_GID` to their owner (usually the output of `id -u` and `id -g`). The defaults are `1000:1000`. Pre-creation matters because Docker-created bind directories may be owned by `root` or `nobody`. The MCP service uses the host user namespace so files retain the configured ownership while the process itself remains unprivileged.

Then call `stage_markdown_summary` with the same arguments as `suggest_markdown_summary`. Each call creates a distinct, complete Markdown file with `status: pending`; it does not append to the daily note or mark events reviewed. This keeps concurrent writers isolated.

After reviewing a proposal, apply it with:

```bash
curl -sS http://127.0.0.1:8788/mcp \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc": "2.0",
    "id": 3,
    "method": "tools/call",
    "params": {
      "name": "apply_markdown_proposal",
      "arguments": { "proposal_id": "proposal_123" }
    }
  }'
```

The target note must be a plain filename under `LOG_INBOX_DAILY_NOTES_HOST_DIR`. The apply operation writes a complete replacement through a temporary file, includes an idempotency marker, marks its evidence reviewed in SQLite, and removes the consumed pending proposal. If acknowledgement fails, the proposal remains pending for retry. This is the single-writer consolidation boundary; producers never append to a shared daily note.

Compose also enables automatic staging every 30 seconds. Unreviewed events remain quiet for 30 seconds before they are grouped by `task_id`, then `session_id`, then `fingerprint`; events without one of those identifiers are staged separately. The worker drains retained unstaged events in bounded batches. Successfully staged event IDs are recorded in SQLite so they are not proposed repeatedly. Set `LOG_INBOX_AUTO_STAGE_INTERVAL_SECONDS=0` to disable the worker.

The LLM receives a bounded projection (16 KiB message and 8 KiB metadata per event), but the complete accepted, redacted event remains queryable in SQLite. The context fields shown above remain in that projection even when other metadata is too large.

## Specs

- [Roadmap and delivery milestones](docs/roadmap.md)
- [Product brief](docs/specs/product-brief.md)
- [Architecture](docs/specs/architecture.md)
- [Ingest API](docs/specs/ingest-api.md)
- [MCP tools](docs/specs/mcp-tools.md)
- [LLM consolidation workflow](docs/specs/llm-consolidation.md)
- [Storage model](docs/specs/storage.md)
- [Docker deployment](docs/specs/docker.md)
- [Security](docs/specs/security.md)
- [Vault writing policy](docs/specs/vault-policy.md)
- [Agent activity reporting](docs/agent-integration.md)

## License

Log Inbox is open-source software licensed under the [MIT License](LICENSE).
