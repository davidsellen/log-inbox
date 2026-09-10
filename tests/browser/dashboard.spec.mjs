import { expect, test } from "@playwright/test";
import { readFile } from "node:fs/promises";

const dailyHtml = await readFile(
  new URL("../../crates/mcp-server/assets/daily.html", import.meta.url),
  "utf8"
);

function dailyResponse(date = "2026-09-08", { origin = "generated", freshness = "current", applyStatus = null, dismissed = false, lateEvidence = false, lateDeferred = false } = {}) {
  const events = [{ id: "evt_1", source: "codex/fedora", timestamp: `${date}T09:00:00Z`, message: "Validated the Daily workflow." }];
  if (lateEvidence) events.push({ id: "evt_late", source: "codex/fedora", timestamp: `${date}T10:00:00Z`, message: "Late deployment evidence." });
  return {
    workspace_id: "workspace_fixture",
    local_date: date,
    timezone: "Europe/Stockholm",
    start_utc: `${date}T00:00:00Z`,
    end_utc: `${date}T23:59:59Z`,
    destination_path: `Work Log/2026/Sep/Daily log ${date}.md`,
    day: { generation_status: "ready", review_status: dismissed ? "dismissed" : "in_review" },
    automated_evidence: {
      events,
      returned_count: events.length,
      truncated: false,
      limit: 500
    },
    manual_entries: [{ id: "manual_1", text: "Discussed the trade-off with the team.", references: [], created_at: `${date}T08:00:00Z`, updated_at: `${date}T08:00:00Z` }],
    current_revision: {
      id: "revision_1",
      revision_number: 1,
      snapshot_id: "snapshot_1",
      origin,
      content: {
        schema_version: 1,
        manual_entry_ids: ["manual_1"],
        open_questions: [],
        workstreams: [{
          id: "source:codex%2Ffedora|task:test",
          title: "Daily workflow",
          evidence_event_ids: ["evt_1"],
          canonical_links: [],
          outcome: [{ text: "Built a predictable Daily review.", evidence_event_ids: ["evt_1"] }],
          decision: [], trade_off: [], validation: [], blocker: [], follow_up: []
        }]
      }
    },
    current_snapshot: { id: "snapshot_1", event_ids: ["evt_1"] },
    current_snapshot_evidence: [{ event_id: "evt_1", available: true, position: 0, event_digest: "digest", disposition: null }],
    active_late_evidence_deferrals: lateDeferred ? [{ workspace_id: "workspace_fixture", local_date: date, revision_id: "revision_1", event_id: "evt_late", available: true, event_digest: "late_digest", deferred_at: `${date}T11:00:00Z`, reopened_at: null }] : [],
    candidate_freshness: lateEvidence ? (lateDeferred ? "current" : "update_available") : freshness,
    new_evidence_count: lateEvidence ? (lateDeferred ? 0 : 1) : (freshness === "update_available" ? 1 : 0),
    expired_evidence_count: 0,
    evidence_complete: true,
    preview_markdown: "### My notes\n\n- Discussed the trade-off with the team.\n\n### Automated activity\n\n#### Daily workflow\n\n- **Outcome:** Built a predictable Daily review.",
    apply_status: applyStatus
  };
}

function knowledgeCollection(overrides = {}) {
  return { id: "knowledge_fixture", workspace_id: "workspace_fixture", label: "Product context", purpose: "Product behavior and decisions", roots: ["Products/Alpha"], exclusions: ["Products/Alpha/Archive"], enabled: true, revision_digest: "a".repeat(64), created_at: "2026-09-01T00:00:00Z", updated_at: "2026-09-09T12:00:00Z", ...overrides };
}

