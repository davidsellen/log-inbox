import { expect, test } from "@playwright/test";
import { readFile } from "node:fs/promises";

const dashboardHtml = await readFile(
  new URL("../../crates/mcp-server/assets/dashboard.html", import.meta.url),
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
