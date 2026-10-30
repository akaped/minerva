// The Reminders page of a workspace: its reminders, one-off or repeating
// ("Car insurance, every year") and grouped by category, shared by everyone in
// the workspace, together with every ⏰ time written in its notes.

const UNITS = ["day", "week", "month", "year"];
// Categories take a colour from their name, so it never changes as others
// are added. Hues far enough apart to tell at a glance.
const COLORS = [
  "#4f5bd5",
  "#d0572f",
  "#1f8a5b",
  "#b0399a",
  "#a8741a",
  "#0e8a9a",
  "#c23a4f",
];
const DAY = 86400e3;

/** "Once", "Every month", "Every 2 years". */
export function repeatLabel(repeat) {
  if (!repeat) return "Once";
  return repeat.every === 1
    ? `Every ${repeat.unit}`
    : `Every ${repeat.every} ${repeat.unit}s`;
}

/** A ⏰ time as written in a note ("2026-11-02 09:00"), in local time. */
export function stampTime(stamp) {
  const [date, time] = stamp.split(" ");
  const [y, m, d] = date.split("-").map(Number);
  const [h, min] = time.split(":").map(Number);
  return new Date(y, m - 1, d, h, min).getTime();
}

/** How far `at` is from `now`, both in ms: "today", "in 3 days", "2 days ago". */
export function relative(at, now) {
  const start = (t) => new Date(t).setHours(0, 0, 0, 0);
  const days = Math.round((start(at) - start(now)) / DAY);
  if (days === 0) return at < now ? "Today, now due" : "Today";
  if (days === 1) return "Tomorrow";
  if (days === -1) return "Yesterday";
  if (days > 0 && days < 60) return `In ${days} days`;
  if (days < 0 && days > -60) return `${-days} days ago`;
  const months = Math.round(Math.abs(days) / 30.4);
  if (months < 24)
    return days > 0 ? `In ${months} months` : `${months} months ago`;
  const years = Math.round(Math.abs(days) / 365.25);
  return days > 0 ? `In ${years} years` : `${years} years ago`;
}

/**
 * The page's sections, each sorted by time. Workspace reminders stay due
 * until marked done; times in notes simply pass.
 */
export function groupReminders(items, now) {
  const groups = { due: [], week: [], month: [], later: [], past: [] };
  for (const r of items) {
    if (r.at <= now) groups[r.kind === "own" ? "due" : "past"].push(r);
    else if (r.at - now <= 7 * DAY) groups.week.push(r);
    else if (r.at - now <= 31 * DAY) groups.month.push(r);
    else groups.later.push(r);
  }
  for (const list of Object.values(groups)) list.sort((a, b) => a.at - b.at);
  groups.past.reverse();
  return groups;
}

