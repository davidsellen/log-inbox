import {
  $,
  localDate,
  validDate,
  el,
  action,
  identityLabel,
} from "./daily-helpers.js";
import { createNavigation } from "./daily-navigation.js";
import { createReferenceNotes } from "./daily-references.js";
import { createSettings } from "./daily-settings.js";
// Daily owns the selected day, draft edits, generation, and polling.
const state = {
  csrf: null,
  data: null,
  overview: null,
  contextDetails: null,
  contextDraft: null,
  comparison: null,
  comparisonDecision: null,
  comparisonRevisionId: null,
  date: "",
  view: "daily",
  saving: false,
  applyPreview: null,
};
let dayLoadVersion = 0;
const notice = (message, error = false) => {
  const el = $("notice");
  el.textContent = message || "";
  el.className = `notice${error ? " error" : ""}`;
  el.hidden = !message;
};

// Session and requests: every mutation passes through this CSRF boundary.
async function api(path, options = {}) {
  const mutation = options.method && options.method !== "GET";
  if (
    mutation &&
    /^\/api\/v2\/daily\//.test(path) &&
    (state.loadingDay || state.dayLoadError)
  )
    throw new Error("Wait until this day has loaded before making changes.");
  const headers = {
    "Content-Type": "application/json",
    ...(options.headers || {}),
  };
  if (mutation && state.csrf) headers["X-CSRF-Token"] = state.csrf;
  if (mutation) state.pendingWrites = (state.pendingWrites || 0) + 1;
  try {
    const response = await fetch(path, { ...options, headers });
    const body =
      response.status === 204
        ? null
        : await response
            .json()
            .catch(() => ({ error: `Request failed (${response.status})` }));
    if (!response.ok) {
      const error = new Error(
        body?.error || `Request failed (${response.status})`,
      );
      error.status = response.status;
      throw error;
    }
    return body;
  } finally {
    if (mutation) state.pendingWrites--;
  }
}

function shiftDay(amount) {
  const date = new Date(`${state.date}T00:00:00Z`);
  date.setUTCDate(date.getUTCDate() + amount);
  selectDay(date.toISOString().slice(0, 10));
}

async function boot() {
  const url = new URL(location.href);
  const requested = url.searchParams.get("date");
  const requestedDate = validDate(requested) ? requested : null;
  const requestedView =
    url.searchParams.get("view") === "knowledge" ? "knowledge" : "daily";
  try {
    const session = await api("/api/v2/auth/session");
    state.csrf = session.csrf_token;
    $("login-panel").hidden = true;
    $("unlock").hidden = Boolean(state.csrf);
    $("settings").hidden = false;
    $("settings").disabled = !state.csrf;
    await settings.loadWorkspaceSettings();
    if (
      !settings.workspaceSettings.active_profile ||
      !settings.workspaceSettings.binding_matches
    ) {
      await selectView(requestedView, false);
      $("daily-app").hidden = false;
      const message = settings.workspaceSettings.active_profile
        ? "The mounted workspace changed. Review workspace settings before choosing Knowledge folders."
        : "Review and save workspace settings before choosing Knowledge folders.";
      referenceNotes.knowledgeNotice(message, true);
      notice(
        settings.workspaceSettings.active_profile
          ? "The mounted workspace changed. Review Daily settings before continuing."
          : "Review and save Daily settings before generating your first record.",
        true,
      );
      if (state.csrf) settings.openSettings();
      return;
    }
    await settings.loadAutomation();
    state.overview = await api("/api/v2/daily/overview");
    state.date = requestedDate || state.overview.today;
    $("day").value = state.date;
    await settings.loadMigration();
    await loadDay();
    await selectView(requestedView, false);
    $("daily-app").hidden = false;
    if (
      settings.migration?.cutover_status !== "completed" &&
      settings.migration?.items?.length
    ) {
      notice(
        "Older Log Inbox data is waiting in Settings. Review the migration before relying on this Daily view.",
        true,
      );
    }
  } catch (error) {
    if (error.status === 401) showLogin();
    else {
      $("daily-app").hidden = false;
      notice(error.message, true);
    }
  }
}

function showLogin() {
  $("login-panel").hidden = false;
  $("daily-app").hidden = true;
  $("unlock").hidden = true;
  $("owner-secret").focus();
}

// Loading and rendering the selected day. Polling uses the same load version below.
async function loadDay() {
  const date = state.date;
  const version = ++dayLoadVersion;
  notice("");
  clearTimeout(refreshTimer);
  setDayLoadState(true);
  try {
    const [data, overview, contextDetails] = await Promise.all([
      api(`/api/v2/daily/${date}`),
      api("/api/v2/daily/overview"),
      api(`/api/v2/daily/${date}/context`).catch((error) => ({
        status: "unavailable",
        message: error.message,
        workstreams: [],
      })),
    ]);
    if (version !== dayLoadVersion || date !== state.date) return false;
    state.data = data;
    state.overview = overview;
    state.contextDetails = contextDetails;
    state.applyPreview = null;
    state.contextDraft = null;
    setDayLoadState(false);
    render();
    return true;
  } catch (error) {
    if (version === dayLoadVersion && date === state.date)
      setDayLoadState(false, error.message);
    return false;
  } finally {
    if (version === dayLoadVersion) scheduleRefresh();
  }
}

const statusLabels = {
  missed: "Needs review",
  not_started: "Not started",
  notes_unreviewed: "Notes to review",
  scheduled: "Scheduled",
  generating: "Generating",
  generation_failed: "Generation failed",
  in_review: "Review",
  update_available: "Update available",
  applied: "Applied",
  dismissed: "Dismissed",
  apply_attention: "Apply needs attention",
};

function renderOverview() {
  const overview = state.overview;
  const root = $("recent-days");
  root.replaceChildren();
  if (!overview) return;
  const summary = [];
  if (overview.missed_count) summary.push(`${overview.missed_count} missed`);
  if (overview.update_count)
    summary.push(`${overview.update_count} with updates`);
  if (overview.failed_count) summary.push(`${overview.failed_count} failed`);
  $("missed-summary").textContent = summary.join(" · ") || overview.timezone;
  for (const day of overview.days) {
    const button = document.createElement("button");
    button.className = `recent-day ${day.status}`;
    button.setAttribute(
      "aria-current",
      day.local_date === state.date ? "date" : "false",
    );
    const date = document.createElement("span");
    date.textContent = new Intl.DateTimeFormat(undefined, {
      weekday: "short",
      month: "short",
      day: "numeric",
      timeZone: "UTC",
    }).format(new Date(`${day.local_date}T00:00:00Z`));
    const status = document.createElement("span");
    status.className = "day-state";
    const expiry = day.expired_evidence_count
      ? ` · ${day.expired_evidence_count} expired`
      : "";
    status.textContent = `${statusLabels[day.status] || day.status}${expiry}`;
    button.append(date, status);
    button.addEventListener("click", () => selectDay(day.local_date));
    root.append(button);
  }
  $("recent").hidden = !overview.days.length;
}

