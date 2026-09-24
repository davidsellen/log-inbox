import { $, validDate, isPlainLinkClick } from "./daily-helpers.js";
// Navigation owns history and dialog lifecycle, not Daily's form structure.
export function createNavigation({
  state,
  notice,
  loadDay,
  unsavedDayChanges,
  referenceNotes,
  getSettings,
}) {
  let navigationIndex = 0;

  let restoringHistory = false;
  let dailyPosition = null;

  const dialogBaselines = new WeakMap();

  const dialogOpeners = new WeakMap();

  function routeUrl(route) {
    const url = new URL(location.href);
    if (route.date) url.searchParams.set("date", route.date);
    if (["knowledge", "settings"].includes(route.view))
      url.searchParams.set("view", route.view);
    else url.searchParams.delete("view");
    if (route.view === "settings")
      url.searchParams.set("section", route.section || "general");
    else url.searchParams.delete("section");
    return `${url.pathname}${url.search}${url.hash}`;
  }

  function currentRoute() {
    return {
      date: state.date,
      view: state.view,
      dialog: document.querySelector("dialog[open]")?.id || null,
      target: state.navigationTarget || null,
      section: state.view === "settings" ? state.settingsSection : null,
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

  function dialogHasChanges(dialog) {
    if (dialog.dataset.readonly === "true") return false;
    return (
      dialogBaselines.has(dialog) &&
      dialogBaselines.get(dialog) !== dialogValues(dialog)
    );
  }

  function navigationBusy() {
    return state.saving || state.pendingWrites > 0 || getSettings().saving;
  }

  function canLeave(route) {
    if (navigationBusy()) {
      (state.view === "settings" ? getSettings().pageNotice : notice)(
        "Please wait for the current save to finish.",
        true,
      );
      return false;
    }
    const leavingDay =
      route.date !== state.date ||
      (route.view === "daily" &&
        state.view === "daily" &&
        route.target?.kind === "draft" &&
        state.editingDraft);
    const changedDialog = [...document.querySelectorAll("dialog[open]")].some(
      (dialog) => dialog.id !== route.dialog && dialogHasChanges(dialog),
    );
    const leavingSettings =
      state.view === "settings" &&
      route.view !== "settings" &&
      getSettings().hasChanges();
    return (
      !(
        (leavingDay && unsavedDayChanges()) ||
        changedDialog ||
        leavingSettings
      ) || confirm("Discard unsaved changes and leave this view?")
    );
  }

  function showView(view) {
    state.view = ["knowledge", "settings"].includes(view) ? view : "daily";
    const knowledge = state.view === "knowledge";
    const settings = state.view === "settings";
    $("main-shell").hidden = settings;
    $("settings-page").hidden = !settings;
    $("home-link").href = state.date
      ? `?date=${encodeURIComponent(state.date)}`
      : "/";
    if (state.view === "daily")
      $("home-link").setAttribute("aria-current", "page");
    else $("home-link").removeAttribute("aria-current");
    $("daily-panel").hidden = knowledge || settings;
    $("knowledge-panel").hidden = !knowledge;
    document.title = `Log Inbox · ${settings ? "Settings" : knowledge ? "Reference notes" : "Daily"}`;
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
      view: ["knowledge", "settings"].includes(route.view)
        ? route.view
        : "daily",
      dialog: route.dialog || null,
      target: route.target || null,
      section: route.view === "settings" ? route.section || "general" : null,
    };
    if (route.date && !validDate(route.date)) return false;
    if (!canLeave(route)) {
      $("day").value = state.date;
      return false;
    }
    const previous = currentRoute();
    const changedDay = route.date !== state.date;
    if (previous.view === "daily" && route.view !== "daily")
      dailyPosition = {
        date: state.date,
        scroll: scrollY,
        focus: document.activeElement,
      };
    if (previous.view === "settings" && route.view !== "settings")
      getSettings().discard();
    state.navigationTarget = route.target;
    closeVisibleDialogs();
    if (changedDay) {
      state.editingDraft = false;
      state.date = route.date;
      $("day").value = route.date;
    }
    showView(route.view);
    if (route.view === "settings") {
      getSettings().prepare();
      getSettings().showSection(route.section);
      route.section = state.settingsSection;
    }
    if (fromHistory) navigationIndex = index;
    else if (JSON.stringify(previous) !== JSON.stringify(route))
      writeHistory(
        route,
        !(previous.view === "settings" && route.view === "settings"),
      );
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
        route.view === "settings"
          ? $(
              previous.view === "settings"
                ? `settings-${route.section}-title`
                : "settings-title",
            )
          : route.view === "daily"
            ? $("day-title")
            : $("knowledge-panel").querySelector("h1");
      if (
        route.view === "daily" &&
        previous.view !== "daily" &&
        dailyPosition?.date === route.date
      ) {
        dailyPosition.focus?.focus({ preventScroll: true });
        scrollTo({ top: dailyPosition.scroll, behavior: "instant" });
      } else if (
        (!route.target || route.view !== "daily") &&
        (!previous.dialog || changedDay || previous.view !== route.view)
      ) {
        if (document.activeElement !== $("settings-section-select")) {
          heading.tabIndex = -1;
          heading.focus({ preventScroll: true });
          if (route.view === "settings")
            scrollTo({ top: 0, behavior: "instant" });
        }
      }
    }
    if (state.date === route.date && state.navigationTarget === route.target)
      dispatchEvent(
        new CustomEvent("daily:navigated", {
          detail: {
            ...route,
            loaded,
            restoreDaily:
              route.view === "daily" &&
              previous.view !== "daily" &&
              dailyPosition?.date === route.date,
          },
        }),
      );
    return loaded && state.date === route.date && state.view === route.view;
  }

  async function selectDay(date, target = null) {
    return navigateRoute({ date, view: "daily", target });
  }

  async function selectView(view, updateUrl = true, section = null) {
    if (updateUrl)
      return navigateRoute({
        date: state.date,
        view,
        section,
        target: state.navigationTarget,
      });
    showView(view);
    if (view === "settings") {
      getSettings().prepare();
      getSettings().showSection(section);
      $("settings-title").focus({ preventScroll: true });
    }
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
    if (
      dialog.dataset.readonly === "true" &&
      history.state?.route?.dialog === dialog.id
    ) {
      history.back();
      return true;
    }
    HTMLDialogElement.prototype.close.call(dialog);
    dialogOpeners.get(dialog)?.focus();
    if (history.state?.route?.dialog === dialog.id) {
      writeHistory(currentRoute(), false);
    }
    return true;
  }

  function installNavigation() {
    $("home-link").addEventListener("click", (event) => {
      if (!isPlainLinkClick(event) || !$("login-panel").hidden) return;
      event.preventDefault();
      selectView("daily");
    });
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
        section: url.searchParams.get("section"),
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
        getSettings().hasChanges() ||
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
    dialogHasChanges,
    navigationBusy,
    installNavigation,
  };
}
