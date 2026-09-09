import { expect, test } from "@playwright/test";
import { readFile } from "node:fs/promises";

const dashboardHtml = await readFile(
  new URL("../../crates/mcp-server/assets/dashboard.html", import.meta.url),
  "utf8"
);
const dailyHtml = await readFile(
  new URL("../../crates/mcp-server/assets/daily.html", import.meta.url),
  "utf8"
);

const preferences = {
  ingest_url: "http://127.0.0.1:8787/v1/logs",
  agent_name: "codex",
  source_prefix: "codex",
  default_host: "test-host",
  extra_instructions: "",
  daily_consolidation_prompt: ""
};

function proposal(id, day, title) {
  return {
    proposal_id: id,
    target_note: `Daily log Sep ${Number(day.slice(-2))}`,
    confidence: "high",
    provider: "test",
    created_at: `${day}T10:30:00Z`,
    evidence_start: `${day}T08:00:00Z`,
    evidence_end: `${day}T10:00:00Z`,
    evidence_event_ids: [`evt_${id}`],
    canonical_links: [],
    link_candidates: [],
    supersedes_proposal_ids: [],
    consolidation_job_id: null,
    link_context_revision: "fixture",
    revision: `revision_${id}`,
    stale: false,
    markdown: `### ${title}\n\n- Completed useful work.`
  };
}

async function mockDashboard(page, { dashboardStatus = 200 } = {}) {
  await page.route("http://log-inbox.test/**", async (route) => {
    const url = new URL(route.request().url());
    if (url.pathname === "/") {
      await route.fulfill({ status: 200, contentType: "text/html", body: dashboardHtml });
      return;
    }
    if (url.pathname === "/api/dashboard") {
      if (dashboardStatus !== 200) {
        await route.fulfill({
          status: dashboardStatus,
          contentType: "application/json",
          body: JSON.stringify({ error: "Dashboard fixture unavailable" })
        });
        return;
      }
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          preferences,
          instructions: "Fixture agent instructions",
          proposals: [
            proposal("sep_07", "2026-09-07", "September 7 activity"),
            proposal("sep_08", "2026-09-08", "September 8 activity")
          ],
          consolidations: []
        })
      });
      return;
    }
    if (url.pathname === "/api/vault/connection") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          mode: "mounted",
          vault_id: "workspace_fixture",
          name: "/vault",
          revision: "fixture",
          note_count: 0,
          daily_notes_path: "/vault-daily"
        })
      });
      return;
    }
    if (url.pathname === "/api/knowledge") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          vault: {
            id: "workspace_fixture",
            name: "/vault",
            revision: "fixture",
            total_markdown_files: 0,
            linkable_notes: 0
          },
          destinations: [],
          suggestions: [],
          folders: [],
          protected_paths: []
        })
      });
      return;
    }
    if (url.pathname === "/api/linking") {
      await route.fulfill({
        status: 200,
        contentType: "application/json",
        body: JSON.stringify({
          catalog: { notes: [] },
          rules: [],
          observed: [],
          ignored: [],
          event_count: 0
        })
      });
      return;
    }
    await route.fulfill({ status: 404, body: "not found" });
  });
}

test("the selected calendar date filters proposals", async ({ page }) => {
  await mockDashboard(page);
  await page.goto("http://log-inbox.test/");

  const date = page.getByLabel("Day to filter and consolidate");
  await date.fill("2026-09-07");
  await date.dispatchEvent("change");

  await expect(page.locator("#queue-count")).toHaveText(
    "1 awaiting review for selected day"
  );
  await expect(page.getByText("September 7 activity", { exact: true })).toBeVisible();
  await expect(page.getByText("September 8 activity", { exact: true })).toHaveCount(0);
});

test("primary navigation works from the keyboard", async ({ page }) => {
  await mockDashboard(page);
  await page.goto("http://log-inbox.test/");

  const knowledge = page.getByRole("button", { name: "Knowledge", exact: true });
  await knowledge.focus();
  await page.keyboard.press("Enter");

  await expect(knowledge).toHaveAttribute("aria-selected", "true");
  await expect(page.locator("#linking-panel")).toHaveClass(/active/);
  await expect(page.getByRole("button", { name: "Structure", exact: true })).toBeVisible();
});