function renderDaily() {
  const data = state.data;
  renderOverview();
  $("day-title").textContent = new Date(
    `${data.local_date}T12:00:00`,
  ).toLocaleDateString(undefined, {
    weekday: "long",
    year: "numeric",
    month: "long",
    day: "numeric",
  });
  $("destination").textContent =
    `Markdown destination: ${data.destination_path}`;
  const freshness =
    data.candidate_freshness === "update_available"
      ? " · New evidence is available"
      : "";
  const expiry = data.expired_evidence_count
    ? ` · ${data.expired_evidence_count} source event${data.expired_evidence_count === 1 ? " has" : "s have"} expired`
    : "";
  const dismissed = data.day?.review_status === "dismissed";
  $("status").textContent = data.current_revision
    ? `Revision ${data.current_revision.revision_number}${dismissed ? " · Dismissed" : ""}${freshness}${expiry}`
    : "No candidate yet";
  $("dismiss").hidden = !data.current_revision;
  $("dismiss").textContent = dismissed ? "Reopen" : "Dismiss";
  $("dismiss").disabled = !state.csrf || state.saving;
  $("generate").textContent = data.current_revision
    ? "Regenerate"
    : "Generate candidate";
  $("generate").disabled = !state.csrf || state.saving || dismissed;
  $("add-note").disabled = !state.csrf;
  renderManual(data.manual_entries || []);
  renderCandidate(data.current_revision?.content);
  renderEvidence(
    data.automated_evidence?.events || [],
    data.current_snapshot_evidence || [],
    data.active_late_evidence_deferrals || [],
  );
  renderContext();
  $("preview").textContent =
    data.preview_markdown ||
    "Generate a candidate to see the exact Markdown preview.";
  const apply = data.apply_status;
  const pending = apply && apply.state !== "finalized";
  const invalidContext = state.contextDetails?.status === "invalid";
  $("write-status").textContent =
    apply?.state === "finalized"
      ? "Applied to Markdown"
      : pending
        ? "Apply needs attention"
        : dismissed
          ? "Dismissed — nothing will be written"
          : "Nothing is written yet";
  $("apply-attention").hidden = !pending;
  if (pending) {
    $("apply-attention-message").textContent =
      apply.state === "failed"
        ? "The write stopped before Log Inbox could replace the note."
        : apply.state === "reconciliation_required"
          ? "The note no longer matches the exact state approved for Apply."
          : "An interrupted Apply is waiting to be verified.";
    $("apply-attention-detail").textContent =
      apply.failure_reason ||
      "Log Inbox will verify the journal, destination, and owned temporary file before doing anything.";
    $("retry-apply").hidden = !apply.can_retry;
    $("retry-apply").disabled = !state.csrf || state.saving;
  }
  $("review-apply").hidden = !data.current_revision || dismissed;
  $("review-apply").disabled =
    !state.csrf || state.saving || pending || invalidContext;
}

// Frozen reference context belongs to the Daily revision, not reference setup.
function contextReason(note) {
  if (note.attached === false) return "Removed from the current candidate";
  const fields = (note.matched_fields || []).map(identityLabel).join(", ");
  if (note.reason === "saved_mapping" || note.reason === "mapping")
    return fields ? `Saved link for ${fields}` : "Saved link";
  return fields ? `Exact note match for ${fields}` : "Exact note match";
}

function contextKey(workstreamId, notePath) {
  return JSON.stringify([workstreamId, notePath]);
}

function initContextDraft() {
  const revisionId = state.data?.current_revision?.id;
  if (state.contextDraft?.revisionId === revisionId) return;
  const excluded = new Set();
  for (const workstream of state.contextDetails?.workstreams || [])
    for (const note of workstream.notes || [])
      if (note.excluded) excluded.add(contextKey(workstream.id, note.path));
  state.contextDraft = { revisionId, baseline: new Set(excluded), excluded };
}

function contextDraftChanged() {
  const draft = state.contextDraft;
  if (!draft || draft.baseline.size !== draft.excluded.size)
    return Boolean(draft);
  for (const key of draft.baseline) if (!draft.excluded.has(key)) return true;
  return false;
}

function contextComparisonEligible() {
  const revision = state.data?.current_revision;
  const status = state.contextDetails?.status;
  return Boolean(
    state.csrf &&
    revision &&
    ["generated", "regenerated"].includes(revision.origin) &&
    state.data?.current_context_snapshot?.used_note_count &&
    ["current", "changed"].includes(status) &&
    state.data?.day?.review_status === "in_review" &&
    !state.data?.apply_status &&
    !contextDraftChanged() &&
    $("save-candidate").hidden,
  );
}

function updateContextAdjustmentControls() {
  const changed = contextDraftChanged();
  const button = $("regenerate-context");
  button.hidden = !changed;
  button.disabled = !state.csrf || state.saving;
  const dismissed = state.data?.day?.review_status === "dismissed";
  $("generate").disabled = !state.csrf || state.saving || dismissed || changed;
  const status = $("context-adjustment-status");
  status.hidden = !changed;
  status.textContent = changed
    ? "Pending choices are not saved yet. Regenerate to create a new candidate with them."
    : "";
  const compare = $("compare-context");
  compare.hidden = !contextComparisonEligible();
  compare.disabled = state.saving;
}

function toggleContextExcerpt(workstreamId, notePath, include) {
  const key = contextKey(workstreamId, notePath);
  if (include) state.contextDraft.excluded.delete(key);
  else state.contextDraft.excluded.add(key);
  updateContextAdjustmentControls();
}

function renderContext() {
  const card = $("context-card");
  const details = state.contextDetails;
  const snapshot = state.data?.current_context_snapshot;
  if (!state.data?.current_revision) {
    card.hidden = true;
    return;
  }
  card.hidden = false;
  initContextDraft();
  const status = details?.status || "unavailable";
  const badge = $("context-status");
  badge.textContent =
    status === "current"
      ? "Up to date"
      : status === "changed"
        ? "Setup changed"
        : status === "invalid"
          ? "Source unavailable"
          : status === "none"
            ? "No Knowledge"
            : "Check unavailable";
  badge.className = `pill${status === "current" || status === "none" ? "" : " paused"}`;
  $("context-kind").textContent =
    details?.mode === "bounded_knowledge"
      ? "Canonical links and bounded note excerpts informed this revision."
      : "Canonical links only · no note text was used.";
  $("context-message").textContent =
    details?.message ||
    "Current Knowledge could not be checked; the candidate keeps its frozen context.";
  const root = $("context-list");
  root.replaceChildren();
  for (const workstream of details?.workstreams || []) {
    const section = el("div", "context-workstream");
    const title =
      state.data.current_revision.content?.workstreams?.find(
        (item) => item.id === workstream.id,
      )?.title || "Workstream";
    section.append(el("strong", "", title));
    for (const note of workstream.notes || []) {
      section.append(
        el("div", "", note.title),
        el("div", "subtle", `${note.path} · ${contextReason(note)}`),
      );
      if (details.mode === "bounded_knowledge") {
        const choice = el("label", "check-field");
        const input = document.createElement("input");
        input.type = "checkbox";
        input.checked = !state.contextDraft.excluded.has(
          contextKey(workstream.id, note.path),
        );
        input.disabled = !state.csrf || state.saving;
        input.addEventListener("change", () =>
          toggleContextExcerpt(workstream.id, note.path, input.checked),
        );
        choice.append(
          input,
          el("span", "", "Use this note in the next generation"),
        );
        section.append(choice);
      }
      if (note.excerpt?.text) {
        const excerpt = document.createElement("details");
        const summary = document.createElement("summary");
        summary.textContent = "Excerpt sent to the model";
        excerpt.append(
          summary,
          el("div", "context-excerpt", note.excerpt.text),
        );
        section.append(excerpt);
      } else if (note.excluded)
        section.append(el("div", "subtle", "Not sent for this revision."));
    }
    root.append(section);
  }
  if (!root.children.length)
    root.append(
      el(
        "div",
        "empty",
        snapshot?.diagnostics?.resolution_failed
          ? "Knowledge matching was unavailable; Daily evidence still produced this candidate."
          : "This revision used Daily evidence and your notes only.",
      ),
    );
  updateContextAdjustmentControls();
  $("context-technical").hidden = !snapshot;
  $("context-technical-text").textContent = snapshot
    ? `Frozen with revision ${state.data.current_revision.revision_number} · resolver ${snapshot.resolver_version || "unknown"} · ${snapshot.excerpt_count || 0} excerpt${snapshot.excerpt_count === 1 ? "" : "s"} · snapshot ${snapshot.snapshot_digest.slice(0, 12)}`
    : "";
}

