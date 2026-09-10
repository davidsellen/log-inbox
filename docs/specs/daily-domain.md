# Daily Domain Contract

Status: M0 contract. This document fixes the terms and state boundaries used by the refocused Daily workflow. It does not describe the pre-refocus proposal-file runtime.

## Invariants

- A workspace has a generated identity that does not depend on its mount path, browser storage, or display name.
- A day is a workspace-local calendar date. The server, not the browser, resolves its UTC boundaries from the day's frozen IANA timezone.
- Evidence, proposal revisions, review decisions, and Apply operations are separate records. Changing one never silently changes another.
- Every event in an evidence snapshot is included, explicitly omitted, identified as duplicate/superseded by another event, or unresolved.
- Proposal revisions are immutable. Editing or regenerating creates a new revision and atomically changes the day's current-revision pointer.
- The model may describe evidence but cannot select the destination, authorize a canonical link, decide review state, or Apply Markdown.
- Only one writer implementation may target a workspace. A read-only preview of the new workflow may coexist with the legacy runtime before cutover.

## Records

### Workspace profile

| Field | Contract |
|---|---|
| `workspace_id` | Generated opaque ID; stable across mount-path changes |
| `status` | `pending_review`, `active`, or `disabled` |
| `root_binding` | Reviewed host-side identity used to detect workspace replacement |
| `timezone` | Valid IANA name used for new days and scheduler decisions |
| `daily_root` | Reviewed relative directory inside the workspace |
| `daily_pattern` | Validated supported date-token pattern |
| `template_path` | Optional reviewed relative Markdown template |
| `link_style` | `markdown` or `wikilink`; detected value remains an explicit saved choice |

Workspace changes use preview, review, then Save. Existing days keep their frozen interpretation.

### Day

The unique key is `(workspace_id, local_date)`. A day stores the effective timezone, resolved half-open UTC range, destination path, template revision, and managed-block identity used for that date. These values freeze when the first evidence snapshot or manual draft is created. Later settings changes affect only untouched days unless the owner explicitly reconciles a historical day.

Day status is projected from independent dimensions:

- Generation: `none`, `queued`, `running`, `ready`, or `failed`.
- Review: `unresolved`, `in_review`, `ready_to_apply`, `dismissed`, or `applied`.
- Freshness: `current` or `update_available`.

An empty calendar date is not a missed day. A missed day has eligible unresolved evidence and no current review decision.

### Evidence snapshot

An evidence snapshot is immutable and belongs to one day. It contains an ordered list of event IDs plus a digest of each redacted ingestion envelope. Event time determines day attribution; receipt time determines raw retention. A generation attempt always names one snapshot.

If any event in the current snapshot has expired, generation never replaces that revision from the surviving subset. An unchanged request returns the preserved revision, and manual-note-only changes may create a new structured revision against the original snapshot. New automated evidence requires restoration of the missing source evidence before regeneration, or an explicit event-by-event deferral bound to the preserved revision; no partial model summary is presented as a complete replacement.

Future-dated events outside the configured tolerance are quarantined. They remain inspectable but cannot enter automatic generation until explicitly resolved. Expired evidence cannot be reconstructed from a surviving summary.

Raw expiry clears only the snapshot's live-event reference. The immutable evidence ID, digest, ordering, and recorded disposition survive, so an already reviewed exact revision may still be applied when no newer evidence exists. The UI must disclose incomplete evidence; regeneration that would require expired raw content must refuse or use an explicit amendment path.

### Workstream and proposal revision

A proposal revision contains structured workstreams. A workstream has a stable revision-local ID, grouping identities, an optional reviewed canonical subject, adaptive factual fields, references, and the evidence IDs supporting each factual field.

Supported factual fields are Outcome, Decision, Trade-off, Validation, Blocker, and Follow-up. Empty or unsupported fields are absent, not filled with boilerplate. Manual notes are stored and rendered separately under My notes; model generation never rewrites their prose.

Revision origin is `generated`, `structured_edit`, `manual`, `regenerated`, or `advanced_markdown`. Advanced Markdown is a detached override: returning to structured generation or discarding it requires confirmation, and evidence decisions remain explicit records.

### Review decision

A review decision addresses one evidence digest in one snapshot. Its disposition is `include`, `omit`, `duplicate_of`, or `superseded_by`, with actor, time, and optional reason. Omission is reversible while the evidence exists. A later event version is unresolved even when an earlier version was omitted.

Editing rendered Markdown does not create review decisions. Applying is allowed only when every snapshot item has a disposition and every factual evidence reference validates against that snapshot.

A dismissal is an explicit reversible resolution of one exact proposal revision and content hash. It suppresses reminders without changing the revision, evidence, or Markdown. A later event makes the day Update available instead of inheriting the dismissal. Reopening restores review of the same revision and discloses any expired source evidence.

A late-evidence deferral is a separate reversible resolution of one event ID and digest against one current revision. It means “leave this event for a future reconciliation,” not Include, Omit, reviewed, or deleted. Active deferrals are excluded from that revision's freshness and Apply checks. They close automatically when a new revision becomes current, and later events never inherit them.

### Apply operation

An Apply operation records the exact approved proposal revision and hash, destination, expected managed-block revision, intended new block hash, and recovery material before touching Markdown.

States are `prepared`, `writing`, `written`, `finalized`, `failed`, and `reconciliation_required`. Retries use the same operation ID. On restart, the service compares the journal with the file and either finalizes an already completed write or requires reconciliation; it never appends a duplicate block.

## Date and Scheduling Semantics

- Browser APIs send `YYYY-MM-DD`; they do not send UTC boundaries.
- The server resolves `[local midnight, next local midnight)` using the frozen timezone, including 23- and 25-hour days.
- Ambiguous local scheduler times choose the first occurrence; nonexistent times advance to the first valid instant on that local date. The actual instant is recorded.
- Automatic generation targets the previous local day and defaults to 00:15. Startup and periodic reconciliation claim due work transactionally.
- Late evidence marks `update_available`. It never replaces an edited revision automatically.

## Authorization Boundary

Read-only health and the authenticated dashboard shell do not imply mutation access. Manual entry, generation, edit, omission, dismissal, settings changes, and Apply all require an authenticated session, CSRF validation, allowed host/origin, and their specific mutation scope. Initial Apply is dashboard-only and additionally requires the exact reviewed revision and destination.

## Cutover Boundary

The coordinated cutover is delivered. Before the Daily writer became the only runtime writer, the migration performed these reviewed steps:

1. Quiesce legacy per-task and daily workers.
2. Back up and migrate app data with an idempotent journal.
3. Review the workspace binding and destination dry-run.
4. Enable the authenticated new writer.
5. Remove legacy writer routes, browser filesystem state, proposal files, and split write mounts.

The ordinary runtime now contains no legacy routes, workers, browser filesystem state, proposal files, or split mounts. Remaining legacy database records and optional external source files are migration inputs only. A failed or interrupted cutover preserves them and its journal; no compatibility alias can route a legacy write through the Daily writer.
