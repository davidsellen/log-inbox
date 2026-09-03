# Docker Deployment

Docker Compose is the default way to run the system locally.

## Services

### collector

- Binds to `127.0.0.1:8787` by default.
- Receives HTTP log events.
- Writes to `/data`.
- Should be reachable from local VMs through an explicitly configured host address or port forward.

### mcp

- Binds to `127.0.0.1:8788` by default.
- Reads `/data` as read-only when possible.
- Exposes MCP tools for an agent.

### ollama

- Runs local model inference inside the Compose network.
- Does not publish port `11434` to the host.
- Stores downloaded models in `ollama-data`.
- Defaults to the text-only `granite3.3:2b` model for bounded log consolidation.

### ollama-pull

- Runs once after Ollama becomes healthy.
- Pulls `LOG_INBOX_LLM_MODEL` when it is not already present.
- Must complete successfully before the MCP service starts.

## Volumes

- `log-inbox-data` stores SQLite/JSONL data.
- `ollama-data` stores local model files.

## VM Access

For local virtual machines, prefer one of:

- VM-to-host NAT gateway address, when stable.
- Explicit port forward from host to collector.
- Shared folder file drop plus a host-side forwarder.

Do not expose the collector on all interfaces unless the network is trusted and API keys are configured.

## First Run

```bash
cp .env.example .env
docker compose up --build
```

The first run downloads the configured local model and can take several minutes. Later starts reuse the model volume.