// Manual notes and the structured draft editor.
function renderManual(entries) {
  const root = $("manual-list");
  root.replaceChildren();
  if (!entries.length) {
    const p = document.createElement("div");
    p.className = "empty";
    p.textContent = "No manual notes for this day.";
    root.append(p);
    return;
  }
  for (const entry of entries) {
    const row = document.createElement("div");
    row.className = "manual-item";
    const text = document.createElement("div");
    text.textContent = entry.text;
    row.append(text);
    if (entry.references.length) {
      const refs = document.createElement("div");
      refs.className = "references";
      for (const url of entry.references) {
        const a = document.createElement("a");
        a.href = url;
        a.target = "_blank";
        a.rel = "noreferrer";
        a.textContent = url;
        refs.append(a);
      }
      row.append(refs);
    }
    const actions = document.createElement("div");
    actions.className = "dialog-actions";
    actions.append(
      action("Delete", () => deleteManual(entry), {
        disabled: !state.csrf || state.saving,
      }),
    );
    row.append(actions);
    root.append(row);
  }
}

async function deleteManual(entry) {
  if (
    !confirm(
      "Delete this manual note? Existing candidate history is preserved, and a candidate using it will need to be regenerated.",
    )
  )
    return;
  state.saving = true;
  render();
  try {
    await api(
      `/api/v2/daily/${state.date}/manual/${encodeURIComponent(entry.id)}`,
      { method: "DELETE" },
    );
    await loadDay();
    notice(
      "Manual note deleted. Regenerate the candidate if it included this note.",
    );
  } catch (error) {
    notice(error.message, true);
  } finally {
    state.saving = false;
    if (state.data) render();
  }
}

const fieldLabels = {
  outcome: "Outcome",
  decision: "Decision",
  trade_off: "Trade-off",
  validation: "Validation",
  blocker: "Blocker",
  follow_up: "Follow-up",
  activity: "Activity",
};

function renderCandidateEditor(content) {
  const root = $("candidate");
  root.replaceChildren();
  $("save-candidate").hidden = true;
  if (!content) {
    const p = document.createElement("div");
    p.className = "empty";
    p.textContent =
      "No generated candidate. Your notes and evidence remain available.";
    root.append(p);
    return;
  }
  if (!content.workstreams.length) {
    const p = document.createElement("div");
    p.className = "empty";
    p.textContent = "This candidate contains only your notes.";
    root.append(p);
    return;
  }
  content.workstreams.forEach((workstream, index) => {
    const section = document.createElement("section");
    section.className = "workstream";
    const title = document.createElement("input");
    title.className = "title-input";
    title.value = workstream.title;
    title.disabled = !state.csrf;
    title.setAttribute("aria-label", `Workstream ${index + 1} title`);
    title.addEventListener("input", () => {
      $("save-candidate").hidden = false;
      updateContextAdjustmentControls();
    });
    section.append(title);
    for (const [key, label] of Object.entries(fieldLabels)) {
      for (const fact of workstream[key] || []) {
        const row = document.createElement("div");
        row.className = "fact";
        const lab = document.createElement("label");
        lab.textContent = label;
        const input = document.createElement("textarea");
        input.rows = 2;
        input.value = fact.text;
        input.disabled = !state.csrf;
        input.dataset.workstream = String(index);
        input.dataset.field = key;
        input.dataset.fact = String((workstream[key] || []).indexOf(fact));
        input.addEventListener("input", () => {
          $("save-candidate").hidden = false;
          updateContextAdjustmentControls();
        });
        row.append(lab, input);
        section.append(row);
      }
    }
    root.append(section);
  });
}

// Activity browsing and optional include/omit decisions.
function renderEvidence(events, decisions, deferrals) {
  const root = $("evidence");
  root.replaceChildren();
  const decisionsById = new Map(decisions.map((item) => [item.event_id, item]));
  const deferralsById = new Map(deferrals.map((item) => [item.event_id, item]));
  const deferredCount = deferrals.length;
  $("evidence-count").textContent =
    `${events.length} events${deferredCount ? ` · ${deferredCount} left for later` : ""}`;
  if (!events.length) {
    root.append(el("div", "empty", "No automated evidence for this day."));
    filterActivity();
    return;
  }
  if (decisions.length) {
    const toolbar = el("div", "evidence-toolbar");
    const status = el("span", "subtle");
    status.dataset.evidencePending = "";
    const includeAll = action(
      "Include all",
      () => stageAllEvidence("include"),
      { disabled: !state.csrf },
    );
    const omitAll = action("Omit all", () => stageAllEvidence("omit"), {
      disabled: !state.csrf,
    });
    const applyAll = action("Apply all", applyAllEvidence, {
      primary: true,
      disabled: !state.csrf,
    });
    applyAll.dataset.applyAllEvidence = "";
    toolbar.append(status, includeAll, omitAll, applyAll);
    root.append(toolbar);
  }
  for (const event of events) {
    const row = el("div", "evidence-item");
    row.tabIndex = -1;
    row.dataset.activitySearch =
      `${event.message} ${event.source}`.toLowerCase();
    const message = el("div", "evidence-message", event.message);
    const meta = el(
      "div",
      "evidence-meta",
      `${event.source} · ${new Date(event.timestamp).toLocaleTimeString()}`,
    );
    row.append(message, meta);
    const controls = el("div", "evidence-controls");
    if (deferralsById.has(event.id)) {
      const reopen = action(
        "Reopen",
        () => toggleLateEvidence(event.id, true),
        { disabled: !state.csrf },
      );
      controls.append(
        el("span", "subtle", "Left for later — not part of this revision"),
        reopen,
      );
      row.append(controls);
    } else if (decisionsById.has(event.id)) {
      const decision = decisionsById.get(event.id);
      const select = document.createElement("select");
      select.disabled = !state.csrf;
      select.dataset.evidenceId = event.id;
      select.dataset.savedDisposition = decision?.disposition || "include";
      select.setAttribute(
        "aria-label",
        `Evidence decision: ${event.message.slice(0, 80)}`,
      );
      for (const [value, label] of [
        ["include", "Include"],
        ["omit", "Omit"],
      ]) {
        const option = document.createElement("option");
        option.value = value;
        option.textContent = label;
        select.append(option);
      }
      if (
        decision?.disposition &&
        !["include", "omit"].includes(decision.disposition)
      ) {
        const option = document.createElement("option");
        option.value = decision.disposition;
        option.textContent = `${decision.disposition.replaceAll("_", " ")} (saved)`;
        select.append(option);
      }
      select.value = decision?.disposition || "include";
      const apply = action(
        "Update",
        () => decideEvidence(event.id, select.value),
        { disabled: true },
      );
      select.addEventListener("change", () => {
        apply.disabled =
          !state.csrf || select.value === select.dataset.savedDisposition;
        refreshEvidenceToolbar();
      });
      controls.append(select, apply);
      row.append(controls);
    } else if (state.data.current_revision) {
      const defer = action(
        "Leave for later",
        () => toggleLateEvidence(event.id, false),
        { disabled: !state.csrf },
      );
      controls.append(
        el("span", "subtle", "Arrived after this revision"),
        defer,
      );
      row.append(controls);
    }
    root.append(row);
  }
  refreshEvidenceToolbar();
  filterActivity();
}