async function mockDaily(page, { dailyStatus = 200, origin = "generated", freshness = "current", workspaceProfile = undefined, applyStatus = null, migrationItems = [], lateEvidence = false, knowledgeCollections = [], knowledgeStatus = 200 } = {}) {
  const requests = [];
  let loggedIn = false;
  let currentApplyStatus = applyStatus;
  let migrationCompleted = false;
  let dismissed = false;
  let automationSaved = false;
  let automationSettings = { workspace_id: "workspace_fixture", enabled: false, generation_time: "00:15", catch_up_days: 7, raw_retention_days: 30, audit_retention_days: 365, recovery_retention_days: 30, updated_at: "1970-01-01T00:00:00Z" };
  let lateDeferred = false;
  let collections = structuredClone(knowledgeCollections);
  let activeProfile = workspaceProfile === undefined ? { id: "workspace_fixture", status: "active", root_binding: "binding_fixture", timezone: "Europe/Stockholm", daily_root: "Work Log", daily_pattern: "{year}/{month_name}/Daily log {month_name} {day}.md", template_path: null, link_style: "markdown", created_at: "2026-09-01T00:00:00Z", updated_at: "2026-09-01T00:00:00Z" } : workspaceProfile;
  await page.route("http://daily.log-inbox.test/**", async route => {
    const request = route.request();
    const url = new URL(request.url());
    requests.push({ path: url.pathname, method: request.method(), body: request.postData() ? request.postDataJSON() : null });
    if (url.pathname === "/") return route.fulfill({ status: 200, contentType: "text/html", body: dailyHtml });
    if (url.pathname === "/api/v2/auth/login" && request.method() === "POST") {
      loggedIn = true;
      return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ csrf_token: "csrf_fixture" }) });
    }
    if (url.pathname === "/api/v2/auth/session") return route.fulfill({ status: loggedIn ? 200 : 401, contentType: "application/json", body: JSON.stringify(loggedIn ? { authenticated: true } : { error: "Sign in required" }) });
    if (url.pathname === "/api/v2/settings/workspace" && request.method() === "GET") return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ workspace_path: "/workspace", active_profile: activeProfile, binding_matches: Boolean(activeProfile) }) });
    if (url.pathname === "/api/v2/settings/workspace/preview" && request.method() === "POST") return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ settings: request.postDataJSON(), destination_example: "Journal/2026-09-08.md", preview_digest: "preview_fixture", changes_saved: false }) });
    if (url.pathname === "/api/v2/settings/workspace" && request.method() === "PUT") {
      const saved = request.postDataJSON();
      activeProfile = { id: activeProfile?.id || "workspace_fixture", status: "active", root_binding: "binding_fixture", ...saved.settings, created_at: activeProfile?.created_at || "2026-09-01T00:00:00Z", updated_at: "2026-09-09T12:00:00Z" };
      return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ active_profile: activeProfile, destination_example: "Journal/2026-09-08.md", binding_matches: true, changes_saved: true }) });
    }
    if (url.pathname === "/api/v2/settings/automation" && request.method() === "GET") return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ settings: automationSettings, saved: automationSaved, recent_runs: [], writes_markdown_automatically: false }) });
    if (url.pathname === "/api/v2/settings/automation" && request.method() === "PUT") {
      automationSaved = true;
      automationSettings = { workspace_id: "workspace_fixture", ...request.postDataJSON(), updated_at: "2026-09-09T13:00:00Z" };
      delete automationSettings.expected_updated_at;
      return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ settings: automationSettings, saved: true, writes_markdown_automatically: false }) });
    }
    if (url.pathname === "/api/v2/knowledge/collections" && request.method() === "GET") {
      if (knowledgeStatus !== 200) return route.fulfill({ status: knowledgeStatus, contentType: "application/json", body: JSON.stringify({ error: "Knowledge fixture unavailable" }) });
      return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ workspace_id: "workspace_fixture", collections, count: collections.length, limit: 8 }) });
    }
    if (url.pathname === "/api/v2/knowledge/folders" && request.method() === "GET") return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ folders: ["Products/Alpha", "Products/Beta"], limit: 20 }) });
    if (url.pathname === "/api/v2/knowledge/collections/preview" && request.method() === "POST") {
      const collection = request.postDataJSON();
      return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ collection: { ...collection, label: collection.label.trim(), purpose: collection.purpose.trim(), roots: [...collection.roots].sort(), exclusions: [...collection.exclusions].sort() }, matched_note_count: 3, eligible_note_count: 2, oversized_note_count: 1, total_bytes: 2048, missing_roots: [], preview_digest: "preview_knowledge_fixture", changes_saved: false }) });
    }
    if (url.pathname === "/api/v2/knowledge/collections" && request.method() === "POST") {
      const input = request.postDataJSON();
      const saved = knowledgeCollection({ id: "knowledge_created", ...input.collection, updated_at: "2026-09-10T10:00:00Z" });
      collections.push(saved);
      return route.fulfill({ status: 201, contentType: "application/json", body: JSON.stringify({ collection: saved, changes_saved: true }) });
    }
    const collectionMatch = url.pathname.match(/^\/api\/v2\/knowledge\/collections\/([^/]+)$/);
    if (collectionMatch && request.method() === "PUT") {
      const input = request.postDataJSON();
      const index = collections.findIndex(item => item.id === collectionMatch[1]);
      const saved = { ...collections[index], ...input.collection, updated_at: "2026-09-10T11:00:00Z" };
      collections[index] = saved;
      return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ collection: saved, changes_saved: true }) });
    }
    if (collectionMatch && request.method() === "DELETE") {
      collections = collections.filter(item => item.id !== collectionMatch[1]);
      return route.fulfill({ status: 204 });
    }
    if (url.pathname === "/api/v2/migration/cutover" && request.method() === "GET") return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ operation_id: "refocus_fixture", report_digest: "f".repeat(64), workspace_id: "workspace_fixture", root_binding: "binding_fixture", ready: true, cutover_status: migrationCompleted ? "completed" : "not_started", completed_operation_id: migrationCompleted ? "refocus_fixture" : null, items: migrationCompleted ? [] : migrationItems, blockers: [], warnings: migrationItems.length ? ["Malformed proposal will be preserved."] : [] }) });
    if (url.pathname === "/api/v2/migration/cutover" && request.method() === "POST") {
      migrationCompleted = true;
      return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ operation_id: "refocus_fixture", status: "completed", backup_file: "backup.sqlite3", imported_items: migrationItems.length, cleaned_files: 0, preserved_items: migrationItems.length }) });
    }
    if (url.pathname === "/api/v2/daily/overview" && request.method() === "GET") return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ today: "2026-09-08", timezone: "Europe/Stockholm", missed_count: 1, days: [{ local_date: "2026-09-08", status: freshness === "update_available" ? "update_available" : "in_review", event_count: 1, manual_entry_count: 1, revision_number: 1, new_evidence_count: freshness === "update_available" ? 1 : 0, manual_entries_changed: false, expired_evidence_count: 0, schedule_state: null, schedule_error: null }, { local_date: "2026-09-07", status: "missed", event_count: 2, manual_entry_count: 0, revision_number: null, new_evidence_count: 0, manual_entries_changed: false, expired_evidence_count: 0, schedule_state: null, schedule_error: null }] }) });
    if (url.pathname.endsWith("/apply-preview") && request.method() === "GET") return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ workspace_id: "workspace_fixture", local_date: "2026-09-08", destination_path: "Work Log/2026/Sep/Daily log 2026-09-08.md", revision_id: "revision_1", revision_content_hash: "a".repeat(64), block_id: "day_fixture", will_create_note: false, template_used: null, previous_block: "<!-- log-inbox:daily:day_fixture:begin -->\nOld\n<!-- log-inbox:daily:day_fixture:end -->", next_block: "<!-- log-inbox:daily:day_fixture:begin -->\nReviewed Daily\n<!-- log-inbox:daily:day_fixture:end -->", expected_old_block_hash: "b".repeat(64), intended_new_block_hash: "c".repeat(64), expected_target_exists: true, expected_original_content_hash: "e".repeat(64), updated_content_hash: "d".repeat(64) }) });
    if (url.pathname.endsWith("/retry") && request.method() === "POST") {
      currentApplyStatus = { ...currentApplyStatus, state: "finalized", failure_reason: null, can_retry: false };
      return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ operation: currentApplyStatus }) });
    }
    if (url.pathname.endsWith("/apply") && request.method() === "POST") return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ operation: { state: "finalized" }, destination_path: request.postDataJSON().destination_path, idempotent: false }) });
    const match = url.pathname.match(/^\/api\/v2\/daily\/(\d{4}-\d{2}-\d{2})$/);
    if (match && request.method() === "GET") {
      if (dailyStatus !== 200) return route.fulfill({ status: dailyStatus, contentType: "application/json", body: JSON.stringify({ error: "Daily fixture unavailable" }) });
      return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(dailyResponse(match[1], { origin, freshness, applyStatus: currentApplyStatus, dismissed, lateEvidence, lateDeferred })) });
    }
    if (url.pathname.endsWith("/late-evidence/evt_late") && ["POST", "DELETE"].includes(request.method())) {
      lateDeferred = request.method() === "POST";
      return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ deferral: { revision_id: "revision_1", event_id: "evt_late", reopened_at: lateDeferred ? null : "2026-09-08T12:00:00Z" } }) });
    }
    if (url.pathname.endsWith("/dismiss") && ["POST", "DELETE"].includes(request.method())) {
      dismissed = request.method() === "POST";
      return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ dismissal: { revision_id: "revision_1" }, expired_evidence_count: 0, evidence_complete: true }) });
    }
    if (url.pathname.endsWith("/candidate") && request.method() === "PUT") return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ id: "revision_2" }) });
    return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({}) });
  });
  return requests;
}

