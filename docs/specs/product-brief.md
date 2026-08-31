# Product Brief

## Problem

Host machines, virtual machines, and local services produce useful diagnostic logs, but those logs are scattered across terminals, Docker, Windows, and device-specific locations. An agent can help summarize and connect those logs to Markdown work notes, but raw logs should not be pasted into chat or dumped into the vault.

## Desired Outcome

Create a local log inbox that accepts logs from many producers, stores them durably, exposes review tools to an agent, and lets the agent write concise summaries into a Markdown vault when there is a durable result.

## Non-Goals

- Do not use MCP as the device ingestion protocol.
- Do not write raw log streams directly into the Markdown vault.
- Do not require every producer to install an agent-specific client.
- Do not expose the collector publicly by default.

## First Useful Version

- Docker Compose starts a local collector and MCP server.
- Hosts and VMs can send JSON logs over HTTP.
- Logs are stored in SQLite or JSONL on a Docker volume.
- An agent can list sources, search recent logs, read a bounded window, and mark entries reviewed.
- LLM-assisted consolidation can propose Markdown summaries and vault patches for review.
- Vault updates are summaries with links or references to the source window, not raw log dumps.
