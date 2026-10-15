import { $, action, el, preparationRunLabel } from "./daily-helpers.js";

// A read-only overview of the bounded records already exposed by Daily and Settings.
export function createDashboard({ api, openDay, openSettings }) {
  const data = { overview: null, automation: null, history: null };
  const failed = new Set();
  let loading = false;
  let updated = "";
  const labels = {
    apply_attention: "Save needs attention",
    generation_failed: "Preparation failed",
    generating: "Preparing",
    in_review: "Ready for review",
    missed: "Not prepared",
    notes_unreviewed: "Notes not drafted",
    scheduled: "Scheduled",
    update_available: "New activity available",
  };
  const prettyDate = (date) =>
    new Date(`${date}T12:00:00Z`).toLocaleDateString(undefined, {
      month: "short",
      day: "numeric",
      timeZone: "UTC",
    });
  const duration = (ms) =>
    ms < 60000
      ? `${Math.round(ms / 1000)} s`
      : `${(ms / 60000).toFixed(1)} min`;
  const utcDay = (date) => Date.parse(`${date}T00:00:00Z`) / 86400000;

  function status(id, sources) {
    const unavailable = sources.filter((source) => failed.has(source));
    const node = $(id);
    node.hidden = !unavailable.length;
    node.textContent = unavailable.length
      ? `Could not refresh ${unavailable.join(" and ")}. ${unavailable.some((source) => data[source]) ? "Earlier information is marked as last known." : "Refresh to retry."}`
      : "";
  }

  function renderPreparation() {
    const box = $("dashboard-preparation");
    box.replaceChildren();
    status("dashboard-preparation-status", ["overview", "automation"]);
    const overview = data.overview,
      automation = data.automation;
    const settings = automation?.settings;
    const enabled = Boolean(automation?.saved && settings?.enabled);
    const last =
      failed.has("overview") || failed.has("automation") ? "Last known: " : "";
    const summary = el("dl", "dashboard-preparation-grid");
    function fact(label, value) {
      const item = el("div");
      item.append(el("dt", "subtle", label), el("dd", "", `${last}${value}`));
      summary.append(item);
    }
    fact(
      "Automatic preparation",
      settings
        ? enabled
          ? `On · daily at ${settings.generation_time}${overview?.timezone ? ` (${overview.timezone})` : ""}`
          : "Off"
        : "Unavailable",
    );
    const active = overview?.active_generation;
    const selected = active?.model_selection;
    fact(
      "Currently preparing",
      overview
        ? active
          ? `${prettyDate(active.local_date)}${selected?.model ? ` · ${selected.provider || selected.connection_id || "Provider"} / ${selected.model}` : ""}`
          : "No active preparation"
        : "Unavailable",
    );
    const runs = new Map(
      (overview?.days || [])
        .filter((day) => day.schedule_state)
        .map((day) => [day.local_date, day.schedule_state]),
    );
    for (const run of automation?.recent_runs || [])
      runs.set(run.local_date, run.state);
    let inactive = 0;
    for (const [date, state] of runs) {
      if (state !== "pending" || !settings || !overview) continue;
      const age = utcDay(overview.today) - utcDay(date);
      if (!enabled || age < 1 || age > settings.catch_up_days) inactive += 1;
    }
    box.append(summary);
    if (inactive)
      box.append(
        el(
          "p",
          "subtle",
          `${inactive} saved pending run${inactive === 1 ? " is" : "s are"} inactive because automatic preparation is off or the date is outside its catch-up window. They are not counted as queued.`,
        ),
      );
    box.append(
      el(
        "p",
        "subtle",
        "Automatic preparation creates drafts for review. Saving to Markdown always requires your confirmation.",
      ),
    );
    if (active)
      box.append(
        action(
          failed.has("overview")
            ? "Open last known active day"
            : "Open active day",
          () => openDay(active.local_date),
        ),
      );
  }

  function renderReview() {
    const box = $("dashboard-review");
    box.replaceChildren();
    status("dashboard-review-status", ["overview", "automation"]);
    if (!data.overview) {
      box.append(
        el("p", "subtle", "Recent days are unavailable. Refresh to retry."),
      );
      return;
    }
    const days = (data.overview.days || []).filter(
      (day) =>
        labels[day.status] &&
        (day.local_date !== data.overview.today ||
          ["apply_attention", "generation_failed"].includes(day.status)),
    );
    if (!days.length) {
      box.append(
        el(
          "p",
          "subtle",
          `${failed.has("overview") ? "Last known: " : ""}No days need attention in this recent window.`,
        ),
        action("Open today", () => openDay(data.overview.today)),
      );
      return;
    }
    const list = el("ul", "dashboard-review-list");
    for (const day of days) {
      const item = el("li");
      const runSettings = data.automation?.settings;
      const label =
        day.status !== "scheduled"
          ? labels[day.status]
          : day.revision_number
            ? "Ready for review"
            : runSettings && day.schedule_state
              ? preparationRunLabel(
                  { state: day.schedule_state, local_date: day.local_date },
                  {
                    ...runSettings,
                    enabled: Boolean(
                      data.automation.saved && runSettings.enabled,
                    ),
                  },
                  data.overview.today,
                )
              : "Saved preparation record · eligibility not checked";
      const button = action("", () => openDay(day.local_date));
      button.className = "attention-item";
      button.append(
        el("span", "", prettyDate(day.local_date)),
        el(
          "span",
          "subtle",
          `${failed.has("overview") || (day.status === "scheduled" && failed.has("automation")) ? "Last known: " : ""}${label}`,
        ),
      );
      item.append(button);
      list.append(item);
    }
    box.append(
      el(
        "p",
        "subtle",
        "Recent window only. Today's routine activity stays quiet.",
      ),
      list,
    );
  }

  function renderModels() {
    const box = $("dashboard-models");
    box.replaceChildren();
    status("dashboard-models-status", ["history"]);
    if (!data.history) {
      box.append(
        el(
          "p",
          "subtle",
          "Model timing history is unavailable. Refresh to retry.",
        ),
      );
      return;
    }
    const attempts = data.history.attempts || [],
      groups = new Map();
    let unknown = 0;
    for (const attempt of attempts) {
      const model = attempt.model_selection;
      if (!model?.model || !(attempt.total_groups > 0)) {
        unknown += 1;
        continue;
      }
      if (attempt.state === "running") continue;
      const key = JSON.stringify([
        model.connection_id || model.provider,
        model.model,
        model.profile,
        model.configuration_digest,
        attempt.operation_type,
        attempt.reference_mode,
      ]);
      if (!groups.has(key))
        groups.set(key, {
          model,
          operation: attempt.operation_type,
          references: attempt.reference_mode,
          samples: [],
          failures: 0,
          other: 0,
          total: 0,
        });
      const group = groups.get(key);
      group.total += 1;
      if (["failed", "timed_out"].includes(attempt.state)) group.failures += 1;
      else if (["canceled", "interrupted"].includes(attempt.state))
        group.other += 1;
      if (
        attempt.state === "succeeded" &&
        Number.isFinite(attempt.elapsed_ms) &&
        attempt.elapsed_ms >= 0
      )
        group.samples.push(attempt);
    }
    if (!groups.size)
      box.append(
        el(
          "p",
          "subtle",
          "No completed attempts with recorded model and workload yet.",
        ),
      );
    else {
      if (failed.has("history"))
        box.append(el("p", "subtle", "Last known model results."));
      const list = el("ul", "generation-history-list");
      for (const group of [...groups.values()].slice(0, 5)) {
        const item = el("li");
        const times = group.samples
          .map((attempt) => attempt.elapsed_ms)
          .sort((a, b) => a - b);
        const middle = Math.floor(times.length / 2);
        const median = times.length
          ? times.length % 2
            ? times[middle]
            : (times[middle - 1] + times[middle]) / 2
          : null;
        const counts = group.samples.map((attempt) => attempt.total_groups);
        const measured =
          median === null
            ? "No completed timing sample"
            : `${times.length} completed sample${times.length === 1 ? "" : "s"} · median ${duration(median)} · ${Math.min(...counts)}–${Math.max(...counts)} groups per sample`;
        item.append(
          el(
            "strong",
            "",
            `${group.model.provider || group.model.connection_id || "Unknown connection"} / ${group.model.model}`,
          ),
          el(
            "p",
            "subtle",
            `${group.model.profile || "Unknown profile"} · ${group.operation || "Unknown operation"} · references: ${group.references || "unknown"}`,
          ),
          el("p", "", measured),
          el(
            "p",
            "subtle",
            `${group.failures} failed or timed out / ${group.total} recorded terminal attempts; ${group.other} canceled or interrupted`,
          ),
        );
        list.append(item);
      }
      box.append(list);
      if (groups.size > 5)
        box.append(
          el(
            "p",
            "subtle",
            `Showing the 5 most recent of ${groups.size} model setups. Generation settings contains the full recent history.`,
          ),
        );
    }
    if (unknown)
      box.append(
        el(
          "p",
          "subtle",
          `${unknown} attempt${unknown === 1 ? " has" : "s have"} unknown model identity or no recorded inference progress and ${unknown === 1 ? "is" : "are"} excluded from timing comparisons.`,
        ),
      );
  }

  function render() {
    const overview = data.overview;
    $("dashboard-window").textContent = overview
      ? `${failed.has("overview") ? "Last known: " : ""}Recent window ending ${prettyDate(overview.today)} · ${overview.timezone}${updated ? `. Updated ${updated}; Refresh for current status.` : ""}`
      : "Recent activity and preparation";
    renderPreparation();
    renderReview();
    renderModels();
  }
  async function load() {
    if (loading) return;
    loading = true;
    $("dashboard-refresh").disabled = true;
    $("dashboard-panel").setAttribute("aria-busy", "true");
    const sources = [
      ["overview", "/api/v2/daily/overview"],
      ["automation", "/api/v2/settings/automation"],
      ["history", "/api/v2/settings/generation/history"],
    ];
    const results = await Promise.allSettled(
      sources.map(([, path]) => api(path)),
    );
    results.forEach((result, index) => {
      const source = sources[index][0];
      if (result.status === "fulfilled") {
        data[source] = result.value;
        if (source === "overview")
          updated = new Date(
            result.value.server_now || Date.now(),
          ).toLocaleTimeString(undefined, {
            hour: "2-digit",
            minute: "2-digit",
            timeZone: result.value.timezone,
          });
        failed.delete(source);
      } else failed.add(source);
    });
    loading = false;
    $("dashboard-refresh").disabled = false;
    $("dashboard-panel").setAttribute("aria-busy", "false");
    render();
  }
  $("dashboard-refresh").addEventListener("click", load);
  $("dashboard-preparation-settings").addEventListener("click", () =>
    openSettings("preparation"),
  );
  $("dashboard-model-settings").addEventListener("click", () =>
    openSettings("generation"),
  );
  return { load, render };
}