async function openDaily(page, date = "2026-09-08") {
  await page.goto(`http://daily.log-inbox.test/?date=${date}`);
  await page.getByLabel("Owner secret").fill("fixture owner secret");
  await page.getByRole("button", { name: "Sign in" }).click();
  await expect(page.locator("#daily-app")).toBeVisible();
}

test("refocused Daily shows one date, destination, notes, and exact preview", async ({ page }) => {
  const requests = await mockDaily(page);
  await openDaily(page);

  await expect(page.getByRole("heading", { name: /Tuesday, September 8, 2026/i })).toBeVisible();
  await expect(page.getByText(/Markdown destination: Work Log\/2026\/Sep/)).toBeVisible();
  await expect(page.getByText("Discussed the trade-off with the team.", { exact: true })).toBeVisible();
  await expect(page.getByLabel("Workstream 1 title")).toHaveValue("Daily workflow");
  await expect(page.locator("#preview")).toContainText("Built a predictable Daily review.");
  await expect(page.getByRole("button", { name: /Mon, Sep 7 Needs review/i })).toBeVisible();
  await expect(page.locator("#missed-summary")).toHaveText("1 missed");

  await page.getByLabel("Daily log date").fill("2026-09-07");
  await page.getByLabel("Daily log date").dispatchEvent("change");
  await expect.poll(() => requests.some(request => request.path === "/api/v2/daily/2026-09-07")).toBe(true);
});

