import { $, el, action } from "./daily-helpers.js";
// Settings owns destination, automation, and migration forms and requests.
export function createSettings({
  app,
  api,
  notice,
  openDialog,
  closeDialog,
  rememberDialogFields,
  dialogHasChanges,
  selectDay,
  selectView,
  generate,
  loadDay,
  hasPendingChanges,
}) {
  const state = {
    workspaceSettings: null,
    automation: null,
    settingsPreview: null,
    migration: null,
  };
  async function loadWorkspaceSettings() {
    state.workspaceSettings = await api("/api/v2/settings/workspace");
  }

  async function loadAutomation() {
    state.automation = await api("/api/v2/settings/automation");
  }

  function renderAutomationSettings() {
    const box = $("automation-box");
    const profile = state.workspaceSettings?.active_profile;
    const fields = [
      $("automation-time"),
      $("automation-catchup"),
      $("retention-raw"),
      $("retention-audit"),
      $("retention-recovery"),
    ];
    for (const field of fields)
      field.required = Boolean(profile && state.automation);
    if (!profile || !state.automation) {
      box.hidden = true;
      return;
    }
    box.hidden = false;
    const automation = state.automation;
    const settings = automation.settings;
    $("automation-enabled").checked = settings.enabled;
    $("automation-time").value = settings.generation_time;
    $("automation-catchup").value = settings.catch_up_days;
    $("retention-raw").value = settings.raw_retention_days;
    $("retention-audit").value = settings.audit_retention_days;
    $("retention-recovery").value = settings.recovery_retention_days;
    const status = $("automation-status");
    status.hidden = false;
    status.className = "notice";
    status.textContent = automation.saved
      ? `Saved. Automatic preparation is ${settings.enabled ? "on" : "off"}. Retention maintenance is active; no Markdown is written automatically.`
      : "Defaults shown. Scheduling and retention are inactive until you choose Save preparation & retention.";
    const runs = $("automation-runs");
    runs.replaceChildren();
    const recent = (automation.recent_runs || []).slice(0, 5);
    if (!recent.length) {
      runs.textContent = "No automatic preparation runs yet.";
      return;
    }
    const heading = document.createElement("strong");
    heading.textContent = "Recent preparation runs";
    runs.append(heading);
    for (const run of recent) {
      const row = document.createElement("div");
      row.textContent = `${run.local_date}: ${run.state}${run.attempts > 1 ? ` (${run.attempts} attempts)` : ""}${run.last_error ? ` — ${run.last_error}` : ""}`;
      runs.append(row);
    }
  }

  function enhanceAutomationRuns() {
    const runs = $("automation-runs");
    if (!runs || !state.automation?.recent_runs?.length) return;
    const recent = (state.automation.recent_runs || []).slice(0, 5);
    runs.replaceChildren(el("strong", "", "Recent preparation runs"));
    for (const run of recent) {
      const row = el("div", "preparation-run");
      const failed = run.state === "failed";
      const summary = el(
        "div",
        "",
        `${run.local_date}: ${run.state}${run.attempts > 1 ? ` (${run.attempts} attempts)` : ""}`,
      );
      row.append(summary);
      if (failed) {
        const detail = document.createElement("details");
        const title = document.createElement("summary");
        title.textContent = "What this means";
        const message =
          run.last_error || "The model did not produce a usable candidate.";
        const guidance = message.includes("omitted evidence")
          ? "The model left out evidence links. Retry the day after checking that the source events are still available."
          : message.includes("structured schema")
            ? "The model returned the wrong shape. Retry once; if it repeats, disable automatic preparation and check the configured local model."
            : "Preparation could not complete. Retry the day manually to see the full error.";
        detail.append(el("p", "", guidance));
        if (run.last_error) {
          const raw = document.createElement("details");
          const rawTitle = document.createElement("summary");
          rawTitle.textContent = "Show technical details";
          raw.append(rawTitle, el("div", "subtle", run.last_error));
          detail.append(raw);
        }
        row.append(detail);
        const actions = el("div", "actions");
        actions.append(
          action("Open day", () => selectDay(run.local_date)),
          action(
            "Retry now",
            async () => {
              if (await selectDay(run.local_date)) await generate();
            },
            { primary: true, disabled: !app.csrf },
          ),
        );
        row.append(actions);
      } else if (run.state === "claimed")
        row.append(
          el(
            "div",
            "subtle",
            "Preparation is currently running; refresh this dialog in a moment.",
          ),
        );
      runs.append(row);
    }
  }

  function openSettings() {
    const profile = state.workspaceSettings?.active_profile;
    $("workspace-path").textContent =
      `Mounted workspace: ${state.workspaceSettings?.workspace_path || "Unavailable"}`;
    $("setting-timezone").value =
      profile?.timezone ||
      Intl.DateTimeFormat().resolvedOptions().timeZone ||
      "UTC";
    $("setting-daily-root").value = profile?.daily_root || "Work Log";
    $("setting-daily-pattern").value =
      profile?.daily_pattern ||
      "{year}/{month_name}/Daily log {month_name} {day}.md";
    $("setting-template").value = profile?.template_path || "";
    $("setting-link-style").value = profile?.link_style || "markdown";
    state.settingsPreview = null;
    $("settings-preview").hidden = true;
    $("save-settings").disabled = true;
    renderAutomationSettings();
    enhanceAutomationRuns();
    renderMigration();
    openDialog($("settings-dialog"));
  }

  async function loadMigration() {
    state.migration = await api("/api/v2/migration/cutover");
    renderMigration();
  }

  function renderMigration() {
    const box = $("migration-box");
    if (!state.migration) {
      box.hidden = true;
      return;
    }
    const migration = state.migration;
    const completed = migration.cutover_status === "completed";
    box.hidden = !completed && !migration.items.length;
    if (box.hidden) return;
    $("migration-summary").textContent = completed
      ? `Migration completed (${migration.completed_operation_id}). Preserved items remain available in app storage.`
      : `${migration.items.length} older item${migration.items.length === 1 ? "" : "s"} found. ${migration.ready ? "Ready for reviewed migration." : "Resolve the blockers below before migration."}`;
    const list = $("migration-messages");
    list.replaceChildren();
    for (const message of [...migration.blockers, ...migration.warnings]) {
      const item = document.createElement("li");
      item.textContent = message;
      list.append(item);
    }
    $("commit-migration").hidden = completed;
    $("commit-migration").disabled = !app.csrf || !migration.ready;
  }

  async function commitMigration() {
    const migration = state.migration;
    if (!migration || !migration.ready) return;
    if (
      !confirm(
        "Create a verified backup and migrate the listed legacy data? Existing Markdown notes will not be changed.",
      )
    )
      return;
    $("commit-migration").disabled = true;
    try {
      const result = await api("/api/v2/migration/cutover", {
        method: "POST",
        body: JSON.stringify({
          operation_id: migration.operation_id,
          report_digest: migration.report_digest,
        }),
      });
      await Promise.all([loadMigration(), loadAutomation()]);
      renderAutomationSettings();
      notice(
        `Migration completed. ${result.imported_items} items preserved or imported; ${result.cleaned_files} unchanged pending files cleaned. ${result.retried_preparation_runs || 0} blocked preparation run${result.retried_preparation_runs === 1 ? " was" : "s were"} queued again. Backup: ${result.backup_file}.`,
      );
    } catch (error) {
      notice(error.message, true);
      $("commit-migration").disabled = false;
    }
  }

  function workspaceDraft() {
    return {
      timezone: $("setting-timezone").value,
      daily_root: $("setting-daily-root").value,
      daily_pattern: $("setting-daily-pattern").value,
      template_path: $("setting-template").value || null,
      link_style: $("setting-link-style").value,
    };
  }

  async function previewSettings(event) {
    event.preventDefault();
    try {
      state.settingsPreview = await api("/api/v2/settings/workspace/preview", {
        method: "POST",
        body: JSON.stringify(workspaceDraft()),
      });
      $("settings-preview").textContent =
        `Example: ${state.settingsPreview.destination_example}. Nothing has been saved or created.`;
      $("settings-preview").className = "notice";
      $("settings-preview").hidden = false;
      $("save-settings").disabled = false;
    } catch (error) {
      $("settings-preview").textContent = error.message;
      $("settings-preview").className = "notice error";
      $("settings-preview").hidden = false;
      $("save-settings").disabled = true;
    }
  }

  async function saveSettings() {
    if (!state.settingsPreview) return;
    const active = state.workspaceSettings?.active_profile;
    try {
      await api("/api/v2/settings/workspace", {
        method: "PUT",
        body: JSON.stringify({
          settings: state.settingsPreview.settings,
          preview_digest: state.settingsPreview.preview_digest,
          expected_profile_id: active?.id || null,
          expected_updated_at: active?.updated_at || null,
        }),
      });
      rememberDialogFields($("settings-dialog"), (id) =>
        id.startsWith("setting-"),
      );
      if (!dialogHasChanges($("settings-dialog")))
        closeDialog($("settings-dialog"), true);
      await loadWorkspaceSettings();
      await loadMigration();
      if (!app.date) {
        app.overview = await api("/api/v2/daily/overview");
        app.date = app.overview.today;
        $("day").value = app.date;
      }
      if (!hasPendingChanges()) await loadDay();
      notice(
        "Daily settings saved. Existing days keep their previous destinations.",
      );
    } catch (error) {
      $("settings-preview").textContent = error.message;
      $("settings-preview").className = "notice error";
      $("settings-preview").hidden = false;
    }
  }

  async function saveAutomation() {
    const fields = [
      $("automation-time"),
      $("automation-catchup"),
      $("retention-raw"),
      $("retention-audit"),
      $("retention-recovery"),
    ];
    const invalid = fields.find((field) => !field.checkValidity());
    if (invalid) {
      invalid.reportValidity();
      return;
    }
    const button = $("save-automation");
    button.disabled = true;
    const status = $("automation-status");
    try {
      const result = await api("/api/v2/settings/automation", {
        method: "PUT",
        body: JSON.stringify({
          enabled: $("automation-enabled").checked,
          generation_time: $("automation-time").value,
          catch_up_days: Number($("automation-catchup").value),
          raw_retention_days: Number($("retention-raw").value),
          audit_retention_days: Number($("retention-audit").value),
          recovery_retention_days: Number($("retention-recovery").value),
          expected_updated_at: state.automation?.saved
            ? state.automation.settings.updated_at
            : null,
        }),
      });
      await loadAutomation();
      renderAutomationSettings();
      enhanceAutomationRuns();
      rememberDialogFields(
        $("settings-dialog"),
        (id) => !id.startsWith("setting-"),
      );
      if (result.requeued_failed_runs)
        status.textContent = `Saved. Requeued ${result.requeued_failed_runs} failed preparation run${result.requeued_failed_runs === 1 ? "" : "s"}; they will retry automatically. No Markdown is written automatically.`;
      if (!hasPendingChanges()) await loadDay();
    } catch (error) {
      status.textContent = error.message;
      status.className = "notice error";
      status.hidden = false;
    } finally {
      button.disabled = false;
    }
  }
  function bindEvents() {
    $("settings").addEventListener("click", openSettings);
    $("close-settings").addEventListener("click", () =>
      closeDialog($("settings-dialog")),
    );
    $("settings-form").addEventListener("submit", previewSettings);
    $("save-settings").addEventListener("click", saveSettings);
    $("save-automation").addEventListener("click", saveAutomation);
    $("commit-migration").addEventListener("click", commitMigration);
    $("reference-settings").addEventListener("click", () =>
      selectView("knowledge"),
    );
  }
  return {
    bindEvents,
    loadWorkspaceSettings,
    loadAutomation,
    loadMigration,
    openSettings,
    get workspaceSettings() {
      return state.workspaceSettings;
    },
    get migration() {
      return state.migration;
    },
  };
}
