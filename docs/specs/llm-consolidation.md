# Daily Model Generation

The model turns one server-resolved day of bounded automated evidence into a structured review candidate. It never chooses the destination, writes Markdown, resolves review decisions, or rewrites owner-authored notes.

## Active flow

1. The server resolves the selected calendar day and reads its eligible events.
2. Rust groups events using namespaced repository, work-item, pull-request, task, session, and source identities.
3. The service freezes event IDs and content digests in an immutable evidence snapshot.
4. The configured OpenAI-compatible endpoint receives the bounded automated projection.
5. Rust validates the response schema, group membership, evidence coverage, factual evidence IDs, canonical links, and text limits.
6. A valid result becomes an immutable proposal revision in SQLite. Invalid, incomplete, or unavailable model output is a visible failure.
7. The server renders the reviewed structure deterministically for preview. Only explicit dashboard Apply invokes the Markdown writer.

Manual-only days need no model. Mixed days send only automated evidence to the provider and keep manual prose verbatim in a separate section. There is no raw-log or free-form Markdown fallback.

## Structured output

Each supplied workstream may contain evidence-backed Outcome, Decision, Trade-off, Validation, Blocker, and Follow-up facts plus open questions. Every factual item names evidence from that workstream. Unknown fields, invented or missing evidence, cross-group evidence, placeholder titles, unsupported facts, and empty factual workstreams are rejected.

Model-controlled text is escaped. Active links come only from server-owned validated context. Provider requests, responses, retries, concurrency, and total input are bounded independently from raw evidence retention.

Optional Knowledge retrieval and UI-managed model connections are M4 and M5 roadmap work; a useful Daily candidate does not depend on them.