function activityRows() {
  return [...document.querySelectorAll("#evidence .evidence-item")].filter(
    (row) => !row.hidden,
  );
}

function filterActivity() {
  const query = $("activity-search").value.trim().toLowerCase();
  for (const row of document.querySelectorAll("#evidence .evidence-item")) {
    row.hidden = !row.dataset.activitySearch.includes(query);
    row.removeAttribute("aria-current");
  }
  const rows = activityRows();
  $("activity-position").textContent = `${rows.length} matching items`;
  $("activity-empty").hidden = rows.length > 0;
  $("activity-previous").disabled = !rows.length;
  $("activity-next").disabled = !rows.length;
  $("evidence").scrollTop = 0;
}

function moveActivity(direction) {
  const rows = activityRows();
  if (!rows.length) return;
  const current = rows.findIndex(
    (row) => row.getAttribute("aria-current") === "true",
  );
  const index =
    current < 0
      ? direction > 0
        ? 0
        : rows.length - 1
      : Math.max(0, Math.min(rows.length - 1, current + direction));
  rows.forEach((row) => row.removeAttribute("aria-current"));
  const row = rows[index];
  row.setAttribute("aria-current", "true");
  const panel = $("evidence");
  panel.scrollTop +=
    row.getBoundingClientRect().top - panel.getBoundingClientRect().top - 5;
  row.focus({ preventScroll: true });
  $("activity-position").textContent = `Item ${index + 1} of ${rows.length}`;
}

function evidenceDecisionSelects() {
  return [...document.querySelectorAll("#evidence select[data-evidence-id]")];
}

function refreshEvidenceToolbar() {
  const status = document.querySelector("[data-evidence-pending]");
  const apply = document.querySelector("[data-apply-all-evidence]");
  if (!status || !apply) return;
  const pending = evidenceDecisionSelects().filter(
    (select) => select.value !== select.dataset.savedDisposition,
  ).length;
  status.textContent = pending
    ? `${pending} change${pending === 1 ? "" : "s"} will be used when saving`
    : "All activity is included unless you omit it. No item-by-item review required.";
  apply.disabled = !state.csrf || state.saving || pending === 0;
}

function stageAllEvidence(disposition) {
  for (const select of evidenceDecisionSelects()) {
    select.value = disposition;
    select.dispatchEvent(new Event("change"));
  }
  refreshEvidenceToolbar();
}

async function saveEvidenceChanges() {
  const decisions = evidenceDecisionSelects()
    .filter((select) => select.value !== select.dataset.savedDisposition)
    .map((select) => ({
      event_id: select.dataset.evidenceId,
      disposition: select.value,
    }));
  if (!decisions.length) return 0;
  await api(`/api/v2/daily/${state.date}/evidence`, {
    method: "PUT",
    body: JSON.stringify({
      expected_revision_id: state.data.current_revision.id,
      decisions,
    }),
  });
  await loadDay();
  return decisions.length;
}

async function applyAllEvidence() {
  state.saving = true;
  refreshEvidenceToolbar();
  try {
    const count = await saveEvidenceChanges();
    if (count)
      notice(`Applied ${count} evidence decision${count === 1 ? "" : "s"}.`);
  } catch (error) {
    notice(error.message, true);
  } finally {
    state.saving = false;
    refreshEvidenceToolbar();
  }
}

async function decideEvidence(eventId, disposition) {
  if (!disposition) return reopenEvidence(eventId);
  try {
    await api(
      `/api/v2/daily/${state.date}/evidence/${encodeURIComponent(eventId)}`,
      {
        method: "PUT",
        body: JSON.stringify({
          expected_revision_id: state.data.current_revision.id,
          disposition,
        }),
      },
    );
    await loadDay();
  } catch (error) {
    notice(error.message, true);
  }
}

async function reopenEvidence(eventId) {
  try {
    await api(
      `/api/v2/daily/${state.date}/evidence/${encodeURIComponent(eventId)}`,
      {
        method: "DELETE",
        body: JSON.stringify({
          expected_revision_id: state.data.current_revision.id,
        }),
      },
    );
    await loadDay();
  } catch (error) {
    notice(error.message, true);
  }
}

async function toggleLateEvidence(eventId, reopen) {
  try {
    await api(
      `/api/v2/daily/${state.date}/late-evidence/${encodeURIComponent(eventId)}`,
      {
        method: reopen ? "DELETE" : "POST",
        body: JSON.stringify({
          expected_revision_id: state.data.current_revision.id,
        }),
      },
    );
    await loadDay();
    notice(
      reopen
        ? "Late evidence reopened. Regenerate after restoring any expired source evidence."
        : "Late evidence left for later. The preserved revision can now be reviewed and applied.",
    );
  } catch (error) {
    notice(error.message, true);
  }
}

// Generation and comparison commands. These never Apply Markdown automatically.
async function generate(referenceMode) {
  if (state.saving) return;
  if (hasPendingChanges()) {
    notice(
      "Save or cancel your pending edits and choices before preparing a new draft.",
      true,
    );
    return;
  }
  const withoutReferences = referenceMode === "none";
  const editedWithNewEvidence =
    state.data?.current_revision?.origin === "structured_edit" &&
    (state.data?.current_revision?.content?.schema_version === 2 ||
      state.data?.candidate_freshness === "update_available" ||
      withoutReferences);
  if (
    editedWithNewEvidence &&
    !confirm(
      "Regenerating will replace your structured edits with a new draft. Continue?",
    )
  )
    return;
  const date = state.date;
  state.retryError = null;
  state.saving = true;
  render();
  notice("Starting a new preparation attempt…");
  try {
    const result = await api(`/api/v2/daily/${date}/generate`, {
      method: "POST",
      body: JSON.stringify({
        replace_edited: editedWithNewEvidence,
        ...(withoutReferences ? { reference_mode: "none" } : {}),
      }),
    });
    if (state.date !== date) return;
    if (result.attempt) {
      state.data.generation_attempt = result.attempt;
      state.overview.active_generation = result.attempt;
      renderGeneration();
    }
    await loadDay();
    if (!result.attempt) notice("Draft ready. Review it before saving.");
  } catch (error) {
    if (state.date === date) {
      state.retryError = {
        date,
        message: `New attempt could not start: ${error.message}`,
      };
      notice(state.retryError.message, true);
    }
  } finally {
    state.saving = false;
    if (state.data) render();
  }
}

