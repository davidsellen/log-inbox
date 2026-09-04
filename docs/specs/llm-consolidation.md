# LLM Consolidation Workflow

LLM consolidation turns selected log windows into proposed Markdown for a vault. It is separate from ingestion and storage.

## Principles

- Raw logs are evidence, not vault content.
- The LLM summarizes bounded event groups, never open-ended streams.
- The LLM proposes Markdown patches; the agent applies them only after policy checks.
- Canonical vault links come from existing notes, aliases, and product indexes.
- Every summary should preserve enough evidence to re-open the source window.

## Flow

```text
1. Collect logs through HTTP ingest.
2. Group logs by source, correlation ID, time window, severity, or active workstream.
3. Read candidate vault context: daily note, product index, product notes, and aliases.
4. Ask the LLM for:
   - concise conclusion;
   - target note;
   - canonical links;
   - proposed Markdown;
   - evidence event IDs and source window;
   - confidence and open questions.
5. Validate the proposal against vault policy.
6. Apply a Markdown patch or leave it as a review suggestion.
7. Mark source events reviewed with the note reference.
```

## Proposal Inbox

The preferred service-orchestrated handoff is an append-only folder of Markdown proposals inside, or watched by, the vault:

1. Write one uniquely named file per bounded event group to `pending/`.
2. Write through a temporary file and atomically rename it into view.
3. Let the user's Markdown tools index proposals without editing the daily note.
4. Have one consolidator review pending proposals, patch canonical notes, acknowledge the evidence in SQLite, and delete handled proposals.
5. Mark evidence events reviewed only after the canonical patch succeeds.

The MCP service may run an automatic staging worker. It waits for a quiet period, groups unstaged events by `task_id`, `session_id`, or `fingerprint`, writes one proposal per group, and records proposal state without marking the evidence reviewed. Events without a correlation identifier remain isolated to avoid merging unrelated work.

Each model request has a finite HTTP timeout, and group failures are isolated so one malformed or slow event cannot block later proposals. All workers share a fair single-request gate for the configured model endpoint; time spent waiting for that gate does not consume the HTTP timeout. Failed groups remain unstaged and are eligible for a later retry.

The dashboard may request a whole-day consolidation across staged, reviewed, and unstaged events. The event IDs are frozen in a SQLite-backed job before an asynchronous worker calls the model. Jobs are idempotent per event snapshot, recover interrupted work after restart, expose durable status to the browser, and support cooperative cancellation of an in-flight model future. A completed job creates one previewable proposal that records the older pending proposals fully covered by its evidence. Those older proposals remain available until the consolidated proposal is applied, then they are removed. The complete editorial consolidation prompt is user-editable and stored in SQLite. It may shape presentation but cannot override evidence boundaries, redaction, allowed-link validation, or the output schema.

Concurrent producers never share a target file. The consolidator should use an advisory lock plus a content hash check before replacing a canonical note so an edit from any Markdown tool cannot be silently overwritten. A daily-note template or query may show pending proposals by `created_at` without appending links for each proposal.

Storage and model limits are intentionally separate. The store retains every accepted redacted byte. For a whole-day prompt, the service selects the latest terminal event for each correlated task, or the latest event when the task is still open, while retaining every original event ID as proposal evidence. The LLM receives bounded messages and metadata plus an explicit notice when content was omitted from that model call.

## Prompt Contract

The LLM request should include only the relevant bounded log slice and compact vault context.

Required input:

- task or review goal;
- source window and event IDs;
- normalized log events;
- candidate product/feature notes;
- current target note content when patching;
- known repository, branch, PR, work item, build, and commit metadata.

Required output:

```json
{
  "target_note": "Daily log Aug 31",
  "canonical_links": ["[[Customer Portal]]"],
  "summary_bullets": [
    "Confirmed the failed export requests were caused by missing CRM route metadata."
  ],
  "details": {
    "source": "windows/iis",
    "window": "2026-08-31T13:40:00Z/2026-08-31T13:50:00Z",
    "event_ids": ["evt_123", "evt_124"],
    "links": []
  },
  "confidence": "medium",
  "open_questions": []
}
```

## Markdown Rules

Daily-note output should usually be:

```md
## [[Product or Feature]] short workstream

- Outcome that matters.
- Cause or decision if known.
- Validation or follow-up state.

Details: source `name` · window `start/end` · events `evt_123`, `evt_124`
```

Product-note output should update durable setup, behavior, troubleshooting, or caveats. It should not add daily progress details.

## Review Gates

Require human or explicit agent review when:

- the target is a product note rather than a daily note;
- the conclusion changes documented product behavior;
- logs may contain secrets or personal data;
- confidence is low;
- the patch removes existing content;
- the source window is large or ambiguous.

## Implementation Options

### Agent-Orchestrated

The MCP server only exposes read/search/mark-reviewed tools. The agent calls the LLM and patches the vault. This is safest for the first version because the agent already has filesystem access and can inspect note context.

### Service-Orchestrated

The MCP server or a background worker calls an LLM API and stores proposed summaries in the proposal inbox. The agent later reviews and applies them. This is useful when recurring summaries should be prepared before an agent session starts.

### Direct Auto-Write

The service writes Markdown directly into the vault. This should be avoided for the first version except for tightly scoped daily-note append-only summaries with strict redaction and review policies.
