# Choosing generation models

Daily selects **Human summary (Semantic)** when a saved default model is available
and any required external consent is saved. It requests grouping, source highlight
selection and a final cited recap for daily review or meetings. Without an eligible
default, Daily selects **Source reports (no model)**, which retains original report
wording without a recap. The server API default and scheduled preparation remain
source-based. **Change model** selects a saved, tested provider/model for one
Semantic attempt without changing the saved default. A temporary choice stays with
that day while navigating or visiting Settings and is consumed on success. Failed
attempts retain their mode/model for retry; explicit mode choices take precedence.
The displayed mode always describes the next request.

The **Current draft** caption describes the displayed draft and its mode, independently of the next selection. Older drafts without provenance say that their generation model is unavailable. Source-based drafts, manual-only drafts and activity records do not use a model. Changing models never silently overwrites structured edits: confirm replacement first. Failures retain the last usable draft, which can still be read, copied, edited, and saved.

## Add or change a model

Open **Settings → Generation**. Choose and save the model default for future explicit Semantic preparation. Scheduled preparation always uses source-based mode without a model. To add models, expand **Manage connections and models**:

1. Select an existing connection or choose **Add a connection**. Give a new connection a name and an OpenAI-compatible API base URL, normally ending in `/v1`; an API key is optional.
2. Save the connection. Environment-configured endpoint and credentials are read-only, but additional models can still be tested and saved for that connection.
3. **Find available models**, or enter a model ID manually when discovery is unavailable.
4. **Test & save model** sends synthetic activity through the production structured-output request and validator. Only successful models become choices. The original environment model remains available for compatibility with existing installations.
5. Choose a saved model as the default, or return to Daily and choose it for one attempt.

A saved synthetic compatibility result proves the request/response contract on that probe, not accurate grouping or useful summaries on real activity. Semantic preparation may still fail; source-based preparation remains available without a model.

A discovered ID is not a compatibility guarantee. Tests support `max_tokens` with temperature and `max_completion_tokens` without temperature; the successful request profile is saved per model. Models must support the Daily structured JSON contract. Authentication, missing models, unsupported contracts, timeouts, and temporary service failures have separate recovery messages. Local models must be installed on their server; Log Inbox does not download them.

Requests originate from the Log Inbox service. In Docker, `localhost` means the app container, not the host browser. Plain HTTP is supported for loopback and the private Compose `ollama` hostname. Other endpoints require HTTPS and publicly routable addresses. Private LAN endpoints are not currently supported. Redirects and proxy environment settings are not followed; resolved destination addresses are validated and pinned for each captured attempt.

## External services and consent

Before a nonlocal connection receives real activity, explicitly allow that connection to receive activity messages and authorized link metadata, including source labels and canonical link labels/paths. Manual-note prose and reference-note excerpts remain local. Synthetic connection tests and model discovery do not contain your activity.

Saving an external default selects it only for explicit Semantic preparation; scheduled preparation uses no model. Changing the endpoint resets its consent and tested models. A pre-existing nonlocal environment configuration needs consent before Semantic preparation. No provider is silently substituted on failure. A running attempt keeps its captured endpoint, credentials, model and request profile; settings changes affect subsequent attempts.

## API-key storage

No-key connections and environment-provided API keys work without an encryption master key. UI-managed keys are encrypted in app storage using XChaCha20Poly1305 with random nonces and workspace/connection/endpoint-bound associated data. API responses expose only whether a key exists. An empty edit field preserves the key; **Remove stored API key** clears it explicitly.

To enable saving UI-managed keys, create a private key file outside the repository and Markdown workspace:

```sh
umask 077
openssl rand -hex 32 > /absolute/private/path/provider-master.key
```

Use an existing private directory and give the service user read access. Set `LOG_INBOX_LLM_MASTER_KEY_HOST_FILE` to that absolute host path in the deployment environment, then start:

```sh
docker compose -f docker-compose.yml -f docker-compose.providers.yml up --build -d
```

For a direct service launch, set `LOG_INBOX_LLM_MASTER_KEY_FILE` to a readable key file. It accepts 32 raw bytes or 64 hexadecimal characters, with surrounding whitespace allowed for hex. The optional Compose override mounts the key read-only at `/run/secrets/log_inbox_provider_master_key`; ordinary Compose startup is unchanged.

Back up app data and the master key separately and protect both. The key cannot be recovered from the database. Missing or incorrect keys block only affected provider operations, preserving existing drafts. Restore the matching key to recover access. To rotate a key, finish or cancel active attempts, replace the mounted key, restart, and explicitly re-enter each UI-managed API key; old encrypted values cannot be decrypted with the replacement. Test the connections before using Semantic preparation again. Do not leave old key backups with app-data copies.

