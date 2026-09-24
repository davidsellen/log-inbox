import { $, el } from "./daily-helpers.js";

// Search is navigation state, never a draft or an evidence decision.
export function createHistory({
  state,
  api,
  openDialog,
  closeDialog,
  selectDay,
  renderEvidence,
  renderCandidate,
}) {
  const dialog = $("history-dialog");
  const results = $("history-results");
  let query = "",
    cursor = null,
    request = 0,
    returnScroll = 0,
    returnButton = null;
  let busy = false;
  const labels = {
    note: "My note",
    activity: "Activity",
    draft: "Current draft",
  };

  function highlighted(text, term) {
    const fragment = document.createDocumentFragment();
    const lower = text.toLowerCase();
    const needle = term.toLowerCase();
    // Map folded UTF-16 offsets back to original text, including Unicode expansion.
    const offsets = [];
    let offset = 0;
    for (const char of text) {
      for (let i = 0; i < char.toLowerCase().length; i++) offsets.push(offset);
      offset += char.length;
    }
    offsets.push(text.length);
    let from = 0,
      at = 0;
    while (needle && (at = lower.indexOf(needle, at)) >= 0) {
      const start = offsets[at],
        end = offsets[at + needle.length];
      fragment.append(
        document.createTextNode(text.slice(from, start)),
        el("mark", "", text.slice(start, end)),
      );
      from = end;
      at += needle.length;
    }
    fragment.append(document.createTextNode(text.slice(from)));
    return fragment;
  }

  async function search(more = false) {
    const term = $("history-query").value.trim();
    if (!term) {
      $("history-status").textContent =
        "Enter a phrase, PR number, or error to search.";
      return;
    }
    if (busy && more) return;
    const version = ++request;
    busy = true;
    $("history-status").textContent = "Searching retained history…";
    $("history-more").disabled = true;
    if (!more) {
      query = term;
      cursor = null;
      results.replaceChildren();
      returnButton = null;
    }
    try {
      const params = new URLSearchParams({ q: query });
      if (more && cursor) params.set("cursor", cursor);
      const page = await api(`/api/v2/history/search?${params}`);
      if (version !== request) return;
      let group = results.lastElementChild;
      for (const hit of page.matches) {
        if (group?.dataset.date !== hit.local_date) {
          group = el("section", "");
          group.dataset.date = hit.local_date;
          group.append(
            el(
              "h3",
              "",
              new Intl.DateTimeFormat(undefined, {
                dateStyle: "full",
                timeZone: "UTC",
              }).format(new Date(`${hit.local_date}T00:00:00Z`)),
            ),
          );
          results.append(group);
        }
        const button = el("button", "history-match");
        const excerpt = el("span", "");
        excerpt.append(highlighted(hit.excerpt, query));
        button.append(el("span", "subtle", labels[hit.kind]), excerpt);
        button.addEventListener("click", () => {
          returnScroll = results.scrollTop;
          returnButton = button;
          selectDay(hit.local_date, { ...hit, query });
        });
        group.append(button);
      }
      cursor = page.next_cursor;
      $("history-more").hidden = !cursor;
      const count = results.querySelectorAll(".history-match").length;
      $("history-status").textContent = count
        ? `${count} matches shown${cursor ? ". More results available." : "."}`
        : "No matches in retained history. Try a shorter phrase or identifier.";
    } catch (error) {
      if (version === request)
        $("history-status").textContent =
          `Search failed. ${error.message} Use ${more ? "More results" : "Search"} to try again.`;
    } finally {
      if (version === request) {
        busy = false;
        $("history-more").disabled = false;
      }
    }
  }

  function targetElement(target) {
    if (target.kind === "note")
      return [...$("manual-list").querySelectorAll("[data-note-id]")].find(
        (node) => node.dataset.noteId === target.id,
      );
    if (target.kind === "activity")
      return [...$("evidence").querySelectorAll("[data-event-id]")].find(
        (node) => node.dataset.eventId === target.id,
      );
    if (state.data?.current_revision?.id !== target.id) return null;
    return [
      ...$("readable-draft").querySelectorAll("[data-history-group]"),
    ].find((node) => node.dataset.historyGroup === target.target);
  }

  function markTarget(target, focus) {
    const node = targetElement(target);
    if (!node) return false;
    node.hidden = false;
    for (let parent = node; parent; parent = parent.parentElement)
      if (parent.tagName === "DETAILS") parent.open = true;
    node.classList.add("history-target");
    node.tabIndex = -1;
    const walker = document.createTreeWalker(node, NodeFilter.SHOW_TEXT);
    const texts = [];
    while (walker.nextNode()) {
      const text = walker.currentNode;
      if (
        !text.parentElement.closest("button,select,textarea,mark") &&
        text.textContent.toLowerCase().includes(target.query.toLowerCase())
      )
        texts.push(text);
    }
    for (const text of texts)
      text.replaceWith(highlighted(text.textContent, target.query));
    if (focus) {
      node.scrollIntoView({ block: "nearest" });
      node.focus({ preventScroll: true });
    }
    return true;
  }

  async function reveal(route) {
    $("back-to-results").hidden =
      !route.target || !returnButton || !!route.dialog;
    if (route.dialog === "history-dialog") {
      results.scrollTop = returnScroll;
      (returnButton || $("history-query")).focus({ preventScroll: true });
      return;
    }
    document
      .querySelectorAll(".history-target")
      .forEach((node) => node.classList.remove("history-target"));
    document.querySelectorAll("#daily-panel mark").forEach((mark) => {
      const parent = mark.parentNode;
      mark.replaceWith(document.createTextNode(mark.textContent));
      parent.normalize();
    });
    const target = route.target;
    if (
      !target ||
      !route.loaded ||
      route.dialog ||
      route.view !== "daily" ||
      route.restoreDaily
    )
      return;
    try {
      if (target.kind === "draft") {
        state.editingDraft = false;
        renderCandidate(state.data.current_revision?.content);
      }
      if (target.kind === "activity") {
        $("activity-search").value = "";
        $("activity-search").dispatchEvent(new Event("input"));
      }
      if (target.kind === "activity" && !targetElement(target)) {
        const event = await api(
          `/api/v2/daily/${route.date}/activity/${encodeURIComponent(target.id)}`,
        );
        if (state.navigationTarget !== target || state.date !== route.date)
          return;
        const evidence = state.data.automated_evidence.events;
        evidence.push(event);
        renderEvidence(
          evidence,
          state.data.current_snapshot_evidence || [],
          state.data.active_late_evidence_deferrals || [],
        );
      }
      if (markTarget(target, true)) return;
    } catch {
      /* A deleted/expired result is not a failed day navigation. */
    }
    if (state.navigationTarget !== target) return;
    const notice = $("notice");
    notice.textContent =
      "This match is no longer available in the current day. You can go Back to results.";
    notice.hidden = false;
    notice.tabIndex = -1;
    notice.focus();
  }

  $("search-history").addEventListener("click", () => {
    openDialog(dialog);
    $("history-query").focus();
  });
  $("close-history").addEventListener("click", () => closeDialog(dialog));
  $("history-form").addEventListener("submit", (event) => {
    event.preventDefault();
    search();
  });
  $("history-more").addEventListener("click", () => search(true));
  $("back-to-results").addEventListener("click", () => history.back());
  addEventListener("daily:navigated", (event) => reveal(event.detail));
  return {
    restoreHighlight() {
      if (
        state.view === "daily" &&
        state.navigationTarget &&
        state.data?.local_date === state.date
      )
        markTarget(state.navigationTarget, false);
    },
  };
}