async function regenerateWithContext() {
  if (!contextDraftChanged()) return;
  const replacesEdits =
    state.data?.current_revision?.origin === "structured_edit";
  if (
    replacesEdits &&
    !confirm(
      "Regenerating with these context choices will replace your structured edits with a new candidate. Continue?",
    )
  )
    return;
  const context_exclusions = [...state.contextDraft.excluded].map((key) => {
    const [workstream_id, note_path] = JSON.parse(key);
    return { workstream_id, note_path };
  });
  state.saving = true;
  render();
  notice("");
  try {
    await api(`/api/v2/daily/${state.date}/generate`, {
      method: "POST",
      body: JSON.stringify({
        replace_edited: replacesEdits,
        expected_revision_id: state.contextDraft.revisionId,
        context_exclusions,
      }),
    });
    await loadDay();
  } catch (error) {
    notice(error.message, true);
  } finally {
    state.saving = false;
    if (state.data) render();
  }
}

function resetComparisonDialog() {
  state.comparison = null;
  state.comparisonDecision = null;
  $("comparison-form").reset();
  $("comparison-note").value = "";
  $("comparison-progress").textContent =
    "Creating two private comparison drafts…";
  $("comparison-progress").className = "notice";
  $("comparison-progress").hidden = false;
  $("comparison-arms").hidden = true;
  $("comparison-form").hidden = true;
  $("comparison-result").hidden = true;
  $("comparison-finished-actions").hidden = true;
  $("comparison-retry-actions").hidden = true;
}

function comparisonArm(id) {
  return (state.comparison?.arms || []).find(
    (arm) => String(arm.id || arm.label || "").toLowerCase() === id,
  );
}

function renderComparison() {
  const a = comparisonArm("a");
  const b = comparisonArm("b");
  if (!state.comparison?.id || !a || !b)
    throw new Error("The comparison response is incomplete.");
  $("draft-a-preview").textContent =
    a.preview_markdown || "No preview was returned.";
  $("draft-b-preview").textContent =
    b.preview_markdown || "No preview was returned.";
  $("comparison-progress").hidden = true;
  $("comparison-arms").hidden = false;
  $("comparison-form").hidden = false;
  $("comparison-form").querySelector("input").focus();
}

async function startComparison() {
  if (!contextComparisonEligible()) return;
  const revisionId = state.data.current_revision.id;
  resetComparisonDialog();
  state.comparisonRevisionId = revisionId;
  if (!$("comparison-dialog").open) openDialog($("comparison-dialog"));
  $("compare-context").disabled = true;
  try {
    const result = await api(
      `/api/v2/daily/${state.date}/context-comparisons`,
      {
        method: "POST",
        body: JSON.stringify({ expected_revision_id: revisionId }),
      },
    );
    state.comparison = result.comparison || result;
    renderComparison();
  } catch (error) {
    $("comparison-progress").textContent =
      `Comparison could not be created: ${error.message}`;
    $("comparison-progress").className = "notice error";
    $("comparison-retry-actions").hidden = false;
  } finally {
    $("compare-context").disabled = false;
  }
}

function comparisonAssignment(result) {
  return (
    result.assignment ||
    result.arm_assignment ||
    state.comparison?.assignment ||
    {}
  );
}

async function submitComparison(event) {
  event.preventDefault();
  if (!state.comparison || !$("comparison-form").reportValidity()) return;
  const form = new FormData($("comparison-form"));
  const body = {
    expected_revision_id: state.comparisonRevisionId,
    usefulness: form.get("usefulness"),
    less_editing: form.get("less_editing"),
    continue_with: form.get("continue_with"),
    note: $("comparison-note").value.trim(),
  };
  $("save-comparison").disabled = true;
  $("comparison-result").hidden = true;
  try {
    const result = await api(
      `/api/v2/daily/${state.date}/context-comparisons/${encodeURIComponent(state.comparison.id)}/decision`,
      { method: "POST", body: JSON.stringify(body) },
    );
    state.comparisonDecision = result;
    const assignment = comparisonAssignment(result);
    const withContext = ["a", "b"].find(
      (id) =>
        assignment[id] === "with_context" || assignment[id] === "knowledge",
    );
    const withoutContext = ["a", "b"].find(
      (id) =>
        assignment[id] === "without_context" ||
        assignment[id] === "no_knowledge",
    );
    const reveal =
      withContext && withoutContext
        ? `Draft ${withContext.toUpperCase()} used Knowledge; Draft ${withoutContext.toUpperCase()} did not. `
        : "";
    $("comparison-result").textContent =
      `${reveal}Draft ${String(body.continue_with).toUpperCase()} is now your current candidate.`;
    $("comparison-result").className = "notice";
    $("comparison-result").hidden = false;
    $("comparison-form").hidden = true;
    $("comparison-finished-actions").hidden = false;
    await loadDay();
  } catch (error) {
    $("comparison-result").textContent =
      `Your choice could not be saved: ${error.message}`;
    $("comparison-result").className = "notice error";
    $("comparison-result").hidden = false;
  } finally {
    $("save-comparison").disabled = false;
  }
}

function closeComparison() {
  if ($("comparison-dialog").open) closeDialog($("comparison-dialog"));
  if (state.comparisonDecision)
    notice("Comparison saved. Review the selected candidate before Apply.");
}

// Saving a revision is separate from reviewing and applying its Markdown block.
async function saveCandidate() {
  const content = structuredClone(state.data.current_revision.content);
  document
    .querySelectorAll("#candidate .title-input")
    .forEach((input, index) => {
      content.workstreams[index].title = input.value;
    });
  document.querySelectorAll("#candidate textarea").forEach((input) => {
    content.workstreams[Number(input.dataset.workstream)][input.dataset.field][
      Number(input.dataset.fact)
    ].text = input.value;
  });
  try {
    await api(`/api/v2/daily/${state.date}/candidate`, {
      method: "PUT",
      body: JSON.stringify({
        expected_revision_id: state.data.current_revision.id,
        content,
      }),
    });
    state.editingDraft = false;
    await loadDay();
    notice("Edits saved as a new revision.");
  } catch (error) {
    notice(error.message, true);
  }
}

async function reviewApply() {
  if (state.saving) return;
  if (!$("save-candidate").hidden) {
    notice("Save your candidate edits before reviewing Apply.", true);
    return;
  }
  state.saving = true;
  $("review-apply").disabled = true;
  try {
    await saveEvidenceChanges();
    state.applyPreview = await api(`/api/v2/daily/${state.date}/apply-preview`);
    $("apply-target").textContent =
      `Destination: ${state.applyPreview.destination_path}${state.applyPreview.will_create_note ? " · new note" : ""}`;
    $("apply-before").textContent =
      state.applyPreview.previous_block || "(No managed block yet)";
    $("apply-after").textContent = state.applyPreview.next_block;
    openDialog($("apply-dialog"));
  } catch (error) {
    notice(error.message, true);
  } finally {
    state.saving = false;
    $("review-apply").disabled = !state.csrf;
  }
}

async function confirmApply() {
  if (!state.applyPreview) return;
  const preview = state.applyPreview;
  $("confirm-apply").disabled = true;
  try {
    const result = await api(`/api/v2/daily/${state.date}/apply`, {
      method: "POST",
      body: JSON.stringify({
        expected_revision_id: preview.revision_id,
        expected_revision_content_hash: preview.revision_content_hash,
        destination_path: preview.destination_path,
        expected_old_block_hash: preview.expected_old_block_hash,
        intended_new_block_hash: preview.intended_new_block_hash,
        expected_target_exists: preview.expected_target_exists,
        expected_original_content_hash: preview.expected_original_content_hash,
        expected_updated_content_hash: preview.updated_content_hash,
      }),
    });
    closeDialog($("apply-dialog"));
    await loadDay();
    notice(
      `${result.idempotent ? "Already applied" : "Applied"} to ${result.destination_path}.`,
    );
  } catch (error) {
    notice(error.message, true);
  } finally {
    $("confirm-apply").disabled = false;
  }
}

