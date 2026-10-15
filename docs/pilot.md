# Personal Pilot Guide

The pilot decides whether Log Inbox reduces the work of maintaining an accurate daily engineering record. It is product validation, not productivity tracking. Record only the small amount of owner feedback needed to judge the workflow; do not copy raw logs or note contents into the worksheet.

## Before starting

1. Confirm the dashboard and collector health checks pass.
2. In Settings, review the mounted workspace, timezone, Daily destination example, preparation schedule, and retention policy. Save only settings you intend to activate.
3. Start without reference context for a baseline. Reference folders and mappings are optional and never choose the Daily destination. Do not treat setup effort as value by itself.
4. Use real engineering days. An active day is one with work you expected to appear; empty calendar days do not count.

## Daily loop

Navigation stays inside the page: Settings → Manage reference notes opens setup, and Back to Daily returns to the same draft. Browser Back/Forward restores the date and view; Back also closes an open dialog. Switching views keeps Daily edits in memory. Leaving an edited day or closing an edited form asks before discarding; refresh is not needed to navigate.

If a day cannot load, its editing controls remain unavailable and Retry loading day retries that date. The previous day's draft is never shown as the newly selected day's content. Unsaved edits are not durable across a full page reload.

For each active day:

1. Open Daily and select the date. The server, not the browser, computes that date's timezone window.
2. Add short **My notes** entries for useful work that was not captured automatically. They remain visibly separate and are not rewritten by the model.
3. Generate or review the current candidate. Check outcomes, decisions, trade-offs, validation, blockers, follow-up, evidence coverage, and canonical links.
4. Correct structured facts or explicitly omit irrelevant evidence. Regeneration never silently replaces an edited candidate.
5. Review the exact destination and managed-block diff, then Apply. Nothing writes Markdown before this confirmation.
6. If a prior day was missed, use the date picker, Previous/Next, or Search history and use the same loop. Scheduled preparation may create a review candidate, but never auto-Applies it.

## Ten-day worksheet

Keep this outside the Markdown workspace if you do not want pilot administration in the vault. One row per active day is enough.

| Active day | Correct without factual edits? | Review effort (low/medium/high) | Valuable evidence missing? | Operational problem or follow-up |
|---|---|---|---|---|
| YYYY-MM-DD | yes/no | low/medium/high | short category or none | short note or none |

Stop and treat the pilot as blocked if there is data loss, an unauthorized write, a silent overwrite, silently discarded evidence, or a need for database/proposal-folder repair. Ordinary wording edits or a model failure are findings, not reasons to hide the day.

## Reference-context comparison

On representative current, unedited candidates that contain matched reference notes, choose **Compare Knowledge**. The two drafts use the same frozen evidence, model, and generation contract; their assignment stays hidden until you choose. Record which is more useful, which needs less editing, and the arm you want to continue with. The selected arm becomes a new immutable candidate and still requires the normal Apply review.

Use several different work types rather than repeating one repository/task shape. At the end, review whether reference context improved factual usefulness or reduced editing often enough to justify its configuration effort and latency. For the future shared-brain slice, test one product and two modules end to end: a human-maintained note, an approved bounded agent read, a normal activity report, and a Daily draft that discloses what context it used. Add local FTS only if concrete missed-context cases show exact mappings and bounded retrieval are insufficient.

## Exit review

### Daily quality regression sample

For the October 2026 quality delivery, include these cases in the real-model review.
Use synthetic equivalents when testing another provider; do not export private
historical evidence merely to run a benchmark.

| Evidence pattern | Required reading of the resulting draft |
|---|---|
| One work item has numeric, string, URL and object IDs across planning, implementation and deployment sessions | One coherent workstream; distinct work items remain distinct even on the same branch |
| Investigation completes; configuration defect confirmed, probable cause, successful login still pending | Diagnosis is complete, resolution remains unverified |
| Remediation plan accepted; no implementation or tests performed | Accepted plan only; no implementation or passed-test claim |
| Product note describes CRM ownership; event reports a Forms authentication investigation | Only the investigation is reported as daily work; product background supplies terminology only |
| Earlier tests were not run; later narrow checks pass | Each validation keeps its actual scope; no inferred full acceptance |
| Earlier blocker followed by a later event that does not resolve it | The blocker remains visible; terminal task status alone does not close it |
| Two facts have different sources and one source is omitted during review | Only the fact with no remaining included support disappears |

Record unsupported accomplishments, lost pending states, duplicate bullets, mistaken
groups, review effort and elapsed generation time for both reference-context arms.
Per-fact event citations prove membership and make inspection possible; they do not
prove semantic entailment. A deterministic schema test passing is not evidence that
a model preserves these meanings. Keep the real-model and owner-evaluation gates
pending until those results are actually observed.

The M3 pilot passes after ten active days with no database repair, manual proposal-folder cleanup, data-loss issue, or authorization blocker. Summarize recurring corrections and operational problems before changing defaults.

The M4 context evaluation passes only when representative blind comparisons show improved usefulness or reduced editing with manageable latency and setup effort. A neutral or negative result is valid: Knowledge remains optional, and the comparison evidence should guide whether retrieval expands at all.
