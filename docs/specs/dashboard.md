# Daily and Knowledge Dashboard

The service exposes one authenticated browser workflow at `/`, with a Daily review surface and optional Knowledge collection settings. It is not a vault explorer or generic note editor.

## Workflow

1. Select a calendar date. The server resolves the day in the saved IANA timezone and returns only that interval.
2. Add optional owner-authored notes. They are saved immediately in SQLite, remain separate under **My notes**, and are never rewritten by the model.
3. Generate a structured candidate from the day's bounded automated evidence.
4. Edit Outcome, Decision, Trade-off, Validation, Blocker, and Follow-up facts or explicitly include/omit evidence. Each save creates an immutable revision in SQLite.
5. Review the exact destination and managed-block replacement.
6. Confirm Apply. This is the only action that writes Markdown.

The screen states whether an action is read-only, saves app state, or writes Markdown. Late evidence is shown as an update; it never silently replaces an edited candidate.

## Settings

First run requires a reviewed workspace profile: timezone, relative Daily root, supported filename pattern, optional existing template, and link style. Preview validates and displays an example without saving. Save persists the profile but creates no folders or notes.

After the workspace profile exists, the same dialog exposes candidate preparation and retention as a separate explicit save. Until that policy is saved, displayed defaults are inactive. Enabling preparation schedules review candidates after a day ends and shows recent run outcomes; it never writes Markdown. Saving also activates the disclosed raw-evidence, audit, and Apply-recovery retention periods even when preparation remains disabled.

After setup, a compact recent-days rail keeps the Daily habit visible without becoming a calendar or vault browser. It always includes profile-local Today and adds only meaningful past dates, with one primary state such as Needs review, Update available, Review, or Applied. Evidence expiry is a secondary warning. Selecting a day opens the same single-day review surface.

Dismiss removes the exact visible candidate from reminders without writing or deleting anything. Reopen restores that same revision and warns when raw source evidence has expired. A dismissed day must be reopened before regeneration. Evidence that arrives after a preserved revision is labeled separately; **Leave for later** explicitly defers that exact event so the older complete revision can still be reviewed and applied, and **Reopen** reverses the choice.

Settings also exposes a reviewed, idempotent migration when legacy database or proposal-file state is detected. Migration mounts exist only in the Compose override and are never part of the ordinary runtime.

## Knowledge links

The Knowledge tab is a small review queue for durable names found in recent retained evidence. Product, project, repository, application, service, module, work-item, and pull-request names can be linked to an existing canonical note or ignored. Source names, branches, task/session IDs, messages, and other transient diagnostics are not shown as linking work.

**Names to review** is the primary surface. Choosing Link opens a bounded title/path search within enabled source collections; it never opens a file browser or returns note bodies. Choosing Ignore only removes that identity from the review list, and **Review again** reverses it. **Saved links** are grouped by canonical note and expose explicit change, pause/enable, and remove actions. Imported advanced rules remain visible but cannot be silently simplified by the basic editor.

Changes to links and ignored names are saved immediately in SQLite after the explicit action. They affect only new or regenerated Daily candidates; they never rewrite the current candidate or Markdown. Existing reviewed candidates therefore stay predictable.

The Daily sidebar shows **Context used** for every candidate. Used notes are grouped under the candidate workstream; when a local model received bounded source text, the exact frozen excerpt is available under **Excerpt sent to the model**. Link-only revisions say explicitly that no note text was used. For bounded-context revisions, the owner can include or exclude each note for the next generation. These choices remain an unsaved draft until **Regenerate with adjustments** creates a new immutable candidate and context snapshot; they never change the visible revision, source note, grouping, or canonical link in place. Normal regeneration preserves the current frozen exclusions.

A small status distinguishes immutable frozen context from the current Knowledge setup. Ordinary setup or content changes are advisory; a removed or excluded source used by the revision is marked unavailable and blocks Apply until regeneration. Resolver, excerpt count, and snapshot ID stay under Technical details. Adjustment requests are bound to the exact visible revision so another session cannot silently apply stale choices.

For an eligible unedited candidate, **Compare without Knowledge** creates two private shadow drafts from the same frozen evidence, manual-note IDs, active model, and generation contract. Draft A/B identity stays hidden until the owner records which is more useful, which would take less editing, and which to continue with. Starting or closing the comparison changes no candidate or Markdown. Saving one decision reveals the assignment and atomically promotes only the selected validated arm as a new immutable revision.

## Source collections

Source collections are secondary setup under Knowledge. They define small, named sets of folders used to constrain canonical-note resolution, bounded local-model excerpts, and note search. Daily remains useful with no collections, but names cannot be manually linked until at least one collection provides safe note choices. Each collection shows only its name, purpose, included roots, exclusions, and active state; it does not expose a file tree or note browser.

Creating or editing a collection is review-first. The owner enters one to eight relative roots and optional exclusions, then reviews matched/eligible counts and any missing or oversized sources before Save is enabled. Missing reviewed roots are not created. Saving changes only SQLite configuration and never creates, moves, edits, or deletes Markdown. Pause controls whether future candidates may use the collection; Remove deletes only the definition.

Folder discovery is a bounded typeahead inside collection path fields. It returns safe relative directory paths only—never filenames or Markdown content. Knowledge loads independently from Daily, remains visible but read-only after a reload until changes are unlocked, and has an addressable `?view=knowledge` URL with browser-history navigation.

## Boundaries

- No browser directory picker, IndexedDB vault, file tree, or general vault browsing.
- No proposal inbox, per-task staging queue, or background legacy writer.
- No automatic Apply and no edits outside the owned Daily block.
- The current release exposes no MCP route. Structured MCP inspection and separately authorized Apply are future roadmap work.

The authoritative route and state contracts are in [Refocused Dashboard API](refocus-api.md) and [Daily domain](daily-domain.md).
