import { el, action } from "./daily-helpers.js";

// Editorial prose keeps its own citations; it is never spliced like a source quote.
export function recapFactText(fact, excluded) {
  if (
    !fact?.evidence_event_ids?.length ||
    fact.evidence_event_ids.some((id) => excluded.has(id))
  )
    return null;
  return fact.text;
}

function sourceChoices(content, events) {
  const byId = new Map(events.map((event) => [event.id, event]));
  return [
    ...new Set(content.workstreams.flatMap((work) => work.evidence_event_ids)),
  ].map((id) => ({
    id,
    label: byId.get(id)?.message || `Retained source ${id}`,
  }));
}

function factEditor(label, fact, choices, maxLength, changed) {
  const row = el("div", "recap-fact");
  row.dataset.originalCitations = JSON.stringify(
    fact?.evidence_event_ids || [],
  );
  const input = el("textarea");
  input.setAttribute("aria-label", label);
  input.rows = label === "Day summary" ? 4 : 2;
  input.maxLength = maxLength;
  input.value = fact?.text || "";
  const sources = el("select");
  sources.multiple = true;
  sources.size = Math.min(3, Math.max(1, choices.length));
  sources.setAttribute("aria-label", `${label} supporting reports`);
  for (const choice of choices) {
    const option = el("option", "", choice.label.slice(0, 140));
    option.value = choice.id;
    option.title = choice.label;
    option.selected = fact?.evidence_event_ids.includes(choice.id) || false;
    sources.append(option);
  }
  input.addEventListener("input", changed);
  sources.addEventListener("change", changed);
  row.append(
    el("label", "", label),
    input,
    el("span", "subtle", "Supporting reports · select one or more"),
    sources,
  );
  return row;
}

function readFact(row) {
  const selected = [...row.querySelector("select").selectedOptions].map(
    (option) => option.value,
  );
  const original = JSON.parse(row.dataset.originalCitations);
  return {
    text: row.querySelector("textarea").value,
    evidence_event_ids: [
      ...original.filter((id) => selected.includes(id)),
      ...selected.filter((id) => !original.includes(id)),
    ],
  };
}

export function readRecapEditor() {
  const root = document.getElementById("recap-editor");
  if (!root || root.dataset.active !== "true") return null;
  return {
    summary: readFact(root.querySelector(".recap-summary")),
    topics: [...root.querySelectorAll(".recap-topic-editor")].map((topic) => {
      const value = { title: topic.querySelector("input").value };
      for (const key of ["done", "pending"]) {
        const fact = readFact(
          topic.querySelector(`[data-recap-field="${key}"]`),
        );
        if (fact.text.trim()) value[key] = fact;
      }
      return value;
    }),
  };
}

export function renderRecapEditor(root, content, events, editable, changed) {
  if (content.schema_version !== 1 || !content.workstreams.length) return;
  const section = el("section", "recap-editor");
  section.id = "recap-editor";
  const add = action(
    "Add short recap",
    () => activate({ summary: null, topics: [{}] }),
    { disabled: !editable },
  );
  const body = el("div");
  const choices = sourceChoices(content, events);
  const topics = el("div");
  let topicNumber = 0;
  const addTopic = action("Add topic", () => {
    appendTopic({});
    changed();
  });
  function appendTopic(topic) {
    const index = ++topicNumber;
    const box = el("fieldset", "recap-topic-editor");
    const title = el("input");
    title.value = topic.title || "";
    title.maxLength = 80;
    title.setAttribute("aria-label", `Topic ${index} title`);
    title.addEventListener("input", changed);
    box.append(el("legend", "", "Topic"), title);
    for (const key of ["done", "pending"]) {
      const label = `${key === "done" ? "Done" : "Pending"} for topic ${index}`;
      const row = factEditor(label, topic[key], choices, 320, changed);
      row.dataset.recapField = key;
      box.append(row);
    }
    box.append(
      action("Remove topic", () => {
        box.remove();
        addTopic.disabled = false;
        changed();
      }),
    );
    topics.append(box);
    addTopic.disabled = topics.children.length >= 5;
  }
  function activate(recap, notify = true) {
    section.dataset.active = "true";
    body.replaceChildren();
    topics.replaceChildren();
    topicNumber = 0;
    const summary = factEditor(
      "Day summary",
      recap.summary,
      choices,
      1000,
      changed,
    );
    summary.classList.add("recap-summary");
    for (const topic of recap.topics) appendTopic(topic);
    body.append(
      el("h3", "", "Short recap"),
      el(
        "p",
        "subtle",
        "Write a short paragraph and up to five topics. Done and Pending reflect your interpretation of the selected reports; review them before saving.",
      ),
      summary,
      topics,
      addTopic,
      action("Remove recap", () => {
        section.dataset.active = "false";
        body.hidden = true;
        add.hidden = false;
        changed();
      }),
    );
    body.hidden = false;
    add.hidden = true;
    if (notify) changed();
  }
  section.append(add, body);
  root.append(section);
  if (content.recap) activate(content.recap, false);
  for (const control of section.querySelectorAll(
    "input, textarea, select, button",
  ))
    if (!editable) control.disabled = true;
}

export function renderRecapView(root, recap, excluded, events) {
  const byId = new Map(events.map((event) => [event.id, event]));
  let hidden = false;
  function appendFact(parent, fact, label) {
    const text = recapFactText(fact, excluded);
    if (text === null) {
      if (fact) hidden = true;
      return false;
    }
    const paragraph = el("p", "recap-prose");
    if (label) paragraph.append(el("strong", "", `${label}: `));
    paragraph.append(document.createTextNode(text));
    parent.append(paragraph);
    const sources = el("details", "recap-sources");
    sources.append(
      el(
        "summary",
        "",
        `${label || "Summary"} sources · ${fact.evidence_event_ids.length}`,
      ),
    );
    const list = el("ul");
    for (const id of fact.evidence_event_ids)
      list.append(
        el("li", "", byId.get(id)?.message || `Retained source ${id}`),
      );
    sources.append(list);
    parent.append(sources);
    return true;
  }
  root.append(el("h3", "", "Day summary"));
  appendFact(root, recap.summary);
  for (const topic of recap.topics) {
    const section = el("section", "recap-topic");
    section.append(el("h3", "", topic.title));
    const done = appendFact(section, topic.done, "Done");
    const pending = appendFact(section, topic.pending, "Pending");
    if (done || pending) root.append(section);
  }
  root.append(
    el("p", "subtle", "Editable recap · review before saving to your note."),
  );
  if (hidden)
    root.append(
      el(
        "p",
        "notice",
        "Some recap text is hidden because its supporting reports are omitted. Edit the recap or restore those reports.",
      ),
    );
}
