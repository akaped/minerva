// The overview page as a dashboard of widgets the user arranges: reorder by
// dragging (or with the arrow buttons), resize, configure, add, and remove.
// Widgets sit in rows that always fill the page width: a widget's size is a
// preference, and the widgets of a row grow or shrink to fill it. The layout is
// saved per account on the server.
import { intervalLabel, rich } from "./flashcards.js";

export const WIDGETS = {
  welcome: {
    name: "Welcome",
    size: "full",
    about: "A short hello with a link to start writing.",
  },
  folders: {
    name: "Folders",
    size: "full",
    about: "The top-level folders of this workspace.",
  },
  recent: {
    name: "Recently updated",
    size: "l",
    about: "The notes you and others changed last.",
  },
  favorites: { name: "Favorites", size: "s", about: "Your starred notes." },
  journal: {
    name: "Journal",
    size: "s",
    about: "This month, with the days that have an entry.",
  },
  flashcards: {
    name: "Flashcards",
    size: "m",
    about: "Cards due for review; answer them right here.",
  },
  mentions: {
    name: "Mentions",
    size: "m",
    about: "Notes where someone mentioned you with @.",
  },
  chart: {
    name: "Chart",
    size: "m",
    about: "A database property as bars or a pie.",
  },
};
const SIZES = [
  ["s", "Small"],
  ["m", "Medium"],
  ["l", "Large"],
  ["full", "Full width"],
];
const DEFAULT = [
  { type: "welcome", size: "full" },
  { type: "folders", size: "full" },
  { type: "recent", size: "l" },
  { type: "favorites", size: "s" },
  { type: "journal", size: "s" },
  { type: "flashcards", size: "m" },
];
const COLUMNS = 4;
const SPAN = { s: 1, m: 2, l: 3, full: 4 };
const fresh = (w) => ({
  id: crypto.randomUUID().slice(0, 8),
  options: {},
  ...w,
});

/**
 * Splits the widgets into rows. A widget with `newRow` starts one; a row holds
 * at most four widgets, and a fifth moves on to start the next row.
 */
export function rows(widgets) {
  const out = [];
  for (const w of widgets) {
    const row = out.at(-1);
    if (!row || w.newRow || row.length === COLUMNS) out.push([w]);
    else row.push(w);
  }
  return out;
}
/**
 * The columns each widget of a row spans, filling the row exactly: the largest
 * shrink first when the preferred sizes do not fit, and the extra space goes to
 * the largest when they leave a gap.
 */
export function spans(row) {
  const out = row.map((w) => SPAN[w.size] || 1);
  const largest = (pick) =>
    out.reduce((best, v, i) => (pick(v, out[best]) ? i : best), 0);
  while (out.reduce((a, b) => a + b, 0) > COLUMNS)
    out[largest((v, b) => v > b)] -= 1;
  while (out.reduce((a, b) => a + b, 0) < COLUMNS)
    out[largest((v, b) => v > b)] += 1;
  return out;
}
/** Marks where rows start, so the layout survives saving as a flat list. */
function settle(widgets) {
  rows(widgets).forEach((row) => row.forEach((w, i) => (w.newRow = i === 0)));
  return widgets;
}
/** Older layouts have no rows: pack them by their sizes, as they were drawn. */
function packed(widgets) {
  if (widgets.some((w) => "newRow" in w)) return settle(widgets);
  let used = COLUMNS;
  for (const w of widgets) {
    const span = SPAN[w.size] || 1;
    w.newRow = used + span > COLUMNS;
    used = w.newRow ? span : used + span;
  }
  return settle(widgets);
}

/**
 * Draws the dashboard into `root`. `app` supplies the app's data and helpers:
 * { api, esc, icon, data, workspaceId, notes, folderCards, journal,
 *   openNote, openJournal, createNote, navigate, error, toast }.
 */
