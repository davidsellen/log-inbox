import { $, el, action, identityLabel, noteTitle } from "./daily-helpers.js";
// Reference setup owns its collections, links, search timers, and form drafts.
// Daily supplies only the actions needed to return to or update its draft.
export function createReferenceNotes({
  app,
  api,
  notice,
  openDialog,
  closeDialog,
  generate,
  activeAttempt,
}) {
  const state = {
    knowledge: null,
    knowledgeReview: null,
    knowledgeLoading: false,
    editingCollection: null,
    collectionDraft: { roots: [], exclusions: [] },
    collectionPreview: null,
    folderTimer: null,
    noteTimer: null,
    linkingIdentity: null,
    editingMapping: null,
    selectedNote: null,
  };
  function knowledgeNotice(message, error = false) {
    const node = $("knowledge-notice");
    node.textContent = message || "";
    node.className = `notice${error ? " error" : ""}`;
    node.hidden = !message;
  }

  function collectionPathLabel(path) {
    return path === "." ? "Workspace root" : path;
  }

  async function loadKnowledge() {
    const setupOpen = $("knowledge-content").querySelectorAll(
      ":scope > .knowledge-details",
    )[1]?.open;
    state.knowledgeLoading = true;
    $("knowledge-content").innerHTML =
      '<div class="card">Loading Knowledge…</div>';
    const [collections, review] = await Promise.allSettled([
      api("/api/v2/knowledge/collections"),
      api("/api/v2/knowledge/review?limit=100"),
    ]);
    state.knowledge =
      collections.status === "fulfilled" ? collections.value : null;
    state.knowledgeReview = review.status === "fulfilled" ? review.value : null;
    renderKnowledge(
      collections.status === "rejected" ? collections.reason : null,
      review.status === "rejected" ? review.reason : null,
    );
    compactKnowledge();
    const setup = $("knowledge-content").querySelectorAll(
      ":scope > .knowledge-details",
    )[1];
    if (setup && setupOpen) setup.open = true;
    state.knowledgeLoading = false;
  }

  function compactKnowledge() {
    const root = $("knowledge-content");
    if (!root || root.querySelector(":scope > .knowledge-details")) return;
    const intro = el(
      "div",
      "notice",
      "Knowledge is optional. Daily already summarizes your logs; only link a name here when you want future candidates to use a specific canonical note.",
    );
    root.prepend(intro);
    const sections = [...root.querySelectorAll(":scope > .knowledge-section")];
    if (!sections.length) return;
    const review = document.createElement("details");
    review.className = "knowledge-details";
    review.open = true;
    const reviewSummary = document.createElement("summary");
    reviewSummary.textContent = "Review names (optional)";
    review.append(reviewSummary);
    sections[0].parentNode.insertBefore(review, sections[0]);
    review.append(sections[0]);
    if (sections.length > 1) {
      const setup = document.createElement("details");
      setup.className = "knowledge-details";
      const setupSummary = document.createElement("summary");
      setupSummary.textContent = "Saved links and setup";
      setup.append(setupSummary);
      sections[1].parentNode.insertBefore(setup, sections[1]);
      for (const section of sections.slice(1)) setup.append(section);
    }
  }

  function simpleMapping(mapping) {
    return (
      mapping.selectors?.length === 1 &&
      mapping.selectors[0].operator === "exact"
    );
  }

  function renderKnowledge(collectionError = null, reviewError = null) {
    const root = $("knowledge-content");
    root.replaceChildren();
    const collections = state.knowledge;
    const review = state.knowledgeReview;
    const collectionCount = collections?.count || 0;
    const unresolvedCount = review?.unresolved?.total_count || 0;
    $("knowledge-count").textContent =
      `${unresolvedCount} name${unresolvedCount === 1 ? "" : "s"} to review · ${review?.mappings?.length || 0} saved link${review?.mappings?.length === 1 ? "" : "s"}`;
    if (!app.csrf)
      knowledgeNotice(
        "Reviewing Knowledge read-only. Unlock changes to link or ignore names.",
      );
    else if (review?.evidence?.truncated)
      knowledgeNotice(
        `Suggestions use the latest ${review.evidence.considered_count} retained events. Older evidence is not included in this review.`,
      );
    else knowledgeNotice("");
    root.append(
      createUnresolvedNames(
        review,
        collectionCount,
        collectionError,
        reviewError,
      ),
    );
    root.append(createSavedLinks(review));
    root.append(createIgnoredNames(review));
    root.append(createSourceCollections(collections, collectionError));
    appendReferenceDiagnostics(root, review?.diagnostics);
  }

  function createUnresolvedNames(
    review,
    collectionCount,
    collectionError,
    reviewError,
  ) {
    const unresolvedCount = review?.unresolved?.total_count || 0;
    const unresolved = el("section", "knowledge-section");
    const unresolvedHead = el("div", "card-head");
    unresolvedHead.append(
      el("h2", "", "Names to review"),
      el("span", "subtle", `${unresolvedCount}`),
    );
    unresolved.append(
      unresolvedHead,
      el(
        "p",
        "subtle",
        "Link durable product and engineering names to canonical notes, or ignore noise. Changes affect only new or regenerated candidates.",
      ),
    );
    const unresolvedList = el("div", "review-list");
    if (reviewError || review?.review_status === "unavailable") {
      const error =
        reviewError?.message ||
        {
          mapping_outside_collections:
            "A saved link points outside the enabled source collections. Add or enable the collection that contains its note, then retry.",
          catalog_limit:
            "The enabled source collections are too large to review at once. Narrow the collection roots, then retry.",
          resolver_error:
            "Reference-note matching is unavailable. Check your source collections and saved links, then retry.",
        }[
          review?.diagnostics?.resolution_error_code ||
            (review?.diagnostics?.resolution_failed ? "resolver_error" : "")
        ] ||
        "Review is temporarily unavailable.";
      unresolvedList.append(
        el("div", "empty", `Names could not be reviewed: ${error}`),
        action("Retry", loadKnowledge),
      );
    } else if (!review?.unresolved?.identities?.length)
      unresolvedList.append(
        el(
          "div",
          "empty",
          review
            ? "Everything found is linked or ignored."
            : "No review data is available.",
        ),
      );
    else
      for (const identity of review.unresolved.identities) {
        const row = el("div", "review-row");
        const summary = el("div");
        summary.append(
          el("div", "review-name", identity.value),
          el(
            "div",
            "subtle",
            `${identityLabel(identity.field)} · ${identity.event_count} event${identity.event_count === 1 ? "" : "s"}`,
          ),
        );
        const actions = el("div", "actions");
        actions.append(
          action("Link", () => openLink(identity), {
            primary: true,
            disabled: !app.csrf || !collectionCount,
          }),
          action("Ignore", () => ignoreIdentity(identity), {
            disabled: !app.csrf,
          }),
        );
        row.append(summary, actions);
        unresolvedList.append(row);
      }
    unresolved.append(unresolvedList);
    if (!collectionCount && !collectionError)
      unresolved.append(
        el(
          "div",
          "notice",
          "Add a source collection before linking names so search stays bounded to notes you trust.",
        ),
      );
    return unresolved;
  }

  function createSavedLinks(review) {
    const mappings = el("section", "knowledge-section");
    const mappingsHead = el("div", "card-head");
    mappingsHead.append(
      el("h2", "", "Saved links"),
      el("span", "subtle", `${review?.mappings?.length || 0}`),
    );
    mappings.append(
      mappingsHead,
      el(
        "p",
        "subtle",
        "Daily uses these links when the exact name appears. Existing reviewed candidates stay unchanged until you regenerate them.",
      ),
    );
    const mappingList = el("div");
    const grouped = new Map();
    for (const item of review?.mappings || []) {
      const path = item.mapping.canonical_note_path;
      if (!grouped.has(path)) grouped.set(path, []);
      grouped.get(path).push(item);
    }
    if (!grouped.size)
      mappingList.append(el("div", "empty", "No saved links yet."));
    for (const [path, items] of grouped) {
      const group = el("div", "mapping-group");
      group.append(
        el("div", "mapping-target", noteTitle(path)),
        el("div", "subtle", path),
      );
      for (const item of items) {
        const mapping = item.mapping;
        const row = el("div", "mapping-row");
        const exact = simpleMapping(mapping);
        const selector = exact ? mapping.selectors[0] : null;
        const label = exact
          ? `${identityLabel(selector.field)}: ${selector.value}`
          : `Imported or advanced rule · ${mapping.selectors?.length || 0} conditions`;
        const summary = el("div", "", label);
        if (item.target_status !== "ready")
          summary.append(
            el(
              "span",
              "pill paused",
              item.target_status === "target_missing"
                ? "Target missing"
                : "Paused",
            ),
          );
        const actions = el("div", "actions");
        if (exact)
          actions.append(
            action(
              "Change",
              () =>
                openLink(
                  { field: selector.field, value: selector.value },
                  mapping,
                ),
              { disabled: !app.csrf },
            ),
            action(
              mapping.enabled ? "Pause" : "Enable",
              () => toggleMapping(mapping),
              { disabled: !app.csrf },
            ),
          );
        actions.append(
          action("Remove", () => removeMapping(mapping), {
            disabled: !app.csrf,
          }),
        );
        row.append(summary, actions);
        group.append(row);
      }
      mappingList.append(group);
    }
    mappings.append(mappingList);
    return mappings;
  }

  function createIgnoredNames(review) {
    const ignored = el("section", "knowledge-section");
    const ignoredDetails = document.createElement("details");
    ignoredDetails.className = "knowledge-details";
    const ignoredSummary = document.createElement("summary");
    ignoredSummary.textContent = `Ignored names (${review?.ignored?.length || 0})`;
    ignoredDetails.append(ignoredSummary);
    const ignoredList = el("div", "review-list");
    if (!review?.ignored?.length)
      ignoredList.append(el("div", "empty", "No ignored names."));
    else
      for (const item of review.ignored) {
        const row = el("div", "review-row");
        row.append(
          el("div", "", `${identityLabel(item.field)}: ${item.value}`),
          action("Review again", () => reopenIgnored(item), {
            disabled: !app.csrf,
          }),
        );
        ignoredList.append(row);
      }
    ignoredDetails.append(ignoredList);
    ignored.append(ignoredDetails);
    return ignored;
  }

  function createSourceCollections(collections, collectionError) {
    const collectionCount = collections?.count || 0;
    const sources = el("section", "knowledge-section");
    const sourceDetails = document.createElement("details");
    sourceDetails.className = "knowledge-details";
    if (!collectionCount || collectionError) sourceDetails.open = true;
    const sourceSummary = document.createElement("summary");
    sourceSummary.textContent = `Source collections (${collectionCount})`;
    sourceDetails.append(
      sourceSummary,
      el(
        "p",
        "subtle",
        "Optional guardrails that limit which note titles and paths can be searched. They do not browse, create, move, or edit files.",
      ),
    );
    const controls = el("div", "actions");
    controls.append(
      action("New collection", () => openCollection(), {
        primary: true,
        disabled:
          !app.csrf ||
          Boolean(collections && collections.count >= collections.limit),
      }),
    );
    sourceDetails.append(controls);
    const list = el("div", "knowledge-list");
    list.id = "knowledge-list";
    if (collectionError) {
      list.append(
        el(
          "div",
          "empty",
          `Collections could not be loaded: ${collectionError.message}`,
        ),
        action("Retry", loadKnowledge),
      );
    } else renderCollections(list, collections);
    sourceDetails.append(list);
    sources.append(sourceDetails);
    return sources;
  }

  function appendReferenceDiagnostics(root, diagnostics) {
    if (diagnostics) {
      const total =
        (diagnostics.missing_root_count || 0) +
        (diagnostics.oversized_note_count || 0) +
        (diagnostics.unreadable_note_count || 0) +
        (diagnostics.invalid_note_count || 0) +
        (diagnostics.invalid_mapping_count || 0) +
        (diagnostics.ambiguous_group_count || 0);
      if (total) {
        const details = document.createElement("details");
        details.className = "diagnostics";
        const summary = document.createElement("summary");
        summary.textContent = `Diagnostics (${total})`;
        details.append(
          summary,
          el(
            "div",
            "",
            `${diagnostics.missing_root_count || 0} missing roots · ${diagnostics.oversized_note_count || 0} oversized notes · ${diagnostics.unreadable_note_count || 0} unreadable notes · ${diagnostics.invalid_note_count || 0} invalid notes · ${diagnostics.invalid_mapping_count || 0} invalid links · ${diagnostics.ambiguous_group_count || 0} ambiguous groups`,
          ),
        );
        root.append(details);
      }
    }
  }

  function renderCollections(root, data) {
    if (!data) {
      root.append(el("div", "empty", "Collection setup is unavailable."));
      return;
    }
    if (!data.collections.length) {
      root.append(
        el(
          "div",
          "empty",
          "No source collections. Daily still works, but canonical-note search is unavailable.",
        ),
      );
      return;
    }
    for (const collection of data.collections) {
      const card = el("article", "collection-card");
      const head = el("div", "collection-head");
      const summary = el("div");
      const title = el("div", "collection-title");
      title.append(
        el("h3", "", collection.label),
        el(
          "span",
          `pill${collection.enabled ? "" : " paused"}`,
          collection.enabled ? "Active" : "Paused",
        ),
      );
      summary.append(title, el("div", "", collection.purpose));
      const actions = el("div", "actions");
      actions.append(
        action("Edit", () => openCollection(collection), {
          disabled: !app.csrf,
        }),
        action(
          collection.enabled ? "Pause" : "Enable",
          () => toggleCollection(collection),
          { disabled: !app.csrf },
        ),
        action("Remove", () => removeCollection(collection), {
          disabled: !app.csrf,
        }),
      );
      head.append(summary, actions);
      card.append(
        head,
        collectionPaths("Includes", collection.roots),
        collectionPaths("Excludes", collection.exclusions),
      );
      root.append(card);
    }
  }

  function collectionPaths(label, paths) {
    const section = document.createElement("div");
    section.className = "path-section";
    const heading = document.createElement("strong");
    heading.textContent = label;
    const list = document.createElement("div");
    list.className = "path-list";
    if (!paths.length) {
      const empty = document.createElement("span");
      empty.className = "subtle";
      empty.textContent = "No exclusions";
      list.append(empty);
    } else
      for (const path of paths) {
        const item = document.createElement("span");
        item.className = "path";
        item.textContent = collectionPathLabel(path);
        list.append(item);
      }
    section.append(heading, list);
    return section;
  }

  function openCollection(collection = null) {
    state.editingCollection = collection;
    state.collectionLabelEdited = false;
    $("collection-advanced").open = Boolean(collection);
    $("collection-folder").value = collection?.roots?.[0] || "";
    $("collection-folder").dataset.previous = collection?.roots?.[0] || "";
    state.collectionDraft = {
      roots: [...(collection?.roots || [])],
      exclusions: [...(collection?.exclusions || [])],
    };
    state.collectionPreview = null;
    $("collection-dialog-title").textContent = collection
      ? "Edit collection"
      : "New collection";
    $("collection-label").value = collection?.label || "Reference notes";
    $("collection-purpose").value =
      collection?.purpose ||
      "Product and engineering background for daily drafts";
    $("collection-enabled").checked = collection?.enabled ?? true;
    $("collection-root-input").value = "";
    $("collection-exclusion-input").value = "";
    $("knowledge-preview").hidden = true;
    $("save-collection").disabled = true;
    $("save-collection").textContent = collection
      ? "Save changes"
      : "Create collection";
    renderCollectionPaths();
    openDialog($("collection-dialog"));
  }

  function closeCollection() {
    closeDialog($("collection-dialog"));
  }

  function invalidateCollectionPreview() {
    state.collectionPreview = null;
    $("knowledge-preview").hidden = true;
    $("save-collection").disabled = true;
  }

  function renderCollectionPaths() {
    const first = state.collectionDraft.roots[0] || "";
    $("collection-folder").value = first;
    $("collection-folder").dataset.previous = first;
    if (!state.editingCollection && !state.collectionLabelEdited)
      $("collection-label").value =
        first && first !== "."
          ? first.split("/").filter(Boolean).pop()
          : "Reference notes";
    for (const [kind, nodeId] of [
      ["roots", "collection-roots"],
      ["exclusions", "collection-exclusions"],
    ]) {
      const root = $(nodeId);
      root.replaceChildren();
      for (const [index, path] of state.collectionDraft[kind].entries()) {
        const chip = document.createElement("span");
        chip.className = "path editable-path";
        const text = document.createElement("span");
        text.textContent = collectionPathLabel(path);
        const remove = document.createElement("button");
        remove.type = "button";
        remove.setAttribute("aria-label", `Remove ${path}`);
        remove.textContent = "×";
        remove.addEventListener("click", () => {
          state.collectionDraft[kind].splice(index, 1);
          invalidateCollectionPreview();
          renderCollectionPaths();
        });
        chip.append(text, remove);
        root.append(chip);
      }
    }
  }

  function addCollectionPath(kind, inputId) {
    const input = $(inputId);
    const value = input.value.trim().replace(/\/+$/, "");
    if (!value) {
      knowledgePreviewError(
        "Enter a relative folder path. Use . for the workspace root.",
      );
      return;
    }
    const limit = kind === "roots" ? 8 : 32;
    if (state.collectionDraft[kind].length >= limit) {
      knowledgePreviewError(
        `${kind === "roots" ? "Include" : "Exclude"} folders are limited to ${limit}.`,
      );
      return;
    }
    if (state.collectionDraft[kind].includes(value)) {
      knowledgePreviewError("That folder is already listed.");
      return;
    }
    state.collectionDraft[kind].push(value);
    input.value = "";
    invalidateCollectionPreview();
    renderCollectionPaths();
  }

  function collectionDraft() {
    return {
      label: $("collection-label").value,
      purpose: $("collection-purpose").value,
      roots: [...state.collectionDraft.roots],
      exclusions: [...state.collectionDraft.exclusions],
      enabled: $("collection-enabled").checked,
    };
  }

  function knowledgePreviewError(message) {
    const node = $("knowledge-preview");
    node.textContent = message;
    node.className = "notice error";
    node.hidden = false;
    $("save-collection").disabled = true;
  }

  async function previewCollection(event) {
    event.preventDefault();
    if (!$("collection-form").reportValidity()) return;
    if (!state.collectionDraft.roots.length) {
      knowledgePreviewError("Add at least one include folder.");
      return;
    }
    $("preview-collection").disabled = true;
    try {
      const preview = await api("/api/v2/knowledge/collections/preview", {
        method: "POST",
        body: JSON.stringify(collectionDraft()),
      });
      state.collectionPreview = preview;
      state.collectionDraft = {
        roots: [...preview.collection.roots],
        exclusions: [...preview.collection.exclusions],
      };
      $("collection-label").value = preview.collection.label;
      $("collection-purpose").value = preview.collection.purpose;
      renderCollectionPaths();
      const skipped = preview.oversized_note_count
        ? ` ${preview.oversized_note_count} over 1 MB will be skipped.`
        : "";
      const missing = preview.missing_roots?.length
        ? ` ${preview.missing_roots.join(", ")} does not exist yet and will not be created.`
        : "";
      const node = $("knowledge-preview");
      node.textContent = `Review: ${preview.matched_note_count} Markdown note${preview.matched_note_count === 1 ? "" : "s"} match; ${preview.eligible_note_count} eligible.${skipped}${missing} Nothing has been saved.`;
      node.className = "notice";
      node.hidden = false;
      $("save-collection").disabled = false;
    } catch (error) {
      knowledgePreviewError(error.message);
    } finally {
      $("preview-collection").disabled = false;
    }
  }

  async function saveCollection() {
    const preview = state.collectionPreview;
    if (!preview) return;
    const current = state.editingCollection;
    const path = current
      ? `/api/v2/knowledge/collections/${encodeURIComponent(current.id)}`
      : "/api/v2/knowledge/collections";
    $("save-collection").disabled = true;
    $("preview-collection").disabled = true;
    try {
      await api(path, {
        method: current ? "PUT" : "POST",
        body: JSON.stringify({
          collection: preview.collection,
          preview_digest: preview.preview_digest,
          expected_updated_at: current?.updated_at || null,
        }),
      });
      closeDialog($("collection-dialog"), true);
      await loadKnowledge();
      if (state.referenceIdentity) {
        openLink(state.referenceIdentity);
        $("note-search").value = state.referenceIdentity.value;
        searchKnowledgeNotes();
      }
      knowledgeNotice(
        current
          ? `Changes to “${preview.collection.label}” were saved.`
          : `“${preview.collection.label}” was created.`,
      );
    } catch (error) {
      if (error.status === 409 && current) {
        await loadKnowledge();
        state.editingCollection =
          state.knowledge?.collections.find((item) => item.id === current.id) ||
          current;
        invalidateCollectionPreview();
        knowledgePreviewError(
          "This collection changed in another session. Your edits are still here; review them against the latest saved version and try again.",
        );
      } else knowledgePreviewError(error.message);
    } finally {
      $("preview-collection").disabled = false;
    }
  }

  async function toggleCollection(collection) {
    const draft = {
      label: collection.label,
      purpose: collection.purpose,
      roots: collection.roots,
      exclusions: collection.exclusions,
      enabled: !collection.enabled,
    };
    try {
      const preview = await api("/api/v2/knowledge/collections/preview", {
        method: "POST",
        body: JSON.stringify(draft),
      });
      await api(
        `/api/v2/knowledge/collections/${encodeURIComponent(collection.id)}`,
        {
          method: "PUT",
          body: JSON.stringify({
            collection: preview.collection,
            preview_digest: preview.preview_digest,
            expected_updated_at: collection.updated_at,
          }),
        },
      );
      await loadKnowledge();
      knowledgeNotice(
        preview.collection.enabled
          ? `“${collection.label}” will be used for exact Daily note links.`
          : `“${collection.label}” is paused and will not be used by Daily.`,
      );
    } catch (error) {
      await loadKnowledge();
      knowledgeNotice(
        error.status === 409
          ? "This collection changed in another session. The latest version was loaded; try again."
          : error.message,
        true,
      );
    }
  }

  async function removeCollection(collection) {
    if (
      !confirm(
        `Remove “${collection.label}”? This removes the collection definition only. Markdown files will not be changed.`,
      )
    )
      return;
    try {
      await api(
        `/api/v2/knowledge/collections/${encodeURIComponent(collection.id)}`,
        {
          method: "DELETE",
          body: JSON.stringify({ expected_updated_at: collection.updated_at }),
        },
      );
      await loadKnowledge();
      knowledgeNotice(
        `“${collection.label}” was removed. Markdown files were not changed.`,
      );
    } catch (error) {
      await loadKnowledge();
      knowledgeNotice(
        error.status === 409
          ? "This collection changed in another session. The latest version was loaded; try again."
          : error.message,
        true,
      );
    }
  }

  function searchKnowledgeFolders(input) {
    clearTimeout(state.folderTimer);
    const query = input.value.trim();
    if (query.length < 2) return;
    state.folderTimer = setTimeout(async () => {
      try {
        const data = await api(
          `/api/v2/knowledge/folders?query=${encodeURIComponent(query)}&limit=20`,
        );
        const list = $("knowledge-folders");
        list.replaceChildren();
        for (const folder of data.folders) {
          const option = document.createElement("option");
          option.value = folder;
          list.append(option);
        }
      } catch (_) {
        /* Validation remains server-authoritative at preview. */
      }
    }, 180);
  }

  function openLink(identity, mapping = null) {
    state.linkingIdentity = identity;
    state.editingMapping = mapping;
    state.selectedNote = mapping
      ? {
          path: mapping.canonical_note_path,
          title: noteTitle(mapping.canonical_note_path),
        }
      : null;
    $("link-dialog-title").textContent = mapping
      ? "Change saved link"
      : "Link a name";
    $("link-identity").textContent =
      `${identityLabel(identity.field)}: ${identity.value}`;
    $("note-search").value = "";
    $("note-results").replaceChildren();
    $("note-results").hidden = true;
    renderSelectedNote();
    openDialog($("link-dialog"));
    $("note-search").focus();
  }

  function closeLink(saved = false) {
    if (!closeDialog($("link-dialog"), saved === true)) return;
    clearTimeout(state.noteTimer);
    $("link-identity").textContent = "";
    $("note-results").replaceChildren();
    $("note-results").hidden = true;
    state.linkingIdentity = null;
    state.editingMapping = null;
    state.selectedNote = null;
  }

  function renderSelectedNote() {
    const node = $("selected-note");
    if (!state.selectedNote) {
      node.hidden = true;
      node.textContent = "";
      $("save-link").disabled = true;
      return;
    }
    node.hidden = false;
    node.className = "notice";
    node.textContent = `Selected: ${state.selectedNote.title} · ${state.selectedNote.path}`;
    $("save-link").disabled = !app.csrf;
  }

  function searchKnowledgeNotes() {
    clearTimeout(state.noteTimer);
    const query = $("note-search").value.trim();
    const root = $("note-results");
    state.selectedNote = null;
    renderSelectedNote();
    root.replaceChildren();
    if (query.length < 2) {
      root.hidden = true;
      return;
    }
    root.hidden = false;
    root.append(el("div", "empty", "Searching…"));
    state.noteTimer = setTimeout(async () => {
      try {
        const data = await api(
          `/api/v2/knowledge/notes?query=${encodeURIComponent(query)}&limit=20`,
        );
        root.replaceChildren();
        if (!data.notes.length) {
          root.append(
            el(
              "div",
              "empty",
              "No matching note in enabled source collections.",
            ),
          );
          return;
        }
        for (const note of data.notes) {
          const button = el("button", "search-result");
          button.type = "button";
          button.setAttribute("aria-selected", "false");
          button.append(
            el("strong", "", note.title),
            el("span", "subtle", note.path),
          );
          button.addEventListener("click", () => {
            root
              .querySelectorAll("button")
              .forEach((item) => item.setAttribute("aria-selected", "false"));
            button.setAttribute("aria-selected", "true");
            state.selectedNote = note;
            renderSelectedNote();
          });
          root.append(button);
        }
      } catch (error) {
        root.replaceChildren(
          el("div", "empty", `Note search failed: ${error.message}`),
        );
      }
    }, 180);
  }

  async function saveLink() {
    const identity = state.linkingIdentity;
    const note = state.selectedNote;
    if (!identity || !note) return;
    const mapping = {
      field: identity.field,
      value: identity.value,
      canonical_note_path: note.path,
      enabled: true,
    };
    const current = state.editingMapping;
    $("save-link").disabled = true;
    try {
      await api(
        current
          ? `/api/v2/knowledge/mappings/${encodeURIComponent(current.id)}`
          : "/api/v2/knowledge/mappings",
        {
          method: current ? "PUT" : "POST",
          body: JSON.stringify(
            current
              ? { mapping, expected_updated_at: current.updated_at }
              : { mapping },
          ),
        },
      );
      closeLink(true);
      await loadKnowledge();
      const setup = $("knowledge-content").querySelectorAll(
        ":scope > .knowledge-details",
      )[1];
      if (setup) setup.open = true;
      knowledgeNotice(
        `Saved ${identity.value} → ${note.title}. Regenerate a Daily candidate to use the link.`,
      );
      if (state.referenceIdentity) {
        state.referenceIdentity = null;
        notice(
          `Reference linked to ${note.title}. Update the draft when ready.`,
        );
        $("notice").append(
          action("Update draft", generate, {
            primary: true,
            disabled: Boolean(activeAttempt()),
          }),
        );
      }
    } catch (error) {
      $("save-link").disabled = false;
      const node = $("selected-note");
      node.hidden = false;
      node.className = "notice error";
      node.textContent =
        error.status === 409
          ? "This link changed in another session. Close and review the latest saved links."
          : error.message;
    }
  }

  async function toggleMapping(mapping) {
    if (!simpleMapping(mapping)) return;
    const selector = mapping.selectors[0];
    try {
      await api(
        `/api/v2/knowledge/mappings/${encodeURIComponent(mapping.id)}`,
        {
          method: "PUT",
          body: JSON.stringify({
            mapping: {
              field: selector.field,
              value: selector.value,
              canonical_note_path: mapping.canonical_note_path,
              enabled: !mapping.enabled,
            },
            expected_updated_at: mapping.updated_at,
          }),
        },
      );
      await loadKnowledge();
      knowledgeNotice(
        `${selector.value} is ${mapping.enabled ? "paused" : "enabled"}. Existing candidates were not changed.`,
      );
    } catch (error) {
      await loadKnowledge();
      knowledgeNotice(
        error.status === 409
          ? "This link changed in another session. The latest links were loaded."
          : error.message,
        true,
      );
    }
  }

  async function removeMapping(mapping) {
    if (
      !confirm(
        `Remove the saved link to “${noteTitle(mapping.canonical_note_path)}”? Existing candidates will not be changed.`,
      )
    )
      return;
    try {
      await api(
        `/api/v2/knowledge/mappings/${encodeURIComponent(mapping.id)}`,
        {
          method: "DELETE",
          body: JSON.stringify({ expected_updated_at: mapping.updated_at }),
        },
      );
      await loadKnowledge();
      knowledgeNotice(
        "Saved link removed. Existing candidates were not changed.",
      );
    } catch (error) {
      await loadKnowledge();
      knowledgeNotice(
        error.status === 409
          ? "This link changed in another session. The latest links were loaded."
          : error.message,
        true,
      );
    }
  }

  async function ignoreIdentity(identity) {
    try {
      await api("/api/v2/knowledge/ignored", {
        method: "POST",
        body: JSON.stringify({ field: identity.field, value: identity.value }),
      });
      await loadKnowledge();
      knowledgeNotice(
        `Ignored ${identity.value}. You can restore it under Ignored names.`,
      );
    } catch (error) {
      await loadKnowledge();
      knowledgeNotice(error.message, true);
    }
  }

  async function reopenIgnored(item) {
    try {
      await api(`/api/v2/knowledge/ignored/${encodeURIComponent(item.id)}`, {
        method: "DELETE",
      });
      await loadKnowledge();
      knowledgeNotice(
        `${item.value} is back in review when it appears in retained evidence.`,
      );
    } catch (error) {
      await loadKnowledge();
      knowledgeNotice(error.message, true);
    }
  }

  async function linkWorkstream(workstream) {
    await loadKnowledge();
    const ids = new Set(workstream.evidence_event_ids || []);
    const events = (app.data.automated_evidence?.events || []).filter((event) =>
      ids.has(event.id),
    );
    const identities = (
      state.knowledgeReview?.unresolved?.identities || []
    ).filter((identity) =>
      events.some((event) => {
        const value = event.metadata?.[identity.field];
        return Array.isArray(value)
          ? value.includes(identity.value)
          : value === identity.value;
      }),
    );
    if (!identities.length) {
      notice(
        "No unlinked product or project name was found in this draft's sources. Manage reference notes in Settings.",
      );
      return;
    }
    const begin = (identity) => {
      state.referenceIdentity = identity;
      if (!state.knowledge?.count) {
        openCollection();
      } else {
        openLink(identity);
        $("note-search").value = identity.value;
        searchKnowledgeNotes();
      }
    };
    if (identities.length === 1) {
      begin(identities[0]);
      return;
    }
    notice("Choose the name to link to a reference note:");
    for (const identity of identities.slice(0, 8))
      $("notice").append(
        action(`${identityLabel(identity.field)}: ${identity.value}`, () =>
          begin(identity),
        ),
      );
  }
  function bindEvents() {
    $("close-collection").addEventListener("click", closeCollection);
    $("cancel-collection").addEventListener("click", closeCollection);
    $("collection-form").addEventListener("submit", previewCollection);
    $("save-collection").addEventListener("click", saveCollection);
    $("add-root").addEventListener("click", () =>
      addCollectionPath("roots", "collection-root-input"),
    );
    $("add-exclusion").addEventListener("click", () =>
      addCollectionPath("exclusions", "collection-exclusion-input"),
    );
    for (const id of [
      "collection-label",
      "collection-purpose",
      "collection-enabled",
    ]) {
      $(id).addEventListener("input", invalidateCollectionPreview);
    }
    for (const id of ["collection-root-input", "collection-exclusion-input"]) {
      $(id).addEventListener("input", (event) =>
        searchKnowledgeFolders(event.target),
      );
      $(id).addEventListener("keydown", (event) => {
        if (event.key !== "Enter") return;
        event.preventDefault();
        addCollectionPath(
          id === "collection-root-input" ? "roots" : "exclusions",
          id,
        );
      });
    }
    $("note-search").addEventListener("input", searchKnowledgeNotes);
    $("save-link").addEventListener("click", saveLink);
    $("close-link").addEventListener("click", closeLink);
    $("cancel-link").addEventListener("click", closeLink);
    $("collection-label").addEventListener("input", () => {
      state.collectionLabelEdited = true;
    });
    $("collection-advanced").addEventListener("toggle", () => {
      if ($("collection-advanced").open) renderCollectionPaths();
    });
    $("collection-folder").addEventListener("input", (event) => {
      const input = event.target;
      const value = input.value.trim().replace(/\/+$/, "");
      const previous = input.dataset.previous;
      state.collectionDraft.roots = state.collectionDraft.roots.filter(
        (path) => path !== previous && path !== value,
      );
      if (value) state.collectionDraft.roots.unshift(value);
      input.dataset.previous = value;
      if (!state.editingCollection && !state.collectionLabelEdited)
        $("collection-label").value =
          value && value !== "."
            ? value.split("/").filter(Boolean).pop()
            : "Reference notes";
      invalidateCollectionPreview();
      searchKnowledgeFolders(input);
    });
  }
  function dialogValues(dialog) {
    if (dialog.id === "collection-dialog")
      return [["paths", JSON.stringify(state.collectionDraft)]];
    if (dialog.id === "link-dialog")
      return [["note", state.selectedNote?.path || ""]];
    return [];
  }
  return {
    bindEvents,
    loadKnowledge,
    knowledgeNotice,
    linkWorkstream,
    dialogValues,
    isLoaded: () => Boolean(state.knowledge),
    isLoading: () => state.knowledgeLoading,
  };
}