async function retryApply() {
  const operation = state.data?.apply_status;
  if (!operation) return;
  $("retry-apply").disabled = true;
  try {
    const result = await api(
      `/api/v2/daily/${state.date}/apply/${encodeURIComponent(operation.id)}/retry`,
      { method: "POST", body: "{}" },
    );
    await loadDay();
    const next = result.operation.state;
    notice(
      next === "finalized"
        ? "Apply verified and finalized."
        : next === "reconciliation_required"
          ? "The destination differs from the approved state. Nothing was overwritten; review the details before trying a new Apply."
          : `Apply is ${next}.`,
      next !== "finalized",
    );
  } catch (error) {
    await loadDay();
    notice(error.message, true);
  }
}

async function toggleDismiss() {
  const dismissed = state.data?.day?.review_status === "dismissed";
  const action = dismissed ? "reopen" : "dismiss";
  if (
    !dismissed &&
    !confirm(
      "Dismiss this exact candidate? It will leave the review queue without writing or deleting anything.",
    )
  )
    return;
  try {
    const result = await api(`/api/v2/daily/${state.date}/dismiss`, {
      method: dismissed ? "DELETE" : "POST",
      body: JSON.stringify({
        expected_revision_id: state.data.current_revision.id,
      }),
    });
    await loadDay();
    const expired = result.expired_evidence_count
      ? ` ${result.expired_evidence_count} source event(s) have expired; the surviving summary is not the raw evidence.`
      : "";
    notice(
      dismissed
        ? `Day reopened.${expired}`
        : "Day dismissed. Nothing was written or deleted.",
    );
  } catch (error) {
    notice(error.message, true);
  }
}

let refreshTimer;

let refreshDelay = 2000;

function runningAttempt(attempt) {
  return attempt && ["running", "queued"].includes(attempt.state);
}

function activeAttempt() {
  return (
    state.overview?.active_generation ||
    (runningAttempt(state.data?.generation_attempt)
      ? state.data.generation_attempt
      : null)
  );
}

function hasPendingChanges() {
  return (
    state.editingDraft ||
    !$("save-candidate").hidden ||
    contextDraftChanged() ||
    evidenceDecisionSelects().some(
      (input) =>
        input.dataset.userChanged &&
        input.value !== input.dataset.savedDisposition,
    )
  );
}

async function useActivityRecord() {
  if (state.saving || hasPendingChanges()) return;
  state.saving = true;
  render();
  try {
    await api(`/api/v2/daily/${state.date}/activity-record`, {
      method: "POST",
    });
    state.retryError = null;
    await loadDay();
    notice(
      "Activity record ready. Review and save when ready; nothing has been written to your note.",
    );
  } catch (error) {
    notice(`Could not create an activity record: ${error.message}`, true);
  } finally {
    state.saving = false;
    if (state.data) render();
  }
}

function renderCandidate(content) {
  renderCandidateEditor(content);
  $("candidate").closest("section.card").querySelector("h2").textContent =
    content?.schema_version === 2
      ? "Activity record · not AI-summarized"
      : "Daily draft";
  $("candidate").hidden = Boolean(content) && !state.editingDraft;
  const root = $("readable-draft");
  root.replaceChildren();
  root.hidden = !content || Boolean(state.editingDraft);
  $("edit-draft").hidden = !content;
  $("edit-draft").textContent = state.editingDraft
    ? "Cancel editing"
    : "Edit draft";
  $("edit-draft").disabled = !state.csrf;
  if (!content) return;
  if (!content.workstreams.length)
    root.append(el("p", "subtle", "Your draft contains your notes above."));
  const excluded = new Set(
    (state.data.current_snapshot_evidence || [])
      .filter((item) =>
        ["omit", "duplicate_of", "superseded_by"].includes(item.disposition),
      )
      .map((item) => item.event_id),
  );
  if (content.schema_version === 2) {
    for (const workstream of content.workstreams) {
      const entries = (workstream.activity || []).filter((entry) =>
        entry.evidence_event_ids.some((id) => !excluded.has(id)),
      );
      if (!entries.length) continue;
      const group = document.createElement("details");
      group.className = "workstream";
      group.append(
        el(
          "summary",
          "",
          `${workstream.title} · ${entries.length} activit${entries.length === 1 ? "y" : "ies"}`,
        ),
      );
      for (const entry of entries)
        group.append(el("p", "evidence-message", entry.text));
      root.append(group);
    }
    const attempt = state.data.generation_attempt;
    if (attempt?.error) {
      const details = document.createElement("details");
      details.className = "diagnostics";
      details.append(
        el("summary", "", "Previous AI attempt"),
        el(
          "p",
          "",
          `${attempt.finished_at ? new Date(attempt.finished_at).toLocaleString() + " · " : ""}${attempt.error}`,
        ),
      );
      root.append(details);
    }
    return;
  }
  for (const workstream of content.workstreams) {
    const section = el("section", "workstream");
    section.append(el("h3", "", workstream.title));
    let count = 0;
    for (const [key, label] of Object.entries(fieldLabels)) {
      const facts = (workstream[key] || []).filter((fact) =>
        (fact.evidence_event_ids || []).some((id) => !excluded.has(id)),
      );
      if (!facts.length) continue;
      count += facts.length;
      section.append(el("strong", "", label));
      const list = document.createElement("ul");
      for (const fact of facts) list.append(el("li", "", fact.text));
      section.append(list);
    }
    if (!count) continue;
    for (const link of workstream.canonical_links || [])
      section.append(el("p", "subtle", `Reference: ${link}`));
    if (!workstream.canonical_links?.length)
      section.append(
        action(
          "Add a reference note",
          () => referenceNotes.linkWorkstream(workstream),
          {
            disabled: !state.csrf,
          },
        ),
      );
    root.append(section);
  }
  if (content.open_questions?.length) {
    root.append(el("h3", "", "Open questions"));
    const list = document.createElement("ul");
    for (const question of content.open_questions)
      list.append(el("li", "", question));
    root.append(list);
  }
}

