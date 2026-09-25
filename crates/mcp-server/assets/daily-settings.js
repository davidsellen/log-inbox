import { $, el, action, isPlainLinkClick } from "./daily-helpers.js";
// Settings owns destination, automation, and migration forms and requests.
export function createSettings({
  app,
  api,
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
    agentGuidance: null,
    agentFieldsDraft: [],
    initialized: false,
    saving: false,
  };
  const forms = {
    destination: "settings-form",
    preparation: "automation-box",
  };
  const baselines = new Map();
  function values(section) {
    return JSON.stringify(
      [...$(forms[section]).querySelectorAll("input,textarea,select")].map(
        (input) => [input.id, input.value, input.checked],
      ),
    );
  }
  function dirty(section) {
    return Boolean(
      forms[section] &&
      state.initialized &&
      baselines.get(section) !== values(section),
    );
  }
  function updateDirtyLabels() {
    $("reference-settings").href =
      `?date=${encodeURIComponent(app.date)}&view=knowledge`;
    for (const link of document.querySelectorAll("[data-settings-link]")) {
      if (!link.dataset.label) link.dataset.label = link.textContent;
      const section = link.dataset.settingsLink;
      link.textContent = `${link.dataset.label}${dirty(section) ? " · Unsaved" : ""}`;
      $("settings-section-select").querySelector(
        `option[value="${section}"]`,
      ).textContent = link.textContent;
      link.href = `?date=${encodeURIComponent(app.date)}&view=settings&section=${section}`;
    }
    updateSaveButtons();
    $("save-agent-guidance").disabled =
      !app.csrf || state.saving || !agentGuidanceDirty();
  }
  function agentGuidanceDirty() {
    return (
      Boolean(state.agentGuidance) &&
      JSON.stringify(state.agentFieldsDraft) !==
        JSON.stringify(state.agentGuidance.fields)
    );
  }
  function renderAgentGuidance() {
    const data = state.agentGuidance;
    if (!data) return;
    const fieldRoot = $("agent-guidance-fields");
    fieldRoot.replaceChildren();
    const coverage = new Map(
      (data.coverage || []).map((item) => [item.field, item]),
    );
    for (const field of data.available_fields || []) {
      const label = el("label", "check-field");
      const input = document.createElement("input");
      input.type = "checkbox";
      input.value = field;
      input.checked = state.agentFieldsDraft.includes(field);
      input.disabled = !app.csrf || state.saving;
      const counts = coverage.get(field);
      const title = counts
        ? `${field.replaceAll("_", " ")} · ${counts.present} present, ${counts.missing} missing`
        : field.replaceAll("_", " ");
      label.append(input, el("span", "", title));
      fieldRoot.append(label);
    }
    const total = data.sample_size || 0;
    $("agent-guidance-coverage").textContent = total
      ? `Last 30 days · ${total}${data.sample_truncated ? "+" : ""} recent events checked. Counts show events with each recommended field.`
      : "No recent events to check yet. Send a start and terminal event to see metadata coverage.";
    const producers = (data.producers || []).map((producer) => {
      const agents = producer.agents?.length
        ? ` · ${producer.agents.join(", ")}`
        : "";
      const missing = producer.missing?.length
        ? ` · missing in recent events: ${producer.missing.map((item) => `${item.field} (${item.missing})`).join(", ")}`
        : " · all selected recommendations present";
      return `${producer.source} · ${producer.events} events${agents}${missing}`;
    });
    $("agent-guidance-sources").replaceChildren(
      ...producers.map((text) => el("p", "subtle", text)),
    );
    $("agent-guidance-snippet").textContent = data.snippet || "";
    $("save-agent-guidance").disabled =
      !app.csrf || state.saving || !agentGuidanceDirty();
  }
  async function loadAgentGuidance() {
    try {
      state.agentGuidance = await api("/api/v2/settings/agent-guidance");
      state.agentFieldsDraft = [...state.agentGuidance.fields];
      renderAgentGuidance();
    } catch (error) {
      $("agent-guidance-coverage").textContent =
        `Agent metadata guidance is unavailable: ${error.message}`;
      $("save-agent-guidance").disabled = true;
      $("copy-agent-guidance").disabled = true;
    }
  }
  async function saveAgentGuidance() {
    try {
      state.saving = true;
      renderAgentGuidance();
      state.agentGuidance = await api("/api/v2/settings/agent-guidance", {
        method: "PUT",
        body: JSON.stringify({ fields: state.agentFieldsDraft }),
      });
      state.agentFieldsDraft = [...state.agentGuidance.fields];
      renderAgentGuidance();
      $("agent-guidance-coverage").textContent +=
        " Recommendations saved; copy the updated instructions into your agent guidance files.";
    } catch (error) {
      $("agent-guidance-coverage").textContent = error.message;
    } finally {
      state.saving = false;
      renderAgentGuidance();
    }
  }
  async function copyAgentGuidance() {
    try {
      if (!state.agentGuidance?.snippet)
        throw new Error("Instructions are unavailable");
      await navigator.clipboard.writeText(state.agentGuidance?.snippet || "");
      pageNotice(
        "Agent instructions copied. Paste them into the AGENTS.md or equivalent files your agents use.",
      );
    } catch {
      pageNotice(
        "Clipboard access is unavailable. Open Preview instructions and copy the text manually.",
        true,
      );
    }
  }
  function recommendAgentField(field) {
    if (!state.agentGuidance?.available_fields.includes(field)) return;
    if (!state.agentFieldsDraft.includes(field))
      state.agentFieldsDraft.push(field);
    $("agent-guidance-options").open = true;
    renderAgentGuidance();
    pageNotice(
      `${field.replaceAll("_", " ")} added to the draft guidance checklist. Save recommendations, then copy the updated agent instructions.`,
    );
  }
  function updateSaveButtons() {
    const allowed = !!app.csrf && !state.saving;
    $("save-automation").disabled =
      !allowed ||
      (!dirty("preparation") && !!state.automation?.saved) ||
      !$("automation-box").checkValidity();
    $("save-settings").disabled =
      !allowed ||
      !state.settingsPreview ||
      (!dirty("destination") &&
        !!state.workspaceSettings?.active_profile &&
        state.workspaceSettings.binding_matches);
  }
  function markSaved(section) {
    baselines.set(section, values(section));
    updateDirtyLabels();
  }
  function pageNotice(message, error = false) {
    const node = $("settings-page-notice");
    node.textContent = message;
    node.hidden = !message;
    node.className = `notice${error ? " error" : ""}`;
  }
  async function saving(form, task) {
    if (state.saving || !app.csrf) return;
    state.saving = true;
    const opener = document.activeElement;
    $(form).inert = true;
    try {
      await task();
    } finally {
      state.saving = false;
      $(form).inert = false;
      updateSaveButtons();
      if (
        document.activeElement === document.body ||
        $(form).contains(document.activeElement)
      ) {
        const target =
          !opener.disabled && opener.isConnected && !opener.closest("[hidden]")
            ? opener
            : $(form).querySelector('[role="status"]');
        if (target) {
          if (target !== opener) target.tabIndex = -1;
          target.focus({ preventScroll: true });
        }
      }
    }
  }
  function showSection(section) {
    section = forms[section] ? section : "general";
    app.settingsSection = section;
    for (const panel of document.querySelectorAll("[data-settings-section]"))
      panel.hidden = panel.dataset.settingsSection !== section;
    for (const link of document.querySelectorAll("[data-settings-link]")) {
      if (link.dataset.settingsLink === section)
        link.setAttribute("aria-current", "page");
      else link.removeAttribute("aria-current");
    }
    $("settings-section-select").value = section;
    updateDirtyLabels();
  }
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
            "Preparation is currently running. Open the day to follow its progress.",
          ),
        );
      runs.append(row);
    }
  }

  function refreshAccess() {
    $("settings-readonly").hidden = !!app.csrf;
    for (const form of Object.values(forms))
      for (const control of $(form).querySelectorAll(
        "input,select,textarea,button",
      ))
        control.disabled = !app.csrf;
    $("save-settings").disabled = !app.csrf || !state.settingsPreview;
    $("commit-migration").disabled = !app.csrf || !state.migration?.ready;
    updateSaveButtons();
  }
  function prepare() {
    refreshAccess();
    if (state.initialized) return;
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
    $("preparation-unavailable").hidden = !!profile;
    refreshAccess();
    pageNotice(
      !profile ? "Choose and save a Markdown destination to get started." : "",
    );
    state.initialized = true;
    for (const section of Object.keys(forms)) markSaved(section);
  }

  function openSettings(section) {
    const ready =
      state.workspaceSettings?.active_profile &&
      state.workspaceSettings?.binding_matches;
    return selectView(
      "settings",
      true,
      typeof section === "string" ? section : ready ? "general" : "destination",
    );
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
    $("migration-label").textContent = completed
      ? "Maintenance"
      : "Older data migration";
    box.hidden = !completed && !migration.items.length;
    if (box.hidden) return;
    if (!completed) box.open = true;
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
      await loadMigration();
      if (!dirty("preparation")) {
        await loadAutomation();
        renderAutomationSettings();
        enhanceAutomationRuns();
        markSaved("preparation");
      }
      pageNotice(
        `Migration completed. ${result.imported_items} items preserved or imported; ${result.cleaned_files} unchanged pending files cleaned. ${result.retried_preparation_runs || 0} blocked preparation run${result.retried_preparation_runs === 1 ? " was" : "s were"} queued again. Backup: ${result.backup_file}.`,
      );
    } catch (error) {
      pageNotice(error.message, true);
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
      const draft = workspaceDraft();
      const preview = await api("/api/v2/settings/workspace/preview", {
        method: "POST",
        body: JSON.stringify(draft),
      });
      if (JSON.stringify(draft) !== JSON.stringify(workspaceDraft())) return;
      state.settingsPreview = preview;
      $("settings-preview").textContent =
        `Example: ${state.settingsPreview.destination_example}. Nothing has been saved or created.`;
      $("settings-preview").className = "notice";
      $("settings-preview").hidden = false;
      updateSaveButtons();
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
      markSaved("destination");
      await loadWorkspaceSettings();
      await loadMigration();
      if (!active) {
        await loadAutomation();
        renderAutomationSettings();
        enhanceAutomationRuns();
        markSaved("preparation");
        $("preparation-unavailable").hidden = true;
      }
      if (!app.date) {
        app.overview = await api("/api/v2/daily/overview");
        app.date = app.overview.today;
        $("day").value = app.date;
      }
      if (!hasPendingChanges()) await loadDay();
      $("settings-preview").textContent =
        "Destination saved. Existing days keep their previous destinations.";
      $("save-settings").disabled = true;
      state.settingsPreview = null;
      pageNotice(
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
      markSaved("preparation");
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
    $("close-settings").addEventListener("click", () => selectView("daily"));
    $("back-to-settings").addEventListener("click", () =>
      openSettings("general"),
    );
    for (const link of document.querySelectorAll("[data-settings-link]"))
      link.addEventListener("click", (event) => {
        if (!isPlainLinkClick(event)) return;
        event.preventDefault();
        selectView("settings", true, link.dataset.settingsLink);
      });
    $("settings-section-select").addEventListener("change", (event) =>
      selectView("settings", true, event.target.value),
    );
    $("settings-page").addEventListener("input", updateDirtyLabels);
    $("settings-page").addEventListener("change", updateDirtyLabels);
    $("agent-guidance-fields").addEventListener("change", () => {
      state.agentFieldsDraft = [
        ...$("agent-guidance-fields").querySelectorAll("input:checked"),
      ].map((input) => input.value);
      updateDirtyLabels();
    });
    $("save-agent-guidance").addEventListener("click", saveAgentGuidance);
    $("copy-agent-guidance").addEventListener("click", copyAgentGuidance);
    const invalidatePreview = () => {
      state.settingsPreview = null;
      $("save-settings").disabled = true;
      $("settings-preview").hidden = true;
    };
    $("settings-form").addEventListener("input", invalidatePreview);
    $("settings-form").addEventListener("change", invalidatePreview);
    $("settings-form").addEventListener("submit", previewSettings);
    $("save-settings").addEventListener("click", () =>
      saving("settings-form", saveSettings),
    );
    $("automation-box").addEventListener("submit", (event) => {
      event.preventDefault();
      saving("automation-box", saveAutomation);
    });
    $("commit-migration").addEventListener("click", () =>
      saving("migration-box", commitMigration),
    );
    $("reference-settings").addEventListener("click", (event) => {
      if (!isPlainLinkClick(event)) return;
      event.preventDefault();
      selectView("knowledge");
    });
  }
  return {
    bindEvents,
    loadWorkspaceSettings,
    loadAgentGuidance,
    recommendAgentField,
    loadAutomation,
    loadMigration,
    openSettings,
    prepare,
    showSection,
    hasChanges: () => Object.keys(forms).some(dirty) || agentGuidanceDirty(),
    pageNotice,
    discard: () => {
      state.initialized = false;
    },
    get saving() {
      return state.saving;
    },
    get workspaceSettings() {
      return state.workspaceSettings;
    },
    get migration() {
      return state.migration;
    },
  };
}
