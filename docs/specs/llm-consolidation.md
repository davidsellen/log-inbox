# Daily Model Generation

The model turns one server-resolved day of bounded automated evidence into a structured review candidate. It never chooses the destination, writes Markdown, resolves review decisions, or rewrites owner-authored notes.

## Active flow

1. The server resolves the selected calendar day and reads its eligible events.
2. Rust groups events using namespaced repository, work-item, pull-request, task, session, and source identities.
3. The service freezes event IDs and content digests in an immutable evidence snapshot.
4. When Knowledge collections or mappings are enabled, the server resolves reviewed mappings and unique exact title, alias, or typed-reference matches. Conflicting matches produce no link. Repository-wide mappings may authorize a link but cannot merge distinct work items; only reviewed work-item/PR relationships may alias groups.
5. The configured OpenAI-compatible endpoint receives the bounded automated projection and server-authorized canonical links. Note text is not retrieved or sent in the current exact-link stage.
6. Rust validates the response schema, group membership, evidence coverage, factual evidence IDs, canonical links, and text limits.
7. A valid result becomes an immutable proposal revision bound to its evidence and optional Knowledge snapshots in SQLite. Invalid, incomplete, or unavailable model output is a visible failure.
8. The server renders the reviewed structure deterministically for preview. Only explicit dashboard Apply invokes the Markdown writer.

Manual-only days need no model. Mixed days send only automated evidence to the provider and keep manual prose verbatim in a separate section. There is no raw-log or free-form Markdown fallback.

## Structured output

Each supplied workstream may contain evidence-backed Outcome, Decision, Trade-off, Validation, Blocker, and Follow-up facts plus open questions. Every factual item names evidence from that workstream. Unknown fields, invented or missing evidence, cross-group evidence, placeholder titles, unsupported facts, and empty factual workstreams are rejected.

Model-controlled text is escaped. Active links come only from server-owned validated context. Provider requests, responses, retries, concurrency, and total input are bounded independently from raw evidence retention.

Optional Knowledge text retrieval and UI-managed model connections remain M4 and M5 roadmap work; a useful Daily candidate does not depend on them. The active exact-link stage reads bounded note metadata only. It freezes a compact resolution digest, the notes actually linked, matching reasons, group aliases, and link authorization instead of retaining the complete catalog in a proposal snapshot.

## Knowledge source safety

Before Knowledge context is enabled, Markdown sources are read only through the inspected workspace capability and within the reviewed collection bounds. Frontmatter parsing accepts bounded titles, aliases, and allowlisted typed references; malformed metadata excludes the note from context instead of being treated as trusted configuration. Titles fall back from frontmatter to the first top-level heading and then the filename.

Log Inbox removes every structurally valid Daily managed block from a source before deriving usable content or a context digest. Marker examples inside fenced code remain ordinary text. Missing, nested, mismatched, or out-of-order markers—and unterminated fences—fail closed so generated summaries can never reinforce themselves as source evidence.
