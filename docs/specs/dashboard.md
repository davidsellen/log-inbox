# Daily Dashboard

The service exposes one authenticated browser workflow at `/`. It is a review surface for one calendar day, not a vault explorer or generic note editor.

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

After setup, a compact recent-days rail keeps the Daily habit visible without becoming a calendar or vault browser. It always includes profile-local Today and adds only meaningful past dates, with one primary state such as Needs review, Update available, Review, or Applied. Evidence expiry is a secondary warning. Selecting a day opens the same single-day review surface.

Settings also exposes a reviewed, idempotent migration when legacy database or proposal-file state is detected. Migration mounts exist only in the Compose override and are never part of the ordinary runtime.

## Boundaries

- No browser directory picker, IndexedDB vault, file tree, or general vault browsing.
- No proposal inbox, per-task staging queue, or background legacy writer.
- No automatic Apply and no edits outside the owned Daily block.
- The current release exposes no MCP route. Structured MCP inspection and separately authorized Apply are future roadmap work.

The authoritative route and state contracts are in [Refocused Dashboard API](refocus-api.md) and [Daily domain](daily-domain.md).
