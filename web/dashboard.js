// The overview page as a dashboard of widgets the user arranges: reorder by
// dragging (or with the arrow buttons), resize, configure, add, and remove.
// The layout is saved per account on the server.
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
const fresh = (w) => ({
  id: crypto.randomUUID().slice(0, 8),
  options: {},
  ...w,
});

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
  widgets = (widgets || DEFAULT).map(fresh);
  let editing = false,
    dragged = null;
  const save = () =>
    api("account/dashboard", "PUT", { widgets }).catch(app.error);

  function draw() {
    if (!root.isConnected) return;
    root.innerHTML = `<div class="dashboard-bar">${
      editing
        ? `<button class="btn primary" id="dash-add">${icon("plus")}Add widget</button><button class="btn" id="dash-reset">Restore default layout</button><span class="dash-hint">Drag widgets to reorder them.</span><button class="btn primary" id="dash-done">Done</button>`
        : `<button class="btn" id="dash-edit">${icon("grid")}Customize</button>`
    }</div><div class="dashboard ${editing ? "editing" : ""}">${
      widgets.map(frame).join("") ||
      '<div class="empty">Your dashboard is empty. Choose Customize to add widgets.</div>'
    }</div>`;
    bind();
    widgets.forEach((w) => fill(w).catch(() => {}));
  }
  function frame(w, i) {
    const spec = WIDGETS[w.type];
    const title = w.type === "chart" ? w.options.title || spec.name : spec.name;
    const tools = editing
      ? `<div class="widget-tools"><button data-move="${i}:-1" ${i === 0 ? "disabled" : ""} aria-label="Move ${esc(title)} earlier" title="Move earlier">←</button><button data-move="${i}:1" ${i === widgets.length - 1 ? "disabled" : ""} aria-label="Move ${esc(title)} later" title="Move later">→</button><select data-size="${i}" aria-label="Size of ${esc(title)}">${SIZES.map(([v, label]) => `<option value="${v}" ${w.size === v ? "selected" : ""}>${label}</option>`).join("")}</select>${w.type === "chart" ? `<button data-configure="${i}" aria-label="Chart settings" title="Chart settings">⚙</button>` : ""}<button data-remove="${i}" aria-label="Remove ${esc(title)}" title="Remove">×</button></div>`
      : "";
    // The welcome block can be dismissed for good, without customizing.
    const dismiss =
      !editing && w.type === "welcome"
        ? `<button class="widget-dismiss" data-remove="${i}" aria-label="Hide this welcome for good" title="Hide for good">×</button>`
        : "";
    return `<section class="widget size-${w.size} widget-${w.type}" data-widget="${w.id}" ${editing ? 'draggable="true"' : ""} aria-label="${esc(title)}">${w.type === "welcome" ? "" : `<header class="widget-head"><h2>${esc(title)}</h2>${tools}</header>`}${w.type === "welcome" ? tools : ""}${dismiss}<div class="widget-body" id="widget-${w.id}"><p class="widget-loading">Loading…</p></div></section>`;
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
      widgets = DEFAULT.map(fresh);
      save();
      draw();
    });
    on("[data-move]", (b) => {
      const [i, d] = b.dataset.move.split(":").map(Number);
      [widgets[i], widgets[i + d]] = [widgets[i + d], widgets[i]];
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
      widgets.splice(Number(b.dataset.remove), 1);
      save();
      draw();
      app.toast("Widget removed · add it back with Customize → Add widget");
    });
    on("[data-configure]", (b) =>
      chartDialog(widgets[Number(b.dataset.configure)]),
    );
    // Drag and drop to reorder, while customizing.
    on(
      "[data-widget]",
      (el, e) => (
        (dragged = el.dataset.widget),
        e.dataTransfer.setData("text/plain", dragged)
      ),
      "dragstart",
    );
    on(
      "[data-widget]",
      (el, e) => {
        if (!dragged) return;
        e.preventDefault();
        el.classList.add("drop-target");
      },
      "dragover",
    );
    on(
      "[data-widget]",
      (el) => el.classList.remove("drop-target"),
      "dragleave",
    );
    on(
      "[data-widget]",
      (el, e) => {
        e.preventDefault();
        const from = widgets.findIndex((w) => w.id === dragged);
        const to = widgets.findIndex((w) => w.id === el.dataset.widget);
        dragged = null;
        if (from < 0 || to < 0 || from === to) return draw();
        const [moved] = widgets.splice(from, 1);
        widgets.splice(to, 0, moved);
        save();
        draw();
      },
      "drop",
    );
    on("[data-widget]", () => (dragged = null), "dragend");
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
