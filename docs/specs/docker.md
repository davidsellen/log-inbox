# Docker Deployment

Docker Compose runs the local stack.

## Services

- `collector` binds to `127.0.0.1:8787` by default, accepts authenticated events, and writes only to the shared app-data volume.
- `mcp` is the historical service name for the Daily web service. It binds to `127.0.0.1:8788`, reads shared app data, and mounts exactly one Markdown workspace at `LOG_INBOX_WORKSPACE_DIR`. It currently exposes the dashboard and v2 HTTP API, not an MCP route.
- `ollama` is private to the Compose network and stores models in `ollama-data`.
- `ollama-pull` downloads the configured model before the Daily service starts.

## Ordinary volumes

- `log-inbox-data`: SQLite and application state.
- `ollama-data`: local model files.
- `LOG_INBOX_WORKSPACE_HOST_DIR` mounted at `LOG_INBOX_WORKSPACE_DIR`: the existing Markdown workspace used only by reviewed Apply.

The browser cannot select a directory from another machine. Set the host workspace path in `.env`, then review its timezone and Daily convention in Settings.

## First run

```bash
cp .env.example .env
# Set LOG_INBOX_OWNER_SECRET and LOG_INBOX_WORKSPACE_HOST_DIR.
docker compose up --build
```

The first run may take several minutes while the model downloads. Services publish to loopback by default; use an authenticated secure route for nonlocal access.

## Disposable workflow smoke test

Run the same container boundary used by CI against a temporary app database and Markdown workspace:

```bash
tests/smoke/compose_daily.sh
```

The test uses a deterministic fake OpenAI-compatible provider and random loopback ports. It authenticates, saves workspace settings, generates and reviews one Daily candidate, applies it without replacing owner content, restarts the Daily service, verifies persisted state, and then removes its containers, network, volumes, and temporary files. It never mounts the configured workspace or ordinary `log-inbox-data` volume.

## Legacy migration

Only an older installation with proposal/context files should use:

```bash
docker compose -f docker-compose.yml -f docker-compose.migrate.yml up --build
```

The override mounts explicitly configured legacy sources under `/migration`. Review and commit the migration in Settings, then return to ordinary Compose. Those mounts are never document destinations or runtime fallbacks.