test("refocused settings explains and commits reviewed legacy migration", async ({ page }) => {
  const requests = await mockDaily(page, { migrationItems: [{ kind: "proposal_file", source_identity: "proposal_file:broken.md", status: "unparseable" }] });
  page.on("dialog", dialog => dialog.accept());
  await openDaily(page);

  await page.getByRole("button", { name: "Settings" }).click();
  await expect(page.getByRole("heading", { name: "Bring forward older Log Inbox data" })).toBeVisible();
  await expect(page.getByText(/creates and verifies a database backup first/i)).toBeVisible();
  await page.getByRole("button", { name: "Complete migration" }).click();

  await expect.poll(() => requests.find(request => request.path === "/api/v2/migration/cutover" && request.method === "POST")?.body).toEqual({ operation_id: "refocus_fixture", report_digest: "f".repeat(64) });
  await expect(page.locator("#migration-summary")).toContainText("Migration completed");
});

test("refocused Daily saves structured edits against the visible revision", async ({ page }) => {
  const requests = await mockDaily(page);
  await openDaily(page);

  const title = page.getByLabel("Workstream 1 title");
  await title.fill("Daily review experience");
  await page.getByRole("button", { name: "Save edits" }).click();

  await expect.poll(() => requests.find(request => request.path.endsWith("/candidate") && request.method === "PUT")?.body).toMatchObject({
    expected_revision_id: "revision_1",
    content: { workstreams: [{ title: "Daily review experience" }] }
  });
});

test("refocused Daily reviews the exact managed block before Apply", async ({ page }) => {
  const requests = await mockDaily(page);
  await openDaily(page);

  await page.getByRole("button", { name: "Review Apply" }).click();
  await expect(page.getByRole("heading", { name: "Apply to Markdown" })).toBeVisible();
  await expect(page.locator("#apply-target")).toContainText("Work Log/2026/Sep");
  await expect(page.locator("#apply-before")).toContainText("Old");
  await expect(page.locator("#apply-after")).toContainText("Reviewed Daily");
  await page.getByRole("button", { name: "Confirm Apply" }).click();

  await expect.poll(() => requests.find(request => request.path.endsWith("/apply") && request.method === "POST")?.body).toMatchObject({
    expected_revision_id: "revision_1",
    destination_path: "Work Log/2026/Sep/Daily log 2026-09-08.md",
    expected_old_block_hash: "b".repeat(64),
    intended_new_block_hash: "c".repeat(64),
    expected_target_exists: true,
    expected_original_content_hash: "e".repeat(64),
    expected_updated_content_hash: "d".repeat(64)
  });
  await expect(page.getByRole("status")).toContainText("Applied to Work Log/2026/Sep");
});

