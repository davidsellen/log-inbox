# Reading the dashboard client

Start with `daily.js`: its imports show the feature boundaries, its wiring block near the end connects them, and the last five calls bind events and boot the page. These are native browser modules, not a bundle or framework. Nothing is installed in production through npm.

| File | Responsibility |
| --- | --- |
| `daily.html` | Page and dialog structure; no inline event handlers |
| `daily.css` | Layout and presentation |
| `daily.js` | Session/API access, selected day, draft/evidence editing, generation, Apply and polling |
| `daily-navigation.js` | URL/history, view changes, dialog lifecycle and discard guards |
| `daily-references.js` | Reference collections, links, search and their form state |
| `daily-settings.js` | Destination, scheduling, retention and migration forms |
| `daily-helpers.js` | Stateless DOM, date and label helpers |

## Follow a click

Each feature has a `bindEvents` function (`bindDailyEvents` for Daily). Find the control ID there, then follow its named handler. Handlers perform requests through the supplied `api` function, update their state and call rendering functions. Renderers build DOM and attach actions; they do not start network requests merely by rendering.

For example: `#save-candidate` → `saveCandidate` → PUT the revision → `loadDay` → `render`. `#reference-settings` → `selectView` → guarded navigation → `loadKnowledge` when needed.

## State and boundaries

Daily owns the shared session and selected-day state. Reference notes and Settings each keep their own private form state inside their creation function. Their arguments list cross-feature dependencies explicitly. There are no application functions on `window` and no circular module imports.

Navigation receives `unsavedDayChanges` from Daily and extra dialog values from reference notes. It must not inspect draft fields or reach into another feature's private state. `hasPendingChanges` also covers an open editor; `unsavedDayChanges` checks actual content differences for discard warnings. They intentionally answer different questions.

`dayLoadVersion` rejects stale loads and polling responses. Keep these guards, CSRF handling, revision checks, and Apply confirmation intact during cleanup. Switching views preserves in-memory edits, not durable drafts across reloads.

## Local checks

Run `npm run format:client`, then `npm run check:client` and `npm run test:browser`. Prettier is development-only; CI checks formatting. Browser tests exercise both Chromium and Firefox.

Rust embeds each asset with `include_str!` and serves only the explicit allowlist in `dashboard_asset`. A new module needs an allowlist entry and a browser fixture asset entry. No general-purpose filesystem serving is needed.
