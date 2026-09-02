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
