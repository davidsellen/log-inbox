# Product Brief

Status: first usable refocused release; personal-pilot validation remains open. See the [roadmap](../roadmap.md) for delivered behavior and acceptance gates.

## Problem

Engineering outcomes, decisions, validations, failed attempts, and follow-ups are scattered across terminals, repositories, agent sessions, and manual notes. Reconstructing a useful daily record takes effort; dumping raw activity into a notes workspace creates noise rather than durable understanding.

The initial audience is an individual engineer already using coding agents and Markdown notes. The primary job is to answer: what changed, why, what was checked, and what remains?

## Desired Outcome

Provide a single-owner, self-hosted HTTP collector, dashboard, scheduler, MCP interface, and reviewed Markdown writer. Create one current structured daily candidate with immutable revisions; require explicit review before writing one managed block to the resolved daily note.

- **Daily:** generate, edit, inspect evidence, include or omit workstreams, preview the exact diff, and apply.
- **Knowledge:** optionally select existing Markdown context and maintain canonical mappings. A useful draft must work without any collections or mappings.
- **Settings:** configure workspace identity, dates, automation, model access, retention, security, and agent integration.

Keep manual notes separate from automated summaries. Report model failures visibly. Preserve user-owned content outside the managed block, detect managed-block conflicts, and recover interrupted Apply operations without duplicating content. Document the limits of protection against independently writing editors.

## Non-Goals

- General vault management, file browsing, note reorganization, or a Notion-style workspace.
- Product-note updates, decision-record creation, and feature-recap writes without a separate complete review/apply workflow.
- Automatic Apply, raw-log fallback masquerading as a ready summary, or silent overwrites of edited drafts.
- Browser filesystem write mode, per-task proposal staging, or operational proposal files inside the Markdown workspace after cutover.
- Public multi-tenant hosting, multiple active workspaces/models, full editor-plugin emulation, or broad agent write access in the initial delivery.
- MCP as ingestion, mandatory agent-specific producer clients, or productivity/surveillance metrics.

## First Usable Refocused Release

- Preserve the existing HTTP ingestion envelope and useful Rust behavior.
- Scope events, settings, mappings, decisions, and revisions to a stable workspace profile.
- Resolve dates on the server using the workspace timezone; retain event time and receipt time separately.
- Generate validated structured workstreams, with explicit evidence coverage and no automatic raw fallback.
- Support manual entries, structured review, reversible evidence-scoped omission, and visible failures without Knowledge setup.
- Authenticate sensitive access and mutations before enabling the new writable deployment.
- Show the resolved daily destination and block diff; Apply through one reviewed, recoverable writer.
- Cut over through an idempotent migration that preserves source data on failure and removes obsolete runtime paths only when the replacement is ready.

Scheduling, retention-aware late-event handling, bounded Knowledge context, and blind context comparison are implemented. Remote provider management and broader integrations remain gated by the roadmap's privacy, security, and pilot requirements.

## Success Criteria

Start with ten active engineering days. Measure review time, factual corrections, high-value evidence coverage, later recall, and configuration/maintenance effort. Data loss, unauthorized writes, silent draft overwrites, or silently discarded evidence block expansion. Commercial demand and willingness to pay remain hypotheses, not established outcomes.