test("dashboard failures are visible rather than silent", async ({ page }) => {
  await mockDashboard(page, { dashboardStatus: 503 });
  await page.goto("http://log-inbox.test/");

  await expect(page.locator("#health-text")).toHaveText("Unavailable");
  await expect(page.locator("#queue")).toContainText("Dashboard fixture unavailable");
});

function dailyResponse(date = "2026-09-08", { origin = "generated", freshness = "current" } = {}) {
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
    preview_markdown: "### My notes\n\n- Discussed the trade-off with the team.\n\n### Automated activity\n\n#### Daily workflow\n\n- **Outcome:** Built a predictable Daily review."
  };
}

async function mockDaily(page, { dailyStatus = 200, origin = "generated", freshness = "current" } = {}) {
  const requests = [];
  await page.addInitScript(() => sessionStorage.setItem("log-inbox-csrf", "csrf_fixture"));
  await page.route("http://daily.log-inbox.test/**", async route => {
    const request = route.request();
    const url = new URL(request.url());
    requests.push({ path: url.pathname, method: request.method(), body: request.postData() ? request.postDataJSON() : null });
    if (url.pathname === "/") return route.fulfill({ status: 200, contentType: "text/html", body: dailyHtml });
    if (url.pathname === "/api/v2/auth/session") return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ authenticated: true }) });
    const match = url.pathname.match(/^\/api\/v2\/daily\/(\d{4}-\d{2}-\d{2})$/);
    if (match && request.method() === "GET") {
      if (dailyStatus !== 200) return route.fulfill({ status: dailyStatus, contentType: "application/json", body: JSON.stringify({ error: "Daily fixture unavailable" }) });
      return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify(dailyResponse(match[1], { origin, freshness })) });
    }
    if (url.pathname.endsWith("/candidate") && request.method() === "PUT") return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({ id: "revision_2" }) });
    return route.fulfill({ status: 200, contentType: "application/json", body: JSON.stringify({}) });
  });
  return requests;
}

test("refocused Daily shows one date, destination, notes, and exact preview", async ({ page }) => {
  const requests = await mockDaily(page);
  await page.goto("http://daily.log-inbox.test/?date=2026-09-08");

  await expect(page.getByRole("heading", { name: /Tuesday, September 8, 2026/i })).toBeVisible();
  await expect(page.getByText(/Markdown destination: Work Log\/2026\/Sep/)).toBeVisible();
  await expect(page.getByText("Discussed the trade-off with the team.", { exact: true })).toBeVisible();
  await expect(page.getByLabel("Workstream 1 title")).toHaveValue("Daily workflow");
  await expect(page.locator("#preview")).toContainText("Built a predictable Daily review.");

  await page.getByLabel("Daily log date").fill("2026-09-07");
  await page.getByLabel("Daily log date").dispatchEvent("change");
  await expect.poll(() => requests.some(request => request.path === "/api/v2/daily/2026-09-07")).toBe(true);
});

test("refocused Daily saves structured edits against the visible revision", async ({ page }) => {
  const requests = await mockDaily(page);
  await page.goto("http://daily.log-inbox.test/?date=2026-09-08");

  const title = page.getByLabel("Workstream 1 title");
  await title.fill("Daily review experience");
  await page.getByRole("button", { name: "Save edits" }).click();

  await expect.poll(() => requests.find(request => request.path.endsWith("/candidate") && request.method === "PUT")?.body).toMatchObject({
    expected_revision_id: "revision_1",
    content: { workstreams: [{ title: "Daily review experience" }] }
  });
});

test("refocused Daily exposes server failures", async ({ page }) => {
  await mockDaily(page, { dailyStatus: 503 });
  await page.goto("http://daily.log-inbox.test/?date=2026-09-08");

  await expect(page.getByRole("status")).toContainText("Daily fixture unavailable");
});

test("refocused Daily confirms before replacing edits with new evidence", async ({ page }) => {
  const requests = await mockDaily(page, { origin: "structured_edit", freshness: "update_available" });
  await page.goto("http://daily.log-inbox.test/?date=2026-09-08");

  page.once("dialog", dialog => dialog.dismiss());
  await page.getByRole("button", { name: "Regenerate" }).click();
  expect(requests.filter(request => request.path.endsWith("/generate"))).toHaveLength(0);

  page.once("dialog", dialog => dialog.accept());
  await page.getByRole("button", { name: "Regenerate" }).click();
  await expect.poll(() => requests.find(request => request.path.endsWith("/generate"))?.body).toEqual({
    replace_edited: true
  });
});
