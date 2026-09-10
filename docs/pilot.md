# Personal Pilot Guide

The pilot decides whether Log Inbox reduces the work of maintaining an accurate daily engineering record. It is product validation, not productivity tracking. Record only the small amount of owner feedback needed to judge the workflow; do not copy raw logs or note contents into the worksheet.

## Before starting

1. Confirm the dashboard and collector health checks pass.
2. In Settings, review the mounted workspace, timezone, Daily destination example, preparation schedule, and retention policy. Save only settings you intend to activate.
3. Start without Knowledge if you want a baseline. Knowledge collections and mappings are optional and never choose the Daily destination.
4. Use real engineering days. An active day is one with work you expected to appear; empty calendar days do not count.

## Daily loop

For each active day:

1. Open Daily and select the date. The server, not the browser, computes that date's timezone window.
2. Add short **My notes** entries for useful work that was not captured automatically. They remain visibly separate and are not rewritten by the model.
3. Generate or review the current candidate. Check outcomes, decisions, trade-offs, validation, blockers, follow-up, evidence coverage, and canonical links.
4. Correct structured facts or explicitly omit irrelevant evidence. Regeneration never silently replaces an edited candidate.
5. Review the exact destination and managed-block diff, then Apply. Nothing writes Markdown before this confirmation.
6. If a prior day was missed, select it from Recent days and use the same loop. Scheduled preparation may create a review candidate, but never auto-Applies it.

## Ten-day worksheet

Keep this outside the Markdown workspace if you do not want pilot administration in the vault. One row per active day is enough.

| Active day | Correct without factual edits? | Review effort (low/medium/high) | Valuable evidence missing? | Operational problem or follow-up |
|---|---|---|---|---|
| YYYY-MM-DD | yes/no | low/medium/high | short category or none | short note or none |

Stop and treat the pilot as blocked if there is data loss, an unauthorized write, a silent overwrite, silently discarded evidence, or a need for database/proposal-folder repair. Ordinary wording edits or a model failure are findings, not reasons to hide the day.

## Knowledge comparison

On representative current, unedited candidates that contain matched Knowledge notes, choose **Compare Knowledge**. The two drafts use the same frozen evidence, model, and generation contract; their assignment stays hidden until you choose. Record which is more useful, which needs less editing, and the arm you want to continue with. The selected arm becomes a new immutable candidate and still requires the normal Apply review.

Use several different work types rather than repeating one repository/task shape. At the end, review whether Knowledge improved factual usefulness or reduced editing often enough to justify its configuration effort and latency. Add local FTS only if concrete missed-context cases show exact mappings and bounded retrieval are insufficient.

## Exit review

The M3 pilot passes after ten active days with no database repair, manual proposal-folder cleanup, data-loss issue, or authorization blocker. Summarize recurring corrections and operational problems before changing defaults.

The M4 context evaluation passes only when representative blind comparisons show improved usefulness or reduced editing with manageable latency and setup effort. A neutral or negative result is valid: Knowledge remains optional, and the comparison evidence should guide whether retrieval expands at all.
