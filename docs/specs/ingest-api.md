# Ingest API

## Protocol

HTTP JSON is the default ingestion protocol because it is easy from Windows, Linux, VMs, shell scripts, browser-based clients, and services.

gRPC is not part of the first version. It can be added later if high-volume structured service ingestion needs it.

## Authentication

Every write request must include an API key:

```http
Authorization: Bearer <key>
```

Keys are configured through `LOG_INBOX_API_KEYS`.

## Endpoints

### POST /v1/logs

Ingest one event.

```json
{
  "source": "windows/iis",
  "level": "error",
  "timestamp": "2026-08-31T13:45:00Z",
  "message": "Request failed",
  "metadata": {
    "app": "customer-portal",
    "path": "/exports",
    "status": 500
  }
}
```

### POST /v1/logs/batch

Ingest multiple events. The collector should accept partial success only if it returns per-event status.

```json
{
  "events": []
}
```

### GET /health

Return process and storage health.

```json
{
  "status": "ok"
}
```

## Event Rules

- `source` is required and should be stable.
- `message` is required.
- `timestamp` is optional; collector receive time is used when missing.
- `metadata` must be JSON object data, not preformatted text.
- Accepted events are persisted completely after redaction.
- Messages larger than 1 MiB or metadata larger than 512 KiB are rejected instead of silently truncated.
- Producers should split large activity into ordered events sharing `task_id` or `session_id` metadata.
- `sequence`, `event_type`, `repo` or `project`, `branch`, `product`, `modules`, `changed_paths`, `commit`, and validation fields preserve consolidation context.
- Meaningful agent work should emit a correlated opening event and a terminal `complete`, `blocked`, or `failed` event.
- See [Agent activity reporting](../agent-integration.md) for the recommended lifecycle and metadata contract.
