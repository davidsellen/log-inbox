import { expect, test } from "@playwright/test";
import { readFile } from "node:fs/promises";

const dailyHtml = await readFile(
  new URL("../../crates/mcp-server/assets/daily.html", import.meta.url),
  "utf8"
);

function dailyResponse(date = "2026-09-08", { origin = "generated", freshness = "current", applyStatus = null } = {}) {
  return {
    workspace_id: "workspace_fixture",
    local_date: date,
    timezone: "Europe/Stockholm",
    start_utc: `${date}T00:00:00Z`,
    end_utc: `${date}T23:59:59Z`,
    destination_path: `Work Log/2026/Sep/Daily log ${date}.md`,
    day: { generation_status: "ready", review_status: "in_review" },
    automated_evidence: {
      events: [{ id: "evt_1", source: "codex/fedora", timestamp: `${date}T09:00:00Z`, message: "Validated the Daily workflow." }],
      returned_count: 1,
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
    current_snapshot_evidence: [{ event_id: "evt_1", position: 0, event_digest: "digest", disposition: null }],
    candidate_freshness: freshness,
    preview_markdown: "### My notes\n\n- Discussed the trade-off with the team.\n\n### Automated activity\n\n#### Daily workflow\n\n- **Outcome:** Built a predictable Daily review.",
    apply_status: applyStatus
  };
}

async function mockDaily(page, { dailyStatus = 200, origin = "generated", freshness = "current", workspaceProfile = undefined, applyStatus = null, migrationItems = [] } = {}) {
  const requests = [];
  let loggedIn = false;
  let currentApplyStatus = applyStatus;
  let migrationCompleted = false;
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
    if (url.pathname === "/api/v2/migration/cutover" && request.method() === "GET") return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ operation_id: "refocus_fixture", report_digest: "f".repeat(64), workspace_id: "workspace_fixture", root_binding: "binding_fixture", ready: true, cutover_status: migrationCompleted ? "completed" : "not_started", completed_operation_id: migrationCompleted ? "refocus_fixture" : null, items: migrationCompleted ? [] : migrationItems, blockers: [], warnings: migrationItems.length ? ["Malformed proposal will be preserved."] : [] }) });
    if (url.pathname === "/api/v2/migration/cutover" && request.method() === "POST") {
      migrationCompleted = true;
      return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ operation_id: "refocus_fixture", status: "completed", backup_file: "backup.sqlite3", imported_items: migrationItems.length, cleaned_files: 0, preserved_items: migrationItems.length }) });
    }
    if (url.pathname.endsWith("/apply-preview") && request.method() === "GET") return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ workspace_id: "workspace_fixture", local_date: "2026-09-08", destination_path: "Work Log/2026/Sep/Daily log 2026-09-08.md", revision_id: "revision_1", revision_content_hash: "a".repeat(64), block_id: "day_fixture", will_create_note: false, template_used: null, previous_block: "<!-- log-inbox:daily:day_fixture:begin -->\nOld\n<!-- log-inbox:daily:day_fixture:end -->", next_block: "<!-- log-inbox:daily:day_fixture:begin -->\nReviewed Daily\n<!-- log-inbox:daily:day_fixture:end -->", expected_old_block_hash: "b".repeat(64), intended_new_block_hash: "c".repeat(64), expected_target_exists: true, expected_original_content_hash: "e".repeat(64), updated_content_hash: "d".repeat(64) }) });
    if (url.pathname.endsWith("/retry") && request.method() === "POST") {
      currentApplyStatus = { ...currentApplyStatus, state: "finalized", failure_reason: null, can_retry: false };
      return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ operation: currentApplyStatus }) });
    }
    if (url.pathname.endsWith("/apply") && request.method() === "POST") return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ operation: { state: "finalized" }, destination_path: request.postDataJSON().destination_path, idempotent: false }) });
    const match = url.pathname.match(/^\/api\/v2\/daily\/(\d{4}-\d{2}-\d{2})$/);
    if (match && request.method() === "GET") {
      if (dailyStatus !== 200) return route.fulfill({ status: dailyStatus, contentType: "application/json", body: JSON.stringify({ error: "Daily fixture unavailable" }) });
      return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(dailyResponse(match[1], { origin, freshness, applyStatus: currentApplyStatus })) });
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
  await page.getByRole("button", { name: "Preview" }).click();
  await expect(page.locator("#settings-preview")).toContainText("Nothing has been saved or created");
  await page.getByRole("button", { name: "Save settings" }).click();

  await expect(page.getByRole("heading", { name: "Daily settings" })).toHaveCount(0);
  await expect.poll(() => requests.find(request => request.path === "/api/v2/settings/workspace" && request.method === "PUT")?.body).toMatchObject({
    settings: { daily_root: "Journal", daily_pattern: "{year}/{date}.md" },
    preview_digest: "preview_fixture",
    expected_profile_id: null
  });
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
