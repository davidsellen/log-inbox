# Log Inbox

Spec-first project for collecting logs from hosts, VMs, and local devices into a durable inbox that can feed curated summaries into an Obsidian vault through agent-readable tools.

## Goal

Provide a small local logging system with two separate responsibilities:

- Producers send logs to an ordinary ingest API.
- An agent reads curated log slices through MCP tools and writes human summaries to notes only when useful.

MCP is not the ingest protocol. It is the agent-facing read/search/ack interface. In this local setup, the main managed output is the Obsidian vault.

## Default Deployment

The system should run with Docker Compose:

- `collector`: HTTP ingest API for hosts, VMs, scripts, and services.
- `store`: persistent SQLite or JSONL-backed volume owned by the collector.
- `mcp`: MCP server exposing log review tools to an agent.

See [Docker deployment](docs/specs/docker.md).

## Specs

- [Product brief](docs/specs/product-brief.md)
- [Architecture](docs/specs/architecture.md)
- [Ingest API](docs/specs/ingest-api.md)
- [MCP tools](docs/specs/mcp-tools.md)
- [Storage model](docs/specs/storage.md)
- [Docker deployment](docs/specs/docker.md)
- [Security](docs/specs/security.md)
- [Vault writing policy](docs/specs/vault-policy.md)
