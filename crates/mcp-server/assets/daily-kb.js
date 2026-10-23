import { el, action } from "./daily-helpers.js";

// Product KB capture is independent of Daily generation and Apply.
export function createKnowledgeCapture({ app, api }) {
  const root = el("section", "knowledge-section");
  root.id = "product-kb-submissions";
  let incoming = null;
  let suggestions = null;
  let candidates = null;
  let operations = null;
  let selected = null;
  let preview = null;
  let message = "";
  let busy = false;
  let loadVersion = 0;
  const post = (path, body) =>
    api(path, { method: "POST", body: JSON.stringify(body) });
  const candidatePath = (id) =>
    `/api/v2/knowledge/candidates/${encodeURIComponent(id)}`;

  async function run(work) {
    if (busy) return;
    busy = true;
    message = "";
    render();
    try {
      await work();
    } catch (error) {
      message = error.message;
    } finally {
      busy = false;
      render();
    }
  }

  async function load() {
    const version = ++loadVersion;
    const results = await Promise.allSettled([
      api("/api/v2/knowledge/submissions"),
      api("/api/v2/knowledge/candidates"),
      api("/api/v2/knowledge/promotions"),
      api("/api/v2/knowledge/suggestions"),
    ]);
    if (version !== loadVersion) return;
    [incoming, candidates, operations, suggestions] = results.map((result) =>
      result.status === "fulfilled" ? result.value : null,
    );
    if (results.some((result) => result.status === "rejected"))
      message = "Some Product KB data is unavailable. Reload to retry.";
    if (selected) {
      selected =
        candidates?.candidates?.find((item) => item.id === selected.id) || null;
    }
    render();
  }

  function labelledInput(label, multiline = false) {
    const wrapper = el("label", "field", label);
    const input = el(multiline ? "textarea" : "input");
    input.setAttribute("aria-label", label);
    wrapper.append(input);
    return [wrapper, input];
  }

  function detail(candidate) {
    const node = el("div", "card");
    const payload = candidate.submission;
    node.append(el("h3", "", payload.title), el("p", "", payload.summary));
    node.append(
      el(
        "p",
        "subtle",
        `Product: ${payload.product} · Modules: ${(payload.modules || []).join(", ")} · Plan: ${payload.plan_id} revision ${payload.plan_revision}`,
      ),
    );
    node.append(
      el(
        "p",
        "subtle",
        `Producer: ${candidate.producer_source} · Source event: ${candidate.source_event_id} · Evidence ${candidate.evidence_available ? "available" : "expired or unavailable"}${candidate.superseded ? " · Superseded" : ""}`,
      ),
    );
    node.append(
      el("p", "subtle", `Candidate hash: ${candidate.candidate_hash}`),
      el("p", "subtle", `Source digest: ${candidate.source_event_digest}`),
    );
    for (const [label, values] of [
      ["Claims (producer reported)", payload.claims],
      ["Decisions", payload.decisions],
      ["Constraints", payload.constraints],
      ["Questions", payload.questions],
      ["Artifacts", payload.artifacts],
      ["Commits", payload.commits],
      ["Pull requests", payload.pull_requests],
    ]) {
      if (!values?.length) continue;
      node.append(el("h4", "", label));
      const list = el("ul");
      for (const item of values) {
        if (typeof item === "string") list.append(el("li", "", item));
        else {
          const row = el("li");
          // Show every reported claim qualification and evidence binding as text.
          for (const [key, value] of Object.entries(item))
            row.append(
              el(
                "div",
                "",
                `${key.replaceAll("_", " ")}: ${typeof value === "string" ? value : JSON.stringify(value)}`,
              ),
            );
          list.append(row);
        }
      }
      node.append(list);
    }
    const review = candidate.review;
    node.append(
      el(
        "p",
        "",
        review
          ? `Owner review: ${review.disposition} · ${review.reason}`
          : "Awaiting owner review. Reported acceptance is not owner approval.",
      ),
    );
    if (!review) {
      const [reasonLabel, reason] = labelledInput("Review reason", true);
      node.append(reasonLabel);
      for (const disposition of ["accept", "reject"]) {
        node.append(
          action(
            disposition === "accept"
              ? "Accept exact revision"
              : "Reject exact revision",
            () => {
              const reviewReason = reason.value;
              run(async () => {
                await post(`${candidatePath(candidate.id)}/review`, {
                  expected_candidate_hash: candidate.candidate_hash,
                  disposition,
                  reason: reviewReason,
                });
                await load();
              });
            },
            {
              disabled:
                busy ||
                !app.csrf ||
                (disposition === "accept" &&
                  (!candidate.evidence_available || candidate.superseded)),
            },
          ),
        );
      }
    }
    if (
      review?.disposition === "accept" &&
      candidate.evidence_available &&
      !candidate.superseded
    ) {
      const [pathLabel, path] = labelledInput("Canonical note path");
      path.placeholder = "Products/Product.md";
      node.append(
        pathLabel,
        action(
          "Preview KB update",
          () => {
            const destination = path.value;
            run(async () => {
              preview = await post(
                `${candidatePath(candidate.id)}/promotion/preview`,
                {
                  candidate_hash: candidate.candidate_hash,
                  destination_path: destination,
                },
              );
            });
          },
          { disabled: busy || !app.csrf },
        ),
      );
    }
    return node;
  }

  function promotionPreview() {
    const node = el("div", "card");
    node.append(
      el("h3", "", "Confirm canonical KB update"),
      el(
        "p",
        "",
        `${preview.destination_path} · ${preview.state} · ${preview.operation_id}`,
      ),
    );
    node.append(
      el("h4", "", "Previous KB block"),
      el("pre", "", preview.previous_block || "(No previous KB block)"),
      el("h4", "", "New KB block"),
      el("pre", "", preview.next_block || ""),
    );
    node.append(
      action(
        "Confirm KB update",
        () =>
          run(async () => {
            const operation = preview;
            await post(
              `/api/v2/knowledge/promotions/${encodeURIComponent(operation.operation_id)}/confirm`,
              { preview_hash: operation.preview_hash },
            );
            preview = null;
            await load();
            message = "Canonical KB update confirmed.";
          }),
        {
          disabled:
            busy ||
            !app.csrf ||
            !preview.preview_hash ||
            !["prepared", "writing"].includes(preview.state),
        },
      ),
    );
    if (!["finalized", "cancelled"].includes(preview.state)) {
      node.append(
        action(
          "Discard KB preview",
          () =>
            run(async () => {
              await api(
                `/api/v2/knowledge/promotions/${encodeURIComponent(preview.operation_id)}`,
                { method: "DELETE" },
              );
              preview = null;
              await load();
              message = "KB preview discarded.";
            }),
          { disabled: busy || !app.csrf },
        ),
      );
    }
    return node;
  }

  function render() {
    root.replaceChildren(
      el("h2", "", "Product KB submissions"),
      el(
        "p",
        "subtle",
        "Capture original redacted activity or submissions, review their exact revision, then separately preview and confirm an update to an existing canonical note. New note creation is not supported. Daily generation and Apply do not approve Product KB content.",
      ),
    );
    root.append(
      action("Reload Product KB", () => run(load), { disabled: busy }),
    );
    if (message) root.append(el("p", "notice", message));
    root.append(el("h3", "", "Activity suggestions"));
    root.append(
      el(
        "p",
        "subtle",
        "Producer-reported activity, not independent verification. Confirm product and modules from product navigation before capture. Capture freezes these fields for exact owner review.",
      ),
    );
    if (!suggestions)
      root.append(el("p", "empty", "Activity suggestions unavailable."));
    else {
      if (suggestions.truncated)
        root.append(
          el(
            "p",
            "notice",
            "Only the latest 100 activity suggestions are shown. Older reports remain in source history.",
          ),
        );
      if (!suggestions.suggestions?.length)
        root.append(
          el("p", "empty", "No retained terminal activity to suggest."),
        );
      for (const item of suggestions.suggestions || []) {
        const row = el("div", "review-row");
        row.append(el("h4", "", item.submission.title || "Reported activity"));
        row.append(
          el(
            "p",
            "",
            item.submission.summary ||
              item.event.metadata?.outcome ||
              item.event.message,
          ),
        );
        row.append(
          el("p", "subtle", `${item.event.source} · ${item.event.id}`),
        );
        const evidence = el("details");
        evidence.append(
          el("summary", "", "Original activity and suggested claims"),
          el("p", "", item.event.message),
          el("pre", "", JSON.stringify(item.event.metadata, null, 2)),
          el("pre", "", JSON.stringify(item.submission, null, 2)),
        );
        row.append(evidence);
        if (item.capture_issue)
          row.append(el("p", "notice", item.capture_issue));
        if (item.captured_candidate_id)
          row.append(
            el(
              "p",
              "",
              "Already captured. Select its candidate revision below for review.",
            ),
          );
        else {
          const [productLabel, product] = labelledInput(
            `Confirmed product for ${item.event.id}`,
          );
          const [modulesLabel, modules] = labelledInput(
            `Confirmed modules for ${item.event.id}`,
          );
          product.value = item.submission.product || "";
          modules.value = (item.submission.modules || []).join(", ");
          row.append(
            productLabel,
            modulesLabel,
            el("p", "subtle", "Separate module names with commas."),
            action(
              "Capture activity suggestion",
              () => {
                const attribution = {
                  product: product.value,
                  modules: modules.value
                    .split(",")
                    .map((value) => value.trim())
                    .filter(Boolean),
                };
                run(async () => {
                  const result = await post(
                    `/api/v2/knowledge/suggestions/${encodeURIComponent(item.event.id)}/capture`,
                    {
                      expected_suggestion_hash: item.suggestion_hash,
                      ...attribution,
                    },
                  );
                  selected = result.candidate;
                  preview = null;
                  await load();
                });
              },
              { disabled: busy || !app.csrf || !!item.capture_issue },
            ),
          );
        }
        root.append(row);
      }
    }
    root.append(el("h3", "", "Incoming submissions"));
    if (!incoming)
      root.append(el("p", "empty", "Incoming submissions unavailable."));
    else {
      root.append(
        el(
          "p",
          "subtle",
          `Latest ${incoming.considered_count} retained submission events (limit ${incoming.limit}).${incoming.truncated ? " Older submissions are not included." : ""}`,
        ),
      );
      if (!incoming.events?.length)
        root.append(el("p", "empty", "No submissions in this event window."));
      for (const event of incoming.events || []) {
        const row = el("div", "review-row");
        const payload = event.metadata?.knowledge_submission;
        row.append(
          el(
            "div",
            "",
            `${payload?.title || "Submission"} · ${event.source} · ${event.id}`,
          ),
        );
        const evidence = el("details");
        evidence.append(
          el("summary", "", "Original redacted evidence"),
          el("p", "", event.message),
          el("pre", "", JSON.stringify(payload, null, 2)),
        );
        row.append(
          evidence,
          action(
            "Capture submission",
            () =>
              run(async () => {
                const result = await post("/api/v2/knowledge/candidates", {
                  source_event_id: event.id,
                });
                selected = result.candidate;
                preview = null;
                await load();
              }),
            { disabled: busy || !app.csrf },
          ),
        );
        root.append(row);
      }
    }
    root.append(el("h3", "", "Candidate revisions"));
    if (!candidates) root.append(el("p", "empty", "Candidates unavailable."));
    else {
      if (candidates.truncated)
        root.append(
          el("p", "notice", "Only the latest 100 candidates are shown."),
        );
      if (!candidates.candidates?.length)
        root.append(el("p", "empty", "No captured candidates."));
      for (const candidate of candidates.candidates || [])
        root.append(
          action(
            `${candidate.submission.title} · revision ${candidate.submission.plan_revision} · ${candidate.review?.disposition || "pending"}`,
            () =>
              run(async () => {
                selected = (await api(candidatePath(candidate.id))).candidate;
                preview = null;
              }),
            { disabled: busy },
          ),
        );
    }
    if (selected) root.append(detail(selected));
    root.append(el("h3", "", "Canonical update operations"));
    if (!operations)
      root.append(el("p", "empty", "Update operations unavailable."));
    else
      for (const operation of operations.operations || [])
        root.append(
          action(
            `${operation.destination_path} · ${operation.state} · ${operation.operation_id}`,
            () =>
              run(async () => {
                preview = await api(
                  `/api/v2/knowledge/promotions/${encodeURIComponent(operation.operation_id)}`,
                );
              }),
            { disabled: busy },
          ),
        );
    if (preview) root.append(promotionPreview());
  }
  render();
  return { root, load };
}
