import { $, validDate } from "./daily-helpers.js";
// Navigation owns history and dialog lifecycle, not Daily's form structure.
export function createNavigation({
  state,
  notice,
  loadDay,
  unsavedDayChanges,
  referenceNotes,
}) {
  let navigationIndex = 0;

  let restoringHistory = false;

  const dialogBaselines = new WeakMap();

  const dialogOpeners = new WeakMap();

  function routeUrl(route) {
    const url = new URL(location.href);
    if (route.date) url.searchParams.set("date", route.date);
    if (route.view === "knowledge") url.searchParams.set("view", "knowledge");
    else url.searchParams.delete("view");
    return `${url.pathname}${url.search}${url.hash}`;
  }

  function currentRoute() {
    return {
      date: state.date,
      view: state.view,
      dialog: document.querySelector("dialog[open]")?.id || null,
    };
  }

  function writeHistory(route, push) {
    if (push) navigationIndex++;
    history[push ? "pushState" : "replaceState"](
      { logInboxIndex: navigationIndex, route },
      "",
      routeUrl(route),
    );
  }

  function dialogValues(dialog) {
    const values = [...dialog.querySelectorAll("input,textarea,select")].map(
      (input) => [input.id || input.name, input.value, input.checked],
    );
    values.push(...referenceNotes.dialogValues(dialog));
    return JSON.stringify(values);
  }

  function rememberDialog(dialog) {
    dialogBaselines.set(dialog, dialogValues(dialog));
  }

  function rememberDialogFields(dialog, isSavedField) {
    const previous = new Map(
      JSON.parse(dialogBaselines.get(dialog) || "[]").map((value) => [
        value[0],
        value,
      ]),
    );
    const values = JSON.parse(dialogValues(dialog)).map((value) =>
      isSavedField(value[0]) ? value : previous.get(value[0]) || value,
    );
    dialogBaselines.set(dialog, JSON.stringify(values));
  }

  function dialogHasChanges(dialog) {
    return (
      dialogBaselines.has(dialog) &&
      dialogBaselines.get(dialog) !== dialogValues(dialog)
    );
  }

  function navigationBusy() {
    return state.saving || state.pendingWrites > 0;
  }

  function canLeave(route) {
    if (navigationBusy()) {
      notice("Please wait for the current save to finish.", true);
      return false;
    }
    const leavingDay = route.date !== state.date;
    const changedDialog = [...document.querySelectorAll("dialog[open]")].some(
      (dialog) => dialog.id !== route.dialog && dialogHasChanges(dialog),
    );
    return (
      !((leavingDay && unsavedDayChanges()) || changedDialog) ||
      confirm("Discard unsaved changes and leave this view?")
    );
  }

  function showView(view) {
    state.view = view === "knowledge" ? "knowledge" : "daily";
    const knowledge = state.view === "knowledge";
    $("daily-tab").setAttribute("aria-selected", String(!knowledge));
    $("knowledge-tab").setAttribute("aria-selected", String(knowledge));
    // Daily must remain reachable when the optional Reference notes tab is hidden.
    $("daily-tab").tabIndex = 0;
    $("knowledge-tab").tabIndex = knowledge ? 0 : -1;
    $("daily-panel").hidden = knowledge;
    $("knowledge-panel").hidden = !knowledge;
    document.title = `Log Inbox · ${knowledge ? "Reference notes" : "Daily"}`;
  }

  function closeVisibleDialogs() {
    for (const dialog of document.querySelectorAll("dialog[open]")) {
      HTMLDialogElement.prototype.close.call(dialog);
    }
  }

  async function navigateRoute(
    route,
    { fromHistory = false, index = navigationIndex } = {},
  ) {
    route = {
      date: route.date || state.date,
      view: route.view === "knowledge" ? "knowledge" : "daily",
      dialog: route.dialog || null,
    };
    if (route.date && !validDate(route.date)) return false;
    if (!canLeave(route)) {
      $("day").value = state.date;
      return false;
    }
    const previous = currentRoute();
    const changedDay = route.date !== state.date;
    closeVisibleDialogs();
    if (changedDay) {
      state.editingDraft = false;
      state.date = route.date;
      $("day").value = route.date;
    }
    showView(route.view);
    if (fromHistory) navigationIndex = index;
    else if (JSON.stringify(previous) !== JSON.stringify(route))
      writeHistory(route, true);
    if (route.dialog) {
      const dialog = $(route.dialog);
      if (dialog instanceof HTMLDialogElement)
        HTMLDialogElement.prototype.showModal.call(dialog);
    } else if (previous.dialog) {
      dialogOpeners.get($(previous.dialog))?.focus();
    }
    const loaded = changedDay
      ? await loadDay()
      : !state.loadingDay &&
        !state.dayLoadError &&
        state.data?.local_date === state.date;
    if (
      state.view === "knowledge" &&
      !referenceNotes.isLoaded() &&
      !referenceNotes.isLoading()
    )
      await referenceNotes.loadKnowledge();
    if (
      !route.dialog &&
      state.view === route.view &&
      state.date === route.date
    ) {
      const heading =
        route.view === "daily"
          ? $("day-title")
          : $("knowledge-panel").querySelector("h1");
      if (!previous.dialog || changedDay || previous.view !== route.view) {
        heading.tabIndex = -1;
        heading.focus({ preventScroll: true });
      }
    }
    return loaded && state.date === route.date && state.view === route.view;
  }

  async function selectDay(date) {
    return navigateRoute({ date, view: "daily" });
  }

  async function selectView(view, updateUrl = true) {
    if (updateUrl) return navigateRoute({ date: state.date, view });
    showView(view);
    writeHistory(currentRoute(), false);
    if (
      state.view === "knowledge" &&
      !referenceNotes.isLoaded() &&
      !referenceNotes.isLoading()
    )
      await referenceNotes.loadKnowledge();
    return true;
  }

  function openDialog(dialog) {
    if (dialog.open) return;
    dialogOpeners.set(dialog, document.activeElement);
    rememberDialog(dialog);
    HTMLDialogElement.prototype.showModal.call(dialog);
    writeHistory(currentRoute(), true);
  }

  function closeDialog(dialog, saved = false) {
    if (!dialog.open) return true;
    if (!saved && navigationBusy()) return false;
    if (
      !saved &&
      dialogHasChanges(dialog) &&
      !confirm("Discard unsaved changes and close?")
    )
      return false;
    HTMLDialogElement.prototype.close.call(dialog);
    dialogOpeners.get(dialog)?.focus();
    if (history.state?.route?.dialog === dialog.id) {
      writeHistory(currentRoute(), false);
    }
    return true;
  }

  function installNavigation() {
    history.replaceState(
      { logInboxIndex: 0, route: currentRoute() },
      "",
      location.href,
    );
    $("back-to-daily").addEventListener("click", () => selectView("daily"));
    $("retry-day-load").addEventListener("click", () => loadDay());
    for (const dialog of document.querySelectorAll("dialog")) {
      dialog.addEventListener("cancel", (event) => {
        event.preventDefault();
        closeDialog(dialog);
      });
    }
    addEventListener("popstate", async (event) => {
      if (restoringHistory) {
        restoringHistory = false;
        return;
      }
      const index = event.state?.logInboxIndex ?? 0;
      const url = new URL(location.href);
      const route = event.state?.route || {
        date: url.searchParams.get("date") || state.date,
        view: url.searchParams.get("view"),
      };
      const previousIndex = navigationIndex;
      const navigation = navigateRoute(route, { fromHistory: true, index });
      // Acceptance updates the index synchronously, before any network response.
      if (navigationIndex !== index) {
        restoringHistory = true;
        history.go(previousIndex - index);
      }
      await navigation;
    });
    addEventListener("beforeunload", (event) => {
      if (
        unsavedDayChanges() ||
        [...document.querySelectorAll("dialog[open]")].some(dialogHasChanges) ||
        navigationBusy()
      ) {
        event.preventDefault();
        event.returnValue = "";
      }
    });
  }
  return {
    selectDay,
    selectView,
    openDialog,
    closeDialog,
    rememberDialogFields,
    dialogHasChanges,
    navigationBusy,
    installNavigation,
  };
}
