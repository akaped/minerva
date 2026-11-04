import { $t, $tn, msg } from "./i18n.js";
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
  if (!repeat) return $t("Once");
  const units = {
    day: [msg("Every day"), msg("Every {count} days")],
    week: [msg("Every week"), msg("Every {count} weeks")],
    month: [msg("Every month"), msg("Every {count} months")],
    year: [msg("Every year"), msg("Every {count} years")],
  }[repeat.unit];
  return $tn(repeat.every, units[0], units[1]);
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
  if (days === 0) return at < now ? $t("Today, now due") : $t("Today");
  if (days === 1) return $t("Tomorrow");
  if (days === -1) return $t("Yesterday");
  if (days > 0 && days < 60) return $t("In {count} days", { count: days });
  if (days < 0 && days > -60) return $t("{count} days ago", { count: -days });
  const months = Math.round(Math.abs(days) / 30.4);
  if (months < 24)
    return days > 0
      ? $t("In {count} months", { count: months })
      : $t("{count} months ago", { count: months });
  const years = Math.round(Math.abs(days) / 365.25);
  return days > 0
    ? $t("In {count} years", { count: years })
    : $t("{count} years ago", { count: years });
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
  document.querySelector("#breadcrumb").textContent = $t("Reminders");
  root.innerHTML = `<section class="page reminders-page"><div class="eyebrow">${$t("NOTHING IMPORTANT SLIPS BY")}</div><div class="page-heading"><div><h1>${$t("Reminders")}</h1><p class="subtitle">${$t("Everything coming up in this workspace: its reminders, shared with everyone in it, and every ⏰ time written in its notes.")}</p></div></div><p class="subtitle">${$t("Loading…")}</p></section>`;
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
    chip("all", $t("All"), items.length) +
    categories.map((c) => chip(c, c, counts.get(c), color(c))).join("") +
    (uncategorized && categories.length
      ? chip("none", $t("No category"), uncategorized)
      : "") +
    (notes.length ? chip("notes", $t("From notes"), notes.length) : "");
  const row = (r) => {
    const d = new Date(r.at);
    const when = `${relative(r.at, now)} · ${d.toLocaleString(undefined, { weekday: "short", day: "numeric", month: "short", year: d.getFullYear() === new Date(now).getFullYear() ? undefined : "numeric", hour: "2-digit", minute: "2-digit" })}`;
    const meta =
      r.kind === "own"
        ? `${r.repeat ? `<span class="reminder-repeat">↻ ${esc(repeatLabel(r.repeat))}</span>` : ""}${r.category ? `<span class="reminder-cat" style="--cat:${color(r.category)}">${esc(r.category)}</span>` : ""}${r.created_by && r.created_by !== ctx.me ? `<span>${$t("Added by {created_by}", { created_by: esc(r.created_by) })}</span>` : ""}${r.sent ? `<span title="${$t("The email for this time was sent to everyone in the workspace")}">${$t("✉ Sent")}</span>` : ""}`
        : `<button class="text-link" data-open-note="${esc(r.note_id)}">In ${esc(r.icon || "")} ${esc(r.note_title || $t("Untitled"))}</button>${r.emailed ? `<span title="${$t("You get this block by email")}">${$t("✉ By email")}</span>` : ""}`;
    const actions =
      r.kind === "own" && r.can_edit
        ? `<button class="btn" data-done="${esc(r.id)}" title="${r.repeat ? $t("Done: move to the next time") : $t("Done: remove it")}">${$t("✓ Done")}</button><button class="btn" data-edit="${esc(r.id)}" aria-label="Edit ${esc(r.title)}">${$t("Edit")}</button><button class="btn" data-delete="${esc(r.id)}" aria-label="Delete ${esc(r.title)}">×</button>`
        : "";
    return `<li class="reminder-row ${r.kind === "own" && r.at <= now ? "due" : ""}"><div class="reminder-date" aria-hidden="true"><b>${d.getDate()}</b><span>${esc(d.toLocaleString(undefined, { month: "short" }))}</span></div><div class="reminder-main"><strong>${esc(r.title)}</strong><div class="reminder-meta"><span class="reminder-when">${esc(when)}</span>${meta}</div></div><div class="reminder-actions">${actions}</div></li>`;
  };
  const section = (title, list, extra = "") =>
    list.length
      ? `<section class="reminder-group ${extra}"><h2>${title} <span class="count">${list.length}</span></h2><ul>${list.map(row).join("")}</ul></section>`
      : "";
  const empty = !items.length
    ? `<div class="empty"><h3>${$t("Nothing to remember yet.")}</h3>${$t("Add reminders like “Car insurance, every year” or “Blood test, every month”. Times written in notes with /remindme show here too.")}</div>`
    : !shown.length
      ? `<div class="empty">${$t("Nothing here.")}</div>`
      : "";
  root.innerHTML = `<section class="page reminders-page"><div class="eyebrow">${$t("NOTHING IMPORTANT SLIPS BY")}</div><div class="page-heading"><div><h1>${$t("Reminders")}</h1><p class="subtitle">${$t("Everything coming up in this workspace: its reminders, shared with everyone in it, and every ⏰ time written in its notes.")}</p></div><div class="toolbar-group"><button class="btn" id="calendar-feed" title="${$t("See your reminders in Google Calendar, Apple Calendar, Outlook or Thunderbird")}">${$t("📅 Add to your calendar")}</button>${ctx.canAdd ? `<button class="btn primary" id="new-reminder">${$t("+ New reminder")}</button>` : ""}</div></div>${mailEnabled ? "" : `<p class="reminder-notice">${$t("This server does not send email, so reminders are only shown here. Come back to this page to see what is due.")}</p>`}${items.length ? `<nav class="reminder-filters" aria-label="${$t("Categories")}">${filters}</nav>` : ""}${section($t("Due"), groups.due, "due")}${section($t("Next 7 days"), groups.week)}${section($t("Next 30 days"), groups.month)}${section($t("Later"), groups.later)}${groups.past.length ? `<details class="reminder-past"><summary>${$t("Past times in notes")} <span class="count">${groups.past.length}</span></summary><ul>${groups.past.slice(0, 100).map(row).join("")}</ul></details>` : ""}${empty}</section>`;

  const again = () => renderReminders(ctx).catch(ctx.error);
  const find = (id) => items.find((r) => r.kind === "own" && r.id === id);
  const add = root.querySelector("#new-reminder");
  if (add) add.onclick = () => dialog(ctx, null, categories, again);
  root.querySelector("#calendar-feed").onclick = () =>
    calendarDialog(ctx).catch(ctx.error);
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
              ? $t("Done. Next time: {date}", {
                  date: new Date(next.at * 1000).toLocaleDateString(undefined, {
                    dateStyle: "medium",
                  }),
                })
              : $t("Done."),
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
        if (!confirm($t("Delete the reminder “{title}”?", { title: r.title })))
          return;
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
  modal.innerHTML = `<form class="reminder-form"><h2>${r ? $t("Edit reminder") : $t("New reminder")}</h2><label>${$t("Name")}<input name="title" required maxlength="200" placeholder="${$t("e.g. Car insurance")}" value="${esc(r?.title || "")}"></label><label>${r ? $t("Next time") : $t("When")}<input name="at" type="datetime-local" required value="${local(r ? r.at : soon.getTime())}"></label><label>${$t("Repeats")}<select name="preset">${option("", $t("Does not repeat"))}${option("day", $t("Every day"))}${option("week", $t("Every week"))}${option("month", $t("Every month"))}${option("year", $t("Every year"))}${option("custom", $t("Custom…"))}</select></label><div class="reminder-custom" ${preset === "custom" ? "" : "hidden"}><label>${$t("Every")}<input name="every" type="number" min="1" max="1000" value="${repeat?.every || 2}"></label><label>${$t("Unit")}<select name="unit">${UNITS.map((u) => `<option value="${u}" ${repeat?.unit === u ? "selected" : ""}>${u}s</option>`).join("")}</select></label></div><label>${$t("Category")}<input name="category" maxlength="40" list="reminder-categories" placeholder="${$t("e.g. car, health, home")}" value="${esc(r?.category || "")}" autocomplete="off"></label><datalist id="reminder-categories">${categories.map((c) => `<option value="${esc(c)}">`).join("")}</datalist>${categories.length ? `<div class="reminder-suggest">${categories.map((c) => `<button type="button" data-category="${esc(c)}" style="--cat:${color(c)}">${esc(c)}</button>`).join("")}</div>` : ""}<div class="form-error" role="alert"></div><footer><button type="button" class="btn" id="cancel">${$t("Cancel")}</button><button class="btn primary" type="submit">${r ? $t("Save") : $t("Add reminder")}</button></footer></form>`;
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
      // Calendar apps keep a repeat at this local time across DST changes.
      zone: Intl.DateTimeFormat().resolvedOptions().timeZone || "",
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

