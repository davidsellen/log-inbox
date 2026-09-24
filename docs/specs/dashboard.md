# Daily and Knowledge Dashboard

The service exposes one authenticated browser workflow at `/`, centred on Daily. Reference notes are optional setup reached from Settings; existing Knowledge deep links remain usable. It is not a vault explorer or generic note editor.

## Workflow

The compact global header contains the home link and a quiet Settings action. Healthy connectivity has no badge. Network failures and server errors show a shared connection warning with Retry connection; checking connectivity does not reload the page or discard edits. Existing day-load and operation-specific recovery actions remain available.

Truly empty dates use one compact, date-aware state instead of empty notes, draft and evidence cards. Past days say nothing was recorded (not that no work occurred), today explains automatic activity, and future dates allow adding a note ahead of time. Date navigation and Search history remain available. Notes or arriving activity restore the normal workflow; generation failures, saved revisions and Apply recovery must never be hidden by this empty state. Evidence controls appear only when the selected date has events, and a no-match message appears only for a search. Service-wide intake and source details live in Settings → General, not beside a selected day's evidence.

1. Select a calendar date. The server resolves the day in the saved IANA timezone and returns only that interval.
2. Add optional owner-authored notes. They are saved immediately in SQLite, remain separate under **My notes**, and are never rewritten by the model.
3. Choose Create draft from the day's bounded automated evidence. The day itself shows generation state, elapsed time and the configured timeout, with Cancel or Retry as appropriate.
4. Optionally edit Outcome, Decision, Trade-off, Validation, Blocker, and Follow-up facts or omit evidence. Untouched evidence is included by default; no item-by-item decision is required. Pending omissions are saved automatically before the exact Apply preview. Structured edits create an immutable revision in SQLite.
5. Read or copy the draft; copying does not approve it or write Markdown. Edit when necessary.
6. Choose Review & save, inspect the exact destination and managed-block replacement, then Save to note. This is the only action that writes Markdown.

The screen states whether an action is read-only, saves app state, or writes Markdown. Late evidence is shown as an update; it never silently replaces an edited candidate.

Daily uses one centered reading column: a date heading and compact toolbar, then notes, draft, activity and a help disclosure. The brand returns to Daily through the existing guarded router; no lone tab row remains. Add note belongs to My notes, while generation, recovery, editing, copy and reviewed saving belong to the draft. Before a draft exists, only its explanation and Destination disclosure are shown, not an empty save panel. With a draft, Preview file contains the destination and exact Markdown. Reference-context details are collapsed; invalid-reference and interrupted-Apply recovery remain visible outside disclosures.

When preparation fails without a current draft, **Use activity record** is an opt-in alternative. It replaces the empty draft area with **Activity record · not AI-summarized**, showing collapsed task groups with counts and expandable original messages. My notes stays separate. Editing, omission, Copy, and reviewed Apply use the existing controls; no evidence decision is mandatory. Previous AI failure details are collapsed inside the record. No model or Knowledge request is made, and existing drafts are never replaced. **Create AI summary** asks for replacement confirmation.

Generation is server-owned and survives browser navigation. Daily polls active work and exposes failures beside the draft instead of requiring a trip to Settings. Another active day is identified explicitly; duplicate generation is disabled. Sources, exact Markdown and help remain available through disclosure controls. Activity received reports collector receipts, not a promise of continuous producer connectivity.

Automated evidence opens in a bounded scrollable panel, including when preparation fails or no draft exists. Search matches message and source; Previous/Next moves focus through matching items and shows the current position. Filtering never changes inclusion. Generation status is grouped with the draft, with Browse activity, Cancel, and Retry actions as appropriate; it does not float over other recovery controls. Generated draft sections are bounded separately so long drafts do not push evidence out of reach. Activity receipts and connected sources appear with activity.

## Settings

Settings is a URL-addressable full page with three sections: General, Markdown destination, and Preparation & retention. Desktop uses native section links; small screens use a labelled section selector. Only one section is visible at a time. Section changes preserve edits and replace the current history entry; Back to Daily restores the selected date, scroll position and unsaved Daily draft without refreshing the page. Leaving with unsaved settings asks once for confirmation. Escape does not dismiss a page.

Each form has its own explicit save, unsaved indicator and local feedback. Saving one section never marks another section's edits as saved. Optional migration details, mounted workspace information and recent preparation runs use disclosure controls. Reference notes remain optional setup, with return links to Settings and Daily.

General contains optional reference-note setup and maintenance, not a recent-day count setting. Saves are disabled for invalid or unchanged values, except first-time preparation-policy activation and workspace setup. Destination still requires a matching preview. Completed migration appears under Maintenance; pending migration remains expanded.

First run requires a reviewed workspace profile: timezone, relative Daily root, supported filename pattern, optional existing template, and link style. Preview validates and displays an example without saving. Save persists the profile but creates no folders or notes.

After the workspace profile exists, Preparation & retention exposes both policies together under a separate explicit save. Until that policy is saved, displayed defaults are inactive. Enabling preparation schedules review candidates after a day ends and shows recent run outcomes; it never writes Markdown. Saving also activates the disclosed raw-evidence, audit, and Apply-recovery retention periods even when preparation remains disabled.

Dismiss removes the exact visible candidate from reminders without writing or deleting anything. Reopen restores that same revision and warns when raw source evidence has expired. A dismissed day must be reopened before regeneration. Evidence that arrives after a preserved revision is labeled separately; **Leave for later** explicitly defers that exact event so the older complete revision can still be reviewed and applied, and **Reopen** reverses the choice.

