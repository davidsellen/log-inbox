import { $ } from "./daily-helpers.js";

// Connection setup is independent of Daily loading: a provider outage must never
// prevent reviewing or saving a previously prepared draft.
export function createGenerationModels({ app, api, openSettings }) {
  let data = null;
  let busy = false;
  let loading = false;
  let error = "";
  let selectedConnection = null;
  let baseline = "";
  const choices = new Map();
  const pendingChoices = new Map();
  const key = (selection) =>
    selection
      ? JSON.stringify({
          connection_id: selection.connection_id,
          model: selection.model,
        })
      : "";
  const connection = (id) => data?.connections?.find((item) => item.id === id);
  const label = (selection) =>
    selection
      ? `${selection.connection_name || selection.provider || connection(selection.connection_id)?.name || selection.connection_id} / ${selection.model}`
      : "No model configured";
  const fields = () =>
    JSON.stringify([
      $("generation-connection-name").value,
      $("generation-endpoint").value,
      $("generation-api-key").value,
    ]);
  function feedback(message, failed = false) {
    $("generation-settings-notice").textContent = message;
    $("generation-settings-notice").classList.toggle("error", failed);
  }
  let historyRecords = [];
  let historyLoading = false;
  function durationLabel(milliseconds) {
    if (milliseconds == null) return "Time unavailable";
    const seconds = Math.round(milliseconds / 1000);
    return seconds < 60
      ? `${seconds}s`
      : `${Math.floor(seconds / 60)}m ${seconds % 60}s`;
  }
  async function loadHistory() {
    if (historyLoading) return;
    historyLoading = true;
    $("generation-history-refresh").disabled = true;
    $("generation-history-status").textContent = "Loading timing records…";
    try {
      const result = await api("/api/v2/settings/generation/history");
      historyRecords = result.attempts || [];
      const list = $("generation-history-list");
      list.replaceChildren();
      for (const attempt of historyRecords) {
        const row = document.createElement("li");
        const model = attempt.model_selection;
        const title = document.createElement("strong");
        title.textContent = model?.model
          ? `${model.provider || model.connection_id || "Provider"} / ${model.model}`
          : "Unknown model (older attempt)";
        const detail = document.createElement("p");
        const status = (attempt.state || "unknown").replaceAll("_", " ");
        const operation =
          {
            manual: "Manual generation",
            scheduled: "Scheduled preparation",
            comparison: "Reference comparison (two arms)",
          }[attempt.operation_type] || "Generation attempt";
        detail.textContent = `${attempt.local_date} · ${operation} · ${status} · ${durationLabel(attempt.elapsed_ms)} · ${attempt.completed_groups}/${attempt.total_groups} workstreams`;
        const context = document.createElement("p");
        context.className = "subtle";
        context.textContent = `${attempt.started_at ? new Date(attempt.started_at).toLocaleString() : "Start unavailable"} · ${attempt.reference_mode === "none" ? "Without reference notes" : "Configured reference notes"}${attempt.failure_code ? ` · ${attempt.failure_code}` : ""}${attempt.total_groups === 0 ? " · No recorded model progress; exclude from speed comparisons" : ""}`;
        row.append(title, detail, context);
        list.append(row);
      }
      $("generation-history-status").textContent = historyRecords.length
        ? `Showing the latest ${historyRecords.length} attempts. Earlier records remain stored. Each Try again is a separate attempt.`
        : "No generation timing records yet. New attempts will appear here.";
      $("generation-history-export").disabled = !historyRecords.length;
    } catch {
      $("generation-history-status").textContent =
        "Timing history could not be loaded. Retry here; your draft remains available.";
    } finally {
      historyLoading = false;
      $("generation-history-refresh").disabled = false;
    }
  }
  function models() {
    return (data?.connections || []).flatMap((item) =>
      (item.models || [])
        .filter((model) => model.tested)
        .map((model) => ({
          connection_id: item.id,
          connection_name: item.name,
          model: model.id,
          external: item.external,
          consent: item.consent,
        })),
    );
  }
  function fillModels(select, value) {
    const signature = JSON.stringify([
      models().map((m) => [key(m), label(m)]),
      value,
    ]);
    if (select.dataset.signature === signature) return;
    select.dataset.signature = signature;
    select.replaceChildren();
    select.append(new Option("Choose a tested model", ""));
    for (const item of models())
      select.append(new Option(label(item), key(item)));
    if (value && ![...select.options].some((option) => option.value === value))
      select.append(
        new Option("Unavailable selection — choose another model", value),
      );
    select.value = value;
  }
  function nextSelection() {
    if (choices.has(app.date)) return choices.get(app.date);
    const failed = app.data?.generation_attempt;
    if (
      ["failed", "timed_out", "interrupted", "canceled"].includes(
        failed?.state,
      ) &&
      failed.model_selection?.model
    )
      return failed.model_selection;
    return data?.default || null;
  }
  function renderDaily() {
    const pending = pendingChoices.get(app.date);
    if (
      pending &&
      app.data?.generation_attempt?.id === pending.id &&
      app.data.generation_attempt.state === "succeeded"
    ) {
      if (key(choices.get(app.date)) === pending.key) choices.delete(app.date);
      pendingChoices.delete(app.date);
    }
    const selection = nextSelection();
    $("next-generation-model").textContent =
      `Next generation uses ${label(selection)}${choices.has(app.date) ? " · this attempt only" : key(selection) !== key(data?.default) ? " · retry selection" : " · saved default"}`;
    $("generation-model-change").disabled =
      busy || Boolean(app.overview?.active_generation);
    $("generation-model-picker").disabled =
      busy || Boolean(app.overview?.active_generation);
    fillModels($("generation-model-picker"), key(selection));
    $("generation-model-notice").textContent = error
      ? `${error} Existing drafts remain available.`
      : connection(selection?.connection_id)?.external &&
          !connection(selection?.connection_id)?.consent
        ? "This external connection needs consent in Settings → Generation before it can receive activity."
        : connection(selection?.connection_id)?.external
          ? "Activity and link metadata go to this external service. Your manual-note prose and reference-note excerpts stay local."
          : "";
    const provenance = app.data?.generation_provenance;
    $("draft-generation-model").hidden = !app.data?.current_revision;
    $("draft-generation-model").textContent = provenance?.model
      ? `Generated with ${label(provenance)}`
      : app.data?.current_revision?.origin === "manual"
        ? "Manual notes · no model used"
        : app.data?.current_revision?.content?.schema_version === 2
          ? "Activity record · no model used"
          : "Generation model unavailable for this older draft";
    const attempt = app.data?.generation_attempt;
    $("attempt-generation-model").hidden = !attempt?.model_selection?.model;
    $("attempt-generation-model").textContent = attempt?.model_selection?.model
      ? `Attempt used ${label(attempt.model_selection)}`
      : "";
  }
  function editConnection(id = "") {
    selectedConnection = id;
    const item = connection(id);
    $("generation-connection-list").value = id;
    $("generation-connection-name").value = item?.name || "";
    $("generation-endpoint").value = item?.base_url || "";
    $("generation-api-key").value = "";
    $("generation-clear-key").checked = false;
    $("generation-clear-key").closest("label").hidden =
      !item?.credential_present;
    for (const control of [
      $("generation-connection-name"),
      $("generation-endpoint"),
      $("generation-api-key"),
      $("generation-clear-key"),
    ])
      control.disabled = busy || !app.csrf || Boolean(item?.read_only);
    $("generation-save-connection").disabled =
      busy || !app.csrf || Boolean(item?.read_only);
    $("generation-delete-connection").hidden = !item || item.read_only;
    $("generation-delete-connection").disabled =
      busy || data?.default?.connection_id === id;
    $("generation-credential-help").textContent = item?.credential_present
      ? "A credential is configured. Leave the field empty to keep it; it is never returned to your browser."
      : data?.credential_storage_available
        ? "API key is optional for servers that do not require one."
        : "No-key connections work immediately. To save an API key, the administrator must mount LOG_INBOX_LLM_MASTER_KEY_FILE first.";
    $("generation-consent").hidden = !item?.external || item.consent;
    $("generation-consent-confirm").checked = false;
    $("generation-model-tools").hidden = !item;
    $("generation-saved-models").textContent = item
      ? `Saved models: ${
          (item.models || [])
            .filter((m) => m.tested)
            .map((m) => m.id)
            .join(", ") || "None yet"
        }`
      : "";
    $("generation-model-id").value = "";
    $("generation-discovered-models").replaceChildren();
    baseline = fields();
    updateConnectionControls();
  }
  function renderSettings({
    preserveConnection = false,
    preserveDefault = false,
  } = {}) {
    const defaultValue = preserveDefault
      ? $("generation-default").value
      : key(data?.default);
    fillModels($("generation-default"), defaultValue);
    const select = $("generation-connection-list");
    select.replaceChildren(new Option("Add a connection", ""));
    for (const item of data?.connections || [])
      select.append(
        new Option(
          `${item.name}${item.read_only ? " · environment" : ""}`,
          item.id,
        ),
      );
    if (selectedConnection === null)
      selectedConnection = data?.connections[0]?.id || "";
    else if (selectedConnection && !connection(selectedConnection))
      selectedConnection = "";
    if (!preserveConnection) editConnection(selectedConnection);
    else select.value = selectedConnection;
    updateDefaultHelp();
  }
  function updateDefaultHelp() {
    const value = $("generation-default").value;
    const selected = value ? JSON.parse(value) : null;
    const item = connection(selected?.connection_id);
    $("generation-default-help").textContent = item?.external
      ? "Saving this default uses the external service for manual generation and scheduled preparation. Activity and link metadata may leave this server; manual-note prose and reference-note excerpts remain local."
      : "Used for new manual drafts and scheduled preparation. A Daily selection changes one attempt only.";
    $("generation-save-default").disabled =
      !app.csrf ||
      busy ||
      !value ||
      value === key(data?.default) ||
      Boolean(item?.external && !item.consent);
    updateConnectionControls();
  }
  function updateConnectionControls() {
    const item = connection(selectedConnection);
    $("generation-save-connection").disabled =
      busy ||
      !app.csrf ||
      Boolean(item?.read_only) ||
      !$("generation-connection-form").checkValidity() ||
      !connectionDirty();
    for (const id of [
      "generation-test-model",
      "generation-consent-button",
      "generation-discover-models",
    ])
      $(id).disabled = busy || !app.csrf || !item;
    $("generation-delete-connection").disabled =
      busy || !app.csrf || data?.default?.connection_id === selectedConnection;
    $("generation-delete-connection").title =
      data?.default?.connection_id === selectedConnection
        ? "Choose another default before removing this connection"
        : "";
    const dirtyDefault = Boolean(
      data && $("generation-default").value !== key(data.default),
    );
    $("generation-settings-unsaved").textContent = [
      dirtyDefault ? "Unsaved model default" : "",
      connectionDirty() ? "Unsaved connection changes" : "",
    ]
      .filter(Boolean)
      .join(" · ");
  }
  async function load() {
    if (loading) return;
    loading = true;
    try {
      const result = await api("/api/v2/settings/generation");
      const preserveConnection = connectionDirty();
      const preserveDefault = Boolean(
        data && $("generation-default").value !== key(data.default),
      );
      data = {
        ...result,
        connections: result.connections || [],
        default: result.default || null,
      };
      error = "";
      renderSettings({ preserveConnection, preserveDefault });
      feedback("");
    } catch {
      error =
        "Model settings could not be loaded. Retry in Settings → Generation.";
      feedback(error, true);
    } finally {
      loading = false;
      renderDaily();
    }
  }
  async function mutate(path, method, body, message) {
    if (busy || !app.csrf) return null;
    busy = true;
    feedback(
      path.endsWith("/models/test")
        ? "Testing the Daily response contract…"
        : "Saving generation settings…",
    );
    $("settings-generation-title")
      .closest("section")
      .setAttribute("aria-busy", "true");
    try {
      const result = await api(`/api/v2/settings/generation${path}`, {
        method,
        ...(body === undefined ? {} : { body: JSON.stringify(body) }),
      });
      // Each form keeps its own edits when another generation setting saves.
      if (
        path.startsWith("/connections") &&
        (method === "POST" || method === "PUT") &&
        !path.includes("/models") &&
        !path.includes("/consent")
      ) {
        if (result?.id) selectedConnection = result.id;
        $("generation-api-key").value = "";
        $("generation-clear-key").checked = false;
        baseline = "";
      }
      await load();
      feedback(message);
      return result;
    } catch (failure) {
      feedback(failure.message, true);
      return null;
    } finally {
      busy = false;
      $("settings-generation-title")
        .closest("section")
        .removeAttribute("aria-busy");
      for (const control of [
        $("generation-connection-name"),
        $("generation-endpoint"),
        $("generation-api-key"),
        $("generation-clear-key"),
      ])
        control.disabled =
          !app.csrf || Boolean(connection(selectedConnection)?.read_only);
      updateDefaultHelp();
      renderDaily();
      if (connection(selectedConnection)) {
        $("generation-save-connection").disabled =
          !app.csrf ||
          connection(selectedConnection).read_only ||
          !connectionDirty();
        $("generation-delete-connection").disabled =
          data?.default?.connection_id === selectedConnection;
      }
    }
  }
  function connectionDirty() {
    return (
      Boolean(baseline && fields() !== baseline) ||
      $("generation-clear-key").checked
    );
  }
  function hasChanges() {
    return (
      connectionDirty() ||
      Boolean(data && $("generation-default").value !== key(data.default))
    );
  }
  function requestSelection({ retry = false } = {}) {
    if (
      !app.data?.automated_evidence?.events?.length &&
      !app.data?.current_snapshot
    )
      return {};
    if (!app.data?.automated_evidence?.events?.length && !choices.has(app.date))
      return {};
    let selection =
      retry && !choices.has(app.date)
        ? app.data?.generation_attempt?.model_selection
        : nextSelection();
    // No browser selection is sent when settings are unavailable; the server
    // resolves its configured default rather than blocking Daily review.
    if (!selection?.connection_id || !selection?.model) return {};
    return { connection_id: selection.connection_id, model: selection.model };
  }
  function bindEvents() {
    $("generation-timing-history").addEventListener("toggle", () => {
      if ($("generation-timing-history").open) void loadHistory();
    });
    $("generation-history-refresh").addEventListener("click", loadHistory);
    $("generation-history-export").addEventListener("click", () => {
      const url = URL.createObjectURL(
        new Blob([JSON.stringify({ attempts: historyRecords }, null, 2)], {
          type: "application/json",
        }),
      );
      const link = document.createElement("a");
      link.href = url;
      link.download = "generation-timing-records.json";
      link.click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
    });

    $("generation-connection-form").addEventListener(
      "input",
      updateConnectionControls,
    );
    $("generation-connection-form").addEventListener(
      "change",
      updateConnectionControls,
    );
    $("generation-model-change").addEventListener("click", () => {
      $("generation-model-options").hidden = !$("generation-model-options")
        .hidden;
      if (!$("generation-model-options").hidden)
        $("generation-model-picker").focus();
    });
    $("generation-model-picker").addEventListener("change", (event) => {
      if (event.target.value)
        choices.set(app.date, JSON.parse(event.target.value));
      else choices.delete(app.date);
      renderDaily();
    });
    $("generation-model-reset").addEventListener("click", () => {
      if (data?.default) choices.set(app.date, data.default);
      else choices.delete(app.date);
      renderDaily();
    });
    $("generation-manage-link").addEventListener("click", () =>
      openSettings("generation"),
    );
    $("generation-reload-settings").addEventListener("click", load);
    $("generation-default").addEventListener("change", updateDefaultHelp);
    $("generation-default-form").addEventListener("submit", async (event) => {
      event.preventDefault();
      if ($("generation-default").value)
        await mutate(
          "/default",
          "PUT",
          JSON.parse($("generation-default").value),
          "Default saved. Future manual drafts and scheduled preparation use this model.",
        );
    });
    $("generation-connection-list").addEventListener("change", (event) => {
      if (hasChanges() && !confirm("Discard unsaved generation settings?")) {
        event.target.value = selectedConnection;
        return;
      }
      editConnection(event.target.value);
    });
    $("generation-connection-form").addEventListener(
      "submit",
      async (event) => {
        event.preventDefault();
        const body = {
          name: $("generation-connection-name").value.trim(),
          base_url: $("generation-endpoint").value.trim(),
        };
        const secret = $("generation-api-key").value;
        if (secret || $("generation-clear-key").checked) body.api_key = secret;
        const result = await mutate(
          `/connections${selectedConnection ? `/${encodeURIComponent(selectedConnection)}` : ""}`,
          selectedConnection ? "PUT" : "POST",
          body,
          "Connection saved. Test a model before selecting it.",
        );
        if (result) {
          $("generation-api-key").value = "";
          $("generation-clear-key").checked = false;
          renderSettings();
        }
      },
    );
    $("generation-delete-connection").addEventListener("click", async () => {
      if (
        confirm(
          "Remove this connection? Existing drafts and model history will be kept.",
        )
      ) {
        const id = selectedConnection;
        await mutate(
          `/connections/${encodeURIComponent(id)}`,
          "DELETE",
          undefined,
          "Connection removed.",
        );
      }
    });
    $("generation-consent-button").addEventListener("click", async () => {
      if (!$("generation-consent-confirm").checked) {
        feedback(
          "Confirm the activity data disclosure before allowing this external connection.",
          true,
        );
        return;
      }
      await mutate(
        `/connections/${encodeURIComponent(selectedConnection)}/consent`,
        "PUT",
        { consent: true },
        "External generation allowed for this connection, including scheduled preparation when selected as default.",
      );
    });
    $("generation-discover-models").addEventListener("click", async () => {
      try {
        const result = await api(
          `/api/v2/settings/generation/connections/${encodeURIComponent(selectedConnection)}/models`,
          { method: "POST", body: JSON.stringify({}) },
        );
        $("generation-discovered-models").replaceChildren(
          ...result.models.map((id) => new Option(id, id)),
        );
        feedback(
          `${result.models.length} models found. Choose or enter an ID, then Test & save model. Discovery alone does not establish compatibility.`,
        );
      } catch (failure) {
        feedback(
          `${failure.message} You can still enter a model ID manually. Install local models on the model server.`,
          true,
        );
      }
    });
    $("generation-model-test-form").addEventListener(
      "submit",
      async (event) => {
        event.preventDefault();
        await mutate(
          `/connections/${encodeURIComponent(selectedConnection)}/models/test`,
          "POST",
          { model: $("generation-model-id").value.trim() },
          "Model tested and saved. It is now available in Daily and the default selector.",
        );
      },
    );
  }
  return {
    bindEvents,
    load,
    renderDaily,
    requestSelection,
    started: (date, attempt) => {
      if (choices.has(date))
        pendingChoices.set(date, {
          id: attempt.id,
          key: key(choices.get(date)),
        });
    },
    hasChanges,
    discard: () => {
      if (data) renderSettings();
    },
    get saving() {
      return busy;
    },
  };
}
