# Architecture

```text
trusted producers --HTTP ingest--> collector --SQLite--> Daily service/dashboard
                                                        | reviewed Apply
                                                        v
                                              one Markdown workspace
```

## Responsibilities

### Collector

- Authenticate producers, redact known secret shapes, validate bounds, and durably store the original event and receipt time.
- Never call the model or access the Markdown workspace.

### Shared store

- Hold events, workspace profiles, manual entries, evidence snapshots and decisions, immutable candidate revisions, sessions, migration journals, and Apply recovery state.
- Keep app state and secrets outside the Markdown workspace.

### Daily service

- Authenticate the owner and enforce exact Host, Origin, CSRF, and scope checks.
- Resolve calendar days and Markdown destinations server-side.
- Generate and validate structured candidates through one configured model connection.
- Render exact previews and perform journaled managed-block writes through the inspected workspace capability.
- Recover interrupted Apply operations without duplicating content.

### Browser dashboard

- Review one day, add trusted manual notes, edit structured facts, decide evidence, and approve the exact Apply plan.
- Never receive ingest or model credentials and never mount/browse a client-side folder.

## Deployment boundary

The ordinary dashboard container receives only the shared app-data volume and one read/write Markdown workspace mount. Legacy proposal/context mounts are available solely through the reviewed migration override. The current release has no MCP route, scheduler, or general knowledge browser; those capabilities follow separate roadmap gates.