/** A calendar app's subscribe link for a feed address (`webcal://`). */
export function webcal(url) {
  return url.replace(/^https?:/, "webcal:");
}

/**
 * Add to your calendar: a private address with your reminders (and, if you
 * like, your journal days) that calendar apps subscribe to. The address is
 * shown once, when it is made; making a new one ends the old one.
 */
async function calendarDialog(ctx, made = null, scope = "mine") {
  const { esc, modal, api, toast } = ctx;
  // Your own feed (every workspace), or this workspace's, to share.
  const path =
    scope === "workspace"
      ? `calendar-feed/${ctx.workspaceId}`
      : "calendar-feed";
  const feed = made || (await api(path));
  const since = feed.created_at
    ? new Date(feed.created_at * 1000).toLocaleDateString(undefined, {
        dateStyle: "medium",
      })
    : "";
  const journal = `<label class="checkbox-label"><input type="checkbox" name="journal" ${feed.journal ? "checked" : ""}>${$t("Include journal days, as all-day events")}</label>`;
  const body = made
    ? `<p>${$t("Subscribe to this address in your calendar app. It is shown only now: copy it before closing.")}</p><label>${$t("Calendar address")}<input name="url" readonly value="${esc(made.url)}"></label><div class="toolbar-group"><button type="button" class="btn primary" id="copy-feed">${$t("Copy address")}</button><a class="btn" href="${esc(webcal(made.url))}">${$t("Open in calendar app")}</a></div><p class="hint">${$t("Google Calendar: Other calendars → + → From URL. Apple Calendar: File → New Calendar Subscription. Outlook: Add calendar → Subscribe from web. Calendars refresh it every few hours.")}</p>`
    : feed.enabled
      ? `<p>${$t("Your calendar address is on, since {since}. For safety it is not shown again: make a new one to see it, and the old one stops working.", { since: esc(since) })}</p>${journal}`
      : scope === "workspace"
        ? `<p>${$t("Get an address with this workspace's reminders and ⏰ times only, to share with people who are not in it. It shows what you can see here, and updates as it changes.")}</p>${journal}`
        : `<p>${$t("Get a private address that calendar apps subscribe to: your reminders, and every ⏰ time in notes you can open, in all your workspaces. It updates as they change.")}</p>${journal}`;
  const scopes = made
    ? ""
    : `<div class="calendar-scope" role="radiogroup" aria-label="${$t("Which calendar")}"><label class="checkbox-label"><input type="radio" name="scope" value="mine" ${scope === "mine" ? "checked" : ""}>${$t("All my workspaces, for me")}</label><label class="checkbox-label"><input type="radio" name="scope" value="workspace" ${scope === "workspace" ? "checked" : ""}>${$t("This workspace, to share")}</label></div>`;
  modal.innerHTML = `<form class="calendar-feed-form"><h2>${$t("Add to your calendar")}</h2>${scopes}${body}<p class="hint">${scope === "workspace" ? $t("Anyone with the address can see this workspace's reminders, without signing in. Share it with care; make a new one or turn it off to stop it.") : $t("Anyone with the address can see these reminders, without signing in. Keep it to yourself; if it leaks, make a new one or turn it off.")}</p><div class="form-error" role="alert"></div><footer>${feed.enabled && !made ? `<button type="button" class="btn danger" id="feed-off">${$t("Turn off")}</button>` : ""}<button type="button" class="btn" id="cancel">${made ? $t("Done") : $t("Cancel")}</button>${made ? "" : `<button class="btn primary" type="submit">${feed.enabled ? $t("New address") : $t("Create address")}</button>`}</footer></form>`;
  if (!modal.open) modal.showModal();
  const form = modal.querySelector("form");
  const fail = (e) =>
    (form.querySelector(".form-error").textContent = e.message);
  form.querySelector("#cancel").onclick = () => modal.close();
  const copy = form.querySelector("#copy-feed");
  form
    .querySelectorAll("[name=scope]")
    .forEach(
      (r) =>
        (r.onchange = () =>
          calendarDialog(ctx, null, r.value).catch((e) => fail(e))),
    );
  if (copy)
    copy.onclick = async () => {
      form.url.select();
      try {
        await navigator.clipboard.writeText(made.url);
        toast($t("Calendar address copied"));
      } catch {
        toast($t("Select the address and copy it."));
      }
    };
  if (form.journal && feed.enabled)
    form.journal.onchange = () =>
      api(path, "PUT", { journal: form.journal.checked })
        .then(() => toast($t("Calendar updated")))
        .catch(fail);
  const off = form.querySelector("#feed-off");
  if (off)
    off.onclick = async () => {
      try {
        await api(path, "DELETE");
        modal.close();
        toast($t("Calendar address turned off"));
      } catch (e) {
        fail(e);
      }
    };
  form.onsubmit = async (e) => {
    e.preventDefault();
    if (
      feed.enabled &&
      !confirm(
        $t(
          "Make a new address? Calendars subscribed to the old one stop updating.",
        ),
      )
    )
      return;
    try {
      const created = await api(path, "POST", {
        journal: !!form.journal?.checked,
      });
      await calendarDialog(ctx, created, scope);
    } catch (err) {
      fail(err);
    }
  };
}