The saved default must be changed before deleting its connection. Deleting a connection preserves historical model provenance. Additive schema migration v27 preserves existing content and environment configuration. Back up SQLite before upgrade; rollback requires restoring the pre-upgrade database and matching app version rather than downgrading a migrated database in place.

## Daily reading and saving

Today's normal preparation, review, scheduled, and update states stay out of **Needs attention**. Failed generation and interrupted saves remain visible. Historical-day reminders retain their existing rules, using the saved workspace timezone, and refresh as status or the local day changes.

Apply review shows the current and approved content without ownership wrapper comments. **Technical details** reveals the exact Markdown including those comments. Saved files retain the comments so Log Inbox can safely replace only its owned section; confirmation hashes, recovery and surrounding owner content are unchanged. Marker examples inside content are preserved.

## Implementation validation

Validated locally on 2026-10-15 in the working tree based on `013782f8af5fd5c8f9f95daa3cf5012158c55ef5`, branch `codex/generation-model-selection`:

| Scenario | Evidence | Result |
| --- | --- | --- |
| Model changes, frozen attempts, edits, migration, missing/wrong key recovery and retention | Workspace Rust suite: 187 passed, 2 opt-in checks ignored | Passed |
| Attention, Apply content/raw review, defaults, overrides, retries, reference adjustments and narrow screens | Complete Chromium/Firefox suite: 192 passed; 10 focused follow-up checks include expired-activity selection | Passed |
| Optional omissions, reviewed/repeated Apply, owner-content preservation, navigation/history and restart | Disposable deterministic Compose Daily smoke | Passed |
| Formatting and static checks | Rustfmt, client Prettier check, Clippy all targets with warnings denied, diff whitespace check | Passed |
| Independent review and usability | Complete working-change review plus fixes; narrow-screen keyboard/overflow checks and visual screenshot inspection | Passed |
| Real local/cloud model quality, latency and actual account credentials | No real provider contacted during validation | Unverified |

Browser and Rust test temp files were redirected into ignored workspace temp directories after the host `/tmp` quota filled. Compose used an isolated Buildx configuration because the host's default Buildx state was unreadable. These are local validation-environment workarounds, not deployment changes. The initial implementation stage published no PR and upgraded no running installation. The later authorized local deployment is recorded below.

## Local deployment verification — 2026-10-15

Deployed the reviewed working tree to the existing loopback Compose instance at `http://127.0.0.1:8788`. A verified online SQLite backup and rollback image tags were retained privately before upgrading schema 26 to 27. Existing revisions and manual notes matched the backup after migration; integrity and foreign-key checks passed.

Provisioned the separate private provider master key and mounted it using the provider override; local Compose defaults retain that override for future restarts. Verified authentication, installed-model discovery, the Daily selector, and a real synthetic generation using `granite3.3:2b`. A temporary fake credential verified encrypted save, secret-free API projection and deletion without any external request. An existing-note Apply preview verified marker-free readable content alongside exact marked Markdown; no Apply was confirmed and no Markdown note was written.

After restarting the dashboard, health, generation settings, credential-storage readiness and deployed HTML/JavaScript equality with the working tree all passed. The temporary connection was removed. Real cloud-provider credentials and cloud generation remain unverified. Deployment artifacts and the private report are kept under ignored `tmp/local-deploy/`; the encryption key remains outside the repository and app-data volume.

## Fast local testing and sign-in troubleshooting

Installed and registered `qwen2.5:0.5b` in the local Ollama instance on 2026-10-15. It passed the production synthetic contract check in 6.0 seconds cold and 1.7 seconds warm; these timings measure the small compatibility prompt, not a real day's consolidation. Granite remained the default at installation; the user subsequently selected Qwen as the saved default. Both are available through Daily → Change model or the Settings default selector. Model discovery and Test & save do not install models.

Interactive verification exposed a missing-Origin error in browser discovery. Browser discovery now uses authenticated POST, preserving CSRF/origin validation; the explicitly authenticated GET compatibility route remains available. Sign-in failures now appear inside the login panel instead of the hidden Daily area, and distinguish the dashboard owner secret from activity/model API keys. The configured owner secret was verified unchanged and valid. Use the value of `LOG_INBOX_OWNER_SECRET` in the local deployment configuration to sign in; never paste it into activity reports or chat.

## Preparation failure diagnosis — 2026-10-15

A real 44-event, 15-group day exposed a limitation of the initial one-event compatibility test: `qwen2.5:0.5b` could pass that test while omitting required validation evidence in real work. Sanitized provider telemetry confirmed a normal `stop` response rather than token truncation. The strict validator rejected the result and no revision or Markdown note was written. A Granite retry validated its first group but was cancelled because CPU-only latency was impractical.