function renderGeneration() {
  const active = activeAttempt();
  const attempt = active || state.data?.generation_attempt;
  const legacyFailure =
    !attempt && state.data?.day?.generation_status === "failed";
  $("generation-status").hidden = !attempt && !legacyFailure;
  $("retry-generation").hidden =
    !!active ||
    !(
      legacyFailure ||
      ["failed", "timed_out", "interrupted", "canceled"].includes(
        attempt?.state,
      )
    );
  $("retry-generation").disabled = !state.csrf || state.saving;
  if (legacyFailure)
    $("generation-message").textContent =
      "Previous preparation did not finish. Your notes are safe. Try again when ready.";
  $("cancel-generation").hidden = !runningAttempt(attempt);
  $("cancel-generation").disabled = !state.csrf;
  $("view-generation").hidden = !active || active.local_date === state.date;
  $("generation-error").hidden = !attempt?.error;
  $("generation-error-text").textContent = attempt?.error || "";
  renderAttemptMessage(attempt);
  const needsReferenceRecovery =
    !active &&
    (state.data?.generation_attempt?.recovery_actions?.includes(
      "retry_without_references",
    ) ||
      state.contextDetails?.status === "invalid");
  $("reference-recovery").hidden = !needsReferenceRecovery;
  $("retry-without-references").disabled =
    !state.csrf || state.saving || Boolean(active);
  $("revision-reference-mode").hidden =
    !state.data?.current_revision ||
    state.data?.revision_reference_mode !== "none";
  $("generate").disabled =
    !state.csrf ||
    state.saving ||
    Boolean(active) ||
    state.data?.day?.review_status === "dismissed";
  $("generate").textContent = active
    ? "Preparing draft…"
    : legacyFailure ||
        (attempt && !["succeeded", "completed"].includes(attempt.state))
      ? "Try again"
      : state.data?.current_revision
        ? "Regenerate"
        : "Create draft";
  if (active) {
    $("regenerate-context").disabled = true;
    $("compare-context").disabled = true;
  }
  let retryError = $("generation-retry-error");
  if (!retryError) {
    retryError = el("p", "");
    retryError.id = "generation-retry-error";
    retryError.setAttribute("role", "alert");
    $("generation-status").append(retryError);
  }
  retryError.hidden = state.retryError?.date !== state.date;
  if (!retryError.hidden) {
    retryError.textContent = state.retryError.message;
    $("generation-status").hidden = false;
  }
  let activityAction = $("use-activity-record");
  if (!activityAction) {
    activityAction = action("Use activity record", useActivityRecord);
    activityAction.id = "use-activity-record";
    $("generation-status").insertBefore(activityAction, $("generation-error"));
  }
  activityAction.hidden =
    !!active ||
    !!state.data?.current_revision ||
    !(
      legacyFailure ||
      ["failed", "timed_out", "interrupted"].includes(attempt?.state) ||
      state.retryError?.date === state.date
    );
  activityAction.disabled = !state.csrf || state.saving;
  if (state.data?.current_revision?.content?.schema_version === 2 && !active) {
    const older =
      !attempt?.started_at ||
      !state.data.current_revision.created_at ||
      Date.parse(attempt.started_at) <
        Date.parse(state.data.current_revision.created_at);
    if (older) {
      $("generation-status").hidden = state.retryError?.date !== state.date;
      $("reference-recovery").hidden = true;
    }
    $("generate").textContent = "Create AI summary";
  }
  renderIntake(state.overview?.intake);
}

function renderAttemptMessage(attempt) {
  if (attempt) {
    const elapsed = Math.max(
      0,
      Math.floor((Date.now() - Date.parse(attempt.started_at)) / 1000),
    );
    const other =
      attempt.local_date !== state.date ? ` for ${attempt.local_date}` : "";
    const stage =
      {
        queued: "Waiting to start",
        preparing: "Preparing sources",
        requesting_model: "Writing draft",
        requesting_model_fallback: "Retrying smaller groups one at a time",
        saving: "Saving prepared draft",
        validating: "Checking draft",
      }[attempt.stage] || "Preparing draft";
    const groups =
      Number.isInteger(attempt.total_groups) && attempt.total_groups > 0
        ? ` · ${attempt.completed_groups || 0} of ${attempt.total_groups} groups complete`
        : "";
    const references =
      attempt.reference_mode === "none" ? " Without reference notes." : "";
    const failed =
      attempt.failure_code === "reference_context_invalid"
        ? "Reference notes need attention"
        : attempt.state === "canceled"
          ? "Preparation cancelled"
          : attempt.state === "interrupted"
            ? "Preparation was interrupted"
            : attempt.state === "timed_out"
              ? "Preparation took too long"
              : "Preparation failed";
    $("generation-message").textContent = runningAttempt(attempt)
      ? `${stage}${other}${groups} · ${elapsed}s elapsed${attempt.timeout_seconds ? ` · ${attempt.timeout_seconds}s limit` : ""}.${references} You can leave and return.`
      : attempt.state === "succeeded"
        ? "Draft ready to review. Nothing has been written."
        : `${failed}. Your notes are safe.`;
    if (!runningAttempt(attempt) && attempt.failure_code === "database_busy")
      $("generation-message").textContent =
        "The database was busy. Retry preparation; your notes and previous draft are unchanged.";
    if (!runningAttempt(attempt) && attempt.finished_at)
      $("generation-message").textContent +=
        ` Last attempt: ${new Date(attempt.finished_at).toLocaleString()}.`;
  }
}

function renderIntake(intake) {
  $("intake-status").textContent = intake
    ? `${intake.today_count} activities received today${intake.latest_received_at ? ` · Latest ${new Date(intake.latest_received_at).toLocaleTimeString()}` : " · Waiting for your agents"}`
    : "";
  $("intake-details").hidden = !intake?.sources?.length;
  $("intake-sources").replaceChildren();
  for (const source of intake?.sources || [])
    $("intake-sources").append(
      el(
        "li",
        "subtle",
        `${source.source} · Last received ${new Date(source.latest_received_at).toLocaleString()}`,
      ),
    );
}

function render() {
  renderDaily();
  renderGeneration();
  $("copy-draft").hidden = !state.data?.current_revision;
  $("copy-draft").disabled = Boolean(state.editingDraft);
  if (state.contextDetails?.status === "none") $("context-card").hidden = true;
}

// Background refresh must not replace unsaved edits or a newer selected day.
function scheduleRefresh() {
  clearTimeout(refreshTimer);
  if (document.hidden || !state.data || !state.csrf) return;
  refreshTimer = setTimeout(
    refreshStatus,
    activeAttempt() ? refreshDelay : Math.max(10000, refreshDelay),
  );
}

async function refreshStatus() {
  if (state.loadingDay || state.dayLoadError || !state.data) {
    scheduleRefresh();
    return;
  }
  const date = state.date,
    version = dayLoadVersion;
  const current = () =>
    date === state.date && version === dayLoadVersion && !state.loadingDay;
  try {
    const [overview, data] = await Promise.all([
      api("/api/v2/daily/overview"),
      api(`/api/v2/daily/${date}`),
    ]);
    if (!current()) return;
    const changed =
      data.current_revision?.id !== state.data.current_revision?.id;
    let context;
    if (changed && !hasPendingChanges() && !navigationBusy())
      context = await api(`/api/v2/daily/${date}/context`).catch(() => ({
        status: "unavailable",
        workstreams: [],
      }));
    if (!current()) return;
    state.overview = overview;
    state.data.generation_attempt = data.generation_attempt;
    refreshDelay = 2000;
    if (changed && context && !hasPendingChanges() && !navigationBusy()) {
      state.data = data;
      state.contextDetails = context;
      render();
    } else {
      renderOverview();
      renderGeneration();
      if (changed)
        notice(
          "A new draft is ready. Save or finish your edits before reloading this day.",
        );
    }
  } catch (error) {
    if (!current()) return;
    if (error.status === 401) {
      state.csrf = null;
      clearTimeout(refreshTimer);
      showLogin();
      return;
    }
    refreshDelay = Math.min(30000, Math.max(4000, refreshDelay * 2));
    $("generation-status").hidden = false;
    $("generation-message").textContent =
      "Connection interrupted. Checking again shortly; your edits are still here.";
  } finally {
    if (current()) scheduleRefresh();
  }
}
function setDayLoadState(loading, error = "") {
  state.loadingDay = loading;
  state.dayLoadError = error;
  const panel = $("daily-panel");
  panel.setAttribute("aria-busy", String(loading));
  const content = panel.querySelector(".grid");
  content.inert = loading || !!error;
  content.hidden =
    !!error || (loading && state.data?.local_date !== state.date);
  const status = $("day-load-status");
  status.hidden = !loading && !error;
  $("day-load-message").textContent = error
    ? `Could not load ${state.date}. ${error}`
    : `Loading ${state.date}…`;
  $("retry-day-load").hidden = !error;
  if (loading || error) {
    for (const button of panel.querySelectorAll(".day-head button"))
      button.disabled = true;
    if (state.data?.local_date !== state.date) {
      $("day-title").textContent = state.date;
      $("destination").textContent = "";
      $("status").textContent = "";
    }
  }
}

