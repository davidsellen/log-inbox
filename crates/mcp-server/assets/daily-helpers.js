// Small, stateless DOM and display helpers shared by the views.
export const $ = (id) => document.getElementById(id);
// Let the browser own new-tab, new-window and non-primary link clicks.
export function isPlainLinkClick(event) {
  return (
    !event.button &&
    !event.ctrlKey &&
    !event.metaKey &&
    !event.shiftKey &&
    !event.altKey
  );
}
export function localDate(date = new Date()) {
  const year = date.getFullYear();
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${year}-${month}-${day}`;
}

export function validDate(value) {
  if (!/^\d{4}-\d{2}-\d{2}$/.test(value || "")) return false;
  const parsed = new Date(`${value}T00:00:00Z`);
  return (
    !Number.isNaN(parsed.valueOf()) &&
    parsed.toISOString().slice(0, 10) === value
  );
}

export function el(tag, className, text) {
  const node = document.createElement(tag);
  if (className) node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

export function action(
  label,
  handler,
  { primary = false, disabled = false } = {},
) {
  const button = el("button", primary ? "primary" : "", label);
  button.type = "button";
  button.disabled = disabled;
  button.addEventListener("click", handler);
  return button;
}

export function identityLabel(field) {
  return (
    {
      product: "Product",
      project: "Project",
      repo: "Repository",
      app: "Application",
      service: "Service",
      module: "Module",
      work_item: "Work item",
      pull_request: "Pull request",
    }[field] || field.replaceAll("_", " ")
  );
}

export function noteTitle(path) {
  const name = (path || "").split("/").pop() || path || "Unknown note";
  return name.replace(/\.md$/i, "");
}