Compatibility tests now include synthetic decision and validation lifecycle evidence. Both production prompt shapes explicitly list required nonempty fact arrays per group, derived from the same predicates as the unchanged strict validator. The generation contract version changed so cached candidates use the clearer prompt. Terminal errors distinguish truncated output, schema, lifecycle coverage, group identity, content, and response-envelope failures. Logs retain normalized finish reasons and numeric usage only; provider text is never included in these errors. Optional malformed telemetry is ignored. Truncated batches retain the existing bounded individual-group fallback; transport errors do not trigger that fallback.

The single-group request schema enforces required decision/validation arrays with `minItems: 1`; batch IDs are restricted to the supplied groups. Existing strict evidence validation remains authoritative. Prompt-contract upgrades invalidate live generation reuse, while an exact previous-contract configuration match preserves safe manual-note amendments to drafts with partially expired evidence.

Preparation-fix validation in the current working tree:

| Scenario | Evidence | Result |
| --- | --- | --- |
| Required arrays, bounded batch fallback, safe errors and malformed optional telemetry | Production-request regressions and workspace Rust suite: 198 passed, 2 opt-in checks ignored | Passed |
| Old v2 draft with one expired and one live source event | Manual-note amendment preserved snapshot and provenance without inference | Passed |
| Retry, failed preparation recovery, sign-in and generation controls | Chromium/Firefox focused suite: 28 passed | Passed |
| Static and formatting checks | Workspace Clippy with warnings denied, Rustfmt, client Prettier, whitespace check | Passed |

Installed `qwen2.5:1.5b` (986 MB) in the existing local Ollama volume. It passed the stronger synthetic contract in 7 seconds using the `max_tokens` profile, then successfully prepared the affected 2026-10-14 day: all 44 events across 15 workstreams validated, a new SQLite draft was saved in 416 seconds, and no Markdown Apply occurred. The 0.5B model remained inconsistent on repeated compatibility checks and remains available only as a small testing option. The verified 1.5B model is now the saved local default for subsequent manual and scheduled preparation.

Chromium confirmed the saved draft and model labels, the intact manual note, and absence of the preparation-failure message. The final disposable Compose workflow passed generation, review, repeated safe Apply, preserved owner content, restart persistence, navigation and history. Independent review found no remaining issues in the diagnostics/schema/cache/documentation follow-up. Those validation runs created no PR or remote deployment.

## Generation timing history

Settings → Generation → Generation timing history shows the latest 100 workspace attempts and downloads those rows as JSON. The existing SQLite attempt/provenance records retain earlier attempts across restart and raw-activity retention; no new schema is needed. Each record includes the captured model/profile, day, start/finish timestamps, elapsed milliseconds, outcome, workstream progress, reference mode, operation and a safe failure code. Legacy model identity and unfinished duration can be unknown. Prompts, note text, response text, credentials and raw error messages are excluded.

Elapsed time measures the whole attempt, including evidence/reference preparation, inference, internal retries and saving. Try again creates a separate attempt. Compatibility tests are excluded. Zero recorded groups must not be treated as a fast model benchmark: cached/manual operations and early failures can have no recorded inference progress. Workload sizes, reference mode and operation differ, and timing alone does not establish summary quality. The downloaded file contains the displayed recent window, not all older retained attempts.

Interrupted attempts can include time until service restart; compare completed successful runs separately from failures, cancellations and interrupted runs. The release build and Git publication include the timing-history follow-up.

Local verification found 55 retained attempts. The successful Qwen 1.5B run’s stored end-to-end duration is 413,094 ms (6m 53s); the earlier 416-second observation included polling delay. Chromium verified the real history and JSON download. Both browser projects passed the new history/export/error-recovery checks, and independent review found no blocking issues. Invalid, backward and missing clock values are tested as unknown durations.

Final timing-history delivery checks: `cargo build --release --workspace` passed; workspace tests passed (201 tests, 2 opt-in checks ignored); the four Chromium/Firefox history scenarios passed, including downloaded JSON contents. Clippy, Rustfmt, client formatting and whitespace checks passed. One timing-sensitive model-switch test timed out during simultaneous release/container compilation; the full suite passed after compilation finished, without production changes. The feature is deployed to the local instance.

## Compact model insights

The header’s **Dashboard** provides a small summary alongside the detailed Settings history. It shows up to five recent model setups from the latest 100 attempts, grouping by connection, model, request profile, configuration digest, operation and reference mode. Each setup shows completed timing sample count, median whole-attempt duration, workstream range and separate failure/timeout and cancellation/interruption counts. Unknown models and attempts without recorded workload are excluded from timing comparisons; interrupted time does not contribute to successful medians. A sample count or timing alone is not a quality score or a model ranking.

The other two Dashboard sections show preparation on/off and current activity, plus recent days needing review. Inactive scheduler history is explained without implying a live queue. Data refresh is explicit; failed reads are labelled unavailable or last known. The view uses existing native HTML/CSS/JavaScript and adds no packages or storage migration.