// Navigation's discard guard stays beside the form fields it understands.
function unsavedDayChanges() {
  const workstreams = state.data?.current_revision?.content?.workstreams || [];
  const changedTitle = [
    ...document.querySelectorAll("#candidate .title-input"),
  ].some((input, index) => input.value !== workstreams[index]?.title);
  const changedFact = [
    ...document.querySelectorAll("#candidate textarea"),
  ].some(
    (input) =>
      input.value !==
      workstreams[Number(input.dataset.workstream)]?.[input.dataset.field]?.[
        Number(input.dataset.fact)
      ]?.text,
  );
  return (
    changedTitle ||
    changedFact ||
    contextDraftChanged() ||
    evidenceDecisionSelects().some(
      (input) =>
        input.dataset.userChanged &&
        input.value !== input.dataset.savedDisposition,
    )
  );
}
// All cross-view dependencies are wired here. Callbacks are invoked after setup.
const referenceNotes = createReferenceNotes({
  app: state,
  api,
  notice,
  openDialog: (...args) => openDialog(...args),
  closeDialog: (...args) => closeDialog(...args),
  generate,
  activeAttempt,
});
const {
  selectDay,
  selectView,
  openDialog,
  closeDialog,
  rememberDialogFields,
  dialogHasChanges,
  navigationBusy,
  installNavigation,
} = createNavigation({
  state,
  notice,
  loadDay,
  unsavedDayChanges,
  referenceNotes,
});
const settings = createSettings({
  app: state,
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
});
async function signIn(event) {
  event.preventDefault();
  try {
    const result = await api("/api/v2/auth/login", {
      method: "POST",
      body: JSON.stringify({
        owner_secret: $("owner-secret").value,
        remember_me: $("remember-login").checked,
      }),
    });
    state.csrf = result.csrf_token;
    $("owner-secret").value = "";
    $("remember-login").checked = false;
    await boot();
  } catch (error) {
    notice(error.message, true);
  }
}

async function addManualNote(event) {
  event.preventDefault();
  const references = $("note-links")
    .value.split("\n")
    .map((value) => value.trim())
    .filter(Boolean);
  try {
    await api(`/api/v2/daily/${state.date}/manual`, {
      method: "POST",
      body: JSON.stringify({ text: $("note-text").value, references }),
    });
    $("note-text").value = "";
    $("note-links").value = "";
    closeDialog($("note-dialog"));
    await loadDay();
    notice("Note added to this day. Generate or regenerate when ready.");
  } catch (error) {
    notice(error.message, true);
  }
}

function toggleDraftEditor() {
  if (
    state.editingDraft &&
    !$("save-candidate").hidden &&
    !confirm("Discard unsaved draft edits?")
  )
    return;
  state.editingDraft = !state.editingDraft;
  renderCandidate(state.data.current_revision.content);
  $("copy-draft").disabled = Boolean(state.editingDraft);
}

async function copyDraft() {
  if (hasPendingChanges()) {
    notice(
      "Save or cancel your pending choices before copying the draft.",
      true,
    );
    return;
  }
  try {
    await navigator.clipboard.writeText(state.data.preview_markdown || "");
    notice("Draft copied. Nothing has been written to your note.");
  } catch {
    notice(
      "Copy unavailable. Open Preview file and select the text to copy.",
      true,
    );
  }
}

async function cancelGeneration() {
  const attempt = activeAttempt();
  if (!attempt) return;
  $("cancel-generation").disabled = true;
  try {
    const result = await api(
      `/api/v2/daily/${attempt.local_date}/generation/${encodeURIComponent(attempt.id)}/cancel`,
      { method: "POST", body: "{}" },
    );
    await refreshStatus();
    if (result.cancel_requested === false)
      notice(
        "This draft is finishing or already finished. Checking its latest state.",
      );
  } catch (error) {
    notice(error.message, true);
  } finally {
    $("cancel-generation").disabled = false;
  }
}

function bindDailyEvents() {
  $("login-form").addEventListener("submit", signIn);
  $("unlock").addEventListener("click", showLogin);
  $("daily-tab").addEventListener("click", () => selectView("daily"));
  $("knowledge-tab").addEventListener("click", () => selectView("knowledge"));
  $("previous-day").addEventListener("click", () => shiftDay(-1));
  $("next-day").addEventListener("click", () => shiftDay(1));
  $("today").addEventListener("click", () =>
    selectDay(state.overview?.today || localDate()),
  );
  $("day").addEventListener("change", (event) => selectDay(event.target.value));
  $("dismiss").addEventListener("click", toggleDismiss);
  $("generate").addEventListener("click", generate);
  $("regenerate-context").addEventListener("click", regenerateWithContext);
  $("compare-context").addEventListener("click", startComparison);
  $("comparison-form").addEventListener("submit", submitComparison);
  $("retry-comparison").addEventListener("click", startComparison);
  $("close-comparison").addEventListener("click", closeComparison);
  $("cancel-comparison").addEventListener("click", closeComparison);
  $("finish-comparison").addEventListener("click", closeComparison);
  $("save-candidate").addEventListener("click", saveCandidate);
  $("review-apply").addEventListener("click", reviewApply);
  $("confirm-apply").addEventListener("click", confirmApply);
  $("retry-apply").addEventListener("click", retryApply);
  const closeApply = () => closeDialog($("apply-dialog"));
  $("close-apply").addEventListener("click", closeApply);
  $("cancel-apply").addEventListener("click", closeApply);
  $("add-note").addEventListener("click", () => openDialog($("note-dialog")));
  const closeNote = () => closeDialog($("note-dialog"));
  $("close-note").addEventListener("click", closeNote);
  $("cancel-note").addEventListener("click", closeNote);
  $("note-form").addEventListener("submit", addManualNote);
  $("evidence").addEventListener("change", (event) => {
    if (event.target.matches("select[data-evidence-id]"))
      event.target.dataset.userChanged = "true";
  });
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) clearTimeout(refreshTimer);
    else refreshStatus();
  });
  $("edit-draft").addEventListener("click", toggleDraftEditor);
  $("copy-draft").addEventListener("click", copyDraft);
  $("cancel-generation").addEventListener("click", cancelGeneration);
  $("view-generation").addEventListener("click", () => {
    const attempt = activeAttempt();
    if (attempt) selectDay(attempt.local_date);
  });
  $("retry-without-references").addEventListener("click", () =>
    generate("none"),
  );
  // These used to be inline HTML handlers; module functions are intentionally private.
  $("retry-generation").addEventListener("click", () => generate());
  $("browse-activity").addEventListener("click", () => {
    $("activity-details").open = true;
    $("activity-search").focus();
  });
  $("activity-search").addEventListener("input", filterActivity);
  $("activity-previous").addEventListener("click", () => moveActivity(-1));
  $("activity-next").addEventListener("click", () => moveActivity(1));
}
bindDailyEvents();
referenceNotes.bindEvents();
settings.bindEvents();
installNavigation();
boot();