test("refocused Daily keeps CSRF authority in memory only", async ({ page }) => {
  await mockDaily(page);
  await openDaily(page);

  await page.reload();
  await expect(page.getByRole("button", { name: "Unlock changes" })).toBeVisible();
  await expect(page.getByRole("button", { name: "+ Add note" })).toBeDisabled();
  await expect(page.getByRole("status")).toContainText("Reviewing read-only after reload");

  await page.getByRole("button", { name: "Unlock changes" }).click();
  await expect(page.getByLabel("Owner secret")).toBeFocused();
});

test("refocused Daily previews and saves first-run destination settings", async ({ page }) => {
  const requests = await mockDaily(page, { workspaceProfile: null });
  await openDaily(page);

  await expect(page.getByRole("heading", { name: "Daily settings" })).toBeVisible();
  await page.getByLabel("Daily folder").fill("Journal");
  await page.getByLabel("File pattern").fill("{year}/{date}.md");
  await page.getByRole("button", { name: "Preview destination" }).click();
  await expect(page.locator("#settings-preview")).toContainText("Nothing has been saved or created");
  await page.getByRole("button", { name: "Save destination" }).click();

  await expect(page.getByRole("heading", { name: "Daily settings" })).toHaveCount(0);
  await expect.poll(() => requests.find(request => request.path === "/api/v2/settings/workspace" && request.method === "PUT")?.body).toMatchObject({
    settings: { daily_root: "Journal", daily_pattern: "{year}/{date}.md" },
    preview_digest: "preview_fixture",
    expected_profile_id: null
  });
});

test("refocused settings make scheduling and retention consent explicit", async ({ page }) => {
  const requests = await mockDaily(page);
  await openDaily(page);

  await page.getByRole("button", { name: "Settings" }).click();
  await expect(page.getByText(/never writes Markdown/i)).toBeVisible();
  await expect(page.locator("#automation-status")).toContainText("inactive until you choose Save");
  await page.getByLabel("Prepare candidates automatically").check();
  await page.getByLabel("Preparation time").fill("06:45");
  await page.getByLabel("Catch-up window (days)").fill("14");
  await page.getByLabel("Raw evidence retention (days)").fill("60");
  await page.getByRole("button", { name: "Save preparation & retention" }).click();

  await expect.poll(() => requests.find(request => request.path === "/api/v2/settings/automation" && request.method === "PUT")?.body).toEqual({
    enabled: true,
    generation_time: "06:45",
    catch_up_days: 14,
    raw_retention_days: 60,
    audit_retention_days: 365,
    recovery_retention_days: 30,
    expected_updated_at: null
  });
  await expect(page.locator("#automation-status")).toContainText("Automatic preparation is on");
  await expect(page.locator("#automation-status")).toContainText("no Markdown is written automatically");
});

test("refocused Daily exposes server failures", async ({ page }) => {
  await mockDaily(page, { dailyStatus: 503 });
  await openDaily(page);

  await expect(page.getByRole("status")).toContainText("Daily fixture unavailable");
});

test("refocused Daily keeps interrupted Apply visible and retryable", async ({ page }) => {
  const requests = await mockDaily(page, { applyStatus: { id: "apply_fixture", state: "reconciliation_required", failure_reason: "The destination changed during Apply.", can_retry: true } });
  await openDaily(page);

  await expect(page.getByRole("heading", { name: "Apply needs attention" })).toBeVisible();
  await expect(page.getByText("The destination changed during Apply.")).toBeVisible();
  await expect(page.getByRole("button", { name: "Review Apply" })).toBeDisabled();
  await page.getByRole("button", { name: "Verify and retry" }).click();

  await expect.poll(() => requests.some(request => request.path.endsWith("/apply/apply_fixture/retry"))).toBe(true);
  await expect(page.getByRole("status")).toContainText("Apply verified and finalized");
  await expect(page.getByRole("heading", { name: "Apply needs attention" })).toHaveCount(0);
});