export async function renderDashboard(root, app) {
  const { api, esc, icon } = app;
  let widgets;
  try {
    widgets = (await api("account/dashboard")).widgets;
  } catch {
    widgets = null;
  }
  widgets = packed((widgets || DEFAULT).map(fresh));
  let editing = false;
  const save = () =>
    settle(widgets) &&
    api("account/dashboard", "PUT", { widgets }).catch(app.error);

  function draw() {
    if (!root.isConnected) return;
    root.innerHTML = `<div class="dashboard-bar">${
      editing
        ? `<button class="btn primary" id="dash-add">${icon("plus")}Add widget</button><button class="btn" id="dash-reset">Restore default layout</button><span class="dash-hint">Drag a widget beside another to share its row, or above or below a row to give it a row of its own.</span><button class="btn primary" id="dash-done">Done</button>`
        : `<button class="btn" id="dash-edit">${icon("grid")}Customize</button>`
    }</div><div class="dashboard ${editing ? "editing" : ""}">${
      rows(widgets)
        .map((row) => {
          const width = spans(row);
          return row
            .map((w) => frame(w, widgets.indexOf(w), width[row.indexOf(w)]))
            .join("");
        })
        .join("") ||
      '<div class="empty">Your dashboard is empty. Choose Customize to add widgets.</div>'
    }</div>`;
    bind();
    widgets.forEach((w) => fill(w).catch(() => {}));
  }
  function frame(w, i, span) {
    const spec = WIDGETS[w.type];
    const title = w.type === "chart" ? w.options.title || spec.name : spec.name;
    const tools = editing
      ? `<div class="widget-tools"><button data-move="${i}:-1" ${i === 0 ? "disabled" : ""} aria-label="Move ${esc(title)} earlier" title="Move earlier">←</button><button data-move="${i}:1" ${i === widgets.length - 1 ? "disabled" : ""} aria-label="Move ${esc(title)} later" title="Move later">→</button><select data-size="${i}" aria-label="Preferred size of ${esc(title)}" title="Preferred size; the row stretches or shrinks it to fill the width">${SIZES.map(([v, label]) => `<option value="${v}" ${w.size === v ? "selected" : ""}>${label}</option>`).join("")}</select>${w.type === "chart" ? `<button data-configure="${i}" aria-label="Chart settings" title="Chart settings">⚙</button>` : ""}<button data-remove="${i}" aria-label="Remove ${esc(title)}" title="Remove">×</button></div>`
      : "";
    // The welcome block can be dismissed for good, without customizing.
    const dismiss =
      !editing && w.type === "welcome"
        ? `<button class="widget-dismiss" data-remove="${i}" aria-label="Hide this welcome for good" title="Hide for good">×</button>`
        : "";
    return `<section class="widget widget-${w.type}" style="--span:${span}" data-widget="${w.id}" aria-label="${esc(title)}">${w.type === "welcome" ? "" : `<header class="widget-head"><h2>${esc(title)}</h2>${tools}</header>`}${w.type === "welcome" ? tools : ""}${dismiss}<div class="widget-body" id="widget-${w.id}"><p class="widget-loading">Loading…</p></div></section>`;
  }
  function bind() {
    const on = (sel, fn, event = "click") =>
      root
        .querySelectorAll(sel)
        .forEach((el) => el.addEventListener(event, (e) => fn(el, e)));
    on("#dash-edit", () => ((editing = true), draw()));
    on("#dash-done", () => ((editing = false), draw()));
    on("#dash-add", addDialog);
    on("#dash-reset", () => {
      widgets = packed(DEFAULT.map(fresh));
      save();
      draw();
    });
    on("[data-move]", (b) => {
      const [i, d] = b.dataset.move.split(":").map(Number);
      const [a, b2] = [widgets[i].newRow, widgets[i + d].newRow];
      [widgets[i], widgets[i + d]] = [widgets[i + d], widgets[i]];
      [widgets[i].newRow, widgets[i + d].newRow] = [a, b2];
      save();
      draw();
      root.querySelector(`[data-move="${i + d}:${d}"]`)?.focus();
    });
    on(
      "[data-size]",
      (s) => {
        widgets[Number(s.dataset.size)].size = s.value;
        save();
        draw();
      },
      "change",
    );
    on("[data-remove]", (b) => {
      const i = Number(b.dataset.remove);
      if (widgets[i].newRow && widgets[i + 1]) widgets[i + 1].newRow = true;
      widgets.splice(i, 1);
      save();
      draw();
      app.toast("Widget removed · add it back with Customize → Add widget");
    });
    on("[data-configure]", (b) =>
      chartDialog(widgets[Number(b.dataset.configure)]),
    );
    // Drag to reorder, while customizing. Pointer events rather than HTML
    // drag and drop, which embedded webviews (the desktop app) do not deliver.
    // Where a drop lands: beside the widget (left or right half), or in a new
    // row above or below its row (the top and bottom edges).
    const place = (el, e) => {
      const r = el.getBoundingClientRect(),
        y = (e.clientY - r.top) / r.height;
      if (y < 0.2) return "above";
      if (y > 0.8) return "below";
      return e.clientX < r.left + r.width / 2 ? "before" : "after";
    };
    const clear = () =>
      root
        .querySelectorAll("[data-drop]")
        .forEach((el) => el.removeAttribute("data-drop"));
    const scroller = () => {
      for (let el = root.parentElement; el; el = el.parentElement)
        if (
          el.scrollHeight > el.clientHeight &&
          /auto|scroll/.test(getComputedStyle(el).overflowY)
        )
          return el;
      return document.scrollingElement;
    };
    const under = (e) =>
      document
        .elementFromPoint(e.clientX, e.clientY)
        ?.closest(".dashboard [data-widget]");
    on(
      "[data-widget]",
      (el, down) => {
        if (
          !editing ||
          down.button !== 0 ||
          down.target.closest("button, select, input, a, textarea")
        )
          return;
        down.preventDefault();
        let moving = false;
        const startScroll = scroller().scrollTop;
        const move = (e) => {
          // A few pixels first, so a click stays a click.
          if (
            !moving &&
            Math.hypot(e.clientX - down.clientX, e.clientY - down.clientY) < 6
          )
            return;
          if (!moving) {
            moving = true;
            el.classList.add("dragging");
            document.body.classList.add("dragging-widget");
          }
          // The widget follows the pointer, also while the page scrolls.
          const scrolled = scroller().scrollTop - startScroll;
          el.style.transform = `translate(${e.clientX - down.clientX}px, ${e.clientY - down.clientY + scrolled}px)`;
          // Near the top or bottom of the window, scroll to reach other rows.
          const edge =
            e.clientY < 70 ? -1 : e.clientY > innerHeight - 70 ? 1 : 0;
          if (edge) scroller().scrollBy(0, edge * 18);
          clear();
          const target = under(e);
          if (target && target !== el) target.dataset.drop = place(target, e);
        };
        const up = (e) => {
          removeEventListener("pointermove", move);
          removeEventListener("pointerup", up);
          removeEventListener("pointercancel", up);
          el.classList.remove("dragging");
          el.style.transform = "";
          document.body.classList.remove("dragging-widget");
          clear();
          const target = moving && e.type === "pointerup" && under(e);
          if (target && target !== el)
            drop(el.dataset.widget, target.dataset.widget, place(target, e));
        };
        addEventListener("pointermove", move);
        addEventListener("pointerup", up);
        addEventListener("pointercancel", up);
      },
      "pointerdown",
    );
  }
  /** Moves widget `id` beside widget `targetId`, or into a row next to its row. */
  function drop(id, targetId, where) {
    const from = widgets.findIndex((w) => w.id === id);
    if (from < 0 || id === targetId) return;
    // Take the widget out; its row keeps its start.
    const [moved] = widgets.splice(from, 1);
    if (moved.newRow && widgets[from]) widgets[from].newRow = true;
    const target = widgets.find((w) => w.id === targetId);
    const row = rows(widgets).find((r) => r.includes(target));
    let at;
    if (where === "before") {
      at = widgets.indexOf(target);
      moved.newRow = target.newRow;
      target.newRow = false;
    } else if (where === "after") {
      at = widgets.indexOf(target) + 1;
      moved.newRow = false;
    } else {
      at =
        where === "above"
          ? widgets.indexOf(row[0])
          : widgets.indexOf(row.at(-1)) + 1;
      moved.newRow = true;
      if (widgets[at]) widgets[at].newRow = true;
    }
    widgets.splice(at, 0, moved);
    save();
    draw();
  }
  function addDialog() {
    const modal = document.querySelector("#modal");
    modal.innerHTML = `<form method="dialog" class="widget-picker"><h2>Add a widget</h2><div class="widget-choices">${Object.entries(
      WIDGETS,
    )
      .map(
        ([type, spec]) =>
          `<button type="button" class="widget-choice" data-add="${type}"><strong>${esc(spec.name)}</strong><small>${esc(spec.about)}</small></button>`,
      )
      .join(
        "",
      )}</div><footer><button class="btn">Close</button></footer></form>`;
    modal.showModal();
    modal.querySelectorAll("[data-add]").forEach((b) =>
      b.addEventListener("click", () => {
        const w = fresh({
          type: b.dataset.add,
          size: WIDGETS[b.dataset.add].size,
        });
        w.newRow = true;
        widgets.push(w);
        save();
        modal.close();
        draw();
        if (w.type === "chart") chartDialog(w);
        root
          .querySelector(`[data-widget="${w.id}"]`)
          ?.scrollIntoView({ block: "center" });
      }),
    );
  }

  // Widget contents.
  async function fill(w) {
    const body = root.querySelector(`#widget-${w.id}`);
    if (!body) return;
    const html = await (WIDGET_BODIES[w.type] || (() => ""))(w, body);
    if (typeof html === "string" && body.isConnected) body.innerHTML = html;
    body
      .querySelectorAll("[data-note]")
      .forEach((b) =>
        b.addEventListener("click", () =>
          app.openNote(b.dataset.note).catch(app.error),
        ),
      );
  }
  const noteList = (notes, empty) =>
    notes.length
      ? `<div class="widget-notes">${notes
          .map(
            (n) =>
              `<button class="widget-note" data-note="${n.id}"><span>${esc(n.icon || "▤")}</span><strong>${esc(n.title)}</strong><small>${esc(new Date(n.updated_at * 1000).toLocaleDateString())}</small></button>`,
          )
          .join("")}</div>`
      : `<p class="widget-empty">${empty}</p>`;
  const WIDGET_BODIES = {
    welcome: (w, body) => {
      body.innerHTML = `<div class="welcome-banner"><div class="welcome-copy"><div class="eyebrow">A FRESH START</div><h2>Good ideas start with a little space.</h2><p>A quick thought, a detailed plan, your next big thing.<br>Bring it all together in your workspace.</p><button class="text-link" id="start-writing">Start writing <span>↗</span></button></div>${app.art()}</div>`;
      body.querySelector("#start-writing").addEventListener("click", () =>
        Promise.resolve()
          .then(() => app.createNote())
          .catch(app.error),
      );
    },
    folders: (w, body) => {
      body.innerHTML = `<div class="folder-grid">${app.folderCards() || '<div class="widget-empty">No folders yet.</div>'}</div>`;
      body
        .querySelectorAll("[data-open-folder]")
        .forEach((b) =>
          b.addEventListener("click", () =>
            app.navigate("folder", b.dataset.openFolder),
          ),
        );
    },
    recent: () =>
      noteList(
        app
          .notes()
          .sort((a, b) => b.updated_at - a.updated_at)
          .slice(0, 8),
        "Nothing written yet.",
      ),
    mentions: async () =>
      noteList(
        await app.api("mentions"),
        "Nobody has mentioned you yet. Type @ in a note to mention someone.",
      ),
    favorites: () =>
      noteList(
        app
          .notes()
          .filter((n) => n.favorite)
          .sort((a, b) => a.title.localeCompare(b.title)),
        "Star a note to keep it here.",
      ),
    journal: (w, body) => {
      const month = w.options.month || app.today().slice(0, 7);
      const { weekdays, cells } = app.calendar(month);
      const [y, m] = month.split("-").map(Number);
      const label = new Date(y, m - 1, 1).toLocaleDateString(undefined, {
        month: "long",
        year: "numeric",
      });
      body.innerHTML = `<div class="calendar compact"><div class="calendar-head"><button class="btn" data-month="-1" aria-label="Previous month">‹</button><strong>${esc(label)}</strong><button class="btn" data-month="1" aria-label="Next month">›</button></div><div class="calendar-grid">${weekdays.map((d) => `<span class="weekday">${esc(d)}</span>`).join("")}${cells}</div><button class="btn widget-action" data-journal-today>${icon("calendar")}Today’s entry</button></div>`;
      body
        .querySelectorAll("[data-journal-date]")
        .forEach((b) =>
          b.addEventListener("click", () =>
            app.openJournal(b.dataset.journalDate).catch(app.error),
          ),
        );
      body
        .querySelector("[data-journal-today]")
        .addEventListener("click", () =>
          app.openJournal(app.today()).catch(app.error),
        );
      body.querySelectorAll("[data-month]").forEach((b) =>
        b.addEventListener("click", () => {
          const d = new Date(y, m - 1 + Number(b.dataset.month), 1);
          w.options.month = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`;
          fill(w);
        }),
      );
    },
    flashcards: async (w, body) => {
      const { decks } = await api("flashcards/decks");
      const due = decks.reduce((n, d) => n + d.due, 0);
      if (!decks.length) {
        body.innerHTML =
          '<p class="widget-empty">No decks yet. Create one in Flashcards.</p>';
        return;
      }
      const deck = decks.find((d) => d.due);
      if (!deck) {
        body.innerHTML = `<p class="widget-empty">All caught up · ${decks.length} deck${decks.length === 1 ? "" : "s"}, nothing due.</p>`;
        return;
      }
      // The deck's next due card, answered right here.
      const { deck: full, now } = await api(`flashcards/decks/${deck.id}`);
      const card = full.cards
        .filter((c) => !c.suspended && c.due <= now)
        .sort((a, b) => a.due - b.due)[0];
      body.innerHTML = `<p class="widget-due"><strong>${due}</strong> card${due === 1 ? "" : "s"} due · ${esc(deck.name)}</p><div class="widget-card"><div class="widget-card-face">${rich(card.front, esc)}</div><div class="widget-card-answer" hidden><hr><div class="widget-card-face">${rich(card.back, esc)}</div><div class="fc-ratings">${["again", "hard", "good", "easy"].map((r) => `<button class="btn" data-rate="${r}">${r[0].toUpperCase() + r.slice(1)}<small>${intervalLabel(card.interval, r)}</small></button>`).join("")}</div></div><button class="btn primary widget-action" data-reveal>Reveal answer</button></div>`;
      body.querySelector("[data-reveal]").addEventListener("click", (e) => {
        body.querySelector(".widget-card-answer").hidden = false;
        e.target.hidden = true;
      });
      body.querySelectorAll("[data-rate]").forEach((b) =>
        b.addEventListener("click", async () => {
          body
            .querySelectorAll("[data-rate]")
            .forEach((x) => (x.disabled = true));
          try {
            await api(
              `flashcards/decks/${deck.id}/cards/${card.id}/review`,
              "POST",
              { rating: b.dataset.rate, revision: card.revision },
            );
          } catch (err) {
            app.error(err);
          }
          fill(w);
        }),
      );
    },
    chart: async (w, body) => {
      const o = w.options;
      if (!o.database) {
        body.innerHTML = `<p class="widget-empty">Choose a database and a property to chart.</p><button class="btn widget-action" data-configure-here>Set up chart</button>`;
        body
          .querySelector("[data-configure-here]")
          .addEventListener("click", () => chartDialog(w));
        return;
      }
      const db = await api("databases/" + o.database);
      const column = db.columns.find((c) => c.id === o.column) || db.columns[0];
      const counts = new Map();
      for (const r of db.rows.filter((r) => !r.archived)) {
        const v = r.cells[column.id];
        const key =
          v === undefined || v === null || v === ""
            ? "(empty)"
            : column.kind === "checkbox"
              ? v
                ? "Yes"
                : "No"
              : String(v);
        counts.set(key, (counts.get(key) || 0) + 1);
      }
      // Select properties keep their option order; others go largest first.
      let groups = [...counts];
      if (column.kind === "select")
        groups.sort(
          (a, b) => column.options.indexOf(a[0]) - column.options.indexOf(b[0]),
        );
      else groups.sort((a, b) => b[1] - a[1]);
      const caption = `${esc(db.name)} · records by ${esc(column.name)}`;
      body.innerHTML = groups.length
        ? `${o.kind === "pie" ? pie(groups, esc) : bars(groups, esc)}<p class="chart-caption">${caption}</p>`
        : `<p class="widget-empty">${esc(db.name)} has no records yet.</p>`;
    },
  };
  function chartDialog(w) {
    const modal = document.querySelector("#modal");
    const dbs = app.data.databases || [];
    if (!dbs.length) {
      app.toast("Create a database first; then chart it here.");
      return;
    }
    const o = w.options;
    modal.innerHTML = `<form id="chart-form"><h2>Chart settings</h2><p>Counts the database’s records by a property.</p><label>Title <small class="hint">(optional)</small><input name="title" maxlength="80" value="${esc(o.title || "")}"></label><label>Database<select name="database">${dbs.map((d) => `<option value="${esc(d.id)}" ${d.id === o.database ? "selected" : ""}>${esc(d.name)}</option>`).join("")}</select></label><label>Property<select name="column"></select></label><label>Chart<select name="kind"><option value="bar" ${o.kind !== "pie" ? "selected" : ""}>Bars</option><option value="pie" ${o.kind === "pie" ? "selected" : ""}>Pie</option></select></label><div class="form-error" role="alert"></div><footer><button type="button" class="btn" id="chart-cancel">Cancel</button><button class="btn primary">Save chart</button></footer></form>`;
    modal.showModal();
    const form = modal.querySelector("#chart-form");
    const columns = async () => {
      const db = await api("databases/" + form.database.value);
      form.column.innerHTML = db.columns
        .filter((c) =>
          ["select", "checkbox", "text", "number", "date"].includes(c.kind),
        )
        .map(
          (c) =>
            `<option value="${esc(c.id)}" ${c.id === o.column ? "selected" : ""}>${esc(c.name)}</option>`,
        )
        .join("");
    };
    columns().catch(app.error);
    form.database.addEventListener("change", () => columns().catch(app.error));
    modal.querySelector("#chart-cancel").onclick = () => modal.close();
    form.onsubmit = (e) => {
      e.preventDefault();
      w.options = {
        title: form.title.value.trim(),
        database: form.database.value,
        column: form.column.value,
        kind: form.kind.value,
      };
      save();
      modal.close();
      draw();
    };
  }
  draw();
}

/** Counts as horizontal bars: one series, one color, values labeled. */
function bars(groups, esc) {
  const max = Math.max(...groups.map((g) => g[1]));
  const total = groups.reduce((n, g) => n + g[1], 0);
  return `<div class="chart-bars" role="table" aria-label="Records per value">${groups
    .map(
      ([label, n]) =>
        `<div class="chart-bar" role="row" title="${esc(label)}: ${n} (${Math.round((n / total) * 100)}%)"><span role="cell" class="chart-label">${esc(label)}</span><span class="chart-track"><span class="chart-fill" style="width:${Math.max(2, (n / max) * 100)}%"></span></span><span role="cell" class="chart-value">${n}</span></div>`,
    )
    .join("")}</div>`;
}
/** Shares of a whole: up to seven slices in fixed color order, the rest folded
 * into "Other"; a legend with counts and percentages names every slice. */
function pie(groups, esc) {
  let slices = groups;
  if (groups.length > 7) {
    const rest = groups.slice(6).reduce((n, g) => n + g[1], 0);
    slices = [...groups.slice(0, 6), ["Other", rest]];
  }
  const total = slices.reduce((n, g) => n + g[1], 0);
  let angle = -Math.PI / 2;
  const arcs = slices
    .map(([label, n], i) => {
      const sweep = (n / total) * Math.PI * 2;
      const start = angle;
      angle += sweep;
      const title = `${esc(label)}: ${n} (${Math.round((n / total) * 100)}%)`;
      if (slices.length === 1)
        return `<circle cx="50" cy="50" r="46" class="slice s${i + 1}"><title>${title}</title></circle>`;
      const point = (a) => `${50 + 46 * Math.cos(a)} ${50 + 46 * Math.sin(a)}`;
      return `<path class="slice s${i + 1}" d="M50 50 L${point(start)} A46 46 0 ${sweep > Math.PI ? 1 : 0} 1 ${point(angle)} Z"><title>${title}</title></path>`;
    })
    .join("");
  return `<div class="chart-pie"><svg viewBox="0 0 100 100" role="img" aria-label="Pie chart">${arcs}</svg><ul class="chart-legend">${slices
    .map(
      ([label, n], i) =>
        `<li><span class="swatch s${i + 1}"></span><span>${esc(label)}</span><strong>${n}</strong><small>${Math.round((n / total) * 100)}%</small></li>`,
    )
    .join("")}</ul></div>`;
}