/** The text of a block with its ⏰ time and Markdown marks left out. */
export function blockText(markdown) {
  return (
    markdown
      .replace(/⏰\d{4}-\d\d-\d\d[ T]\d\d:\d\d/g, "")
      .replace(/^\s*(#{1,6}\s|[-*+]\s(\[[ xX]\]\s)?|\d+[.)]\s|>\s?)/, "")
      .replace(/[*_~`=]{1,2}([^*_~`=]+)[*_~`=]{1,2}/g, "$1")
      .replace(/\s+/g, " ")
      .trim() || "Reminder"
  );
}

function color(name) {
  let h = 0;
  for (const c of name.toLowerCase()) h = (h * 31 + c.codePointAt(0)) >>> 0;
  return COLORS[h % COLORS.length];
}
const local = (ms) =>
  new Date(ms - new Date(ms).getTimezoneOffset() * 60e3)
    .toISOString()
    .slice(0, 16);

let filter = "all";

export async function renderReminders(ctx) {
  const { api, esc, workspaceId, error } = ctx;
  const root = document.querySelector("#content");
  document.querySelector("#breadcrumb").textContent = "Reminders";
  root.innerHTML =
    '<section class="page reminders-page"><div class="eyebrow">NOTHING IMPORTANT SLIPS BY</div><div class="page-heading"><div><h1>Reminders</h1><p class="subtitle">Everything coming up in this workspace: its reminders, shared with everyone in it, and every ⏰ time written in its notes.</p></div></div><p class="subtitle">Loading…</p></section>';
  const result = await api("reminders");
  if (!root.isConnected) return;
  const items = [
    ...result.reminders
      .filter((r) => !r.note_id && r.workspace_id === workspaceId)
      .map((r) => ({ ...r, kind: "own", at: r.at * 1000 })),
    ...result.from_notes
      .filter((r) => r.workspace_id === workspaceId)
      .map((r) => ({
        ...r,
        kind: "note",
        at: stampTime(r.stamp),
        title: blockText(r.text),
        category: "",
      })),
  ];
  draw(ctx, root, items, result.enabled);
}

function draw(ctx, root, items, mailEnabled) {
  const { esc } = ctx;
  const own = items.filter((r) => r.kind === "own");
  const counts = new Map();
  for (const r of own)
    if (r.category) counts.set(r.category, (counts.get(r.category) || 0) + 1);
  const categories = [...counts.keys()].sort((a, b) => a.localeCompare(b));
  const notes = items.filter((r) => r.kind === "note");
  if (
    filter !== "all" &&
    filter !== "notes" &&
    filter !== "none" &&
    !counts.has(filter)
  )
    filter = "all";
  const shown = items.filter((r) =>
    filter === "all"
      ? true
      : filter === "notes"
        ? r.kind === "note"
        : filter === "none"
          ? r.kind === "own" && !r.category
          : r.category === filter,
  );
  const now = Date.now();
  const groups = groupReminders(shown, now);
  const chip = (value, label, n, c) =>
    `<button type="button" data-filter="${esc(value)}" class="${filter === value ? "active" : ""}" ${c ? `style="--cat:${c}"` : ""}>${c ? '<i class="cat-dot" aria-hidden="true"></i>' : ""}${esc(label)} <span class="count">${n}</span></button>`;
  const uncategorized = own.filter((r) => !r.category).length;
  const filters =
    chip("all", "All", items.length) +
    categories.map((c) => chip(c, c, counts.get(c), color(c))).join("") +
    (uncategorized && categories.length
      ? chip("none", "No category", uncategorized)
      : "") +
    (notes.length ? chip("notes", "From notes", notes.length) : "");
  const row = (r) => {
    const d = new Date(r.at);
    const when = `${relative(r.at, now)} · ${d.toLocaleString(undefined, { weekday: "short", day: "numeric", month: "short", year: d.getFullYear() === new Date(now).getFullYear() ? undefined : "numeric", hour: "2-digit", minute: "2-digit" })}`;
    const meta =
      r.kind === "own"
        ? `${r.repeat ? `<span class="reminder-repeat">↻ ${esc(repeatLabel(r.repeat))}</span>` : ""}${r.category ? `<span class="reminder-cat" style="--cat:${color(r.category)}">${esc(r.category)}</span>` : ""}${r.created_by && r.created_by !== ctx.me ? `<span>Added by ${esc(r.created_by)}</span>` : ""}${r.sent ? '<span title="The email for this time was sent to everyone in the workspace">✉ Sent</span>' : ""}`
        : `<button class="text-link" data-open-note="${esc(r.note_id)}">In ${esc(r.icon || "")} ${esc(r.note_title || "Untitled")}</button>${r.emailed ? '<span title="You get this block by email">✉ By email</span>' : ""}`;
    const actions =
      r.kind === "own" && r.can_edit
        ? `<button class="btn" data-done="${esc(r.id)}" title="${r.repeat ? "Done: move to the next time" : "Done: remove it"}">✓ Done</button><button class="btn" data-edit="${esc(r.id)}" aria-label="Edit ${esc(r.title)}">Edit</button><button class="btn" data-delete="${esc(r.id)}" aria-label="Delete ${esc(r.title)}">×</button>`
        : "";
    return `<li class="reminder-row ${r.kind === "own" && r.at <= now ? "due" : ""}"><div class="reminder-date" aria-hidden="true"><b>${d.getDate()}</b><span>${esc(d.toLocaleString(undefined, { month: "short" }))}</span></div><div class="reminder-main"><strong>${esc(r.title)}</strong><div class="reminder-meta"><span class="reminder-when">${esc(when)}</span>${meta}</div></div><div class="reminder-actions">${actions}</div></li>`;
  };
  const section = (title, list, extra = "") =>
    list.length
      ? `<section class="reminder-group ${extra}"><h2>${title} <span class="count">${list.length}</span></h2><ul>${list.map(row).join("")}</ul></section>`
      : "";
  const empty = !items.length
    ? `<div class="empty"><h3>Nothing to remember yet.</h3>Add reminders like “Car insurance, every year” or “Blood test, every month”. Times written in notes with /remindme show here too.</div>`
    : !shown.length
      ? '<div class="empty">Nothing here.</div>'
      : "";
  root.innerHTML = `<section class="page reminders-page"><div class="eyebrow">NOTHING IMPORTANT SLIPS BY</div><div class="page-heading"><div><h1>Reminders</h1><p class="subtitle">Everything coming up in this workspace: its reminders, shared with everyone in it, and every ⏰ time written in its notes.</p></div>${ctx.canAdd ? '<button class="btn primary" id="new-reminder">+ New reminder</button>' : ""}</div>${mailEnabled ? "" : '<p class="reminder-notice">This server does not send email, so reminders are only shown here. Come back to this page to see what is due.</p>'}${items.length ? `<nav class="reminder-filters" aria-label="Categories">${filters}</nav>` : ""}${section("Due", groups.due, "due")}${section("Next 7 days", groups.week)}${section("Next 30 days", groups.month)}${section("Later", groups.later)}${groups.past.length ? `<details class="reminder-past"><summary>Past times in notes <span class="count">${groups.past.length}</span></summary><ul>${groups.past.slice(0, 100).map(row).join("")}</ul></details>` : ""}${empty}</section>`;

  const again = () => renderReminders(ctx).catch(ctx.error);
  const find = (id) => items.find((r) => r.kind === "own" && r.id === id);
  const add = root.querySelector("#new-reminder");
  if (add) add.onclick = () => dialog(ctx, null, categories, again);
  root.querySelectorAll("[data-filter]").forEach(
    (b) =>
      (b.onclick = () => {
        filter = b.dataset.filter;
        draw(ctx, root, items, mailEnabled);
      }),
  );
  root
    .querySelectorAll("[data-open-note]")
    .forEach((b) => (b.onclick = () => ctx.openNote(b.dataset.openNote)));
  root
    .querySelectorAll("[data-edit]")
    .forEach(
      (b) =>
        (b.onclick = () =>
          dialog(ctx, find(b.dataset.edit), categories, again)),
    );
  root.querySelectorAll("[data-done]").forEach(
    (b) =>
      (b.onclick = async () => {
        b.disabled = true;
        try {
          const { next } = await ctx.api(
            `reminders/${b.dataset.done}/done`,
            "POST",
          );
          ctx.toast(
            next
              ? `Done. Next time: ${new Date(next.at * 1000).toLocaleDateString(undefined, { dateStyle: "medium" })}`
              : "Done.",
          );
          await again();
        } catch (e) {
          b.disabled = false;
          ctx.error(e);
        }
      }),
  );
  root.querySelectorAll("[data-delete]").forEach(
    (b) =>
      (b.onclick = async () => {
        const r = find(b.dataset.delete);
        if (!confirm(`Delete the reminder “${r.title}”?`)) return;
        try {
          await ctx.api(`reminders/${r.id}`, "DELETE");
          await again();
        } catch (e) {
          ctx.error(e);
        }
      }),
  );
}

/** The New / Edit reminder dialog. */
function dialog(ctx, r, categories, done) {
  const { esc, modal } = ctx;
  const soon = new Date(Date.now() + DAY);
  soon.setHours(9, 0, 0, 0);
  const repeat = r?.repeat;
  const preset = !repeat ? "" : repeat.every === 1 ? repeat.unit : "custom";
  const option = (value, label) =>
    `<option value="${value}" ${preset === value ? "selected" : ""}>${label}</option>`;
  modal.innerHTML = `<form class="reminder-form"><h2>${r ? "Edit reminder" : "New reminder"}</h2><label>Name<input name="title" required maxlength="200" placeholder="e.g. Car insurance" value="${esc(r?.title || "")}"></label><label>${r ? "Next time" : "When"}<input name="at" type="datetime-local" required value="${local(r ? r.at : soon.getTime())}"></label><label>Repeats<select name="preset">${option("", "Does not repeat")}${option("day", "Every day")}${option("week", "Every week")}${option("month", "Every month")}${option("year", "Every year")}${option("custom", "Custom…")}</select></label><div class="reminder-custom" ${preset === "custom" ? "" : "hidden"}><label>Every<input name="every" type="number" min="1" max="1000" value="${repeat?.every || 2}"></label><label>Unit<select name="unit">${UNITS.map((u) => `<option value="${u}" ${repeat?.unit === u ? "selected" : ""}>${u}s</option>`).join("")}</select></label></div><label>Category<input name="category" maxlength="40" list="reminder-categories" placeholder="e.g. car, health, home" value="${esc(r?.category || "")}" autocomplete="off"></label><datalist id="reminder-categories">${categories.map((c) => `<option value="${esc(c)}">`).join("")}</datalist>${categories.length ? `<div class="reminder-suggest">${categories.map((c) => `<button type="button" data-category="${esc(c)}" style="--cat:${color(c)}">${esc(c)}</button>`).join("")}</div>` : ""}<div class="form-error" role="alert"></div><footer><button type="button" class="btn" id="cancel">Cancel</button><button class="btn primary" type="submit">${r ? "Save" : "Add reminder"}</button></footer></form>`;
  modal.showModal();
  const form = modal.querySelector("form");
  form.preset.onchange = () =>
    (form.querySelector(".reminder-custom").hidden =
      form.preset.value !== "custom");
  form
    .querySelectorAll("[data-category]")
    .forEach(
      (b) => (b.onclick = () => (form.category.value = b.dataset.category)),
    );
  form.querySelector("#cancel").onclick = () => modal.close();
  form.title.focus();
  form.onsubmit = async (e) => {
    e.preventDefault();
    const when = new Date(form.at.value);
    const preset = form.preset.value;
    const body = {
      workspace_id: ctx.workspaceId,
      title: form.title.value,
      category: form.category.value,
      at: Math.floor(when.getTime() / 1000),
      offset: -when.getTimezoneOffset(),
      repeat: !preset
        ? null
        : preset === "custom"
          ? { every: Number(form.every.value), unit: form.unit.value }
          : { every: 1, unit: preset },
    };
    const submit = form.querySelector("[type=submit]");
    submit.disabled = true;
    try {
      await ctx.api(
        r ? `reminders/${r.id}` : "reminders",
        r ? "PUT" : "POST",
        body,
      );
      modal.close();
      await done();
    } catch (err) {
      form.querySelector(".form-error").textContent = err.message;
    } finally {
      submit.disabled = false;
    }
  };
}