test("refocused Daily confirms before replacing edits with new evidence", async ({ page }) => {
  const requests = await mockDaily(page, { origin: "structured_edit", freshness: "update_available" });
  await openDaily(page);

  page.once("dialog", dialog => dialog.dismiss());
  await page.getByRole("button", { name: "Regenerate" }).click();
  expect(requests.filter(request => request.path.endsWith("/generate"))).toHaveLength(0);

  page.once("dialog", dialog => dialog.accept());
  await page.getByRole("button", { name: "Regenerate" }).click();
  await expect.poll(() => requests.find(request => request.path.endsWith("/generate"))?.body).toEqual({
    replace_edited: true
  });
});

test("refocused Daily dismisses and reopens the exact visible candidate", async ({ page }) => {
  const requests = await mockDaily(page);
  await openDaily(page);

  page.once("dialog", dialog => dialog.accept());
  await page.getByRole("button", { name: "Dismiss", exact: true }).click();
  await expect(page.getByRole("button", { name: "Reopen", exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Regenerate" })).toBeDisabled();
  await page.getByRole("button", { name: "Reopen", exact: true }).click();
  await expect(page.getByRole("button", { name: "Dismiss", exact: true })).toBeVisible();

  const dismissRequests = requests.filter(request => request.path.endsWith("/dismiss"));
  expect(dismissRequests.map(request => request.method)).toEqual(["POST", "DELETE"]);
  expect(dismissRequests[0].body).toEqual({ expected_revision_id: "revision_1" });
});

test("refocused Daily explicitly leaves late evidence for later and reopens it", async ({ page }) => {
  const requests = await mockDaily(page, { lateEvidence: true });
  await openDaily(page);

  await page.getByText("Review source events and include or omit them").click();
  await expect(page.getByText("Late deployment evidence.")).toBeVisible();
  await page.getByRole("button", { name: "Leave for later" }).click();
  await expect(page.getByText("Left for later — not part of this revision")).toBeVisible();
  await expect(page.locator("#evidence-count")).toContainText("1 left for later");
  await expect(page.getByRole("status")).toContainText("preserved revision can now be reviewed and applied");
  await page.getByRole("button", { name: "Reopen", exact: true }).click();
  await expect(page.getByRole("button", { name: "Leave for later" })).toBeVisible();

  const lateRequests = requests.filter(request => request.path.endsWith("/late-evidence/evt_late"));
  expect(lateRequests.map(request => request.method)).toEqual(["POST", "DELETE"]);
  expect(lateRequests[0].body).toEqual({ expected_revision_id: "revision_1" });
});

test("Knowledge navigation is lazy, keyboard accessible, and URL-addressable", async ({ page }) => {
  const requests = await mockDaily(page, { knowledgeCollections: [knowledgeCollection()] });
  await openDaily(page);

  expect(requests.filter(request => request.path === "/api/v2/knowledge/collections")).toHaveLength(0);
  const knowledge = page.getByRole("tab", { name: "Knowledge" });
  await knowledge.focus();
  await page.keyboard.press("Enter");

  await expect(knowledge).toHaveAttribute("aria-selected", "true");
  await expect(page).toHaveURL(/view=knowledge/);
  await expect(page.getByRole("heading", { name: "Knowledge collections" })).toBeVisible();
  await expect(page.getByRole("heading", { name: "Product context" })).toBeVisible();
  await expect(page.getByText("Products/Alpha", { exact: true })).toBeVisible();
  await expect(page.getByRole("button", { name: "Structure", exact: true })).toHaveCount(0);
  await expect(page.getByRole("button", { name: "Ignored", exact: true })).toHaveCount(0);
  expect(requests.filter(request => request.path === "/api/v2/knowledge/collections")).toHaveLength(1);
  await page.getByRole("tab", { name: "Daily" }).click();
  await page.goBack();
  await expect(knowledge).toHaveAttribute("aria-selected", "true");
  await expect(page.getByRole("heading", { name: "Product context" })).toBeVisible();
});

test("Knowledge creates a collection only after reviewing the bounded definition", async ({ page }) => {
  const requests = await mockDaily(page);
  await openDaily(page);
  await page.getByRole("tab", { name: "Knowledge" }).click();

  await expect(page.getByRole("heading", { name: "No collections yet" })).toBeVisible();
  await page.getByRole("button", { name: "New collection" }).first().click();
  await page.getByLabel("Collection name").fill(" Product context ");
  await page.getByLabel("What should this context help with?").fill(" Product behavior and decisions ");
  await page.locator("#collection-root-input").fill("Products/Alpha");
  await page.locator("#add-root").click();
  await page.locator("#collection-exclusion-input").fill("Products/Alpha/Archive");
  await page.locator("#add-exclusion").click();
  await page.getByRole("button", { name: "Review collection" }).click();

  await expect(page.locator("#knowledge-preview")).toContainText("3 Markdown notes match; 2 eligible");
  await expect(page.locator("#knowledge-preview")).toContainText("Nothing has been saved");
  await page.getByRole("button", { name: "Create collection" }).click();
  await expect(page.getByRole("heading", { name: "Product context" })).toBeVisible();

  const preview = requests.find(request => request.path.endsWith("/collections/preview"));
  expect(preview.body).toMatchObject({ label: " Product context ", roots: ["Products/Alpha"], exclusions: ["Products/Alpha/Archive"], enabled: true });
  const create = requests.find(request => request.path === "/api/v2/knowledge/collections" && request.method === "POST");
  expect(create.body).toEqual({ collection: { label: "Product context", purpose: "Product behavior and decisions", roots: ["Products/Alpha"], exclusions: ["Products/Alpha/Archive"], enabled: true }, preview_digest: "preview_knowledge_fixture", expected_updated_at: null });
});

test("Knowledge edits, pauses, and removes definitions without implying file changes", async ({ page }) => {
  const requests = await mockDaily(page, { knowledgeCollections: [knowledgeCollection()] });
  page.on("dialog", dialog => dialog.accept());
  await openDaily(page);
  await page.getByRole("tab", { name: "Knowledge" }).click();

  await page.getByRole("button", { name: "Edit" }).click();
  await page.getByLabel("What should this context help with?").fill("Current product decisions");
  await page.getByRole("button", { name: "Review collection" }).click();
  await page.getByRole("button", { name: "Save changes" }).click();
  await expect(page.getByText("Current product decisions", { exact: true })).toBeVisible();

  await page.getByRole("button", { name: "Pause" }).click();
  await expect(page.getByText("Paused", { exact: true })).toBeVisible();
  await expect(page.getByRole("status")).toContainText("paused and will not be used by Daily");
  await page.getByRole("button", { name: "Remove" }).click();
  await expect(page.getByRole("heading", { name: "No collections yet" })).toBeVisible();
  await expect(page.getByRole("status")).toContainText("Markdown files were not changed");

  const updates = requests.filter(request => request.path.endsWith("/knowledge_fixture") && request.method === "PUT");
  expect(updates[0].body.expected_updated_at).toBe("2026-09-09T12:00:00Z");
  expect(updates[1].body.collection.enabled).toBe(false);
  const removal = requests.find(request => request.path.endsWith("/knowledge_fixture") && request.method === "DELETE");
  expect(removal.body).toEqual({ expected_updated_at: "2026-09-10T11:00:00Z" });
});

test("Knowledge stays useful but read-only after a reload loses CSRF authority", async ({ page }) => {
  await mockDaily(page, { knowledgeCollections: [knowledgeCollection()] });
  await openDaily(page);
  await page.getByRole("tab", { name: "Knowledge" }).click();
  await page.reload();

  await expect(page.getByRole("heading", { name: "Product context" })).toBeVisible();
  await expect(page.getByRole("button", { name: "New collection" }).first()).toBeDisabled();
  await expect(page.getByRole("button", { name: "Edit" })).toBeDisabled();
  await expect(page.getByRole("status")).toContainText("Reviewing collections read-only");
  await expect(page.getByRole("button", { name: "Unlock changes" })).toBeVisible();
});

test("Knowledge exposes collection load failures with a retry", async ({ page }) => {
  await mockDaily(page, { knowledgeStatus: 503 });
  await openDaily(page);
  await page.getByRole("tab", { name: "Knowledge" }).click();

  await expect(page.getByText("Collections could not be loaded: Knowledge fixture unavailable")).toBeVisible();
  await expect(page.getByRole("button", { name: "Retry" })).toBeVisible();
});