Settings also exposes a reviewed, idempotent migration when legacy database or proposal-file state is detected. Migration mounts exist only in the Compose override and are never part of the ordinary runtime.

## History search and day navigation

Previous/next, the native date picker, Today and Search history are the complete Daily navigation surface. The recent-day list and its client module have been removed. Today uses the profile-local date and keeps unsaved edits when already on that day; changing dates retains the discard guard. Browser Back/Forward and returning from Settings preserve the existing navigation behavior. The backend overview still supplies profile-local Today, active generation and intake status; existing preference APIs remain compatible.

Search history opens a read-only modal, not another tab. Submit a literal phrase, identifier or error string to search all retained non-deleted manual notes, automated messages/sources and readable current drafts. Applied drafts are searchable while their app content is retained. Internal metadata, superseded revisions, reference-note collections and saved Markdown files are excluded; the overview lookback does not limit search. Empty results therefore do not prove work never happened.

Results are grouped newest-day-first, labelled by source and show highlighted excerpts. More results continues up to 20 matching days / 200 items per response, including continuation within a large day. A result opens the exact item, expands its section and highlights it without changing evidence decisions; activity beyond the initial 500-item day preview is fetched separately. Expired or changed targets produce a readable notice and preserve the return path.

Back to results and browser Back restore the query, loaded results, position and focus. Search inputs never trigger unsaved-edit warnings; genuinely unsaved day edits still do. Escape closes the modal. Query/results are session-memory state, not persistent browser storage or address-bar parameters. Search remains available without a working model. Experimental Ask and safe Markdown-file indexing are roadmap items, not shipped controls.

## Reference notes

Reference-note administration retains the small review queue for durable names found in recent retained evidence. Product, project, repository, application, service, module, work-item, and pull-request names can be linked to an existing canonical note or ignored. Source names, branches, task/session IDs, messages, and other transient diagnostics are not shown as linking work. An empty setup is normal: Daily works without reference notes.

**Names to review** is the primary surface. Choosing Link opens a bounded title/path search within enabled source collections; it never opens a file browser or returns note bodies. Choosing Ignore only removes that identity from the review list, and **Review again** reverses it. **Saved links** are grouped by canonical note and expose explicit change, pause/enable, and remove actions. Imported advanced rules remain visible but cannot be silently simplified by the basic editor.

Changes to links and ignored names are saved immediately in SQLite after the explicit action. They affect only new or regenerated Daily candidates; they never rewrite the current candidate or Markdown. Existing reviewed candidates therefore stay predictable.

The Daily sidebar shows **Context used** for every candidate. Used notes are grouped under the candidate workstream; when a local model received bounded source text, the exact frozen excerpt is available under **Excerpt sent to the model**. Link-only revisions say explicitly that no note text was used. For bounded-context revisions, the owner can include or exclude each note for the next generation. These choices remain an unsaved draft until **Regenerate with adjustments** creates a new immutable candidate and context snapshot; they never change the visible revision, source note, grouping, or canonical link in place. Normal regeneration preserves the current frozen exclusions.

A small status distinguishes immutable frozen context from the current Knowledge setup. Ordinary setup or content changes are advisory; a removed or excluded source used by the revision is marked unavailable and blocks Apply until regeneration. Resolver, excerpt count, and snapshot ID stay under Technical details. Adjustment requests are bound to the exact visible revision so another session cannot silently apply stale choices.

For an eligible unedited candidate, **Compare without Knowledge** creates two private shadow drafts from the same frozen evidence, manual-note IDs, active model, and generation contract. Draft A/B identity stays hidden until the owner records which is more useful, which would take less editing, and which to continue with. Starting or closing the comparison changes no candidate or Markdown. Saving one decision reveals the assignment and atomically promotes only the selected validated arm as a new immutable revision.

## Source collections

Source collections are secondary setup under Knowledge. They define small, named sets of folders used to constrain canonical-note resolution, bounded local-model excerpts, and note search. Daily remains useful with no collections, but names cannot be manually linked until at least one collection provides safe note choices. Each collection shows only its name, purpose, included roots, exclusions, and active state; it does not expose a file tree or note browser.

Creating or editing a collection is review-first. The owner enters one to eight relative roots and optional exclusions, then reviews matched/eligible counts and any missing or oversized sources before Save is enabled. Missing reviewed roots are not created. Saving changes only SQLite configuration and never creates, moves, edits, or deletes Markdown. Pause controls whether future candidates may use the collection; Remove deletes only the definition.

Folder discovery is a bounded typeahead inside collection path fields. It returns safe relative directory paths only—never filenames or Markdown content. Knowledge loads independently from Daily, restores mutation access from a valid session after reload without persisting the CSRF token in browser storage, and has an addressable `?view=knowledge` URL with browser-history navigation.

## Boundaries

- No browser directory picker, IndexedDB vault, file tree, or general vault browsing.
- No proposal inbox, per-task staging queue, or background legacy writer.
- No automatic Apply and no edits outside the owned Daily block.
- The collector exposes only the ingestion-oriented `log_activity` MCP tool. Structured MCP inspection and separately authorized Apply remain future roadmap work.

The authoritative route and state contracts are in [Refocused Dashboard API](refocus-api.md) and [Daily domain](daily-domain.md).
