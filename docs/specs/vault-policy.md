# Markdown Vault Writing Policy

The managed knowledge target is a Markdown vault. Obsidian is only one reader/editor for those files. The vault is for durable conclusions, not raw logs.

## Daily Notes

Write a daily-note section only when log review produces a result worth retaining:

- confirmed root cause;
- durable workaround;
- deployment or environment issue;
- recurring error pattern;
- handoff-worthy blocker.

## Product Notes

Update a product or feature note when the log finding changes durable knowledge about setup, behavior, debugging, or operational caveats.

## LLM-Assisted Updates

LLM output should be treated as a proposed patch, not a fact source.

Before applying Markdown to the vault, the workflow should:

- read the target note and nearby product index;
- map repository, source, component, and metadata to canonical notes;
- summarize only bounded event windows;
- include event IDs, time ranges, PRs, builds, commits, or source paths that support the summary;
- avoid writing uncertain conclusions without a qualifier or follow-up;
- require review for broad product-note changes, secrets, personal data, destructive edits, or unclear ownership.

Automatic writes are acceptable only for narrow daily-log summaries when the source window is bounded, redacted, and clearly connected to the active workstream.

Prefer writing immutable proposal files into a vault inbox over appending directly to a daily note. This lets multiple producers work concurrently without sharing a file. A single consolidator applies reviewed proposals using a temporary-file rename and an idempotency marker, then archives the proposal and acknowledges its evidence.

Canonical product links should come from a user-selected Markdown navigation file or explicit configuration. Never infer a product name merely from repository branding.

## Format

Daily entries should name:

- product or feature note;
- source system;
- time window;
- conclusion;
- validation or follow-up state.

Avoid:

- raw log dumps;
- long stack traces;
- one entry per event;
- storing secrets or personal data.
