import "./pane-auth.js";
import { renderServerOverview } from "./server-overview.js";
import { renderFlashcards } from "./flashcards.js";
import { pluginDialog } from "./plugin-commands.js";
import { createYouTubeEmbeds } from "./youtube.js";
import {
  createNoteTabs,
  isPane,
  initialNote,
  initialChat,
  paneMessage,
} from "./note-tabs.js";
import { renderAIChat, renderAISettings, stopAI } from "./ai.js";
import { renderDashboard } from "./dashboard.js";
import { splitMarkdown, blockCommands } from "./markdown.js";
import { Collaboration } from "./collaboration.js";
import {
  toggleWrap,
  linkSelection,
  noteLink,
  linkQuery,
  mentionQuery,
  findMatches,
  replaceAll,
  countWords,
  normalizeTag,
  htmlToMarkdown,
} from "./editor-tools.js";
("use strict");
const $ = (s, root = document) => root.querySelector(s);
const $$ = (s, root = document) => [...root.querySelectorAll(s)];
const esc = (s) =>
  String(s ?? "").replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ],
  );
const systemTheme = matchMedia("(prefers-color-scheme: dark)");
// Light and dark are built in; more themes come from plugins/themes on the server.
// A plugin theme extends a base: `data-theme` stays light or dark, so every rule
// for that mode still applies, and `data-variant` scopes the plugin's own rules.
const BUILTIN_THEMES = [
  ["system", "System theme"],
  ["light", "Light theme"],
  ["dark", "Dark theme"],
];
let pluginThemes = [],
  publicThemes = [],
  printTemplates = [];
function stored(key, fallback) {
  try {
    return localStorage.getItem(key) ?? fallback;
  } catch {
    return fallback;
  }
}
function store(key, value) {
  try {
    if (value == null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {}
}
const builtinTheme = (id) => BUILTIN_THEMES.some(([b]) => b === id);
function applyTheme() {
  const preference = stored("minerva.theme", "system");
  const root = document.documentElement;
  const plugin = !builtinTheme(preference);
  // Until the plugin list loads, the remembered base avoids flashing the wrong mode.
  const base = plugin
    ? pluginThemes.find((t) => t.id === preference)?.base ||
      stored("minerva.theme.base", "light")
    : preference === "system"
      ? systemTheme.matches
        ? "dark"
        : "light"
      : preference;
  root.dataset.theme = base;
  root.style.colorScheme = base;
  let link = document.getElementById("theme-plugin");
  if (plugin) {
    root.dataset.variant = preference;
    const href = `/plugins/themes/${encodeURIComponent(preference)}.css`;
    if (!link) {
      link = document.createElement("link");
      link.id = "theme-plugin";
      link.rel = "stylesheet";
      document.head.append(link);
    }
    if (link.getAttribute("href") !== href) link.setAttribute("href", href);
  } else {
    delete root.dataset.variant;
    link?.remove();
  }
}
function setTheme(id) {
  store("minerva.theme", id);
  store("minerva.theme.base", pluginThemes.find((t) => t.id === id)?.base);
  applyTheme();
}
function themeOptions() {
  const current = stored("minerva.theme", "system");
  return [
    ...BUILTIN_THEMES,
    ...pluginThemes.map((t) => [
      t.id,
      /theme$/i.test(t.name) ? t.name : `${t.name} theme`,
    ]),
  ]
    .map(
      ([value, label]) =>
        `<option value="${esc(value)}" ${value === current ? "selected" : ""}>${esc(label)}</option>`,
    )
    .join("");
}
function themeSelect() {
  return `<select id="theme" aria-label="Color theme">${themeOptions()}</select>`;
}
function bindThemeSelect() {
  action($("#theme"), "change", (e) => setTheme(e.target.value));
}
// Loads the installed plugin themes, then refreshes the theme picker.
async function loadPlugins() {
  try {
    const response = await fetch("/plugins/index.json");
    if (!response.ok) return;
    const index = await response.json();
    pluginThemes = index.themes || [];
    publicThemes = index.public_themes || [];
    printTemplates = index.pdf_prints || [];
  } catch {
    return;
  }
  const current = stored("minerva.theme", "system");
  // A theme that was uninstalled falls back to the system theme.
  if (!builtinTheme(current) && !pluginThemes.some((t) => t.id === current))
    setTheme("system");
  else setTheme(current);
  const select = $("#theme");
  if (select) select.innerHTML = themeOptions();
}
applyTheme();
systemTheme.addEventListener("change", applyTheme);
loadPlugins();
let live = null,
  needsBlockRefresh = false;
function stopLive() {
  live?.destroy();
  live = null;
}
const paths = {
  home: "m3 10 9-7 9 7v10a1 1 0 0 1-1 1h-5v-7H9v7H4a1 1 0 0 1-1-1z",
  search: "m21 21-5-5 M19 11a8 8 0 1 1-16 0 8 8 0 0 1 16 0",
  folder: "M3 7V5h6l2 2h10v13H3z",
  note: "M5 3h10l4 4v14H5z M14 3v5h5 M8 12h8 M8 16h6",
  globe:
    "M12 3a9 9 0 1 0 0 18a9 9 0 1 0 0-18z M3 12h18 M12 3c2.5 2.5 3.8 5.5 3.8 9s-1.3 6.5-3.8 9 M12 3c-2.5 2.5-3.8 5.5-3.8 9s1.3 6.5 3.8 9",
  link: "M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1 M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1",
  copy: "M9 9h11v11H9z M5 15H4V4h11v1",
  star: "m12 3 2.8 5.7 6.2.9-4.5 4.4 1.1 6.2-5.6-3-5.6 3 1.1-6.2L3 9.6l6.2-.9z",
  trash: "M3 6h18 M9 6V3h6v3 M5 6l1 15h12l1-15 M10 10v7 M14 10v7",
  users:
    "M16 21v-2a4 4 0 0 0-4-4H6a4 4 0 0 0-4 4v2 M20 21v-2a4 4 0 0 0-3-4 M13 7a4 4 0 1 1-8 0 4 4 0 0 1 8 0 M17 3a4 4 0 0 1 0 8",
  plus: "M12 5v14 M5 12h14",
  grid: "M3 3h7v7H3z M14 3h7v7h-7z M3 14h7v7H3z M14 14h7v7h-7z",
  list: "M8 6h13 M8 12h13 M8 18h13 M3 6h.01 M3 12h.01 M3 18h.01",
  arrow: "M5 12h14 M13 6l6 6-6 6",
  logout: "M9 4H4v16h5 M10 12h11 M17 8l4 4-4 4",
  download: "M12 3v12 M7 10l5 5 5-5 M4 15v6h16v-6",
  upload: "M12 16V4 M7 9l5-5 5 5 M4 15v6h16v-6",
  clock: "M12 8v5l3 2 M22 12a10 10 0 1 1-20 0 10 10 0 0 1 20 0",
  menu: "M4 6h16 M4 12h16 M4 18h16",
  canvas: "M3 3h7v7H3z M14 5h7v10h-7z M5 14h6v7H5z",
  check: "m5 12 4 4L19 6",
  calendar: "M4 6h16v14H4z M4 10h16 M8 3v5 M16 3v5",
  template: "M5 4h14v5H5z M5 13h6v7H5z M14 13h5 M14 17h5",
  chevron: "m7 10 5 5 5-5",
  more: "M5 12h.5 M11.75 12h.5 M18.5 12h.5",
  folderOpen: "M3 19V5h6l2 2h8v3 M3 19l3-9h16l-3 9z",
  edit: "M4 20h4L19 9l-4-4L4 16z M14 6l4 4",
  // Brain, after Lucide (ISC license, see THIRD_PARTY_NOTICES.md).
  brain:
    "M12 5a3 3 0 1 0-5.997.125 4 4 0 0 0-2.526 5.77 4 4 0 0 0 .556 6.588A4 4 0 1 0 12 18Z M12 5a3 3 0 1 1 5.997.125 4 4 0 0 1 2.526 5.77 4 4 0 0 1-.556 6.588A4 4 0 1 1 12 18Z M15 13a4.5 4.5 0 0 1-3-4 4.5 4.5 0 0 1-3 4 M17.599 6.5a3 3 0 0 0 .399-1.375 M6.003 5.125A3 3 0 0 0 6.401 6.5 M3.477 10.896a4 4 0 0 1 .585-.396 M19.938 10.5a4 4 0 0 1 .585.396 M6 18a4 4 0 0 1-1.967-.516 M19.967 17.484A4 4 0 0 1 18 18",
};
const icon = (n) =>
  `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${paths[n] || paths.note}"/></svg>`;
let data,
  workspaceId,
  view = "home",
  folderId = null,
  layout = localStorage.getItem("minerva.layout") || "grid",
  note = null,
  mailEnabled = false,
  mode = "document",
  dirty = false,
  conflict = false,
  saveChain = Promise.resolve(),
  closing = Promise.resolve(),
  toastTimer;
const modal = $("#modal");
// A tab that shows only an AI chat, usually beside the note it is about.
const chatPane =
  isPane && /^(note|folder):[a-f0-9-]{36}$/i.test(initialChat || "");
let aiScope = null,
  tagFilter = null,
  // Share-link guests use a reduced editor backed by /api/v1/shared.
  shareMode = false;
const noteTabs =
  !isPane && location.pathname !== "/share"
    ? createNoteTabs({
        esc,
        error,
        onBrowse: () => navigate("all"),
        onChange: () => changedElsewhere(),
      })
    : null;
const videos = createYouTubeEmbeds({
  getNote: () => note,
  edit: (id) => {
    if (editable()) editBlock(id);
  },
  error,
});
if (isPane) document.documentElement.classList.add("editor-pane");
/**
 * The desktop app serves pages from its own address (`minerva://`), where
 * WebKitGTK's media player cannot load files. There, this server's audio is
 * downloaded by the page and played from memory.
 */
if (!/^https?:$/.test(location.protocol)) {
  const loaded = new Map();
  const play = (audio) => {
    const src = audio.getAttribute("src");
    if (!src?.startsWith("/") || audio.dataset.local) return;
    audio.dataset.local = "1";
    if (!loaded.has(src))
      loaded.set(
        src,
        fetch(src, { credentials: "same-origin" })
          .then((r) => (r.ok ? r.blob() : Promise.reject(new Error(r.status))))
          .then((blob) => URL.createObjectURL(blob)),
      );
    loaded
      .get(src)
      .then((url) => (audio.src = url))
      .catch(() => loaded.delete(src));
  };
  new MutationObserver((changes) => {
    for (const change of changes)
      for (const node of change.addedNodes)
        if (node.nodeType === 1)
          (node.matches("audio[src]")
            ? [node]
            : node.querySelectorAll("audio[src]")
          ).forEach(play);
  }).observe(document.documentElement, { childList: true, subtree: true });
}
let paneShape = null,
  paneChanged = false;
function paneState() {
  // Live saves do not show what changed: compare what the sidebar shows, and
  // tell the window once the change is saved, so it reloads the new version.
  if (note) {
    const shape = JSON.stringify([
      note.id,
      note.title,
      note.icon,
      note.favorite,
      note.trashed,
      note.tags,
    ]);
    if (
      paneShape &&
      paneShape !== shape &&
      JSON.parse(paneShape)[0] === note.id
    )
      paneChanged = true;
    paneShape = shape;
  }
  if (paneChanged && !dirty) {
    paneChanged = false;
    paneMessage("changed");
  }
  paneMessage("state", {
    noteId: note?.id,
    title: chatPane ? `AI · ${chatTitle()}` : note?.title || "Browse notes",
    workspace: currentWorkspace()?.name || "",
    dirty: dirty || dbDrafts.size > 0 || dbSaves.size > 0,
  });
}
if (isPane) {
  const hostOrigin = new URLSearchParams(location.search).get("host");
  addEventListener("message", async (e) => {
    if (
      e.source !== parent ||
      e.origin !== hostOrigin ||
      e.data?.channel !== "minerva-host"
    )
      return;
    if (e.data.type === "visibility") videos.setVisible(!!e.data.visible);
    if (e.data.type === "insert-blocks") {
      // The note may still be loading when the tab was just opened for this.
      for (let i = 0; i < 100 && !note; i++)
        await new Promise((r) => setTimeout(r, 100));
      try {
        const count = await appendBlocks(
          (e.data.blocks || []).map(String).slice(0, 500),
        );
        paneMessage("inserted", { request: e.data.request, count });
      } catch (err) {
        paneMessage("inserted", {
          request: e.data.request,
          error: err.message,
        });
      }
    }
    if (e.data.type === "insert-result") {
      const r = insertRequests.get(e.data.request);
      insertRequests.delete(e.data.request);
      if (e.data.error) r?.reject(new Error(e.data.error));
      else r?.resolve(e.data.count);
    }
    if (e.data.type === "flush") {
      try {
        await flush();
        paneMessage("flushed", { request: e.data.request });
      } catch (err) {
        paneMessage("flushed", { request: e.data.request, error: err.message });
      }
    }
  });
  paneMessage("ready");
}
// Rendered HTML by Markdown source, so undo and merges can redraw without a round trip.
const renderedCache = new Map();
let lastFocus = null;
const showModal = modal.showModal.bind(modal);
// Dialogs are labelled by their heading and return focus to where they opened.
modal.showModal = () => {
  if (!modal.open) lastFocus = document.activeElement;
  const heading = $("h2", modal);
  if (heading) {
    heading.id ||= "modal-title";
    modal.setAttribute("aria-labelledby", heading.id);
  }
  showModal();
};
modal.addEventListener("close", () => {
  if (lastFocus?.isConnected) lastFocus.focus();
  lastFocus = null;
});
function announce(message) {
  const region = $("#announcer");
  if (!region) return;
  region.textContent = "";
  setTimeout(() => (region.textContent = message), 30);
}
async function renderMarkdown(markdown) {
  if (renderedCache.has(markdown)) return renderedCache.get(markdown);
  const { html } = await api(shareMode ? "shared/render" : "render", "POST", {
    markdown,
  });
  if (renderedCache.size > 2000) renderedCache.clear();
  renderedCache.set(markdown, html);
  return html;
}
function rememberRendered() {
  note?.blocks.forEach(
    (b, i) =>
      note.rendered?.[i] && renderedCache.set(b.markdown, note.rendered[i]),
  );
}
function serverOrigin() {
  return (
    document.querySelector('meta[name="minerva-server-origin"]')?.content ||
    location.origin
  );
}
function bytes(n) {
  if (n < 1024) return n + " B";
  const units = ["KiB", "MiB", "GiB"];
  let i = -1;
  do {
    n /= 1024;
    i++;
  } while (n >= 1024 && i < units.length - 1);
  return n.toFixed(n < 10 ? 1 : 0) + " " + units[i];
}
function slugify(title) {
  return (
    title
      .normalize("NFKD")
      .replace(/[\u0300-\u036f]/g, "")
      .toLowerCase()
      .replace(/[^a-z0-9]+/g, "-")
      .replace(/^-+|-+$/g, "")
      .slice(0, 80)
      .replace(/-+$/, "") || "note"
  );
}
/** The browser's local calendar date as YYYY-MM-DD. */
function localDate(date = new Date()) {
  const pad = (n) => String(n).padStart(2, "0");
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}`;
}
function parseDate(iso) {
  const [y, m, d] = iso.split("-").map(Number);
  return new Date(y, m - 1, d);
}
function shiftDate(iso, days) {
  const date = parseDate(iso);
  date.setDate(date.getDate() + days);
  return localDate(date);
}
function longDate(iso, options = {}) {
  return parseDate(iso).toLocaleDateString(undefined, {
    weekday: "long",
    year: "numeric",
    month: "long",
    day: "numeric",
    ...options,
  });
}
function workspaceOfNote(n) {
  return data.folders.find((f) => f.id === n?.folder_id)?.workspace_id;
}
function journalNotes(ws = workspaceId) {
  const folder = data.journal_folders?.[ws];
  return data.notes.filter(
    (n) => n.journal && n.folder_id === folder && !n.trashed,
  );
}
function templateNotes(ws = workspaceId) {
  const folder = data.template_folders?.[ws];
  return data.notes.filter(
    (n) => folder && n.folder_id === folder && !n.trashed,
  );
}
function isTemplate(n) {
  const ws = workspaceOfNote(n);
  return Boolean(ws && data.template_folders?.[ws] === n.folder_id);
}
function isEmptyNote(n) {
  return n.blocks.every((b) => !b.markdown.trim());
}
/** Opens the journal entry for a date, creating it (and the Journal folder) if needed. */
async function openJournal(date, ws = workspaceId) {
  await flush();
  const entry = await api("journal", "POST", { workspace_id: ws, date });
  await refresh();
  await openNote(entry.id);
  if (entry.created)
    announce(`Created the journal entry for ${longDate(date)}`);
}
function noteTags(n) {
  return [...new Set([...(n.tags || []), ...(n.inline_tags || [])])];
}
/** Tag counts across the current workspace and notes shared with you. */
function allTags() {
  const counts = new Map();
  for (const n of [...workspaceNotes(), ...(data.shared_notes || [])])
    if (!n.trashed)
      for (const tag of noteTags(n))
        counts.set(tag, (counts.get(tag) || 0) + 1);
  return [...counts].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]));
}
async function openTag(tag) {
  await flush();
  tagFilter = tag;
  await navigate("tag");
}
let mermaidLoader = null;
function loadMermaid() {
  mermaidLoader ??= new Promise((resolve, reject) => {
    const script = document.createElement("script");
    script.src = "/mermaid.js";
    script.onload = () => resolve(window.minervaMermaid);
    script.onerror = () => reject(new Error("Diagrams could not be loaded"));
    document.head.append(script);
  }).then((mermaid) => {
    mermaid.initialize({
      startOnLoad: false,
      securityLevel: "strict",
      // ELK is not bundled; Mermaid 12 would otherwise choose it by default.
      layout: "dagre",
      theme:
        document.documentElement.dataset.theme === "dark" ? "dark" : "default",
    });
    return mermaid;
  });
  return mermaidLoader;
}
/** Replaces rendered ```mermaid code blocks with diagrams (loaded on demand). */
async function renderDiagrams() {
  const codes = $$("pre > code.language-mermaid").filter(
    (code) => !code.closest(".mermaid-error"),
  );
  if (!codes.length) return;
  const mermaid = await loadMermaid();
  for (const code of codes) {
    const pre = code.parentElement;
    if (!pre.isConnected) continue;
    const id = "diagram-" + crypto.randomUUID();
    try {
      const { svg } = await mermaid.render(id, code.textContent);
      const figure = document.createElement("div");
      figure.className = "mermaid-diagram";
      figure.setAttribute("role", "img");
      figure.setAttribute("aria-label", "Diagram");
      figure.innerHTML = svg;
      if (pre.isConnected) pre.replaceWith(figure);
    } catch (e) {
      // Mermaid leaves its error graphic in the page; keep the source instead.
      document.getElementById("d" + id)?.remove();
      document.getElementById(id)?.remove();
      pre.classList.add("mermaid-error");
      pre.title = "Diagram error: " + (e.message || e);
    }
  }
}
let diagramTimer;
new MutationObserver(() => {
  clearTimeout(diagramTimer);
  diagramTimer = setTimeout(() => {
    videos.reconcile();
    renderDiagrams().catch(error);
    renderJournalTasks().catch(error);
    renderDatabaseEmbeds().catch(error);
  }, 60);
}).observe(document.body, { childList: true, subtree: true });
// Links between notes and inline tags inside rendered blocks.
document.addEventListener("click", (e) => {
  const link = e.target.closest?.('a[href^="#note/"]');
  if (link && !shareMode) {
    e.preventDefault();
    openNote(link.getAttribute("href").slice(6)).catch(() =>
      toast("That note is not available to you."),
    );
    return;
  }
  const tag = e.target.closest?.(".block-body .tag, .tag-chip[data-tag]");
  if (tag && !shareMode && !e.target.closest(".tag-remove")) {
    e.preventDefault();
    openTag(tag.dataset.tag || tag.textContent.replace(/^#/, "").toLowerCase());
  }
});
const roleLabel = (role) => (role === "editor" ? "Can edit" : "Can view");
function sharingBadges(sharing) {
  if (!sharing) return "";
  return [
    sharing.published ? '<span class="share-badge public">Public</span>' : "",
    sharing.links ? '<span class="share-badge">Link</span>' : "",
    sharing.members ? '<span class="share-badge">Shared</span>' : "",
  ].join("");
}
async function api(path, method = "GET", body) {
  const r = await fetch("/api/v1/" + path, {
    method,
    headers: { "Content-Type": "application/json", "X-Minerva-Client": "1" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const v = await r.json();
  if (!r.ok) {
    const e = new Error(v.error || "Request failed");
    e.status = r.status;
    throw e;
  }
  // A note tab tells the window around it, which redraws its sidebar.
  if (isPane && changesSidebar(path, method, body, v)) paneMessage("changed");
  return v;
}
function toast(s) {
  $("#toast").textContent = s;
  $("#toast").classList.add("visible");
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => $("#toast").classList.remove("visible"), 4500);
}
function error(e) {
  toast(e.message || String(e));
}
function action(el, event, fn) {
  el?.addEventListener(event, async (e) => {
    try {
      if (
        el.closest(".database-page") &&
        el.id !== "db-reload" &&
        ["click", "change", "input"].includes(event)
      )
        await flush();
      await fn(e);
    } catch (err) {
      error(err);
    }
  });
}
function art() {
  return '<div class="banner-art" aria-hidden="true"><div class="art-sheet"><i></i><i></i><i></i></div><div class="art-sticky">↗</div><div class="art-star">✧</div></div>';
}
function date(t) {
  return new Date(t * 1000).toLocaleDateString(undefined, {
    month: "short",
    day: "numeric",
  });
}
function currentWorkspace() {
  return data.workspaces.find((w) => w.id === workspaceId);
}
function folders() {
  return data.folders.filter((f) => f.workspace_id === workspaceId);
}
function workspaceNotes() {
  const ids = new Set(folders().map((f) => f.id));
  return data.notes.filter((n) => ids.has(n.folder_id));
}
function editable(n = note) {
  return n && !n.trashed && (n.role || data.roles[n.folder_id]) === "editor";
}
function owner() {
  return currentWorkspace()?.owner_id === data.user.id;
}
/** The owner, or a member who can edit the whole workspace: adds folders. */
function editsWorkspace() {
  return owner() || !!currentWorkspace()?.can_edit;
}
async function refresh() {
  data = await api("bootstrap");
  // The account's default workspace first, then any other.
  if (!data.workspaces.some((w) => w.id === workspaceId))
    workspaceId = data.workspaces.some((w) => w.id === data.default_workspace)
      ? data.default_workspace
      : data.workspaces[0]?.id;
  if (noteTabs) {
    // Tabs of deleted or trashed notes close.
    noteTabs.prune(
      new Set(
        [...data.notes, ...(data.shared_notes || [])]
          .filter((n) => !n.trashed)
          .map((n) => n.id),
      ),
    );
    redrawSidebar();
  }
}
/** Redraws the sidebar's folders, favorites and tags from `data`, in place. */
function redrawSidebar() {
  if (data?.user.admin && !localMode) return;
  const scroll = $(".sidebar-scroll"),
    nav = $(".sidebar-scroll > .nav");
  if (!scroll || !nav) return;
  const top = scroll.scrollTop;
  while (nav.nextSibling) nav.nextSibling.remove();
  nav.insertAdjacentHTML("afterend", sidebarSections());
  // Menu items in the redrawn sections (Sharing, Trash, ...) need their
  // handlers again; the main menu above them was not replaced.
  $$("[data-view]", scroll)
    .filter((b) => !nav.contains(b))
    .forEach((b) => action(b, "click", () => navigate(b.dataset.view)));
  $$("[data-folder]", scroll).forEach((b) =>
    action(b, "click", () => navigate("folder", b.dataset.folder)),
  );
  $$("[data-open-tag]", scroll).forEach((b) =>
    action(b, "click", () => openTag(b.dataset.openTag)),
  );
  bindSidebar();
  scroll.scrollTop = top;
}
let changeTimer;
/** A note tab changed notes or folders: catch up shortly after. */
function changedElsewhere() {
  clearTimeout(changeTimer);
  changeTimer = setTimeout(() => refresh().catch(() => {}), 400);
}
// What a note looked like in the sidebar when last saved from this tab.
const sidebarShape = new Map();
/**
 * Writes that change what the sidebar shows: creating, moving, renaming or
 * deleting notes, folders, workspaces and databases. Ordinary note saves only
 * count when their title, icon, favorite, tags, folder or trash state change.
 */
function changesSidebar(path, method, body, result) {
  const [kind, id, sub] = path.split("/");
  const shape = (n) =>
    JSON.stringify([
      n?.title,
      n?.icon,
      n?.favorite,
      n?.trashed,
      n?.tags,
      n?.folder_id,
    ]);
  // A note just loaded: later saves are compared with it.
  if (method === "GET") {
    if (kind === "notes" && id && !sub) sidebarShape.set(id, shape(result));
    return false;
  }
  if (["journal", "templates"].includes(kind)) return method === "POST";
  if (!["notes", "folders", "workspaces", "databases"].includes(kind))
    return false;
  if (sub && !(kind === "notes" && sub === "template")) return false;
  if (kind === "notes" && method === "PUT" && id) {
    const before = sidebarShape.get(id),
      after = shape({ folder_id: before && JSON.parse(before)[5], ...body });
    sidebarShape.set(id, after);
    return before !== after;
  }
  return true;
}
/**
 * First sign-in of an account an administrator created: choose your own
 * password and, if you have none, create your personal workspace, which then
 * opens by default. It cannot be skipped, only left by signing out.
 */
function firstSignIn() {
  if (!data?.must_change_password || localMode) return;
  const workspace = false;
  modal.innerHTML = `<form id="first-sign-in"><h2>Welcome to Minerva, ${esc(data.user.name)}</h2><p>Before you start, replace the temporary password your administrator gave you${workspace ? ", and name your personal workspace: a private place for your own notes" : ""}.</p><label>Temporary password<input name="current_password" type="password" autocomplete="current-password" required></label><label>New password<input name="new_password" type="password" autocomplete="new-password" minlength="12" maxlength="256" required></label><label>Repeat new password<input name="repeat" type="password" autocomplete="new-password" required></label>${workspace ? '<label>Personal workspace<input name="workspace_name" value="Personal" maxlength="200" required></label>' : ""}<div class="form-error" role="alert"></div><footer><button type="button" class="btn" id="first-sign-out">Sign out</button><button class="btn primary" type="submit">Get started</button></footer></form>`;
  const keep = (e) => e.preventDefault();
  modal.addEventListener("cancel", keep);
  modal.showModal();
  $("#first-sign-out").onclick = async () => {
    modal.removeEventListener("cancel", keep);
    modal.close();
    await api("logout", "POST").catch(() => {});
    login();
  };
  $("#first-sign-in").onsubmit = async (e) => {
    e.preventDefault();
    const form = e.target;
    const button = $("[type=submit]", form);
    button.disabled = true;
    try {
      if (form.new_password.value !== form.repeat.value)
        throw new Error("The new passwords do not match.");
      const result = await api("account/password", "POST", {
        current_password: form.current_password.value,
        new_password: form.new_password.value,
        workspace_name: form.workspace_name?.value || "",
      });
      modal.removeEventListener("cancel", keep);
      modal.close();
      if (result.workspace_id) workspaceId = result.workspace_id;
      await refresh();
      shell();
      toast(
        workspace
          ? "You’re all set · this is your personal workspace"
          : "Password changed",
      );
    } catch (err) {
      $(".form-error", form).textContent = err.message;
      button.disabled = false;
    }
  };
}
// The desktop app's own server: personal notes plus offline copies of
// workspaces on other servers, kept in sync in the background.
let localMode = false,
  replicas = [];
function replicaFor(id = workspaceId) {
  return localMode ? replicas.find((r) => r.workspace_id === id) : undefined;
}
function syncLabel(r) {
  const state = r.status?.state;
  const waiting = r.pending
    ? ` · ${r.pending} change${r.pending === 1 ? "" : "s"} waiting`
    : "";
  if (r.conflicts) return `${r.conflicts} to review${waiting}`;
  if (r.revoked || state === "revoked") return "Access removed · read only";
  if (state === "offline") return `Offline${waiting}`;
  if (state === "signed_out") return `Sign in to sync${waiting}`;
  if (state === "synced") return waiting ? `Syncing${waiting}` : "Synced";
  if (state) return `Sync paused${waiting}`;
  return "Waiting for first sync";
}
function workspaceBadge() {
  const r = replicaFor();
  if (r)
    return `<button class="badge sync-badge ${r.conflicts ? "attention" : ""}" id="sync-state" title="Offline copy of a workspace on another server">${esc(syncLabel(r))}</button>`;
  if (localMode) return '<span class="badge">ON THIS COMPUTER</span>';
  const w = currentWorkspace();
  if (!w) return "";
  // My Workspace stays private; other workspaces open their members here.
  if (w.personal)
    return '<span class="badge" title="Only you can see My Workspace">PRIVATE WORKSPACE</span>';
  return `<button class="badge" id="share-workspace" title="Share this workspace">${w.shared ? "SHARED WORKSPACE" : "PRIVATE WORKSPACE"} · Share</button>`;
}
async function loadReplicas() {
  if (!localMode) return;
  const before = JSON.stringify(
    replicas.map((r) => [r.workspace_id, r.access]),
  );
  replicas = await api("local/replicas").catch(() => replicas);
  const badge = $("#sync-state");
  if (badge && replicaFor()) {
    badge.textContent = syncLabel(replicaFor());
    badge.classList.toggle("attention", !!replicaFor().conflicts);
  }
  // A newly copied workspace appears without a reload.
  const after = JSON.stringify(replicas.map((r) => [r.workspace_id, r.access]));
  if (before !== after && data && !note) {
    await refresh();
    shell();
  }
}
/** Sync status of offline copies, and offline edits that need a decision. */
async function syncDialog() {
  const [list, conflicts] = await Promise.all([
    api("local/replicas"),
    api("local/conflicts"),
  ]);
  const name = (c) => {
    const find = (items) => items?.find((x) => x.id === c.entity_id);
    if (c.kind === "note")
      return `Note “${find(data.notes)?.title || "unavailable"}”`;
    if (c.kind === "folder")
      return `Folder “${find(data.folders)?.name || "unavailable"}”`;
    if (c.kind === "database")
      return `Database “${find(data.databases)?.name || "unavailable"}”`;
    if (c.kind === "workspace")
      return `Workspace “${find(data.workspaces)?.name || "unavailable"}”`;
    return "A database record";
  };
  const explain = (c) =>
    c.copy_id
      ? "It was also changed on the server. The server's values are kept, and yours are saved in an archived copy of the record."
      : c.kept
        ? "You deleted it while offline, but it changed on the server, so it was restored."
        : c.status === "merged"
          ? `Your change to ${c.lost.map((f) => f.replace("cells/", "a property")).join(", ")} was replaced by a newer one from the server.`
          : `${c.incoming ? "A change from the server could not be applied here" : "The server refused this change"}: ${c.error || c.status}.`;
  const actions = (c) =>
    c.copy_id
      ? `<button class="btn" data-resolve="${c.id}" data-choice="theirs">Keep theirs</button><button class="btn" data-resolve="${c.id}" data-choice="both">Keep both</button><button class="btn primary" data-resolve="${c.id}" data-choice="mine">Use mine</button>`
      : `<button class="btn" data-resolve="${c.id}" data-choice="ok">OK</button>`;
  modal.innerHTML = `<form method="dialog" class="sync-dialog"><h2>Offline copies</h2><p>Workspaces from other servers kept on this computer. Changes made offline are sent when the server is reachable. Choose which workspaces to keep offline in Servers (<kbd>Ctrl</kbd>+<kbd>Shift</kbd>+<kbd>O</kbd>).</p><ul class="sync-list">${list.map((r) => `<li><strong>${esc(r.name)}</strong><span>${esc(syncLabel(r))}${r.status?.error && r.status.state !== "synced" ? ` — ${esc(r.status.error)}` : ""}</span></li>`).join("") || "<li>No offline copies yet.</li>"}</ul>${conflicts.length ? `<h3>To review</h3><ul class="sync-list conflicts">${conflicts.map((c) => `<li><div><strong>${esc(name(c))}</strong><span>${esc(explain(c))}</span></div><div class="share-actions">${actions(c)}</div></li>`).join("")}</ul>` : ""}<footer><button class="btn">Close</button></footer></form>`;
  modal.showModal();
  $$("[data-resolve]", modal).forEach((b) =>
    action(b, "click", async (e) => {
      e.preventDefault();
      const c = conflicts.find((x) => x.id === b.dataset.resolve);
      if (c.copy_id) await resolveRecord(c, b.dataset.choice);
      await api("local/conflicts/" + c.id, "DELETE");
      await loadReplicas();
      await syncDialog();
    }),
  );
}
/** Settles a record conflict using its archived copy of the offline values. */
async function resolveRecord(c, choice) {
  const all = await Promise.all(
    data.databases.map((d) => api("databases/" + d.id)),
  );
  const db = all.find((d) => d.rows.some((r) => r.id === c.entity_id));
  const copy = db?.rows.find((r) => r.id === c.copy_id);
  const row = db?.rows.find((r) => r.id === c.entity_id);
  if (!copy || !row) return;
  if (choice === "mine") {
    const cells = Object.fromEntries(
      db.columns.map((col) => [col.id, copy.cells[col.id] ?? null]),
    );
    await api(`databases/${db.id}/rows/${row.id}`, "PATCH", {
      revision: row.revision,
      cells,
    });
  }
  if (choice === "both")
    await api(`databases/${db.id}/rows/${copy.id}`, "PATCH", {
      revision: copy.revision,
      archived: false,
    });
  else await api(`databases/${db.id}/rows/${copy.id}`, "DELETE");
}
function login() {
  stopAI();
  stopLive();
  note = null;
  data = null;
  $("#app").innerHTML =
    `<div class="login-shell"><div class="login-art"><div class="brand"><span class="brand-mark">✳</span>minerva</div><div class="eyebrow">A HOME FOR YOUR IDEAS</div><h1>Make room for<br>your next big thought.</h1><p>Notes, plans, and a little possibility.<br>A thoughtful workspace, on your own server.</p>${art()}</div><main class="login-main"><form class="login-form"><div class="eyebrow">YOUR SPACE, CONNECTED</div><h2>Welcome to Minerva</h2><p>Sign in to your server to pick up where you left off.</p><label>Email address<input name="email" type="email" autocomplete="username" placeholder="you@example.com" required></label><label>Password<input name="password" type="password" autocomplete="current-password" placeholder="Your password" required></label><div class="form-error" role="alert"></div><button class="btn primary" type="submit">Sign in ${icon("arrow")}</button>${mailEnabled ? '<p class="forgot"><button type="button" class="text-link" id="forgot-password">Forgot your password?</button></p>' : ""}<p class="footnote">Privately hosted · Powered by Rust<br>Ask your server administrator for an account.</p></form></main></div>`;
  action($("#forgot-password"), "click", forgotPassword);
  $(".login-form").onsubmit = async (e) => {
    e.preventDefault();
    const btn = $("button", e.target);
    btn.disabled = true;
    try {
      const result = await api(
        "login",
        "POST",
        Object.fromEntries(new FormData(e.target)),
      );
      if (result.two_factor_required) return secondFactor(result.challenge);
      await refresh();
      shell();
      firstSignIn();
      if (initialNote) await openNote(initialNote);
    } catch (err) {
      $(".form-error").textContent = err.message;
    } finally {
      btn.disabled = false;
    }
  };
}
/** Asks for a password-reset link by email. */
function forgotPassword() {
  const form = $(".login-form");
  form.innerHTML = `<div class="eyebrow">FORGOT YOUR PASSWORD</div><h2>Reset your password</h2><p>Enter the email address of your account. We will send you a link to choose a new password.</p><label>Email address<input name="email" type="email" autocomplete="username" placeholder="you@example.com" required autofocus></label><div class="form-error" role="alert"></div><button class="btn primary" type="submit">Send the link ${icon("arrow")}</button><p class="footnote"><button type="button" class="text-link" id="back-to-login">Back to sign in</button></p>`;
  $("#back-to-login").onclick = login;
  form.onsubmit = async (e) => {
    e.preventDefault();
    const btn = $("button[type=submit]", form);
    btn.disabled = true;
    try {
      await api("password-reset", "POST", {
        email: form.email.value.trim(),
      });
      form.innerHTML = `<div class="eyebrow">CHECK YOUR EMAIL</div><h2>Look in your inbox</h2><p>If an account uses ${esc(form.email.value.trim())}, an email with a link to choose a new password is on its way. The link works for one hour.</p><p class="footnote"><button type="button" class="text-link" id="back-to-login">Back to sign in</button></p>`;
      $("#back-to-login").onclick = login;
    } catch (err) {
      $(".form-error", form).textContent = err.message;
      btn.disabled = false;
    }
  };
}
/** Opened from an emailed link (`/?reset=<token>`): choose a new password. */
function choosePassword(token) {
  login();
  const form = $(".login-form");
  form.innerHTML = `<div class="eyebrow">YOUR ACCOUNT</div><h2>Choose a password</h2><p>Use at least 12 characters. You then sign in with it.</p><label>New password<input name="password" type="password" autocomplete="new-password" minlength="12" required autofocus></label><label>Repeat it<input name="repeat" type="password" autocomplete="new-password" minlength="12" required></label><div class="form-error" role="alert"></div><button class="btn primary" type="submit">Save password ${icon("arrow")}</button>`;
  form.onsubmit = async (e) => {
    e.preventDefault();
    if (form.password.value !== form.repeat.value)
      return ($(".form-error", form).textContent =
        "The two passwords are different.");
    try {
      const result = await api("password-reset/complete", "POST", {
        token,
        password: form.password.value,
      });
      history.replaceState(null, "", "/");
      login();
      $(".login-form").email.value = result.email || "";
      $(".login-form p").textContent =
        "Your password is saved. Sign in with it.";
    } catch (err) {
      $(".form-error", form).textContent = err.message;
    }
  };
}
/** Second sign-in step: a code from an authenticator app or a recovery code. */
function secondFactor(challenge) {
  const form = $(".login-form");
  let recovery = false;
  const draw = () => {
    form.innerHTML = `<div class="eyebrow">TWO-STEP VERIFICATION</div><h2>Enter your code</h2><p>${recovery ? "Enter one of the recovery codes you saved when you set up two-factor authentication. Each code works once." : "Open your authenticator app and enter the 6-digit code for Minerva."}</p><label>${recovery ? "Recovery code" : "Authentication code"}<input name="code" ${recovery ? 'autocomplete="off" placeholder="xxxx-xxxx"' : 'inputmode="numeric" autocomplete="one-time-code" placeholder="123 456" maxlength="7"'} required autofocus></label><div class="form-error" role="alert"></div><button class="btn primary" type="submit">Verify ${icon("arrow")}</button><p class="footnote"><button type="button" class="text-link" id="toggle-recovery">${recovery ? "Use your authenticator app" : "Use a recovery code"}</button> · <button type="button" class="text-link" id="restart-login">Start over</button></p>`;
    $("input", form).focus();
    $("#toggle-recovery").onclick = () => {
      recovery = !recovery;
      draw();
    };
    $("#restart-login").onclick = login;
  };
  draw();
  form.onsubmit = async (e) => {
    e.preventDefault();
    const btn = $("button[type=submit]", form);
    btn.disabled = true;
    try {
      await api("login/2fa", "POST", { challenge, code: form.code.value });
      await refresh();
      shell();
      firstSignIn();
      if (initialNote) await openNote(initialNote);
    } catch (err) {
      $(".form-error", form).textContent = err.message;
      // An expired or exhausted challenge needs the password again.
      if (err.status === 401 && /password again/.test(err.message))
        setTimeout(login, 2500);
    } finally {
      btn.disabled = false;
    }
  };
}
function adminShell() {
  note = null;
  noteTabs?.hide();
  stopAI();
  videos.stop();
  stopLive();
  if (!["admin-overview", "admin", "providers", "account"].includes(view))
    view = "admin-overview";
  $("#app").innerHTML =
    `<div class="app-shell"><aside class="sidebar"><div class="brand"><span class="brand-mark">✳</span>minerva</div><p class="section-label">SERVER ADMINISTRATION</p><nav class="nav">${[
      ["admin-overview", "Server overview"],
      ["admin", "Users & storage"],
      ["providers", "AI provider settings"],
      ["account", "Account security"],
    ]
      .map(
        ([id, label]) =>
          `<button data-admin-view="${id}" class="${view === id ? "active" : ""}">${esc(label)}</button>`,
      )
      .join(
        "",
      )}</nav><div class="sidebar-bottom"><div class="profile-name">${esc(data.user.name)}</div><p class="subtitle">Management account</p><button class="btn" id="admin-logout">Sign out</button></div></aside><main class="main"><header class="topbar"><button class="mobile-menu" aria-label="Toggle sidebar">${icon("menu")}</button><strong id="breadcrumb">Server administration</strong>${themeSelect()}</header><div id="content" tabindex="-1"></div></main></div>`;
  $$("[data-admin-view]").forEach((b) =>
    action(b, "click", () => navigate(b.dataset.adminView)),
  );
  action($(".mobile-menu"), "click", () =>
    $(".sidebar").classList.toggle("open"),
  );
  action($("#admin-logout"), "click", async () => {
    await noteTabs?.clear();
    await api("logout", "POST");
    login();
  });
  bindThemeSelect();
  if (view === "admin") renderAdmin().catch(error);
  else if (view === "providers")
    renderAISettings({ api, esc, data, error }).catch(error);
  else if (view === "account") renderAccount().catch(error);
  else
    renderServerOverview({ api, esc, formDialog, refresh, error }).catch(error);
}
/**
 * Shares a workspace: invite members of this server, change each person's
 * access, or remove it. Changes apply at once; read-only members only see
 * who is in.
 */
async function shareWorkspace(wid = workspaceId) {
  const name = data.workspaces.find((w) => w.id === wid)?.name || "workspace";
  let info = await api(`workspaces/${wid}/members`);
  let busy = false;
  const avatar = (p) =>
    `<span class="share-avatar" aria-hidden="true">${esc(p.name.slice(0, 1).toUpperCase())}</span>`;
  const roleSelect = (attrs, role) =>
    `<select ${attrs}><option value="viewer" ${role === "viewer" ? "selected" : ""}>Read only</option><option value="editor" ${role === "editor" ? "selected" : ""}>Edit</option></select>`;
  const draw = () => {
    const inside = info.members.filter((m) => m.role);
    const outside = info.members.filter((m) => !m.role && !m.owner);
    const rows = inside
      .sort((a, b) => b.owner - a.owner || a.name.localeCompare(b.name))
      .map(
        (m) =>
          `<li class="share-person">${avatar(m)}<div class="share-person-name"><strong>${esc(m.name)}${m.id === data.user.id ? " <small>(you)</small>" : ""}</strong><small>${esc(m.email)}</small></div>${
            m.owner
              ? '<span class="pill">Owner</span>'
              : info.can_edit
                ? `${roleSelect(`data-member-role="${m.id}" aria-label="Access for ${esc(m.name)}"`, m.role)}<button type="button" class="share-remove" data-member-remove="${m.id}" aria-label="Remove ${esc(m.name)}" title="Remove access">×</button>`
                : `<span class="pill">${m.role === "editor" ? "Edit" : "Read only"}</span>`
          }</li>`,
      )
      .join("");
    modal.innerHTML = `<div class="share-workspace"><header><h2>Share ${esc(name)}</h2><p>${info.can_edit ? "People with <strong>Edit</strong> manage folders, notes, the journal, sites and sharing, like you. <strong>Read only</strong> lets them read everything." : "You can read this workspace. People who can edit it manage who has access."}</p></header>${
      info.can_edit
        ? outside.length
          ? `<form class="share-invite" id="share-invite"><select name="user_id" aria-label="Person to invite" required>${outside.map((m) => `<option value="${m.id}">${esc(m.name)} · ${esc(m.email)}</option>`).join("")}</select>${roleSelect('name="role" aria-label="Access"', "editor")}<button class="btn primary" ${busy ? "disabled" : ""}>Invite</button></form>`
          : '<p class="share-hint">Everyone on this server already has access. Ask your administrator to add accounts.</p>'
        : ""
    }<h3 class="share-subtitle">People with access <span>${inside.length}</span></h3><ul class="share-people">${rows}</ul><div class="form-error" role="alert"></div><footer><button type="button" class="btn primary" id="share-done">Done</button></footer></div>`;
    $("#share-done").onclick = () => modal.close();
    const save = async (user_id, role, message) => {
      busy = true;
      try {
        await api("workspace-access", "PUT", {
          workspace_id: wid,
          user_id,
          role,
        });
        info = await api(`workspaces/${wid}/members`);
        toast(message);
      } catch (err) {
        $(".form-error", modal).textContent = err.message;
      } finally {
        busy = false;
      }
      draw();
    };
    const form = $("#share-invite");
    if (form)
      form.onsubmit = (e) => {
        e.preventDefault();
        const who = info.members.find((m) => m.id === form.user_id.value);
        save(
          form.user_id.value,
          form.role.value,
          `${who?.name || "They"} can now open ${name}`,
        );
      };
    $$("[data-member-role]", modal).forEach(
      (select) =>
        (select.onchange = () =>
          save(select.dataset.memberRole, select.value, "Access updated")),
    );
    $$("[data-member-remove]", modal).forEach(
      (button) =>
        (button.onclick = () => {
          const who = info.members.find(
            (m) => m.id === button.dataset.memberRemove,
          );
          save(
            button.dataset.memberRemove,
            "none",
            `${who?.name || "They"} no longer has access`,
          );
        }),
    );
  };
  draw();
  modal.showModal();
  // Badges and the Sharing page reflect the new members afterwards.
  modal.addEventListener(
    "close",
    async () => {
      await refresh();
      if (view === "sharing") await renderSharing();
      else shell();
    },
    { once: true },
  );
}

function shell() {
  if (data.user.admin && !localMode) {
    adminShell();
    return;
  }
  document.documentElement.classList.toggle("pane-browsing", isPane && !note);
  stopAI();
  videos.stop();
  const w = currentWorkspace();
  $("#app").innerHTML =
    `<a class="skip-link" href="#content">Skip to content</a><div class="app-shell"><aside class="sidebar"><div class="brand"><span class="brand-mark">✳</span>minerva</div><div class="workspace-switch"><button id="workspace" class="workspace-button" aria-haspopup="menu" aria-expanded="false" aria-controls="workspace-menu" aria-label="Current workspace: ${esc(w?.name || "none")}. Switch workspace"><span class="square">${esc(w?.name.slice(0, 1) || "M")}</span><span class="workspace-name">${esc(w?.name || "No workspace")}</span>${icon("chevron")}</button><div class="workspace-menu" id="workspace-menu" role="menu" aria-label="Workspaces" hidden>${workspaceMenuItems()}</div></div><div class="search-row"><button id="search">${icon("search")}Search <span class="shortcut">Ctrl K</span></button><button id="quick-note" aria-label="New note" title="New note">${icon("plus")}</button></div><div class="sidebar-scroll"><nav class="nav" aria-label="Main navigation">${[
      ["home", "home", "Overview"],
      ["all", "note", "All notes"],
      ["journal", "calendar", "Journal"],
      ["templates", "template", "Templates"],
      ["databases", "grid", "Databases"],
      ["flashcards", "template", "Flashcards"],
      ["chat", "brain", "AI chat"],
    ]
      .map(
        ([v, i, t]) =>
          `<button data-view="${v}" class="${view === v && !note ? "active" : ""}">${icon(i)}${t}</button>`,
      )
      .join(
        "",
      )}</nav>${sidebarSections()}</div><div class="sidebar-bottom"><div class="profile"><span class="avatar" title="${localMode ? "On this computer" : "Connected to server"} · v0.2"><i class="dot" aria-hidden="true"></i>${esc(data.user.name.slice(0, 2).toUpperCase())}</span>${localMode ? `<div class="profile-link"><div class="profile-name">On this computer</div><div class="profile-role">Personal notes · offline copies</div></div>` : `<button class="profile-link" data-view="account" title="Account security"><div class="profile-name">${esc(data.user.name)}</div><div class="profile-role">${data.user.admin ? "Server administrator" : "Member"} · Security</div></button><button id="logout" aria-label="Sign out">${icon("logout")}</button>`}</div></div></aside><main class="main"><header class="topbar"><div class="crumb"><button class="mobile-menu" aria-label="Toggle sidebar">${icon("menu")}</button>${icon("home")}<span>${esc(w?.name || "Your server")}</span><span>/</span><strong id="breadcrumb">Overview</strong></div><div class="top-actions">${themeSelect()}${workspaceBadge()}<span class="avatar">${esc(data.user.name[0])}</span></div></header><div id="content" tabindex="-1"></div></main></div>`;
  $$("[data-view]").forEach((b) =>
    action(b, "click", () => navigate(b.dataset.view)),
  );
  $$("[data-folder]").forEach((b) =>
    action(b, "click", () => navigate("folder", b.dataset.folder)),
  );
  $$("[data-open-tag]").forEach((b) =>
    action(b, "click", () => openTag(b.dataset.openTag)),
  );
  workspaceMenu();
  action($("#search"), "click", search);
  action($("#add-folder"), "click", () => createFolder());
  action($("#quick-note"), "click", createNote);
  bindSidebar();
  action($("#logout"), "click", async () => {
    await noteTabs?.clear();
    await flush();
    stopLive();
    await api("logout", "POST");
    login();
  });
  action($(".mobile-menu"), "click", () =>
    $(".sidebar").classList.toggle("open"),
  );
  action($("#sync-state"), "click", syncDialog);
  action($("#share-workspace"), "click", () => shareWorkspace());
  bindThemeSelect();
  if (note) renderEditor();
  else if (view === "admin") renderAdmin().catch(error);
  else if (view === "databases") renderDatabaseList();
  else if (view === "flashcards")
    renderFlashcards({ api, esc, data, error }).catch(error);
  else if (view === "sharing") renderSharing().catch(error);
  else if (view === "journal") renderJournal();
  else if (view === "account") renderAccount().catch(error);
  else if (view === "templates") renderTemplates();
  else if (view === "integrations") renderIntegrations().catch(error);
  else if (view === "providers")
    renderAISettings({ api, esc, data, error }).catch(error);
  else if (view === "chat")
    renderAIChat({
      api,
      esc,
      data,
      workspaceId,
      initialScope: aiScope,
      docked: chatPane,
      dockedTitle: chatPane ? chatTitle() : "",
      openNote,
      openDatabase,
      sendToNote,
      noteFromReply,
      canEditNote: (id) =>
        data.roles[data.notes.find((n) => n.id === id)?.folder_id] === "editor",
      configure: () => navigate("providers"),
      error,
    }).catch(error);
  else overview();
  noteTabs?.place();
  paneState();
}
async function askAI(scope) {
  // Inside a note tab, the chat opens in its own tab beside the note.
  if (isPane && !chatPane && ["note", "folder"].includes(scope.kind)) {
    await flush();
    paneMessage("open-chat", {
      scope: `${scope.kind}:${scope.id}`,
      title:
        scope.kind === "note"
          ? (note?.id === scope.id ? note.title : null) ||
            data.notes.find((n) => n.id === scope.id)?.title
          : data.folders.find((f) => f.id === scope.id)?.name,
    });
    return;
  }
  aiScope = scope;
  await navigate("chat");
}
/** What a chat tab is about, for its tab title and heading. */
function chatTitle() {
  if (!aiScope) return "AI chat";
  if (aiScope.kind === "note")
    return data?.notes.find((n) => n.id === aiScope.id)?.title || "Note";
  return data?.folders.find((f) => f.id === aiScope.id)?.name || "Folder";
}
/** Adds Markdown blocks at the end of the open note. Returns how many. */
async function appendBlocks(markdowns) {
  if (!note || !editable())
    throw new Error("You can only add to a note you can edit.");
  const extra = markdowns.filter((m) => m.trim()).map(newBlock);
  if (!extra.length) return 0;
  live?.boundary();
  // A note that is only an empty block starts with the reply instead.
  if (note.blocks.length === 1 && !note.blocks[0].markdown.trim()) {
    note.blocks = [];
    note.rendered = [];
  }
  const rendered = await Promise.all(
    extra.map((b) => renderMarkdown(b.markdown)),
  );
  note.blocks.push(...extra);
  note.rendered.push(...rendered);
  markDirty();
  renderBlocks();
  $(`[data-block="${CSS.escape(extra[0].id)}"]`)?.scrollIntoView({
    block: "center",
    behavior: "smooth",
  });
  announce(`Added ${extra.length} block${extra.length === 1 ? "" : "s"}`);
  toast(
    `Added ${extra.length} block${extra.length === 1 ? "" : "s"} from AI chat`,
  );
  return extra.length;
}
// Requests from this chat tab to its host, by ID.
const insertRequests = new Map();
/** Sends reply blocks to a note: through the host, or into the open note. */
async function sendToNote(id, blocks) {
  const title = data.notes.find((n) => n.id === id)?.title || "Note";
  if (isPane)
    return new Promise((resolve, reject) => {
      const request = crypto.randomUUID();
      insertRequests.set(request, { resolve, reject });
      paneMessage("insert-blocks", { request, id, title, blocks });
      setTimeout(() => {
        if (insertRequests.delete(request))
          reject(new Error("The note did not respond. Nothing was added."));
      }, 25000);
    });
  if (noteTabs) return noteTabs.insertBlocks({ id, title, blocks });
  if (note?.id === id) return appendBlocks(blocks);
  throw new Error("Open the note to add to it.");
}
/** A new note holding an AI reply, in the chat's folder; then opens it. */
async function noteFromReply(markdown, question, scope) {
  const folder =
    scope.kind === "folder"
      ? scope.id
      : scope.kind === "note"
        ? data.notes.find((n) => n.id === scope.id)?.folder_id
        : null;
  const target = [
    folder,
    ...data.folders
      .filter((f) => f.workspace_id === (workspaceId || f.workspace_id))
      .map((f) => f.id),
  ].find((f) => f && data.roles[f] === "editor");
  if (!target) throw new Error("You need edit access to a folder to save it.");
  const blocks = splitMarkdown(markdown).filter((b) => b.trim());
  // A leading heading becomes the title; otherwise the question does.
  let title = question.replace(/\s+/g, " ").trim();
  if (/^#{1,3} /.test(blocks[0] || ""))
    title = blocks.shift().replace(/^#+ /, "");
  const n = await api("notes", "POST", {
    folder_id: target,
    title: title.slice(0, 120) || "AI reply",
  });
  await api("notes/" + n.id, "PUT", {
    ...n,
    blocks: (blocks.length ? blocks : [""]).map((b, i) => newBlock(b, i)),
  });
  await refresh();
  await openNote(n.id);
  return n.id;
}
async function navigate(v, f = null) {
  noteTabs?.hide();
  videos.stop();
  await flush();
  stopLive();
  clearInterval(dbTimer);
  activeDatabase = null;
  note = null;
  folderId = f;
  view = v;
  await refresh();
  shell();
}
/** The overview: a dashboard of widgets the user arranges (see dashboard.js). */
function dashboard() {
  $("#breadcrumb").textContent = "Overview";
  $("#content").innerHTML =
    `<section class="page"><div class="eyebrow">YOUR WORKSPACE, AT A GLANCE</div><div class="page-heading"><div><h1>A place for your ideas</h1><p class="subtitle">Collect your thoughts. Connect the dots. Make something great.</p></div><button class="btn primary" id="new-note">${icon("plus")}New note</button></div><div id="dashboard-root"></div></section>`;
  action($("#new-note"), "click", createNote);
  renderDashboard($("#dashboard-root"), {
    api,
    esc,
    icon,
    data,
    notes: () => workspaceNotes().filter((n) => !n.trashed),
    folderCards: () =>
      folderChildren(treeFolders(), null).map(folderCard).join(""),
    art,
    today: localDate,
    calendar: (month) =>
      calendarParts(month, new Map(journalNotes().map((n) => [n.journal, n]))),
    openNote,
    openJournal,
    createNote,
    navigate,
    error,
    toast,
  }).catch(error);
}
function overview() {
  if (view === "home") return dashboard();
  let notes = workspaceNotes().filter((n) =>
    view === "trash" ? n.trashed : !n.trashed,
  );
  if (view === "favorites") notes = notes.filter((n) => n.favorite);
  if (view === "folder") notes = notes.filter((n) => n.folder_id === folderId);
  if (view === "tag")
    notes = [...workspaceNotes(), ...(data.shared_notes || [])].filter(
      (n) => !n.trashed && noteTags(n).includes(tagFilter),
    );
  notes.sort((a, b) => b.updated_at - a.updated_at);
  const title =
    view === "home"
      ? "A place for your ideas"
      : view === "all"
        ? "All notes"
        : view === "favorites"
          ? "Your favorites"
          : view === "trash"
            ? "Trash"
            : view === "tag"
              ? "#" + tagFilter
              : folders().find((f) => f.id === folderId)?.name || "Folder";
  $("#breadcrumb").textContent =
    view === "home"
      ? "Overview"
      : view === "folder"
        ? folderPath(folderId)
            .map((f) => f.name)
            .join(" / ")
        : title;
  $("#content").innerHTML =
    `<section class="page"><div class="eyebrow">${view === "home" ? "YOUR WORKSPACE, AT A GLANCE" : "A LITTLE MORE ORGANIZED"}</div><div class="page-heading"><div><h1>${esc(title)}</h1>${view === "folder" ? '<button class="btn" id="ask-folder">Ask AI about this folder</button>' : ""}<p class="subtitle">${view === "home" ? "Collect your thoughts. Connect the dots. Make something great." : view === "trash" ? "Nothing is permanently deleted. Restore a note whenever you need it." : `${notes.length} note${notes.length === 1 ? "" : "s"} · Space to think, write, and explore.`}</p></div>${view !== "trash" ? `<button id="new-note" class="btn primary" ${!folders().some((f) => data.roles[f.id] === "editor") ? "disabled" : ""}>${icon("plus")}New note</button>` : ""}</div>${
      view === "home"
        ? `<div class="welcome-banner"><div class="welcome-copy"><div class="eyebrow">A FRESH START</div><h2>Good ideas start with a little space.</h2><p>A quick thought, a detailed plan, your next big thing.<br>Bring it all together in your workspace.</p><button class="text-link" id="start-writing">Start writing <span>↗</span></button></div>${art()}</div><div class="section-title"><h2>Your folders</h2><span>${folderChildren(treeFolders(), null).length} spaces to keep things together</span></div><div class="folder-grid">${
            folderChildren(treeFolders(), null).map(folderCard).join("") ||
            '<div class="empty">Your administrator can grant you folder access.</div>'
          }</div>`
        : view === "folder" && folderChildren(treeFolders(), folderId).length
          ? `<div class="section-title"><h2>Subfolders</h2></div><div class="folder-grid">${folderChildren(treeFolders(), folderId).map(folderCard).join("")}</div>`
          : ""
    }<div class="notes-toolbar"><div class="tabs"><button class="active">${view === "home" ? "Recently updated" : view === "trash" ? "Deleted notes" : "Your notes"} <span>(${notes.length})</span></button>${view === "folder" && editsWorkspace() ? '<button id="rename-folder">Rename folder</button>' : ""}${view === "folder" && data.roles[folderId] === "editor" ? '<button id="delete-folder" class="danger-text">Delete folder</button>' : ""}</div><div style="display:flex;gap:12px;align-items:center">${view !== "trash" ? `<button id="import" class="btn" title="Import Markdown">${icon("upload")}Import</button>` : notes.some((n) => data.roles[n.folder_id] === "editor") ? `<button id="empty-trash" class="btn danger">${icon("trash")}Empty trash</button>` : ""}<div class="view-toggle"><button data-layout="grid" aria-label="Grid view" class="${layout === "grid" ? "active" : ""}">${icon("grid")}</button><button data-layout="list" aria-label="List view" class="${layout === "list" ? "active" : ""}">${icon("list")}</button></div></div></div><div class="note-${layout}">${notes.map((n) => `<button class="note-card" data-note="${n.id}"><div class="note-preview" aria-hidden="true"><span class="preview-symbol">${esc(n.icon)}</span><div class="preview-lines"><i></i><i></i><i></i></div></div><div class="note-info"><strong>${esc(n.title)}</strong><div class="note-meta">${icon("folder")}<span>${esc(data.folders.find((f) => f.id === n.folder_id)?.name)}</span><span>· ${date(n.updated_at)}</span>${n.favorite ? `<span class="star">☆</span>` : ""}${sharingBadges(n.sharing)}</div></div></button>`).join("")}</div>${!notes.length ? `<div class="empty">${icon("note")}<h3>${view === "trash" ? "All clear. No deleted notes." : "A blank page, endless possibilities."}</h3>${view === "trash" ? "Deleted notes will appear here." : "Create a note to give your ideas a home."}</div>` : ""}<div class="page-footer">A little clarity. A little possibility. Your Minerva.</div></section>`;
  action($("#ask-folder"), "click", () =>
    askAI({ kind: "folder", id: folderId }),
  );
  $$("[data-note]").forEach((b) =>
    action(b, "click", () => openNote(b.dataset.note)),
  );
  $$("[data-open-folder]").forEach((b) =>
    action(b, "click", () => navigate("folder", b.dataset.openFolder)),
  );
  $$("[data-layout]").forEach((b) =>
    action(b, "click", () => {
      layout = b.dataset.layout;
      localStorage.setItem("minerva.layout", layout);
      overview();
    }),
  );
  action($("#new-note"), "click", createNote);
  action($("#start-writing"), "click", createNote);
  action($("#import"), "click", importMarkdown);
  action($("#delete-folder"), "click", () => deleteFolder(folderId));
  action($("#empty-trash"), "click", () => {
    const doomed = notes.filter((n) => data.roles[n.folder_id] === "editor");
    formDialog(
      "Empty the trash?",
      `${doomed.length} note${doomed.length === 1 ? " is" : "s are"} deleted permanently, with history, files, shares, and published pages.${doomed.length < notes.length ? " Notes in folders you can only view stay." : ""} This cannot be undone.`,
      "",
      async () => {
        for (const n of doomed) await api("notes/" + n.id, "DELETE");
        await refresh();
        shell();
        toast("Trash emptied");
      },
      "Delete forever",
    );
  });
  action($("#rename-folder"), "click", () => renameFolder(folderId));
}
function folderCard(f) {
  const notes = data.notes.filter(
    (n) => n.folder_id === f.id && !n.trashed,
  ).length;
  const subfolders = data.folders.filter((x) => x.parent_id === f.id).length;
  return `<button class="folder-card" data-open-folder="${f.id}"><span class="folder-icon">${icon("folder")}</span><div><strong>${esc(f.name)}</strong><small>${notes} note${notes === 1 ? "" : "s"}${subfolders ? ` · ${subfolders} subfolder${subfolders === 1 ? "" : "s"}` : ""} · ${data.roles[f.id] === "editor" ? "Can edit" : "Read only"}</small></div></button>`;
}
function formDialog(title, desc, fields, onSubmit, label = "Create") {
  modal.innerHTML = `<form><h2>${esc(title)}</h2><p>${esc(desc)}</p>${fields}<div class="form-error" role="alert"></div><footer><button type="button" class="btn" id="cancel">Cancel</button><button class="btn primary" type="submit">${esc(label)}</button></footer></form>`;
  modal.showModal();
  $("#cancel", modal).onclick = () => modal.close();
  $("form", modal).onsubmit = async (e) => {
    e.preventDefault();
    const b = $("[type=submit]", modal);
    b.disabled = true;
    try {
      await onSubmit(Object.fromEntries(new FormData(e.target)));
      modal.close();
    } catch (err) {
      $(".form-error", modal).textContent = err.message;
    } finally {
      b.disabled = false;
    }
  };
}
function folderOptions() {
  return folders()
    .filter((f) => data.roles[f.id] === "editor")
    .map(
      (f) =>
        `<option value="${f.id}" ${f.id === folderId ? "selected" : ""}>${esc(f.name)}</option>`,
    )
    .join("");
}
function createNote() {
  if (!folderOptions())
    return toast("You need editor access to a folder first.");
  formDialog(
    "A new beginning",
    "Give your note a name. You can always change it later.",
    `<label>Note title<input name="title" placeholder="Untitled idea" required maxlength="200" autofocus></label><label>Folder<select name="folder_id">${folderOptions()}</select></label>`,
    async (v) => {
      const n = await api("notes", "POST", v);
      await refresh();
      await openNote(n.id);
    },
    "Create note",
  );
}
function createWorkspace() {
  formDialog(
    "Create a workspace",
    "A separate home for a team, project, or part of your life.",
    `<label>Workspace name<input name="name" placeholder="e.g. Design studio" required maxlength="200" autofocus></label>`,
    async (v) => {
      const w = await api("workspaces", "POST", v);
      workspaceId = w.id;
      view = "home";
      note = null;
      await refresh();
      shell();
    },
    "Create workspace",
  );
}
// Administrators delete any workspace; owners delete their personal ones.
function deleteWorkspace(w, after = null) {
  formDialog(
    `Delete “${w.name}”?`,
    "Every folder, note, database, site, published page, and file in this workspace is deleted permanently, and members lose access. This cannot be undone.",
    `<label>Type the workspace name to confirm<input name="confirm" required autocomplete="off"></label>`,
    async (v) => {
      if (v.confirm !== w.name)
        throw new Error("Type the workspace name exactly to confirm.");
      await api("workspaces/" + w.id, "DELETE");
      if (workspaceId === w.id) {
        workspaceId = null;
        note = null;
        folderId = null;
        view = "home";
      }
      await refresh();
      if (after) await after();
      else shell();
      toast("Workspace deleted");
    },
    "Delete workspace",
  );
}
async function openNote(id) {
  if (noteTabs) {
    await flush();
    const target =
      data.notes.find((n) => n.id === id) || (await api("notes/" + id));
    noteTabs.open({
      id,
      title: target.title,
      workspace:
        data.workspaces.find((w) => w.id === workspaceOfNote(target))?.name ||
        "",
    });
    return;
  }
  if (isPane && ((note && note.id !== id) || chatPane)) {
    paneMessage("open-note", {
      id,
      title: data.notes.find((n) => n.id === id)?.title,
    });
    return;
  }
  await flush();
  videos.stop();
  stopLive();
  note = await api("notes/" + id);
  workspaceId = workspaceOfNote(note) || workspaceId;
  rememberRendered();
  dirty = false;
  conflict = false;
  mode = "document";
  live = new Collaboration(
    id,
    api,
    receiveLive,
    (status, pending) => {
      dirty = pending;
      paneState();
      if ($("#save-state")) $("#save-state").textContent = status;
    },
    (e) => {
      if (e.status === 401 || e.status === 403) {
        if (note) {
          note.role = null;
          $$(".block textarea,#note-title").forEach(
            (el) => (el.readOnly = true),
          );
        }
        toast("Access changed. Export any unsaved draft before leaving.");
        stopLive();
      }
    },
  );
  await live.start();
  shell();
}
function receiveLive(snapshot, result) {
  if (!note || note.id !== result.note.id) return;
  const contentChanged =
    note.title !== snapshot.title ||
    JSON.stringify(note.blocks.map((b) => [b.id, b.markdown])) !==
      JSON.stringify(snapshot.blocks.map((b) => [b.id, b.markdown]));
  needsBlockRefresh ||= contentChanged;
  const existing = new Map(note.blocks.map((b) => [b.id, b]));
  note.blocks = snapshot.blocks.map((b, i) => {
    const target = existing.get(b.id) || newBlock("", i);
    Object.assign(target, b);
    return target;
  });
  for (const key of ["title", "icon", "favorite", "trashed", "cover", "tags"])
    note[key] = snapshot[key];
  if (
    result.note.inline_tags &&
    JSON.stringify(result.note.inline_tags) !== JSON.stringify(note.inline_tags)
  ) {
    note.inline_tags = result.note.inline_tags;
    if (!document.activeElement?.closest?.(".tag-add")) renderTagBar();
  }
  updateStats();
  paneState();
  note.revision = result.note.revision;
  note.updated_at = result.note.updated_at;
  if (result.note.sharing) note.sharing = result.note.sharing;
  if (result.can_edit) note.role = "editor";
  else {
    note.role = "viewer";
    $$(".block textarea,#note-title").forEach((el) => (el.readOnly = true));
    $$("#save-note,#favorite,#trash-note,#add-block").forEach(
      (el) => (el.disabled = true),
    );
  }
  const html = new Map(
    result.note.blocks.map((b, i) => [
      b.id,
      { markdown: b.markdown, html: result.note.rendered[i] },
    ]),
  );
  note.rendered = note.blocks.map((b) =>
    html.get(b.id)?.markdown === b.markdown
      ? html.get(b.id).html
      : (renderedCache.get(b.markdown) ?? esc(b.markdown)),
  );
  rememberRendered();
  const active = document.activeElement?.matches(".block textarea")
    ? document.activeElement.closest("[data-block]")
    : null;
  if ($("#blocks")) {
    if (
      !active &&
      !document.activeElement?.matches("#note-title") &&
      needsBlockRefresh
    ) {
      renderBlocks();
      needsBlockRefresh = false;
    } else if (contentChanged)
      $$(".block").forEach((el) => {
        if (el === active) return;
        const i = note.blocks.findIndex((b) => b.id === el.dataset.block);
        if (i >= 0) $(".block-body", el).innerHTML = note.rendered[i];
      });
    const title = $("#note-title");
    if (title && title !== document.activeElement) title.value = note.title;
    if (active && !existing.has(active.dataset.block)) active.remove();
    if (active && !note.blocks.some((b) => b.id === active.dataset.block)) {
      const input = $("textarea", active);
      if (input) {
        input.readOnly = true;
        toast(
          "This block was removed by a collaborator. Copy any draft text before leaving it.",
        );
      }
    }
  }
  if ($("#presence"))
    $("#presence").innerHTML =
      result.peers
        .filter((p) => p.client_id !== live?.clientId)
        .map(
          (p) =>
            `<span class="peer" title="${esc(p.active_block ? "Editing a block" : "Viewing this note")}">${esc(p.name)}</span>`,
        )
        .join("") || '<span class="presence-label">Only you here</span>';
}
function markDirty() {
  dirty = true;
  paneState();
  updateStats();
  if (note && !isEmptyNote(note)) $(".template-fill")?.remove();
  if (live && note) live.commit(note);
  const el = $("#save-state");
  if (el) el.textContent = "Unsaved changes";
}
function queueSave() {
  if (live && note) return live.flush(note);

  if (!note || !dirty || conflict) return saveChain;
  const current = note;
  saveChain = saveChain
    .catch(() => {})
    .then(async () => {
      if (!dirty || current !== note || conflict) return;
      const payload = {
        title: current.title,
        icon: current.icon,
        blocks: structuredClone(current.blocks),
        revision: current.revision,
        favorite: current.favorite,
        trashed: current.trashed,
      };
      dirty = false;
      if ($("#save-state")) $("#save-state").textContent = "Saving…";
      try {
        const saved = await api("notes/" + current.id, "PUT", payload);
        current.revision = saved.revision;
        current.updated_at = saved.updated_at;
        if ($("#save-state"))
          $("#save-state").textContent = dirty
            ? "Unsaved changes"
            : "All changes saved";
      } catch (e) {
        dirty = true;
        if (e.status === 409) conflict = true;
        if ($("#save-state"))
          $("#save-state").textContent = conflict
            ? "Conflict · export draft"
            : "Not saved · retry";
        error(e);
        throw e;
      }
    });
  return saveChain;
}
async function flush() {
  if (document.activeElement?.matches(".block textarea"))
    document.activeElement.blur();
  if (document.activeElement?.id === "note-title")
    document.activeElement.blur();
  await closing;
  if (
    document.activeElement?.matches(
      ".records-table input,.records-table select",
    )
  )
    document.activeElement.blur();
  await Promise.all([...dbSaves]);
  if (dbDrafts.size)
    throw new Error(
      "A database cell has an unsaved value. Save it or use Reload records before leaving.",
    );
  await queueSave();
  if (conflict)
    throw new Error(
      "Export your draft and reload this page to resolve the conflict.",
    );
}
function renderEditor() {
  // Names for mention chips, and the people the `@` picker offers.
  loadPeople().catch(() => {});
  const can = editable();
  $("#breadcrumb").textContent = note.title;
  const sharing = note.sharing || {};
  const shareState = sharing.published
    ? `<a class="public-indicator" href="${esc(serverOrigin())}/${esc(sharing.published)}" target="_blank" rel="noopener">Published at /${esc(sharing.published)}</a>`
    : sharing.links
      ? '<span class="public-indicator">Anyone with a link can open this note</span>'
      : sharing.members
        ? `<span class="share-indicator">Shared with ${sharing.members} member${sharing.members === 1 ? "" : "s"}</span>`
        : "";
  const tools = shareMode
    ? `<button class="btn" id="copy-note-link" title="Copy a link to open this note in another Minerva tab">Copy link</button><button class="btn" id="export">${icon("download")}Export</button>`
    : `<button class="btn" id="favorite" aria-label="Toggle favorite" ${!can ? "disabled" : ""}>${note.favorite ? "★" : "☆"}</button>${localMode ? "" : `<button class="btn" id="share-note" ${data.roles[note.folder_id] !== "editor" || note.trashed ? 'disabled title="Folder editors can share this note"' : ""}>Share${sharing.published || sharing.links ? '<span class="share-badge public">Public</span>' : ""}</button>`}<button class="btn" id="ask-note" title="Chat with AI about this note, side by side">${icon("brain")}Ask AI</button><button class="btn" id="attachments">Files</button><button class="btn" id="history" ${!data.roles[note.folder_id] ? 'disabled title="Version history is available to folder members"' : ""}>${icon("clock")}History</button><button class="btn" id="copy-note-link" title="Copy a link to open this note in another Minerva tab">Copy link</button><button class="btn" id="metadata" title="Document details used by PDF templates">${icon("list")}Metadata</button><button class="btn" id="export">${icon("download")}Export</button><button class="btn" aria-label="${note.trashed ? "Restore note" : "Move note to trash"}" id="trash-note" ${!editable() && !(note.trashed && data.roles[note.folder_id] === "editor") ? "disabled" : ""}>${icon(note.trashed ? "check" : "trash")}${note.trashed ? "Restore" : ""}</button>${note.trashed && data.roles[note.folder_id] === "editor" ? '<button class="btn danger" id="delete-note">Delete forever</button>' : ""}`;
  $("#content").innerHTML =
    `<div class="editor-toolbar" role="toolbar" aria-label="Note tools"><span class="save-status" id="save-state" role="status">${can ? "All changes saved" : "Read only"}</span><span id="presence" class="presence"></span><span class="spacer"></span><button class="btn" id="find-note" title="Find and replace (Ctrl+F)" aria-label="Find and replace">⌕</button><button class="btn" id="focus-toggle" title="Focus mode (Ctrl+Shift+F)" aria-pressed="${document.body.classList.contains("focus-mode")}">Focus</button>${shareMode && !can ? "" : `<button class="btn" id="undo" aria-label="Undo" title="Undo (Ctrl+Z)" ${!can ? "disabled" : ""}>↶</button><button class="btn" id="redo" aria-label="Redo" title="Redo (Ctrl+Shift+Z)" ${!can ? "disabled" : ""}>↷</button><button class="btn" id="save-note" ${!can ? "disabled" : ""}>Save</button>`}${tools}</div>${coverHTML(note.cover, can)}<article class="document ${note.cover ? "has-cover" : ""}">${can ? `<div class="doc-adders">${note.icon ? "" : '<button class="text-link" id="add-icon">☺ Add icon</button>'}${note.cover ? "" : '<button class="text-link" id="add-cover">▧ Add cover</button>'}</div>` : ""}${note.icon ? (can ? `<button class="doc-icon" id="doc-icon" aria-label="Change icon" title="Change icon">${esc(note.icon)}</button>` : `<div class="doc-icon" aria-hidden="true">${esc(note.icon)}</div>`) : ""}${note.journal && !shareMode ? `<nav class="journal-nav" aria-label="Journal days"><button class="btn" data-journal-shift="-1" aria-label="Previous day">‹ ${esc(parseDate(shiftDate(note.journal, -1)).toLocaleDateString(undefined, { month: "short", day: "numeric" }))}</button><span>${esc(longDate(note.journal))}</span><button class="btn" data-journal-shift="1" aria-label="Next day">${esc(parseDate(shiftDate(note.journal, 1)).toLocaleDateString(undefined, { month: "short", day: "numeric" }))} ›</button></nav>` : ""}${!shareMode && isTemplate(note) ? '<span class="template-badge">Template · used by “Fill from template”</span>' : ""}<input id="note-title" data-live-key="title" class="doc-title" aria-label="Note title" maxlength="200" value="${esc(note.title)}" ${!can ? "readonly" : ""}><div class="doc-meta">${shareMode ? "" : `<span>${esc(data.folders.find((f) => f.id === note.folder_id)?.name || "Shared with you")}</span>`}<span>Updated ${date(note.updated_at)}</span><span>Revision ${note.revision}</span><span id="word-count"></span>${shareState}</div><div class="tag-bar" id="tag-bar"></div>${can && !shareMode && isEmptyNote(note) && templateNotes(workspaceOfNote(note)).some((t) => t.id !== note.id) ? `<div class="template-fill"><button class="btn" id="fill-template">${icon("template")}Fill from template</button><span>or just start writing</span></div>` : ""}<div id="blocks" role="list" aria-label="Note blocks"></div>${can ? '<button class="add-block" id="add-block">+ Add a block</button><p class="editor-help" id="editor-help">Click a block to edit · Enter splits it at the cursor · Backspace at the start merges with the block above · Shift+Enter adds a line break · ↑/↓ at the edges move between blocks · Ctrl+Z undoes your changes · / for block types · Ctrl+Shift+P for commands on selected text</p>' : ""}<section class="backlinks" id="backlinks" aria-label="Linked from"></section></article>`;
  renderBlocks();
  renderTagBar();
  renderBacklinks();
  updateStats();
  action($("#find-note"), "click", () => openFind());
  action($("#fill-template"), "click", (e) =>
    fillFromTemplate(e.currentTarget),
  );
  $$("[data-journal-shift]").forEach((b) =>
    action(b, "click", () =>
      openJournal(
        shiftDate(note.journal, Number(b.dataset.journalShift)),
        workspaceOfNote(note),
      ),
    ),
  );
  action($("#focus-toggle"), "click", () => toggleFocus());
  action($("#doc-icon"), "click", (e) => iconPicker(e.currentTarget));
  action($("#add-icon"), "click", (e) => iconPicker(e.currentTarget));
  action($("#add-cover"), "click", (e) => coverPicker(e.currentTarget));
  action($("#change-cover"), "click", (e) => coverPicker(e.currentTarget));
  action($("#save-note"), "click", () => queueSave());
  action($("#undo"), "click", () => undoRedo(false));
  action($("#redo"), "click", () => undoRedo(true));
  action($("#share-note"), "click", () => shareDialog());
  action($("#favorite"), "click", async () => {
    note.favorite = !note.favorite;
    markDirty();
    await queueSave();
    $("#favorite").textContent = note.favorite ? "★" : "☆";
  });
  action($("#export"), "click", exportNote);
  action($("#metadata"), "click", () => metadataDialog());
  action($("#copy-note-link"), "click", async () => {
    await navigator.clipboard.writeText(`${serverOrigin()}/?note=${note.id}`);
    toast("Note link copied");
  });
  action($("#history"), "click", history);
  action($("#attachments"), "click", attachmentDialog);
  action($("#ask-note"), "click", () => askAI({ kind: "note", id: note.id }));
  action($("#add-block"), "click", async () => {
    await flush();
    if (!editable()) return;
    const block = newBlock();
    note.blocks.push(block);
    note.rendered.push("");
    markDirty();
    renderBlocks();
    editBlock(block.id);
  });
  action($("#delete-note"), "click", () =>
    formDialog(
      "Delete this note forever?",
      "The note, its version history, files, shares, and published page are removed permanently. This cannot be undone.",
      "",
      async () => {
        await api("notes/" + note.id, "DELETE");
        note = null;
        view = "trash";
        await refresh();
        shell();
        toast("Note deleted");
      },
      "Delete forever",
    ),
  );
  action($("#trash-note"), "click", () =>
    formDialog(
      note.trashed ? "Restore note?" : "Move note to trash?",
      note.trashed
        ? "This note will return to its folder."
        : "You can restore this note from Trash at any time.",
      "",
      async () => {
        note.trashed = !note.trashed;
        markDirty();
        await queueSave();
        await navigate(note.trashed ? "trash" : "all");
      },
      note.trashed ? "Restore" : "Move to trash",
    ),
  );
  const title = $("#note-title");
  action(title, "input", () => {
    note.title = title.value;
    markDirty();
  });
  action(title, "blur", () => {
    if (!note.title.trim()) {
      note.title = "Untitled";
      title.value = note.title;
    }
    queueSave().catch(() => {});
  });
}
const COVER_GRADIENTS = 8;
function coverURL(cover) {
  const id = cover.match(/^\/api\/v1\/attachments\/([0-9a-f-]{36})$/)?.[1];
  if (id)
    return shareMode
      ? `/api/v1/shared/attachments/${id}/preview`
      : `/api/v1/attachments/${id}/preview`;
  return cover.startsWith("https://") ? cover : "";
}
function coverHTML(cover, can) {
  if (!cover) return "";
  const gradient = cover.match(/^gradient:(\d)$/)?.[1];
  const inner = gradient
    ? ""
    : `<img src="${esc(coverURL(cover))}" alt="" referrerpolicy="no-referrer">`;
  return `<div class="note-cover ${gradient ? `cover-gradient-${gradient}` : ""}">${inner}${can ? '<button class="btn cover-change" id="change-cover">Change cover</button>' : ""}</div>`;
}
/** A small anchored popover; closes on Escape, outside click, or `close()`. */
function popover(anchor, html, label) {
  closePopover();
  const pop = document.createElement("div");
  pop.className = "popover";
  pop.setAttribute("role", "dialog");
  pop.setAttribute("aria-label", label);
  pop.innerHTML = html;
  document.body.append(pop);
  const r = anchor.getBoundingClientRect();
  pop.style.left =
    Math.max(8, Math.min(r.left, innerWidth - pop.offsetWidth - 8)) + "px";
  pop.style.top =
    Math.min(r.bottom + 6, innerHeight - pop.offsetHeight - 8) + scrollY + "px";
  const outside = (e) => {
    if (!pop.contains(e.target) && e.target !== anchor) closePopover();
  };
  const escape = (e) => {
    if (e.key === "Escape") {
      e.stopPropagation();
      closePopover();
      anchor.focus?.();
    }
  };
  setTimeout(() => document.addEventListener("mousedown", outside), 0);
  pop.addEventListener("keydown", escape);
  pop.cleanup = () => document.removeEventListener("mousedown", outside);
  return pop;
}
function closePopover() {
  const pop = $(".popover");
  pop?.cleanup?.();
  pop?.remove();
}
const ICONS =
  "📝 📌 💡 ✳ 📚 🗂 📅 ✅ 🚀 🎯 ⭐ 🔥 💼 🏠 🌱 🧪 🧠 📊 📈 🛠 🔒 🌍 🎨 🎵 📷 ✈️ 🍳 ❤️ 😀 🙂 🤝 💬 📣 🧭 ⚙️ 🐞 🔍 📦 🧾 🏁 🗓 📍 🧩 🪴 ☕ 🎉 🎓 💰 🏃 🧘 📖 ✍️ 🔖 🗒 📎 🧷 ▤ ✦ ◆ ●".split(
    " ",
  );
function setNoteField(key, value) {
  live?.boundary();
  note[key] = value;
  markDirty();
  queueSave().catch(() => {});
  renderEditor();
}
function iconPicker(anchor) {
  const pop = popover(
    anchor,
    `<div class="icon-grid">${ICONS.map((i) => `<button data-icon="${i}" aria-label="Use ${i}">${i}</button>`).join("")}</div><form class="icon-custom"><input name="icon" maxlength="8" placeholder="Or type an emoji" aria-label="Custom icon"><button class="btn">Use</button></form>${note.icon ? '<button class="text-link" data-icon="">Remove icon</button>' : ""}`,
    "Choose an icon",
  );
  $$("[data-icon]", pop).forEach((b) =>
    b.addEventListener("click", () => {
      closePopover();
      setNoteField("icon", b.dataset.icon);
    }),
  );
  $(".icon-custom", pop).onsubmit = (e) => {
    e.preventDefault();
    const value = e.target.icon.value.trim();
    if (!value || new TextEncoder().encode(value).length > 24) return;
    closePopover();
    setNoteField("icon", value);
  };
  $("button", pop).focus();
}
function coverPicker(anchor) {
  const pop = popover(
    anchor,
    `<div class="cover-grid">${Array.from({ length: COVER_GRADIENTS }, (_, i) => `<button class="cover-gradient-${i + 1}" data-cover="gradient:${i + 1}" aria-label="Gradient ${i + 1}"></button>`).join("")}</div><label class="btn cover-upload">Upload image<input type="file" accept="image/png,image/jpeg,image/gif,image/webp" hidden></label><form class="cover-url"><input name="url" type="url" placeholder="https://… image address" aria-label="Image address"><button class="btn">Use</button></form>${note.cover ? '<button class="text-link" data-cover="">Remove cover</button>' : ""}`,
    "Choose a cover",
  );
  $$("[data-cover]", pop).forEach((b) =>
    b.addEventListener("click", () => {
      closePopover();
      setNoteField("cover", b.dataset.cover);
    }),
  );
  $(".cover-url", pop).onsubmit = (e) => {
    e.preventDefault();
    const url = e.target.url.value.trim();
    if (!/^https:\/\/\S+$/.test(url))
      return toast("Use an https:// image address");
    closePopover();
    setNoteField("cover", url);
  };
  $(".cover-upload input", pop).onchange = async (e) => {
    const file = e.target.files[0];
    closePopover();
    if (!file) return;
    try {
      const uploaded = await uploadFile(file, note.id);
      setNoteField("cover", `/api/v1/attachments/${uploaded.id}`);
    } catch (err) {
      error(err);
    }
  };
  $("button", pop).focus();
}
function renderTagBar() {
  const bar = $("#tag-bar");
  if (!bar) return;
  const can = editable();
  const own = note.tags || [];
  const inline = (note.inline_tags || []).filter((t) => !own.includes(t));
  bar.innerHTML = `${own.map((t) => `<span class="tag-chip" data-tag="${esc(t)}" role="link" tabindex="0">#${esc(t)}${can ? `<button class="tag-remove" data-remove-tag="${esc(t)}" aria-label="Remove tag ${esc(t)}">×</button>` : ""}</span>`).join("")}${inline.map((t) => `<span class="tag-chip inline" data-tag="${esc(t)}" role="link" tabindex="0" title="From #${esc(t)} in the text">#${esc(t)}</span>`).join("")}${
    can && !shareMode
      ? `<form class="tag-add"><input name="tag" list="tag-suggestions" placeholder="+ Add tag" aria-label="Add tag" maxlength="41" autocomplete="off"><datalist id="tag-suggestions">${allTags()
          .map(([t]) => `<option value="${esc(t)}">`)
          .join("")}</datalist></form>`
      : ""
  }`;
  $$("[data-remove-tag]", bar).forEach((b) =>
    b.addEventListener("click", (e) => {
      e.stopPropagation();
      setNoteField(
        "tags",
        own.filter((t) => t !== b.dataset.removeTag),
      );
      announce(`Removed tag ${b.dataset.removeTag}`);
    }),
  );
  $$(".tag-chip", bar).forEach((chip) =>
    chip.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && !shareMode) openTag(chip.dataset.tag);
    }),
  );
  const form = $(".tag-add", bar);
  if (form)
    form.onsubmit = (e) => {
      e.preventDefault();
      const tag = normalizeTag(form.tag.value);
      if (!tag)
        return toast(
          "Tags use letters, digits, - _ and /, up to 40 characters",
        );
      if (own.includes(tag)) return;
      if (own.length >= 20) return toast("A note can have up to 20 tags");
      setNoteField("tags", [...own, tag]);
      announce(`Added tag ${tag}`);
      $(".tag-add input")?.focus();
    };
}
function renderBacklinks() {
  const panel = $("#backlinks");
  if (!panel) return;
  const links = shareMode ? [] : note.backlinks || [];
  panel.hidden = !links.length;
  panel.innerHTML = links.length
    ? `<h2>Linked from ${links.length} note${links.length === 1 ? "" : "s"}</h2>${links.map((l) => `<a class="backlink" href="#note/${esc(l.id)}">${esc(l.icon)} ${esc(l.title)}</a>`).join("")}`
    : "";
}
let statsTimer;
function updateStats() {
  clearTimeout(statsTimer);
  statsTimer = setTimeout(() => {
    const el = $("#word-count");
    if (!el || !note) return;
    const { words, characters, minutes } = countWords(note.blocks);
    el.textContent = words
      ? `${words.toLocaleString()} word${words === 1 ? "" : "s"} · ${minutes} min read`
      : "No words yet";
    el.title = `${characters.toLocaleString()} characters`;
  }, 150);
}
let finder = { index: 0, matches: [] };
/** Find and replace within the open note's Markdown. */
function openFind() {
  if (!note || !$("#blocks")) return;
  let bar = $("#find-bar");
  if (!bar) {
    bar = document.createElement("div");
    bar.id = "find-bar";
    bar.className = "find-bar";
    bar.setAttribute("role", "search");
    bar.innerHTML = `<input id="find-query" aria-label="Find in note" placeholder="Find in note"><span id="find-count" class="find-count" aria-live="polite"></span><button class="btn" id="find-prev" aria-label="Previous match" title="Previous (Shift+Enter)">↑</button><button class="btn" id="find-next" aria-label="Next match" title="Next (Enter)">↓</button><label class="checkbox-label" title="Match case"><input type="checkbox" id="find-case">Aa</label>${editable() ? '<input id="find-replace" aria-label="Replace with" placeholder="Replace with"><button class="btn" id="replace-one">Replace</button><button class="btn" id="replace-all">Replace all</button>' : ""}<button class="btn" id="find-close" aria-label="Close find" title="Close (Escape)">×</button>`;
    $(".editor-toolbar").after(bar);
    finder = { index: 0, matches: [] };
    $("#find-query").addEventListener("input", () => runFind());
    $("#find-case").addEventListener("change", () => runFind());
    $("#find-next").onclick = () => runFind(1);
    $("#find-prev").onclick = () => runFind(-1);
    $("#find-close").onclick = closeFind;
    action($("#replace-one"), "click", () => replaceMatches(false));
    action($("#replace-all"), "click", () => replaceMatches(true));
    bar.addEventListener("keydown", (e) => {
      if (e.key === "Escape") {
        e.preventDefault();
        closeFind();
      } else if (e.key === "Enter" && e.target.id === "find-query") {
        e.preventDefault();
        runFind(e.shiftKey ? -1 : 1);
      } else if (e.key === "Enter" && e.target.id === "find-replace") {
        e.preventDefault();
        replaceMatches(false).catch(error);
      }
    });
  }
  const selected = getSelection()?.toString();
  const input = $("#find-query");
  if (selected && !selected.includes("\n")) input.value = selected;
  input.focus();
  input.select();
  runFind();
}
function runFind(move = 0) {
  const query = $("#find-query")?.value || "",
    exact = $("#find-case")?.checked;
  finder.matches = findMatches(note.blocks, query, exact);
  const count = finder.matches.length;
  finder.index = count ? (finder.index + move + count) % count : 0;
  $("#find-count").textContent = count
    ? `${finder.index + 1} of ${count}`
    : query
      ? "No matches"
      : "";
  highlightMatches(query, exact);
  const current = finder.matches[finder.index];
  if (current && move !== 0)
    $(`[data-block="${CSS.escape(current.id)}"]`)?.scrollIntoView({
      block: "center",
      behavior: "smooth",
    });
}
/** Highlights occurrences in rendered blocks; blocks with matches are marked too. */
function highlightMatches(query, exact) {
  const current = finder.matches[finder.index];
  const hits = new Set(finder.matches.map((m) => m.id));
  $$(".block").forEach((el) => {
    el.classList.toggle("find-hit", hits.has(el.dataset.block));
    el.classList.toggle("find-current", el.dataset.block === current?.id);
  });
  if (!window.CSS?.highlights || !window.Highlight) return;
  const all = [],
    focused = [];
  const needle = exact ? query : query.toLowerCase();
  if (needle)
    for (const el of $$(".block")) {
      const body = $(".block-body", el);
      if (!body || $("textarea", body)) continue;
      const walker = document.createTreeWalker(body, NodeFilter.SHOW_TEXT);
      const ordinal =
        current?.id === el.dataset.block
          ? finder.matches.filter((m) => m.id === current.id).indexOf(current)
          : -1;
      let seen = 0;
      for (let node = walker.nextNode(); node; node = walker.nextNode()) {
        const text = exact ? node.data : node.data.toLowerCase();
        for (
          let at = text.indexOf(needle);
          at !== -1;
          at = text.indexOf(needle, at + needle.length)
        ) {
          const range = new Range();
          range.setStart(node, at);
          range.setEnd(node, at + needle.length);
          (seen++ === ordinal ? focused : all).push(range);
        }
      }
    }
  CSS.highlights.set("find-match", new Highlight(...all));
  CSS.highlights.set("find-current", new Highlight(...focused));
}
async function replaceMatches(everything) {
  if (!editable() || !finder.matches.length) return;
  await flush();
  const query = $("#find-query").value,
    replacement = $("#find-replace").value,
    exact = $("#find-case").checked;
  live?.boundary();
  let replaced = 0;
  const targets = everything
    ? [...new Set(finder.matches.map((m) => m.id))]
    : [finder.matches[finder.index].id];
  for (const id of targets) {
    const i = note.blocks.findIndex((b) => b.id === id);
    const block = note.blocks[i];
    if (everything) {
      const result = replaceAll(block.markdown, query, replacement, exact);
      block.markdown = result.text;
      replaced += result.count;
    } else {
      const m = finder.matches[finder.index];
      block.markdown =
        block.markdown.slice(0, m.start) +
        replacement +
        block.markdown.slice(m.end);
      replaced = 1;
    }
    note.rendered[i] = await renderMarkdown(block.markdown);
  }
  markDirty();
  renderBlocks();
  queueSave().catch(() => {});
  announce(`Replaced ${replaced} match${replaced === 1 ? "" : "es"}`);
  runFind();
}
function closeFind() {
  $("#find-bar")?.remove();
  $$(".find-hit, .find-current").forEach((el) =>
    el.classList.remove("find-hit", "find-current"),
  );
  CSS.highlights?.delete("find-match");
  CSS.highlights?.delete("find-current");
  $("#find-note")?.focus();
}
function toggleFocus(force) {
  const on = document.body.classList.toggle("focus-mode", force);
  $("#focus-toggle")?.setAttribute("aria-pressed", String(on));
  announce(on ? "Focus mode on. Press Escape to leave." : "Focus mode off");
}
/** Nesting level of a block: two leading spaces per level, up to six (as on the server). */
function indentLevel(markdown) {
  const first = markdown.split("\n").find((line) => line.trim());
  return first
    ? Math.min(6, Math.floor((first.length - first.trimStart().length) / 2))
    : 0;
}
/** Re-indents every line of a block from one level to another. */
function reindent(markdown, from, to) {
  return markdown
    .split("\n")
    .map((line) => {
      if (to > from) return "  ".repeat(to - from) + line;
      const remove = Math.min(
        line.length - line.trimStart().length,
        (from - to) * 2,
      );
      return line.slice(remove);
    })
    .join("\n");
}
const LIST_MARKER = /^(\s*)(- \[[ xX]\] |[-*+] |(\d+)([.)]) )/;
/**
 * Empty blocks are plain space (people use them for spacing). Only a new note's
 * single empty block shows a hint, so there is somewhere obvious to start.
 */
function emptyBlockHTML() {
  return note?.blocks.length === 1
    ? '<span class="block-placeholder">Write something…</span>'
    : '<span class="block-empty" aria-hidden="true"></span>';
}
function blockHTML(b, i) {
  const position = `block ${i + 1} of ${note.blocks.length}`;
  const level = indentLevel(b.markdown);
  return `<div class="block${level ? ` indent-${level}` : ""}" role="listitem" data-block="${esc(b.id)}"${level ? ` aria-level="${level + 1}"` : ""}>${editable() ? '<div class="block-controls"><button class="block-handle" title="Drag to move · ↑/↓ to move with the keyboard" aria-label="Move block (drag, or press the up and down arrows)">⠿</button><button data-remove title="Delete block" aria-label="Delete block">×</button></div>' : ""}${/^\s*- \[[ xX]\]/.test(b.markdown) ? `<button class="task-toggle" aria-label="Toggle task" ${!editable() ? "disabled" : ""}>${/\[[xX]\]/.test(b.markdown) ? "☑" : "☐"}</button>` : ""}<div class="block-body" tabindex="0" ${editable() ? `role="button" aria-label="Edit ${position}"` : `role="group" aria-label="${position[0].toUpperCase() + position.slice(1)}"`}>${note.rendered?.[i] || emptyBlockHTML()}</div></div>`;
}
/** Moves the block at `i` to index `j` and saves. */
async function moveBlock(i, j) {
  if (i < 0 || j < 0 || j >= note.blocks.length || i === j) return;
  live?.boundary();
  const [block] = note.blocks.splice(i, 1),
    [html] = note.rendered.splice(i, 1);
  note.blocks.splice(j, 0, block);
  note.rendered.splice(j, 0, html);
  markDirty();
  renderBlocks();
  announce(`Block moved to position ${j + 1}`);
  await queueSave();
}
/** Drags a block by its handle; a line shows where it will land. */
function dragBlock(e, el, id) {
  if (e.button !== 0) return;
  e.preventDefault();
  const root = $("#blocks"),
    line = document.createElement("div");
  line.className = "block-drop-line";
  root.append(line);
  el.classList.add("dragging");
  document.body.classList.add("dragging-block");
  let target = null;
  const move = (ev) => {
    const others = $$(".block", root).filter((b) => b !== el);
    const after = others.find((b) => {
      const r = b.getBoundingClientRect();
      return ev.clientY < r.top + r.height / 2;
    });
    target = after ? after.dataset.block : "";
    const top = after
      ? after.offsetTop - 2
      : others.length
        ? others.at(-1).offsetTop + others.at(-1).offsetHeight
        : 0;
    line.style.top = `${top}px`;
  };
  const up = async () => {
    removeEventListener("pointermove", move);
    removeEventListener("pointerup", up);
    removeEventListener("pointercancel", up);
    line.remove();
    el.classList.remove("dragging");
    document.body.classList.remove("dragging-block");
    if (target === null) return;
    const i = note.blocks.findIndex((b) => b.id === id);
    let j = target
      ? note.blocks.findIndex((b) => b.id === target)
      : note.blocks.length;
    if (j > i) j -= 1;
    await moveBlock(i, j).catch(error);
  };
  addEventListener("pointermove", move);
  addEventListener("pointerup", up);
  addEventListener("pointercancel", up);
}
/**
 * Copying a selection across blocks puts the blocks' Markdown on the clipboard
 * as plain text, and the rendered text as HTML for rich editors.
 */
function copyBlocks(e) {
  const sel = getSelection();
  if (sel.isCollapsed || !sel.rangeCount) return;
  const range = sel.getRangeAt(0),
    root = $("#blocks");
  if (!root?.contains(range.commonAncestorContainer)) return;
  if (range.commonAncestorContainer.closest?.("textarea, input")) return;
  const ids = $$(".block", root)
    .filter((b) => range.intersectsNode(b))
    .map((b) => b.dataset.block);
  if (ids.length < 2) return; // inside one block, the browser's copy is right
  const html = document.createElement("div");
  html.append(range.cloneContents());
  $$(".block-controls, .task-toggle", html).forEach((n) => n.remove());
  $$(".block", html).forEach((b) =>
    b.replaceWith(...($(".block-body", b) || b).childNodes),
  );
  e.clipboardData.setData(
    "text/plain",
    note.blocks
      .filter((b) => ids.includes(b.id))
      .map((b) => b.markdown)
      .join("\n\n"),
  );
  e.clipboardData.setData("text/html", html.innerHTML);
  e.preventDefault();
}
document.addEventListener("copy", (e) => note && copyBlocks(e));
// Block handles stay out of the way while text is selected.
document.addEventListener("selectionchange", () =>
  document.body.classList.toggle("selecting-text", !getSelection().isCollapsed),
);
/**
 * Files dropped on the note are uploaded; an image dragged from a web page, or
 * a link to an image or audio file, is shown. A drop on the block being edited
 * goes at its cursor, anywhere else in a new block after the one below it.
 */
function bindMediaDrop(root) {
  const media = (dt) => {
    const html = dt.getData("text/html");
    const src =
      html &&
      new DOMParser().parseFromString(html, "text/html").querySelector("img")
        ?.src;
    if (src && /^https?:/.test(src)) {
      const md = mediaMarkdown(src);
      return md || `![](${src.replace(/[()\s]/g, encodeURIComponent)})`;
    }
    const uri = (dt.getData("text/uri-list") || dt.getData("text/plain"))
      .split("\n")
      .find((l) => l.trim() && !l.startsWith("#"));
    return uri ? mediaMarkdown(uri) : null;
  };
  const wanted = (dt) =>
    dt.types.includes("Files") ||
    dt.types.includes("text/uri-list") ||
    dt.types.includes("text/html");
  root.ondragover = (e) => {
    if (!editable() || !wanted(e.dataTransfer)) return;
    e.preventDefault();
    e.dataTransfer.dropEffect = "copy";
    root.classList.add("media-drop");
  };
  root.ondragleave = (e) => {
    if (!root.contains(e.relatedTarget)) root.classList.remove("media-drop");
  };
  root.ondrop = (e) => {
    root.classList.remove("media-drop");
    if (!editable()) return;
    const after = e.target.closest?.("[data-block]")?.dataset.block || null;
    const files = [...e.dataTransfer.files];
    if (files.length) {
      e.preventDefault();
      uploadAttachments(files, after).catch(error);
      return;
    }
    const md = media(e.dataTransfer);
    if (!md) return;
    e.preventDefault();
    const ta = e.target.closest?.("textarea");
    if (ta) {
      const at = ta.selectionEnd;
      replaceRange(ta, {
        from: at,
        to: at,
        text: md,
        selStart: at + md.length,
        selEnd: at + md.length,
      });
    } else insertBlockAfter(md, after).catch(error);
  };
}
function renderBlocks() {
  setTimeout(() => $("#find-bar") && runFind(), 0);
  const root = $("#blocks");
  bindMediaDrop(root);
  root.innerHTML = note.blocks
    .map((b, i) =>
      mode === "document"
        ? blockHTML(b, i)
        : `<div class="canvas-card" data-card="${esc(b.id)}" style="left:${b.x}px;top:${b.y}px"><div class="canvas-handle">⠿ &nbsp; Block ${i + 1} · drag to arrange</div>${blockHTML(b, i)}</div>`,
    )
    .join("");
  $$(".block", root).forEach((el) => {
    const id = el.dataset.block;
    action($(".task-toggle", el), "click", async () => {
      const i = note.blocks.findIndex((b) => b.id === id),
        b = note.blocks[i];
      live?.boundary();
      b.markdown = b.markdown.replace(/\[([ xX])\]/, (_, v) =>
        v === " " ? "[x]" : "[ ]",
      );
      note.rendered[i] = await renderMarkdown(b.markdown);
      markDirty();
      renderBlocks();
      await queueSave();
    });
    action($(".block-body", el), "click", (e) => {
      if (
        e.target.closest(
          "a, .tag, .journal-tasks button, .db-embed, .youtube-card, audio",
        )
      )
        return;
      // Selecting text to copy it must not open the block for editing.
      if (!getSelection().isCollapsed) return;
      if (editable()) editBlock(id);
    });
    action($(".block-body", el), "keydown", (e) => {
      if (e.target !== e.currentTarget) return;
      if (e.key === "Enter" && editable()) {
        e.preventDefault();
        editBlock(id);
      }
      // Arrow keys move between blocks without entering edit mode.
      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
        const sibling =
          e.key === "ArrowDown"
            ? el.nextElementSibling
            : el.previousElementSibling;
        if (sibling) {
          e.preventDefault();
          $(".block-body", sibling)?.focus();
        }
      }
    });
    const handle = $(".block-handle", el);
    action(handle, "keydown", async (e) => {
      if (e.key !== "ArrowUp" && e.key !== "ArrowDown") return;
      e.preventDefault();
      const i = note.blocks.findIndex((b) => b.id === id);
      await moveBlock(i, i + (e.key === "ArrowUp" ? -1 : 1));
      $(`[data-block="${CSS.escape(id)}"] .block-handle`)?.focus();
    });
    handle?.addEventListener("pointerdown", (e) => dragBlock(e, el, id));
    action($("[data-remove]", el), "click", async () => {
      const i = note.blocks.findIndex((b) => b.id === id);
      live?.boundary();
      note.blocks.splice(i, 1);
      note.rendered.splice(i, 1);
      markDirty();
      renderBlocks();
      const neighbour = $$(".block-body")[Math.min(i, note.blocks.length - 1)];
      (neighbour || $("#add-block"))?.focus();
      announce("Block deleted. Press Ctrl+Z to undo.");
      await queueSave();
    });
  });
  if (mode === "canvas" && editable())
    $$(".canvas-card").forEach((card) => {
      const handle = $(".canvas-handle", card);
      handle.onpointerdown = (e) => {
        if (e.button !== 0) return;
        const b = note.blocks.find((b) => b.id === card.dataset.card),
          sx = e.clientX,
          sy = e.clientY,
          x = b.x,
          y = b.y;
        handle.setPointerCapture(e.pointerId);
        handle.onpointermove = (p) => {
          b.x = Math.max(0, Math.min(1900, x + p.clientX - sx));
          b.y = Math.max(0, Math.min(2000, y + p.clientY - sy));
          card.style.left = b.x + "px";
          card.style.top = b.y + "px";
        };
        handle.onpointerup = () => {
          handle.onpointermove = null;
          markDirty();
          queueSave().catch(() => {});
        };
      };
    });
}
/** True when `position` lies inside an unclosed code fence or `$$` math block. */
function insideCodeFence(text, position) {
  const lines = text.slice(0, position).split("\n");
  const fences = lines.filter((line) => /^\s*(```|~~~)/.test(line)).length;
  const math = lines.filter((line) => line.trim() === "$$").length;
  return fences % 2 === 1 || math % 2 === 1;
}
/** Replaces a range keeping the field's native undo history where supported. */
function replaceRange(ta, change) {
  ta.focus();
  ta.setSelectionRange(change.from, change.to);
  if (!document.execCommand("insertText", false, change.text)) {
    ta.setRangeText(change.text, change.from, change.to);
    ta.dispatchEvent(new Event("input"));
  }
  ta.setSelectionRange(change.selStart, change.selEnd);
}
const FORMATS = {
  bold: ["**", "Bold", "Ctrl+B"],
  italic: ["*", "Italic", "Ctrl+I"],
  strike: ["~~", "Strikethrough", "Ctrl+Shift+X"],
  code: ["`", "Inline code", "Ctrl+E"],
  highlight: ["==", "Highlight", "Ctrl+Shift+H"],
};
function applyFormat(ta, format) {
  const change =
    format === "link"
      ? linkSelection(ta.value, ta.selectionStart, ta.selectionEnd)
      : toggleWrap(
          ta.value,
          ta.selectionStart,
          ta.selectionEnd,
          FORMATS[format][0],
        );
  replaceRange(ta, change);
}
/** Floating toolbar shown above a block while text in it is selected. */
function updateFormatBar(ta) {
  const block = ta.closest(".block");
  let bar = $(".format-bar", block);
  if (ta.selectionStart === ta.selectionEnd || document.activeElement !== ta) {
    bar?.remove();
    return;
  }
  if (bar) return;
  bar = document.createElement("div");
  bar.className = "format-bar";
  bar.setAttribute("role", "toolbar");
  bar.setAttribute("aria-label", "Format selection");
  bar.innerHTML = `<button data-format="bold" title="Bold (Ctrl+B)"><b>B</b></button><button data-format="italic" title="Italic (Ctrl+I)"><i>I</i></button><button data-format="strike" title="Strikethrough (Ctrl+Shift+X)"><s>S</s></button><button data-format="code" title="Inline code (Ctrl+E)"><code>&lt;/&gt;</code></button><button data-format="link" title="Link (Ctrl+K)">🔗</button><button data-format="highlight" class="format-highlight" title="Highlight (Ctrl+Shift+H)">🖍</button>`;
  $$("button", bar).forEach((b) => {
    b.setAttribute("aria-label", b.title);
    // Keep the textarea focused and its selection intact.
    b.addEventListener("mousedown", (e) => e.preventDefault());
    b.addEventListener("click", () => {
      applyFormat(ta, b.dataset.format);
      updateFormatBar(ta);
    });
  });
  block.append(bar);
}
let linkPicker = null;
// The people who can open each note, and names by email, for `@` mentions.
const notePeople = new Map();
const peopleNames = new Map();
async function loadPeople(id = note?.id) {
  if (!id || shareMode) return [];
  if (!notePeople.has(id))
    notePeople.set(
      id,
      api(`notes/${id}/people`).then((people) => {
        for (const p of people) peopleNames.set(p.email, p.name);
        nameMentions(document);
        return people;
      }),
    );
  return notePeople.get(id).catch(() => (notePeople.delete(id), []));
}
/** Mention chips show the person's name; the address stays in the tooltip. */
function nameMentions(root) {
  $$('.mention[title^="Assigned to "]', root).forEach((chip) => {
    const email = chip.title.slice("Assigned to ".length);
    const name = peopleNames.get(email);
    if (name && chip.textContent !== "@" + name) {
      chip.textContent = "@" + name;
      chip.title = email;
      chip.dataset.email = email;
    }
  });
}
if (typeof MutationObserver !== "undefined")
  new MutationObserver((changes) => {
    if (!peopleNames.size) return;
    for (const change of changes)
      for (const node of change.addedNodes)
        if (node.nodeType === 1) nameMentions(node.parentNode || node);
  }).observe(document.documentElement, { childList: true, subtree: true });
/** `@` autocomplete: pick someone who can open the note and mention them. */
async function updateMentionPicker(ta, query) {
  const people = await loadPeople();
  if (mentionQuery(ta.value, ta.selectionStart)?.start !== query.start) return;
  const needle = query.query.toLowerCase();
  const items = people
    .filter((p) => !p.me)
    .filter(
      (p) =>
        !needle ||
        p.name.toLowerCase().includes(needle) ||
        p.email.includes(needle),
    )
    .slice(0, 8)
    .map((p) => ({ ...p, icon: "@", title: p.name }));
  showPicker(ta, query, items, "Mention someone");
}
function showPicker(ta, query, items, label) {
  const block = ta.closest(".block");
  let list = $(".link-picker", block);
  if (!list) {
    list = document.createElement("div");
    list.className = "link-picker";
    list.setAttribute("role", "listbox");
    block.append(list);
  }
  list.setAttribute("aria-label", label);
  linkPicker = { ta, query, items, selected: 0, list };
  drawLinkPicker();
}
/** `[[` autocomplete: pick a note and insert a link that survives renames. */
async function updateLinkPicker(ta) {
  const mention = !shareMode && mentionQuery(ta.value, ta.selectionStart);
  if (mention) return updateMentionPicker(ta, mention);
  const query = linkQuery(ta.value, ta.selectionStart);
  if (!query || shareMode) return closeLinkPicker();
  const results = query.query.trim()
    ? await api("search", "POST", { query: query.query.trim() })
    : [...workspaceNotes(), ...(data.shared_notes || [])]
        .filter((n) => !n.trashed)
        .sort((a, b) => b.updated_at - a.updated_at);
  if (linkQuery(ta.value, ta.selectionStart)?.start !== query.start) return;
  const items = results.filter((n) => n.id !== note.id).slice(0, 8);
  if (
    /^(u|un|unf|unc|tas|todo|jour|open)/i.test(query.query.trim()) ||
    !query.query.trim()
  )
    items.unshift({
      special: "[[uncompleted_journal_tasks]]",
      icon: "☑",
      title: "Unfinished journal tasks (live list)",
    });
  showPicker(ta, query, items, "Link to a note");
}
function drawLinkPicker() {
  const { list, items, selected } = linkPicker;
  list.innerHTML =
    items
      .map(
        (n, i) =>
          `<button role="option" aria-selected="${i === selected}" class="${i === selected ? "selected" : ""}" data-pick="${i}">${n.email ? `<span class="mention-avatar">${esc(n.name.slice(0, 1).toUpperCase())}</span>${esc(n.name)} <small>${esc(n.email)}</small>` : `${esc(n.icon)} ${esc(n.title)}`}</button>`,
      )
      .join("") ||
    `<p class="muted">${linkPicker.list.getAttribute("aria-label") === "Mention someone" ? "Nobody else can open this note yet. Share it, or its workspace, first." : "No matching notes"}</p>`;
  $$("[data-pick]", list).forEach((b) => {
    b.addEventListener("mousedown", (e) => e.preventDefault());
    b.addEventListener("click", () => pickLink(Number(b.dataset.pick)));
  });
}
function pickLink(index) {
  const { ta, query, items } = linkPicker;
  const target = items[index];
  if (!target) return;
  const text = target.email
    ? `@${target.email} `
    : target.special || noteLink(target.title, target.id);
  replaceRange(ta, {
    from: query.start,
    to: ta.selectionStart,
    text,
    selStart: query.start + text.length,
    selEnd: query.start + text.length,
  });
  closeLinkPicker();
  announce(
    target.email ? `Mentioned ${target.name}` : `Linked to ${target.title}`,
  );
}
function closeLinkPicker() {
  linkPicker?.list.remove();
  linkPicker = null;
}
function editBlock(id, caret = null) {
  const el = $(`[data-block="${CSS.escape(id)}"]`);
  if (!el || $("textarea", el)) return;
  const b = note.blocks.find((b) => b.id === id);
  const body = $(".block-body", el);
  const index = () => note.blocks.findIndex((b) => b.id === id);
  body.innerHTML = `<textarea aria-label="Markdown for block ${index() + 1} of ${note.blocks.length}" aria-describedby="editor-help" spellcheck="true"></textarea>`;
  const ta = $("textarea", body);
  ta.dataset.liveKey = "block:" + id;
  ta.value = b.markdown;
  const initial = ta.value;
  ta.rows = 1;
  ta.style.height = ta.scrollHeight + "px";
  ta.focus();
  const at = caret ?? ta.value.length;
  ta.setSelectionRange(at, at);
  let closed = false,
    composition = null;
  // Renders the block and runs `then` with its index (for splits).
  async function close(then) {
    if (closed) return;
    closed = true;
    b.markdown = ta.value;
    const i = index();
    if (i < 0) {
      renderBlocks();
      return;
    }
    markDirty();
    let rendered;
    try {
      rendered = await renderMarkdown(b.markdown);
    } catch (e) {
      closed = false;
      error(e);
      return;
    }
    note.rendered[i] = rendered;
    body.innerHTML = rendered || emptyBlockHTML();
    if (then) await then(i);
    markDirty();
    queueSave().catch(() => {});
  }
  function split(start, end) {
    const marker = ta.value.match(LIST_MARKER);
    // Enter on an empty list item steps out: one level up, or out of the list.
    if (marker && !ta.value.slice(marker[0].length).trim() && start === end) {
      const level = indentLevel(ta.value);
      setValue(level ? reindent(ta.value, level, level - 1) : "");
      announce(level ? `Moved out to level ${level}` : "List ended");
      return;
    }
    // The new block keeps the indentation and continues the list.
    const level = indentLevel(ta.value);
    let prefix = "  ".repeat(level);
    if (marker && start >= marker[0].length) {
      const kind = marker[2];
      prefix += kind.startsWith("- [")
        ? "- [ ] "
        : marker[3]
          ? `${Number(marker[3]) + 1}${marker[4]} `
          : kind;
    }
    const rest = ta.value.slice(end);
    const after = prefix + (prefix ? rest.replace(/^ +/, "") : rest);
    live?.boundary();
    ta.value = ta.value.slice(0, start);
    closing = close(async (i) => {
      const fresh = newBlock(after);
      note.blocks.splice(i + 1, 0, fresh);
      note.rendered.splice(i + 1, 0, renderedCache.get(after) ?? "");
      markDirty();
      renderBlocks();
      editBlock(fresh.id, prefix.length);
      if (rest) announce("Block split");
    });
  }
  /** Replaces the whole field, keeping native undo, and the caret at `caret`. */
  function setValue(value, caret = null) {
    const at = caret ?? value.length;
    replaceRange(ta, {
      from: 0,
      to: ta.value.length,
      text: value,
      selStart: at,
      selEnd: at,
    });
  }
  /** Whether `offset` is where the block's text begins (after indentation and any list marker). */
  function atTextStart(offset) {
    const marker = ta.value.match(LIST_MARKER);
    if (marker) return offset === marker[0].length;
    return !ta.value.slice(0, offset).trim() && indentLevel(ta.value) > 0;
  }
  /** Tab / Shift+Tab: nest under the block above (at most one level deeper). */
  function indent(delta) {
    const i = index(),
      level = indentLevel(ta.value),
      deepest =
        i > 0 ? Math.min(6, indentLevel(note.blocks[i - 1].markdown) + 1) : 0,
      target =
        delta < 0 ? Math.max(0, level - 1) : Math.min(level + 1, deepest);
    if (target <= level && delta > 0) {
      announce("Already nested as deep as allowed here");
      return;
    }
    if (target === level) {
      announce(
        delta > 0 ? "Already nested as deep as allowed here" : "Not indented",
      );
      return;
    }
    const start = ta.selectionStart,
      end = ta.selectionEnd,
      shift = (target - level) * 2;
    const linesBefore = (offset) =>
      ta.value.slice(0, offset).split("\n").length;
    const value = reindent(ta.value, level, target);
    replaceRange(ta, {
      from: 0,
      to: ta.value.length,
      text: value,
      selStart: Math.max(0, start + shift * linesBefore(start)),
      selEnd: Math.max(0, end + shift * linesBefore(end)),
    });
    announce(
      target > level
        ? `Indented to level ${target}`
        : target
          ? `Moved out to level ${target}`
          : "Not indented",
    );
  }
  /** Tab inside a code block inserts or removes two spaces. */
  function codeTab(outdent) {
    const start = ta.selectionStart,
      lineStart = ta.value.lastIndexOf("\n", start - 1) + 1;
    if (!outdent)
      return replaceRange(ta, {
        from: start,
        to: ta.selectionEnd,
        text: "  ",
        selStart: start + 2,
        selEnd: start + 2,
      });
    const remove = Math.min(
      2,
      ta.value.slice(lineStart).length -
        ta.value.slice(lineStart).trimStart().length,
    );
    if (remove)
      replaceRange(ta, {
        from: lineStart,
        to: lineStart + remove,
        text: "",
        selStart: start - remove,
        selEnd: start - remove,
      });
  }
  function mergeIntoPrevious() {
    const i = index(),
      previous = note.blocks[i - 1];
    live?.boundary();
    closed = true;
    const join = previous.markdown.length;
    previous.markdown += ta.value;
    note.blocks.splice(i, 1);
    note.rendered.splice(i, 1);
    markDirty();
    renderBlocks();
    editBlock(previous.id, join);
    announce("Merged with the previous block");
  }
  function mergeNext() {
    const i = index(),
      next = note.blocks[i + 1];
    live?.boundary();
    const join = ta.value.length;
    ta.value += next.markdown;
    b.markdown = ta.value;
    note.blocks.splice(i + 1, 1);
    note.rendered.splice(i + 1, 1);
    $(`[data-block="${CSS.escape(next.id)}"]`)?.remove();
    ta.setSelectionRange(join, join);
    ta.oninput();
    announce("Merged with the next block");
  }
  function moveTo(offset, toEnd) {
    const target = note.blocks[index() + offset];
    if (!target) return false;
    closing = close(() => editBlock(target.id, toEnd ? null : 0));
    return true;
  }
  ta.oninput = () => {
    ta.style.height = "auto";
    ta.style.height = ta.scrollHeight + "px";
    const level = indentLevel(ta.value);
    for (let n = 1; n <= 6; n++)
      el.classList.toggle(`indent-${n}`, n === level);
    if (composition) return;
    b.markdown = ta.value;
    markDirty();
    updateLinkPicker(ta).catch(() => closeLinkPicker());
  };
  for (const event of ["select", "keyup", "mouseup"])
    ta.addEventListener(event, () => updateFormatBar(ta));
  ta.addEventListener("compositionstart", () => {
    ta.dataset.composing = "1";
    composition = live?.beginComposition(
      ta.dataset.liveKey,
      ta.selectionStart,
      ta.selectionEnd,
      ta.value,
    ) || { local: true };
  });
  ta.addEventListener("compositionend", () => {
    delete ta.dataset.composing;
    const state = composition;
    composition = null;
    if (live && !state.local) {
      const merged = live.endComposition(state, ta.value);
      ta.value = merged.value;
      ta.setSelectionRange(merged.caret, merged.caret);
      b.markdown = merged.value;
      dirty = true;
      live.sync().catch(() => {});
    } else ta.oninput();
  });
  ta.onblur = () => {
    if (composition) return;
    $(".format-bar", el)?.remove();
    closeLinkPicker();
    closing = close();
  };
  ta.onkeydown = (e) => {
    // Input methods use Enter, arrows and Backspace while composing.
    if (e.isComposing || e.keyCode === 229 || composition) return;
    if (linkPicker?.ta === ta) {
      const count = linkPicker.items.length;
      if ((e.key === "ArrowDown" || e.key === "ArrowUp") && count) {
        e.preventDefault();
        linkPicker.selected =
          (linkPicker.selected + (e.key === "ArrowDown" ? 1 : count - 1)) %
          count;
        drawLinkPicker();
        return;
      }
      if ((e.key === "Enter" || e.key === "Tab") && count) {
        e.preventDefault();
        pickLink(linkPicker.selected);
        return;
      }
      if (e.key === "Escape") {
        e.preventDefault();
        closeLinkPicker();
        return;
      }
    }
    const shortcut =
      (e.ctrlKey || e.metaKey) && !e.altKey && e.key.toLowerCase();
    const format = {
      b: !e.shiftKey && "bold",
      i: !e.shiftKey && "italic",
      e: !e.shiftKey && "code",
      k: !e.shiftKey && "link",
      h: e.shiftKey && "highlight",
      x: e.shiftKey && "strike",
    }[shortcut];
    if (format) {
      e.preventDefault();
      applyFormat(ta, format);
      return;
    }
    const start = ta.selectionStart,
      end = ta.selectionEnd,
      collapsed = start === end,
      modifier = e.ctrlKey || e.metaKey || e.altKey;
    if (e.key === "Tab" && !modifier) {
      e.preventDefault();
      if (insideCodeFence(ta.value, start)) codeTab(e.shiftKey);
      else indent(e.shiftKey ? -1 : 1);
    } else if (e.key === "Enter" && !e.shiftKey && !modifier) {
      if (insideCodeFence(ta.value, start)) return;
      e.preventDefault();
      split(start, end);
    } else if (
      e.key === "Backspace" &&
      collapsed &&
      !modifier &&
      start > 0 &&
      atTextStart(start)
    ) {
      // At the start of an item's text: move out a level, then drop the marker.
      e.preventDefault();
      const marker = ta.value.match(LIST_MARKER);
      if (indentLevel(ta.value) > 0) indent(-1);
      else if (marker) {
        setValue(ta.value.slice(marker[0].length), 0);
        announce("Changed to text");
      }
    } else if (
      e.key === "Backspace" &&
      collapsed &&
      start === 0 &&
      !modifier &&
      index() > 0
    ) {
      e.preventDefault();
      mergeIntoPrevious();
    } else if (
      e.key === "Delete" &&
      collapsed &&
      start === ta.value.length &&
      !modifier &&
      index() < note.blocks.length - 1
    ) {
      e.preventDefault();
      mergeNext();
    } else if (
      e.key === "ArrowUp" &&
      collapsed &&
      !modifier &&
      !e.shiftKey &&
      !ta.value.slice(0, start).includes("\n")
    ) {
      if (moveTo(-1, true)) e.preventDefault();
    } else if (
      e.key === "ArrowDown" &&
      collapsed &&
      !modifier &&
      !e.shiftKey &&
      !ta.value.slice(end).includes("\n")
    ) {
      if (moveTo(1, false)) e.preventDefault();
    } else if (
      (e.ctrlKey || e.metaKey) &&
      e.key.toLowerCase() === "z" &&
      !e.shiftKey &&
      ta.value === initial
    ) {
      // Nothing to undo inside this field: undo the last note change instead.
      e.preventDefault();
      ta.blur();
      closing.then(() => undoRedo(false)).catch(error);
    } else if (e.key === "Escape") {
      e.preventDefault();
      ta.blur();
      body.focus();
    } else if (
      (e.ctrlKey || e.metaKey) &&
      e.shiftKey &&
      e.key.toLowerCase() === "p"
    ) {
      e.preventDefault();
      const selectedText = ta.value.slice(ta.selectionStart, ta.selectionEnd);
      ta.blur();
      closing.then(() => blockMenu(id, selectedText)).catch(error);
    } else if (e.key === "/" && !ta.value) {
      e.preventDefault();
      ta.blur();
      blockMenu(id);
    }
  };
  ta.onpaste = (e) => {
    const files = [...e.clipboardData.files];
    if (files.length) {
      e.preventDefault();
      ta.blur();
      uploadAttachments(files).catch(error);
      return;
    }
    const html = e.clipboardData.getData("text/html");
    let text = e.clipboardData.getData("text/plain");
    // A link to an image or an audio file shows it.
    const media =
      !insideCodeFence(ta.value, ta.selectionStart) && mediaMarkdown(text);
    if (media) {
      e.preventDefault();
      replaceRange(ta, {
        from: ta.selectionStart,
        to: ta.selectionEnd,
        text: media,
        selStart: ta.selectionStart + media.length,
        selEnd: ta.selectionStart + media.length,
      });
      return;
    }
    // Formatted content from web pages, Google Docs or Word becomes Markdown.
    const fromHTML = html && !insideCodeFence(ta.value, ta.selectionStart);
    if (fromHTML) text = htmlToMarkdown(html) || text;
    if (!text.includes("\n")) {
      if (!fromHTML) return;
      e.preventDefault();
      replaceRange(ta, {
        from: ta.selectionStart,
        to: ta.selectionEnd,
        text,
        selStart: ta.selectionStart + text.length,
        selEnd: ta.selectionStart + text.length,
      });
      return;
    }
    e.preventDefault();
    live?.boundary();
    const merged =
      ta.value.slice(0, ta.selectionStart) +
      text +
      ta.value.slice(ta.selectionEnd);
    const lines = splitMarkdown(merged).filter(
      (line, n) => !fromHTML || n === 0 || line.trim(),
    );
    ta.value = lines.shift();
    const i = index();
    const extra = lines.map(newBlock);
    note.blocks.splice(i + 1, 0, ...extra);
    note.rendered.splice(i + 1, 0, ...extra.map(() => ""));
    closing = close()
      .then(async () => {
        for (const item of extra)
          note.rendered[note.blocks.indexOf(item)] = await renderMarkdown(
            item.markdown,
          );
        renderBlocks();
        announce(`Pasted ${extra.length + 1} blocks`);
      })
      .catch(error);
  };
}
function newBlock(markdown = "", index = note?.blocks.length || 0) {
  return {
    id: crypto.randomUUID(),
    markdown,
    x: 40 + (index % 3) * 310,
    y: 40 + Math.floor(index / 3) * 180,
  };
}
/** Adds `text` at the end of a block (a new one if needed) and keeps editing it. */
async function appendToBlock(existingId, text) {
  let b = note.blocks.find((b) => b.id === existingId);
  live?.boundary();
  if (!b) {
    b = newBlock("");
    note.blocks.push(b);
    note.rendered.push("");
  }
  b.markdown = `${b.markdown.trimEnd()} ${text} `.trimStart();
  note.rendered[note.blocks.indexOf(b)] = await renderMarkdown(b.markdown);
  markDirty();
  renderBlocks();
  editBlock(b.id);
  return b;
}
/**
 * `/record`: records from the microphone and saves the recording as a file of
 * the note, shown with a player after block `afterId`.
 */
async function recordDialog(afterId) {
  if (!navigator.mediaDevices?.getUserMedia || !window.MediaRecorder)
    return toast("This browser cannot record audio.");
  const type = ["audio/webm;codecs=opus", "audio/webm", "audio/mp4"].find((t) =>
    MediaRecorder.isTypeSupported(t),
  );
  if (!type) return toast("This browser cannot record audio.");
  const targetId = note.id;
  let stream, recorder, started, timer, blob;
  let chunks = [],
    size = 0;
  modal.innerHTML = `<form method="dialog" id="record-form"><h2>Record audio</h2><p>The recording is saved with this note, with a player. Up to about 40 minutes.</p><div class="recorder"><button type="button" class="btn primary" id="record-toggle">● Start recording</button><span class="record-time" aria-live="polite">0:00</span></div><audio class="audio-player" id="record-preview" controls hidden></audio><div class="form-error" role="alert"></div><footer><button class="btn" value="cancel" formnovalidate>Cancel</button><button class="btn primary" value="ok" id="record-save" disabled>Save to note</button></footer></form>`;
  modal.showModal();
  const form = $("#record-form"),
    toggle = $("#record-toggle"),
    clock = $(".record-time", form),
    preview = $("#record-preview");
  const stop = () => {
    clearInterval(timer);
    if (recorder?.state === "recording") recorder.stop();
    stream?.getTracks().forEach((t) => t.stop());
  };
  modal.addEventListener("close", stop, { once: true });
  toggle.onclick = async () => {
    if (recorder?.state === "recording") return stop();
    try {
      stream = await navigator.mediaDevices.getUserMedia({ audio: true });
    } catch {
      $(".form-error", form).textContent =
        "Minerva cannot use the microphone. Allow it in your browser's site settings.";
      return;
    }
    chunks = [];
    size = 0;
    recorder = new MediaRecorder(stream, { mimeType: type });
    recorder.ondataavailable = (e) => {
      chunks.push(e.data);
      size += e.data.size;
      // Files hold up to 10 MiB.
      if (size > 9.5 * 1024 * 1024) stop();
    };
    recorder.onstop = () => {
      blob = new Blob(chunks, { type: type.split(";")[0] });
      preview.src = URL.createObjectURL(blob);
      preview.hidden = false;
      toggle.textContent = "● Record again";
      $("#record-save").disabled = false;
    };
    recorder.start(1000);
    started = Date.now();
    toggle.textContent = "■ Stop";
    $("#record-save").disabled = true;
    preview.hidden = true;
    timer = setInterval(() => {
      const s = Math.floor((Date.now() - started) / 1000);
      clock.textContent = `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
    }, 500);
  };
  form.onsubmit = async (e) => {
    if (e.submitter?.value !== "ok" || !blob) return;
    e.preventDefault();
    const stamp = new Date()
      .toISOString()
      .slice(0, 16)
      .replace("T", " ")
      .replace(":", "-");
    const file = new File(
      [blob],
      `Recording ${stamp}.${type.includes("mp4") ? "m4a" : "webm"}`,
      { type: blob.type },
    );
    modal.close();
    if (note?.id !== targetId) return;
    await flush();
    toast("Saving the recording…");
    await insertAttachment(await uploadFile(file, targetId), targetId, afterId);
    toast("Recording saved");
  };
}
/** `/assigntouser`: `@person@example.com` in the block; they get an email. */
async function assignDialog(existingId) {
  await appendToBlock(existingId, "@");
  const ta = $(".block textarea");
  if (!ta) return;
  ta.value = ta.value.replace(/ $/, "");
  ta.setSelectionRange(ta.value.length, ta.value.length);
  ta.dispatchEvent(new Event("input", { bubbles: true }));
  await updateLinkPicker(ta);
}
/** `/remindme`: pick a date and time; the block is emailed to you then. */
function reminderDialog(existingId) {
  const soon = new Date(Date.now() + 3600e3);
  soon.setMinutes(0, 0, 0);
  const local = (d) =>
    new Date(d.getTime() - d.getTimezoneOffset() * 60e3)
      .toISOString()
      .slice(0, 16);
  modal.innerHTML = `<form method="dialog" id="remind-form"><h2>Remind me</h2><p>At this time you get an email with this block and a link to the note.</p><label>When<input name="at" type="datetime-local" required min="${local(new Date())}" value="${local(soon)}"></label><footer><button class="btn" value="cancel" formnovalidate>Cancel</button><button class="btn primary" value="ok">Set reminder</button></footer></form>`;
  modal.showModal();
  const form = $("#remind-form");
  form.onsubmit = async (e) => {
    if (e.submitter?.value !== "ok") return;
    e.preventDefault();
    const when = new Date(form.at.value);
    if (!(when > new Date())) return toast("Choose a time in the future.");
    modal.close();
    const b = await appendToBlock(
      existingId,
      `⏰${form.at.value.replace("T", " ")}`,
    );
    await queueSave();
    await api("reminders", "POST", {
      note_id: note.id,
      block_id: b.id,
      at: Math.floor(when.getTime() / 1000),
    });
    toast(`Reminder set for ${when.toLocaleString()}`);
  };
}
function blockMenu(existingId, capturedSelection = "") {
  if (!editable()) return;
  modal.innerHTML =
    '<h2>Insert a block</h2><input id="command-search" aria-label="Search block commands" placeholder="Search headings, code, files…" autocomplete="off"><div id="command-results"></div><footer><button class="btn" id="close-modal">Close</button></footer>';
  modal.showModal();
  $("#close-modal").onclick = () => modal.close();
  const input = $("#command-search");
  const targetNote = note.id;
  // Reminders arrive by email, so they need a server that sends it.
  let commands = blockCommands.filter(
    (c) => c[1] !== "@remind" || (mailEnabled && !shareMode),
  );
  if (!shareMode)
    api("server-plugins/capabilities")
      .then((result) => {
        if (!modal.open || $("#command-search") !== input) return;
        commands.push(
          ...result.commands.map((c) => [
            `/${c.id} · ${c.name}${c.available ? "" : ` (${c.reason || "configuration required"})`}`,
            "@plugin",
            c.name,
            c,
          ]),
        );
        draw(true);
      })
      .catch(() => {});
  let selected = 0;
  let matches = [];
  async function choose(index) {
    const command = matches[index];
    if (!command || (command[3] && !command[3].available)) return;
    modal.close();
    if (command[1] === "@plugin") {
      if (note?.id !== targetNote) return;
      await flush();
      if (note?.id !== targetNote) return;
      let inserted = false;
      const block = note.blocks.find(
        (b) => b.id === existingId && b.markdown.trim(),
      );
      return pluginDialog({
        command: command[3],
        context: {
          noteId: targetNote,
          blockId: block?.id,
          selection: capturedSelection,
        },
        api,
        esc,
        modal,
        upload: uploadFile,
        insert: async (sections, noteId, afterId) => {
          if (inserted) return flush();
          if (note?.id !== noteId || !editable())
            throw new Error(
              "Reopen the editable source note before inserting.",
            );
          const rendered = await Promise.all(sections.map(renderMarkdown));
          if (note?.id !== noteId || !editable())
            throw new Error("The active note changed.");
          live?.boundary();
          const index = note.blocks.findIndex((b) => b.id === afterId);
          const at = index < 0 ? note.blocks.length : index + 1;
          note.blocks.splice(at, 0, ...sections.map((text) => newBlock(text)));
          note.rendered.splice(at, 0, ...rendered);
          markDirty();
          inserted = true;
          live?.boundary();
          renderBlocks();
          await flush();
          announce("Inserted generated knowledge");
        },
      });
    }
    if (command[1] === null) return attachmentDialog();
    if (command[1] === "@database") return databaseEmbedDialog(existingId);
    if (command[1] === "@assign") return assignDialog(existingId);
    if (command[1] === "@record") return recordDialog(existingId);
    if (command[1] === "@remind") return reminderDialog(existingId);
    let b = note.blocks.find((b) => b.id === existingId);
    if (!b) {
      b = newBlock(command[1]);
      note.blocks.push(b);
      note.rendered.push("");
    } else b.markdown = command[1];
    markDirty();
    renderBlocks();
    editBlock(b.id);
    if (command[1] === "[[")
      updateLinkPicker($(".block textarea")).catch(() => closeLinkPicker());
  }
  function draw(reset = false) {
    matches = commands.filter((c) =>
      `${c[0]} ${c[2]}`
        .toLowerCase()
        .includes(input.value.trim().toLowerCase()),
    );
    if (reset) selected = 0;
    $("#command-results").innerHTML =
      matches
        .map(
          (c, i) =>
            `<button class="search-result ${i === selected ? "command-selected" : ""}" data-command="${i}" ${c[3] && !c[3].available ? "disabled" : ""}>${esc(c[0])}</button>`,
        )
        .join("") || "<p>No matching commands</p>";
    $$("[data-command]", modal).forEach((button) =>
      action(button, "click", () => choose(Number(button.dataset.command))),
    );
  }
  input.oninput = () => draw(true);
  input.onkeydown = (e) => {
    if (e.isComposing) return;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      selected =
        (selected + (e.key === "ArrowDown" ? 1 : -1) + matches.length) %
        (matches.length || 1);
      draw();
      $(".command-selected", modal)?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter") {
      e.preventDefault();
      choose(selected).catch(error);
    }
  };
  draw();
  input.focus();
}
function attachmentMarkdown(file) {
  const label = file.filename.replace(/[\\[\]!*_`<>]/g, "_");
  const url = `/api/v1/attachments/${file.id}`;
  if (file.media_type.startsWith("image/"))
    return `![${label}](${url}/preview)\n\n[Download ${label}](${url})`;
  // The "audio" title shows a player (the address has no file extension).
  if (file.media_type.startsWith("audio/"))
    return `![${label}](${url}/preview "audio")\n\n[Download ${label}](${url})`;
  return `[Download ${label}](${url})`;
}
const IMAGE_LINK =
  /^https?:\/\/\S+\.(png|jpe?g|gif|webp|svg|avif|bmp)(\?\S*)?$/i;
const AUDIO_LINK =
  /^https?:\/\/\S+\.(mp3|wav|ogg|oga|opus|m4a|aac|flac|weba|m3u8?)(\?\S*)?$/i;
/**
 * A pasted or dropped address of an image or audio file, as Markdown that
 * shows it (audio files become a player); `null` for anything else.
 */
function mediaMarkdown(text) {
  const url = text.trim();
  if (!IMAGE_LINK.test(url) && !AUDIO_LINK.test(url)) return null;
  const name = decodeURIComponent(
    new URL(url).pathname.split("/").pop() || "",
  ).replace(/[\\[\]!*_`<>]/g, "_");
  return `![${name}](${url.replace(/[()\s]/g, encodeURIComponent)})`;
}
async function insertAttachment(file, targetId, afterId = null) {
  if (note?.id !== targetId) return;
  await insertBlockAfter(attachmentMarkdown(file), afterId);
}
/** Adds a block after block `afterId` (or at the end) and saves. */
async function insertBlockAfter(markdown, afterId = null) {
  const rendered = await renderMarkdown(markdown);
  live?.boundary();
  const index = note.blocks.findIndex((b) => b.id === afterId);
  const at = index < 0 ? note.blocks.length : index + 1;
  note.blocks.splice(at, 0, newBlock(markdown));
  note.rendered.splice(at, 0, rendered);
  markDirty();
  renderBlocks();
  await queueSave();
}
async function uploadAttachments(files, afterId = null) {
  if (!editable()) return;
  await flush();
  const targetId = note.id;
  // After a block, the last file goes in first so they keep their order.
  for (const file of afterId ? [...files].reverse() : files) {
    toast(`Uploading ${file.name}…`);
    await insertAttachment(await uploadFile(file, targetId), targetId, afterId);
  }
  toast("Files saved. Originals are available for download.");
}
async function uploadFile(file, targetId) {
  if (!file.size || file.size > 10 * 1024 * 1024)
    throw new Error("Files must contain 1 byte to 10 MiB.");
  const response = await fetch(
    `${shareMode ? "/api/v1/shared/attachments" : `/api/v1/notes/${targetId}/attachments`}?filename=${encodeURIComponent(file.name)}`,
    {
      method: "POST",
      headers: {
        "X-Minerva-Client": "1",
        "Content-Type": "application/octet-stream",
      },
      credentials: "same-origin",
      // The bytes, not the File: the Linux desktop app's webview crashes on
      // file bodies sent to the app's own address.
      body: await file.arrayBuffer(),
    },
  );
  const result = await response.json();
  if (!response.ok) throw new Error(result.error || "Upload failed");
  return result;
}
async function attachmentDialog() {
  await flush();
  const targetId = note.id;
  const result = await api(`notes/${targetId}/attachments`);
  if (note?.id !== targetId) return;
  modal.innerHTML = `<h2>Note files</h2><p>Originals are stored securely on this server. 10 MiB per file · 100 MiB per note. Files remain available for note history.</p>
    ${editable() ? '<label class="btn">Upload images or files<input id="attachment-upload" type="file" multiple></label>' : ""}
    <div>${result.attachments.map((file, i) => `<div class="attachment-row"><span>${esc(file.filename)} <small>(${Math.ceil(file.size / 1024)} KiB)</small></span><a class="btn" href="/api/v1/attachments/${file.id}" download>Download</a>${editable() ? `<button class="btn" data-insert-file="${i}">Insert in note</button>` : ""}</div>`).join("") || "<p>No files yet. You can also paste an image into a block.</p>"}</div>
    <footer><button class="btn" id="close-modal">Close</button></footer>`;
  modal.showModal();
  $("#close-modal").onclick = () => modal.close();
  action($("#attachment-upload"), "change", async (e) => {
    const files = [...e.target.files];
    modal.close();
    await uploadAttachments(files);
  });
  $$("[data-insert-file]", modal).forEach((button) =>
    action(button, "click", async () => {
      modal.close();
      await insertAttachment(
        result.attachments[Number(button.dataset.insertFile)],
        targetId,
      );
    }),
  );
}
/** Guests: a plain Markdown download built in the browser. */
function downloadMarkdown() {
  const blob = new Blob(
    [`# ${note.title}\n\n${note.blocks.map((b) => b.markdown).join("\n\n")}\n`],
    { type: "text/markdown" },
  );
  const a = document.createElement("a");
  a.href = URL.createObjectURL(blob);
  a.download = (note.title.replace(/[^\p{L}\p{N} _-]/gu, "") || "note") + ".md";
  a.click();
  setTimeout(() => URL.revokeObjectURL(a.href), 1000);
}
/** The installed template with this ID, or the first one (Standard). */
function printTemplateId(id) {
  return printTemplates.some((t) => t.id === id)
    ? id
    : printTemplates[0]?.id || "";
}
function templateOptions(selected) {
  selected = printTemplateId(selected);
  return printTemplates
    .map(
      (t) =>
        `<option value="${esc(t.id)}" ${t.id === selected ? "selected" : ""}>${esc(t.name)}</option>`,
    )
    .join("");
}
const metaKey = (raw) =>
  raw
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "_")
    .replace(/^_+|_+$/g, "")
    .slice(0, 40);
// Filled in by Minerva when exporting; a note can set its own value instead.
const AUTOMATIC_VARIABLES = {
  title: "the note title",
  date: "the export date (journal entries: their date)",
  generated: "the export date and time",
  updated: "when the note last changed",
  tags: "the note's tags",
};
/**
 * Note metadata: variables the user names and fills in. Exports replace
 * `{{key}}` in the note with the value, and PDF templates place known keys on
 * covers, headers, and tables. Below the fields, the keys that templates use
 * can be added with one click.
 */
function metadataDialog(templateId = note.print_template || "", after = null) {
  const targetId = note.id;
  const readOnly = !editable();
  const current = { ...(note.metadata || {}) };
  const chosen = printTemplates.find((t) => t.id === templateId);
  const used = new Set(chosen?.variables || []);
  const known = [...new Set(printTemplates.flatMap((t) => t.variables || []))]
    .filter((k) => !(k in AUTOMATIC_VARIABLES))
    .sort();
  const chip = (k) =>
    `<button type="button" class="meta-chip" data-meta-key="${esc(k)}" ${readOnly ? "disabled" : ""} title="${esc(
      AUTOMATIC_VARIABLES[k]
        ? `Automatic: ${AUTOMATIC_VARIABLES[k]}. Add it to use your own value.`
        : `Used by ${printTemplates
            .filter((t) => (t.variables || []).includes(k))
            .map((t) => t.name)
            .join(", ")}`,
    )}">{{${esc(k)}}}</button>`;
  const suggestions = () => {
    const free = (k) => !(k in current);
    const first = [...used].filter(free);
    const rest = known.filter((k) => free(k) && !used.has(k));
    return `${first.length ? `<p class="meta-suggest-head">Used by ${esc(chosen.name)}</p><div class="meta-chips">${first.map(chip).join("")}</div>` : ""}${rest.length ? `<p class="meta-suggest-head">${first.length ? "Used by other PDF templates" : "Used by PDF templates"}</p><div class="meta-chips">${rest.map(chip).join("")}</div>` : ""}<p class="meta-suggest-head">Filled in automatically</p><div class="meta-chips">${Object.keys(
      AUTOMATIC_VARIABLES,
    )
      .filter(free)
      .map(chip)
      .join("")}</div>`;
  };
  modal.innerHTML = `<form id="metadata-form"><h2>Metadata</h2><p>Your own variables for this note: name a field, give it a value, and type <code>{{name}}</code> in the note to insert it when exporting. PDF templates also place some of them on covers, headers, and tables; a PDF shows the name of any it is missing, such as <code>{{company}}</code>.</p><div id="meta-rows" class="meta-custom">${Object.keys(
    current,
  )
    .map((k) => metaRow(k, current[k], readOnly))
    .join(
      "",
    )}</div><p class="meta-empty hint" ${Object.keys(current).length ? "hidden" : ""}>No variables yet.</p>${readOnly ? "" : '<button type="button" class="btn" id="meta-add">+ Add variable</button>'}<section class="meta-suggest" aria-label="Known variables"><div id="meta-suggestions">${suggestions()}</div></section><div class="form-error" role="alert"></div><footer><button type="button" class="btn" id="meta-cancel">${readOnly ? "Close" : "Cancel"}</button>${readOnly ? "" : '<button class="btn primary" type="submit">Save</button>'}</footer></form>`;
  const form = $("#metadata-form");
  // Reads the rows into `current`, following renamed and removed keys.
  const collect = () => {
    for (const k of Object.keys(current)) delete current[k];
    $$(".meta-row", form).forEach((row) => {
      const key = metaKey($("[name=key]", row).value);
      if (key) current[key] = $("[name=value]", row).value;
    });
  };
  const refresh = () => {
    collect();
    $(".meta-empty", form).hidden = $$(".meta-row", form).length > 0;
    $("#meta-suggestions", form).innerHTML = suggestions();
    bind();
  };
  const add = (key) => {
    $("#meta-rows", form).insertAdjacentHTML(
      "beforeend",
      metaRow(key, "", false),
    );
    refresh();
    $(
      `#meta-rows .meta-row:last-child [name=${key ? "value" : "key"}]`,
      form,
    ).focus();
  };
  const bind = () => {
    $$("[data-remove-meta]", form).forEach(
      (b) =>
        (b.onclick = () => {
          b.closest(".meta-row").remove();
          refresh();
        }),
    );
    $$("[data-meta-key]", form).forEach(
      (b) => (b.onclick = () => add(b.dataset.metaKey)),
    );
    $$(".meta-row [name=key]", form).forEach(
      (input) => (input.onchange = refresh),
    );
  };
  bind();
  $("#meta-add", form)?.addEventListener("click", () => add(""));
  $("#meta-cancel", form).onclick = () => {
    modal.close();
    if (after) after();
  };
  form.onsubmit = async (e) => {
    e.preventDefault();
    collect();
    try {
      const saved = await api(`notes/${targetId}/metadata`, "PUT", {
        metadata: Object.fromEntries(
          Object.entries(current).filter(([, v]) => String(v).trim()),
        ),
        print_template: note.print_template || "",
      });
      if (note?.id === targetId) Object.assign(note, saved);
      modal.close();
      toast("Metadata saved");
      if (after) after();
    } catch (err) {
      $(".form-error", form).textContent = err.message;
    }
  };
  if (!modal.open) modal.showModal();
}
function metaRow(key, value, readOnly) {
  return `<div class="meta-row" data-key="${esc(key)}"><input name="key" placeholder="Variable name" aria-label="Variable name" value="${esc(key)}" ${readOnly ? "readonly" : ""}><input name="value" placeholder="${esc(key === "logo" || key === "photo" ? "HTTPS image address or a file link" : "Value")}" aria-label="Value" value="${esc(value ?? "")}" ${readOnly ? "readonly" : ""}>${readOnly ? "" : '<button type="button" class="btn" data-remove-meta aria-label="Remove field">✕</button>'}</div>`;
}
const EXPORT_FORMATS = [
  [
    "pdf",
    "PDF",
    "A print-ready document with cover, code highlighting, math and diagrams. Choose “Save as PDF” in the print dialog.",
  ],
  [
    "md",
    "Markdown (.md)",
    "Plain text for any Markdown editor. Note links become [[Title]].",
  ],
  [
    "zip",
    "Markdown + files (.zip)",
    "The note and its attachments, with links rewritten to the files folder. Ready for Obsidian or Joplin.",
  ],
  [
    "html",
    "Web page (.html)",
    "A single self-contained file with styles and images embedded.",
  ],
];
async function exportNote() {
  if (shareMode) return downloadMarkdown();
  await flush();
  const targetId = note.id;
  let saved = {};
  try {
    saved = JSON.parse(localStorage.getItem("minerva.export") || "{}");
  } catch {
    saved = {};
  }
  const format = saved.format || "pdf";
  modal.innerHTML = `<form id="export-form"><h2>Export “${esc(note.title)}”</h2><p>Exports include the latest saved version of this note.</p><div class="export-formats" role="radiogroup" aria-label="Format">${EXPORT_FORMATS.map(([value, label, help]) => `<label class="export-format"><input type="radio" name="format" value="${value}" ${value === format ? "checked" : ""}><span><strong>${label}</strong><small>${help}</small></span></label>`).join("")}</div><fieldset class="export-options"><legend>Options</legend><label class="checkbox-label" data-for="pdf html"><input type="checkbox" name="cover" ${saved.cover === false ? "" : "checked"}>Include cover image</label><label class="checkbox-label" data-for="pdf html"><input type="checkbox" name="meta" ${saved.meta === false ? "" : "checked"}>Show date and tags under the title</label><label class="checkbox-label" data-for="md zip"><input type="checkbox" name="front_matter" ${saved.front_matter === false ? "" : "checked"}>Add front matter (title, tags, date)</label><label data-for="pdf html">Template <select name="template">${templateOptions(note.print_template ?? saved.template ?? "")}</select></label><p class="hint template-help" data-for="pdf html"></p><button type="button" class="btn" id="export-metadata" data-for="pdf html">${icon("list")}Edit metadata…</button><label data-for="pdf">Page size <select name="size"><option value="a4" ${saved.size !== "letter" ? "selected" : ""}>A4</option><option value="letter" ${saved.size === "letter" ? "selected" : ""}>US Letter</option></select></label></fieldset><div class="form-error" role="alert"></div><footer><button type="button" class="btn" id="cancel-export">Cancel</button><button class="btn primary" type="submit">Export</button></footer></form>`;
  modal.showModal();
  const form = $("#export-form");
  const sync = () => {
    const chosen = form.format.value;
    $$("[data-for]", form).forEach(
      (el) => (el.hidden = !el.dataset.for.split(" ").includes(chosen)),
    );
  };
  const describe = () => {
    const t = printTemplates.find((x) => x.id === form.template.value);
    $(".template-help", form).textContent = t
      ? `${t.description} For: ${t.users}.${t.orientation === "landscape" ? " Landscape pages." : ""}`
      : "Minerva's own clean layout.";
  };
  form.addEventListener("change", () => {
    sync();
    describe();
  });
  sync();
  describe();
  $("#export-metadata").onclick = () =>
    metadataDialog(form.template.value, exportNote);
  $("#cancel-export").onclick = () => modal.close();
  form.onsubmit = async (e) => {
    e.preventDefault();
    const options = {
      format: form.format.value,
      cover: form.cover.checked,
      meta: form.meta.checked,
      front_matter: form.front_matter.checked,
      size: form.size.value,
      template: form.template.value,
    };
    try {
      localStorage.setItem("minerva.export", JSON.stringify(options));
    } catch {
      // Preference only.
    }
    const query = new URLSearchParams({
      format: options.format === "pdf" ? "print" : options.format,
      cover: options.cover ? "1" : "0",
      meta: options.meta ? "1" : "0",
      front_matter: options.front_matter ? "1" : "0",
      size: options.size,
      template: options.template,
    });
    // Remember the template for this note (for editors).
    if (editable() && options.template !== (note.print_template || ""))
      api(`notes/${targetId}/metadata`, "PUT", {
        metadata: note.metadata || {},
        print_template: options.template,
      })
        .then((r) => note?.id === targetId && Object.assign(note, r))
        .catch(() => {});
    const url = `/export/${encodeURIComponent(targetId)}?${query}`;
    modal.close();
    if (options.format === "pdf") return printDocument(url);
    const a = document.createElement("a");
    a.href = url;
    a.download = "";
    document.body.append(a);
    a.click();
    a.remove();
    toast("Export downloaded");
  };
}
/** Opens the system print dialog for an export page without leaving the note. */
function printDocument(url) {
  $("#print-frame")?.remove();
  const frame = document.createElement("iframe");
  frame.id = "print-frame";
  frame.title = "Print preview";
  frame.setAttribute("aria-hidden", "true");
  frame.className = "print-frame";
  toast("Preparing PDF…");
  let printed = false;
  const print = () => {
    if (printed) return;
    printed = true;
    window.removeEventListener("message", ready);
    try {
      frame.contentWindow.focus();
      frame.contentWindow.print();
    } catch {
      window.open(url, "_blank");
    }
    setTimeout(() => frame.remove(), 60_000);
  };
  const ready = (e) => {
    if (
      e.source === frame.contentWindow &&
      e.data?.type === "minerva-export-ready"
    )
      print();
  };
  window.addEventListener("message", ready);
  // Print even if the page never reports ready (for example, a slow diagram).
  setTimeout(print, 20_000);
  frame.src = url;
  document.body.append(frame);
}
/** Imports Markdown files, one note per file, into a folder. */
function importMarkdown() {
  if (!folderOptions()) return toast("Editor access is required.");
  formDialog(
    "Bring your notes along",
    "Choose one or more Markdown files. Each file becomes a note, titled from its first heading or its file name; each paragraph becomes an editable block.",
    `<label>Markdown files<input type="file" name="file" accept=".md,.markdown,.txt" multiple required></label><label>Folder<select name="folder_id">${folderOptions()}</select></label><p class="hint" id="import-progress" aria-live="polite"></p>`,
    async (v) => {
      const files = [...$("[name=file]", modal).files];
      const progress = $("#import-progress", modal);
      const failed = [];
      let last = null;
      for (const [i, file] of files.entries()) {
        progress.textContent = `Importing ${i + 1} of ${files.length}: ${file.name}`;
        try {
          if (file.size > 500000) throw new Error("larger than 500 KB");
          // Blank lines separate paragraphs; they are not blocks of their own.
          const lines = splitMarkdown(await file.text()).filter((l) =>
            l.trim(),
          );
          if (lines.length > 2000) throw new Error("more than 2,000 blocks");
          let title = file.name.replace(/\.[^.]+$/, "");
          if (lines[0]?.startsWith("# ")) title = lines.shift().slice(2);
          const n = await api("notes", "POST", {
            folder_id: v.folder_id,
            title: title.slice(0, 200) || "Imported note",
          });
          await api("notes/" + n.id, "PUT", {
            ...n,
            blocks: (lines.length ? lines : [""]).map((s, i) => newBlock(s, i)),
          });
          last = n;
        } catch (err) {
          failed.push(`${file.name} (${err.message})`);
        }
      }
      await refresh();
      const done = files.length - failed.length;
      if (failed.length)
        toast(
          `Imported ${done} of ${files.length}. Not imported: ${failed.join(", ")}`,
        );
      else if (done > 1) toast(`Imported ${done} notes`);
      if (done === 1 && last) await openNote(last.id);
      else navigate("folder", v.folder_id);
    },
    "Import",
  );
}
async function history() {
  await flush();
  const rows = await api("notes/" + note.id + "/history");
  modal.innerHTML = `<h2>Version history</h2><p>The last 50 saved versions of this note.</p><div class="search-results">${
    rows
      .reverse()
      .map(
        (n) =>
          `<div class="history-row"><span>Revision ${n.revision}<br><small>${new Date(n.updated_at * 1000).toLocaleString()}</small></span><button class="btn" data-restore="${n.revision}" ${!editable() ? "disabled" : ""}>Restore</button></div>`,
      )
      .join("") || "<p>No previous versions yet.</p>"
  }</div><footer><button class="btn" id="close-modal">Close</button></footer>`;
  modal.showModal();
  $("#close-modal").onclick = () => modal.close();
  $$("[data-restore]").forEach((b) =>
    action(b, "click", async () => {
      const old = rows.find((n) => n.revision === Number(b.dataset.restore));
      note.title = old.title;
      note.blocks = old.blocks;
      note.icon = old.icon;
      markDirty();
      await queueSave();
      note = await api("notes/" + note.id);
      modal.close();
      renderEditor();
      toast("Previous version restored");
    }),
  );
}
/** Undo or redo this client's last change to the open note. */
async function undoRedo(redo) {
  if (!note || !editable() || !live) return;
  await closing;
  if (!(redo ? live.redo() : live.undo())) {
    announce(redo ? "Nothing to redo" : "Nothing to undo");
    return;
  }
  const snapshot = live.snapshot(),
    previous = new Map(note.blocks.map((b) => [b.id, b]));
  note.title = snapshot.title;
  for (const key of ["icon", "favorite", "trashed", "cover", "tags"])
    note[key] = snapshot[key];
  note.blocks = snapshot.blocks.map((b, i) =>
    Object.assign(previous.get(b.id) || newBlock("", i), b),
  );
  note.rendered = await Promise.all(
    note.blocks.map((b) =>
      renderMarkdown(b.markdown).catch(() => esc(b.markdown)),
    ),
  );
  dirty = true;
  if ($("#note-title")) $("#note-title").value = note.title;
  renderBlocks();
  announce(redo ? "Redone" : "Undone");
}
/**
 * A month for a calendar: weekday names (weeks start on Monday) and day
 * buttons; days with a journal entry are marked, and today is outlined.
 */
function calendarParts(monthIso, entries, today = localDate()) {
  const [year, month] = monthIso.split("-").map(Number);
  const first = new Date(year, month - 1, 1);
  // Weeks start on Monday.
  const offset = (first.getDay() + 6) % 7;
  const length = new Date(year, month, 0).getDate();
  const weekdays = Array.from({ length: 7 }, (_, i) =>
    new Date(2024, 0, 1 + i).toLocaleDateString(undefined, {
      weekday: "short",
    }),
  );
  const cells = [
    ...Array.from(
      { length: offset },
      () => '<span class="day blank" aria-hidden="true"></span>',
    ),
    ...Array.from({ length }, (_, i) => {
      const iso = `${monthIso}-${String(i + 1).padStart(2, "0")}`;
      const entry = entries.get(iso);
      return `<button class="day ${entry ? "has-entry" : ""} ${iso === today ? "today" : ""}" data-journal-date="${iso}" aria-label="${esc(longDate(iso))}${entry ? ", has an entry" : ""}">${i + 1}</button>`;
    }),
  ].join("");
  return { weekdays, cells };
}
let journalMonth = null;
function renderJournal() {
  const today = localDate();
  journalMonth ??= today.slice(0, 7);
  const entries = new Map(journalNotes().map((n) => [n.journal, n]));
  const { weekdays, cells } = calendarParts(journalMonth, entries, today);
  const [year, month] = journalMonth.split("-").map(Number);
  const first = new Date(year, month - 1, 1);
  const recent = [...entries.values()]
    .sort((a, b) => b.journal.localeCompare(a.journal))
    .slice(0, 12);
  const templates = templateNotes();
  const configured = data.journal_templates?.[workspaceId] || "";
  $("#breadcrumb").textContent = "Journal";
  $("#content").innerHTML =
    `<section class="page"><div class="eyebrow">ONE NOTE PER DAY</div><div class="page-heading"><div><h1>Journal</h1><p class="subtitle">Capture the day. Unfinished tasks can follow you with [[uncompleted_journal_tasks]].</p></div><div class="toolbar-group"><input type="date" id="journal-date" value="${today}" aria-label="Journal date"><button class="btn" id="journal-open">Open date</button><button class="btn primary" id="journal-today">${icon("calendar")}Today</button></div></div><div class="journal-layout"><section class="panel calendar" aria-label="Calendar"><div class="calendar-head"><button class="btn" id="month-prev" aria-label="Previous month">‹</button><h2>${esc(first.toLocaleDateString(undefined, { month: "long", year: "numeric" }))}</h2><button class="btn" id="month-next" aria-label="Next month">›</button></div><div class="calendar-grid">${weekdays.map((d) => `<span class="weekday">${esc(d)}</span>`).join("")}${cells}</div></section><section class="panel journal-recent"><div class="panel-heading"><div><h2>Recent entries</h2><p>${entries.size} entr${entries.size === 1 ? "y" : "ies"} in this workspace</p></div></div><div class="journal-list">${recent.map((n) => `<button class="journal-entry" data-note="${n.id}"><strong>${esc(longDate(n.journal))}</strong><small>${n.title !== n.journal ? esc(n.title) : ""}</small></button>`).join("") || '<div class="empty">No entries yet. Start with today.</div>'}</div>${editsWorkspace() ? `<form class="journal-settings" id="journal-settings"><label>Template for new days<select name="template_id"><option value="">Blank page</option>${templates.map((t) => `<option value="${t.id}" ${t.id === configured ? "selected" : ""}>${esc(t.title)}</option>`).join("")}</select></label><button class="btn">Save</button></form>` : ""}</section></div></section>`;
  action($("#journal-today"), "click", () => openJournal(today));
  action($("#journal-open"), "click", () => {
    const value = $("#journal-date").value;
    if (value) return openJournal(value);
  });
  $$("[data-journal-date]").forEach((b) =>
    action(b, "click", () => openJournal(b.dataset.journalDate)),
  );
  $$(".journal-entry").forEach((b) =>
    action(b, "click", () => openNote(b.dataset.note)),
  );
  const moveMonth = (delta) => {
    const date = new Date(year, month - 1 + delta, 1);
    journalMonth = localDate(date).slice(0, 7);
    renderJournal();
  };
  action($("#month-prev"), "click", () => moveMonth(-1));
  action($("#month-next"), "click", () => moveMonth(1));
  action($("#journal-settings"), "submit", async (e) => {
    e.preventDefault();
    await api("journal/settings", "PUT", {
      workspace_id: workspaceId,
      template_id: e.target.template_id.value || null,
    });
    await refresh();
    toast("Journal template saved");
  });
}
function renderTemplates() {
  const templates = templateNotes();
  $("#breadcrumb").textContent = "Templates";
  $("#content").innerHTML =
    `<section class="page"><div class="eyebrow">START FROM SOMETHING GOOD</div><div class="page-heading"><div><h1>Templates</h1><p class="subtitle">Empty notes offer “Fill from template”. Use {{date}}, {{weekday}}, {{long_date}} and {{title}} as placeholders.</p></div><button class="btn primary" id="new-template">${icon("plus")}New template</button></div><div class="note-grid">${templates.map((n) => `<button class="note-card" data-note="${n.id}"><div class="note-preview" aria-hidden="true"><span class="preview-symbol">${esc(n.icon)}</span><div class="preview-lines"><i></i><i></i><i></i></div></div><div class="note-info"><strong>${esc(n.title)}</strong><div class="note-meta"><span>Template</span><span>· ${date(n.updated_at)}</span></div></div></button>`).join("")}</div>${templates.length ? "" : `<div class="empty">${icon("template")}<h3>No templates yet.</h3>Create one for meeting notes, daily journals, or project briefs.</div>`}</section>`;
  $$("[data-note]").forEach((b) =>
    action(b, "click", () => openNote(b.dataset.note)),
  );
  action($("#new-template"), "click", () =>
    formDialog(
      "New template",
      "Write it like a normal note. Placeholders such as {{date}} are filled in when the template is used.",
      `<label>Template name<input name="title" required maxlength="200" placeholder="e.g. Meeting notes" autofocus></label>`,
      async (v) => {
        const created = await api("templates", "POST", {
          ...v,
          workspace_id: workspaceId,
        });
        await refresh();
        await openNote(created.id);
      },
      "Create template",
    ),
  );
}
async function fillFromTemplate(anchor) {
  const templates = templateNotes(workspaceOfNote(note)).filter(
    (t) => t.id !== note.id,
  );
  const pop = popover(
    anchor,
    `<div class="template-list">${templates.map((t) => `<button data-template="${t.id}">${esc(t.icon)} ${esc(t.title)}</button>`).join("")}</div>`,
    "Choose a template",
  );
  $$("[data-template]", pop).forEach((b) =>
    b.addEventListener("click", async () => {
      closePopover();
      try {
        await flush();
        const filled = await api(`notes/${note.id}/template`, "POST", {
          template_id: b.dataset.template,
          date: note.journal || localDate(),
        });
        Object.assign(note, filled);
        rememberRendered();
        renderEditor();
        announce("Template applied");
      } catch (e) {
        error(e);
      }
    }),
  );
  $("button", pop)?.focus();
}
const taskCache = new Map();
/** Fills `[[uncompleted_journal_tasks]]` placeholders for the open note. */
async function renderJournalTasks() {
  const slots = $$("#blocks .journal-tasks:not([data-state])");
  if (!slots.length || !note) return;
  slots.forEach((slot) => (slot.dataset.state = "loading"));
  if (shareMode) {
    slots.forEach(
      (slot) =>
        (slot.innerHTML =
          '<span class="journal-tasks-note">Unfinished journal tasks are shown to signed-in members.</span>'),
    );
    return;
  }
  const noteId = note.id;
  const cached = taskCache.get(noteId);
  let tasks;
  if (cached && Date.now() - cached.at < 5000) tasks = cached.tasks;
  else {
    tasks = await api("journal/tasks", "POST", {
      note_id: noteId,
      today: localDate(),
    });
    taskCache.set(noteId, { at: Date.now(), tasks });
  }
  if (note?.id !== noteId) return;
  const html = `<span class="journal-tasks-panel" role="group" aria-label="Unfinished tasks from earlier journal entries"><span class="journal-tasks-title">☑ Unfinished tasks from earlier entries <small>${tasks.length}</small></span>${tasks.length ? `<span class="journal-tasks-list">${tasks.map((t, i) => `<span class="journal-task"><button class="task-toggle" data-complete-task="${i}" aria-label="Mark “${esc(t.text)}” done">☐</button><span class="journal-task-text">${t.html}</span><a class="journal-task-origin" href="#note/${esc(t.note_id)}" title="Open the entry">${esc(parseDate(t.date).toLocaleDateString(undefined, { month: "short", day: "numeric" }))}</a></span>`).join("")}</span>` : '<span class="journal-tasks-note">Nothing left over. Nice.</span>'}</span>`;
  for (const slot of slots) {
    if (!slot.isConnected) continue;
    slot.innerHTML = html;
    slot.dataset.state = "ready";
    $$("[data-complete-task]", slot).forEach((button) =>
      button.addEventListener("click", async (e) => {
        e.stopPropagation();
        const task = tasks[Number(button.dataset.completeTask)];
        button.disabled = true;
        try {
          const result = await api("journal/tasks/complete", "POST", {
            note_id: noteId,
            today: localDate(),
            text: task.text,
          });
          taskCache.delete(noteId);
          $$("#blocks .journal-tasks").forEach((s) => delete s.dataset.state);
          renderJournalTasks().catch(error);
          announce(
            `Marked done in ${result.completed} entr${result.completed === 1 ? "y" : "ies"}`,
          );
        } catch (err) {
          button.disabled = false;
          error(err);
        }
      }),
    );
  }
}
async function renderAccount() {
  const status = await api("account/2fa");
  $("#breadcrumb").textContent = "Account security";
  $("#content").innerHTML =
    `<section class="page"><div class="eyebrow">YOUR ACCOUNT</div><div class="page-heading"><div><h1>Account security</h1><p class="subtitle">${esc(data.user.name)} · ${esc(data.user.email)}</p></div></div><section class="panel security-panel"><div class="panel-heading"><div><h2>Password</h2><p>Changing it signs you out everywhere else.</p></div></div><button class="btn" id="change-password">Change password</button></section><section class="panel security-panel"><div class="panel-heading"><div><h2>Two-factor authentication</h2><p>Sign in with your password and a code from an authenticator app such as Google Authenticator, Microsoft Authenticator, 1Password, or Aegis.</p></div><span class="pill ${status.enabled ? "pill-on" : ""}">${status.enabled ? "On" : "Off"}</span></div><div class="security-body">${
      status.enabled
        ? `<p>On since ${esc(new Date(status.enabled_at * 1000).toLocaleDateString())}. ${status.recovery_remaining} of 10 recovery codes left.</p><div class="toolbar-group"><button class="btn" id="new-recovery">New recovery codes</button><button class="btn danger" id="disable-2fa">Turn off</button></div>`
        : `<p>Two-factor authentication protects your notes even if your password is stolen.</p><button class="btn primary" id="setup-2fa">Set up two-factor authentication</button>`
    }</div></section></section>`;
  action($("#setup-2fa"), "click", setupTwoFactor);
  action($("#change-password"), "click", () =>
    formDialog(
      "Change your password",
      "Use 12 characters or more. Your other devices will need to sign in again.",
      '<label>Current password<input name="current_password" type="password" autocomplete="current-password" required></label><label>New password<input name="new_password" type="password" autocomplete="new-password" minlength="12" maxlength="256" required></label><label>Repeat new password<input name="repeat" type="password" autocomplete="new-password" required></label>',
      async (v) => {
        if (v.new_password !== v.repeat)
          throw new Error("The new passwords do not match.");
        await api("account/password", "POST", {
          current_password: v.current_password,
          new_password: v.new_password,
        });
        toast("Password changed");
      },
      "Change password",
    ),
  );
  action($("#new-recovery"), "click", () =>
    formDialog(
      "New recovery codes",
      "Your current recovery codes will stop working. Enter a code from your authenticator app to continue.",
      '<label>Authentication code<input name="code" inputmode="numeric" autocomplete="one-time-code" required maxlength="7"></label>',
      async (v) => {
        const result = await api("account/2fa/recovery", "POST", v);
        showRecoveryCodes(result.recovery_codes);
      },
      "Create new codes",
    ),
  );
  action($("#disable-2fa"), "click", () =>
    formDialog(
      "Turn off two-factor authentication?",
      "Your account will be protected by your password only.",
      '<label>Password<input name="password" type="password" autocomplete="current-password" required></label><label>Authentication or recovery code<input name="code" autocomplete="one-time-code" required></label>',
      async (v) => {
        await api("account/2fa/disable", "POST", v);
        toast("Two-factor authentication is off");
        await renderAccount();
      },
      "Turn off",
    ),
  );
}
async function setupTwoFactor() {
  const setup = await api("account/2fa/setup", "POST", {});
  modal.innerHTML = `<form id="totp-form"><h2>Set up two-factor authentication</h2><ol class="totp-steps"><li><strong>Scan this QR code</strong> with your authenticator app.<div class="totp-qr" aria-label="QR code for your authenticator app">${setup.qr_svg}</div><details><summary>Can't scan? Enter the key manually</summary><code class="totp-secret">${esc(setup.secret)}</code><button type="button" class="btn" id="copy-secret">Copy key</button><p>Account: ${esc(data.user.email)} · Time-based · 6 digits</p></details></li><li><strong>Enter the 6-digit code</strong> the app shows for Minerva.<input name="code" inputmode="numeric" autocomplete="one-time-code" placeholder="123 456" maxlength="7" required aria-label="Authentication code"></li></ol><div class="form-error" role="alert"></div><footer><button type="button" class="btn" id="cancel-totp">Cancel</button><button class="btn primary" type="submit">Turn on</button></footer></form>`;
  modal.showModal();
  $("#cancel-totp").onclick = () => modal.close();
  action($("#copy-secret"), "click", async () => {
    await navigator.clipboard.writeText(setup.secret.replace(/ /g, ""));
    toast("Key copied");
  });
  $("#totp-form").onsubmit = async (e) => {
    e.preventDefault();
    const button = $("[type=submit]", modal);
    button.disabled = true;
    try {
      const result = await api("account/2fa/enable", "POST", {
        code: e.target.code.value,
      });
      showRecoveryCodes(result.recovery_codes, true);
    } catch (err) {
      $(".form-error", modal).textContent = err.message;
      button.disabled = false;
    }
  };
}
function showRecoveryCodes(codes, justEnabled = false) {
  const text = codes.join("\n");
  modal.innerHTML = `<h2>${justEnabled ? "Two-factor authentication is on" : "Your new recovery codes"}</h2><p>Save these recovery codes somewhere safe. Each one signs you in once if you lose your phone. They will not be shown again.${justEnabled ? " Your other sessions were signed out." : ""}</p><div class="recovery-codes">${codes.map((c) => `<code>${esc(c)}</code>`).join("")}</div><footer><button class="btn" id="copy-codes">Copy</button><button class="btn" id="download-codes">Download</button><button class="btn primary" id="done-codes">I saved them</button></footer>`;
  if (!modal.open) modal.showModal();
  action($("#copy-codes"), "click", async () => {
    await navigator.clipboard.writeText(text);
    toast("Recovery codes copied");
  });
  action($("#download-codes"), "click", () => {
    const a = document.createElement("a");
    a.href = URL.createObjectURL(
      new Blob([`Minerva recovery codes for ${data.user.email}\n\n${text}\n`], {
        type: "text/plain",
      }),
    );
    a.download = "minerva-recovery-codes.txt";
    a.click();
    setTimeout(() => URL.revokeObjectURL(a.href), 1000);
  });
  $("#done-codes").onclick = () => {
    modal.close();
    renderAccount().catch(error);
  };
}
let sharingFilter = "all";
/** Who can see what, across every workspace you belong to. */
async function renderSharing() {
  const info = await api("sharing");
  if (view !== "sharing") return;
  $("#breadcrumb").textContent = "Sharing";
  const origin = serverOrigin();
  const role = (r) => (r === "editor" ? "Edit" : "Read only");
  const people = (list) =>
    list
      .map(
        (p) =>
          `<span class="person-chip" title="${esc(role(p.role))}">${esc(p.name)}<small>${p.role === "editor" ? "Edit" : "View"}</small></span>`,
      )
      .join("");
  const published = info.managed.filter((n) => n.sharing.published);
  const links = info.managed.reduce((n, x) => n + x.sharing.links, 0);
  const stats = [
    [
      "users",
      info.workspaces_shared.length + info.workspaces_received.length,
      "Shared workspaces",
    ],
    [
      "note",
      info.managed.filter((n) => n.sharing.members).length,
      "Notes shared with people",
    ],
    ["link", links, "Active links"],
    ["globe", published.length, "Public pages"],
  ];
  const statsHTML = `<div class="sharing-stats">${stats.map(([i, n, label]) => `<div class="sharing-stat">${icon(i)}<strong>${n}</strong><span>${label}</span></div>`).join("")}</div>`;

  // Workspaces.
  const workspacesHTML = `<section class="panel sharing-section"><div class="panel-heading"><div><h2>Shared workspaces</h2><p>Everyone in a workspace sees all of its folders and notes. Share one from its badge at the top of the page, or here.</p></div></div>${
    info.workspaces_shared.length || info.workspaces_received.length
      ? `<div class="sharing-rows">${info.workspaces_shared
          .map(
            (w) =>
              `<div class="sharing-row"><div class="sharing-what"><span class="square">${esc(w.name.slice(0, 1))}</span><div><strong>${esc(w.name)}</strong><small>Yours · shared with ${w.members.length} ${w.members.length === 1 ? "person" : "people"}</small></div></div><div class="sharing-who">${people(w.members)}</div><div class="sharing-actions"><button class="btn" data-manage-workspace="${w.id}">Manage</button><button class="btn" data-open-workspace="${w.id}">Open</button></div></div>`,
          )
          .join("")}${info.workspaces_received
          .map(
            (w) =>
              `<div class="sharing-row"><div class="sharing-what"><span class="square">${esc(w.name.slice(0, 1))}</span><div><strong>${esc(w.name)}</strong><small>Shared with you by ${esc(w.owner)}</small></div></div><div class="sharing-who"><span class="pill">${role(w.role)}</span></div><div class="sharing-actions"><button class="btn" data-open-workspace="${w.id}">Open</button></div></div>`,
          )
          .join("")}</div>`
      : '<p class="sharing-empty">No shared workspaces yet. Open a workspace and use the badge at the top right to invite people.</p>'
  }</section>`;

  // Notes you share or publish, by workspace.
  const filters = [
    ["all", "All", () => true],
    ["people", "With people", (n) => n.sharing.members],
    ["links", "With a link", (n) => n.sharing.links],
    ["published", "Published", (n) => n.sharing.published],
  ];
  const keep = filters.find(([id]) => id === sharingFilter)[2];
  const notes = info.managed.filter(keep);
  const groups = new Map();
  for (const n of notes) {
    const key = n.workspace || "Other";
    if (!groups.has(key)) groups.set(key, []);
    groups.get(key).push(n);
  }
  const noteRow = (n) =>
    `<div class="sharing-row"><div class="sharing-what"><span class="sharing-icon">${esc(n.icon || "▤")}</span><div><button class="text-link" data-open-note="${n.id}">${esc(n.title)}</button>${n.trashed ? "<small>In the trash</small>" : `<small>Updated ${date(n.updated_at)}</small>`}</div></div><div class="sharing-who">${people(n.people)}${n.sharing.links ? `<span class="pill">${icon("link")}${n.sharing.links} link${n.sharing.links === 1 ? "" : "s"}</span>` : ""}${n.sharing.published ? `<a class="pill pill-public" href="${esc(origin)}/${esc(n.sharing.published)}" target="_blank" rel="noopener">${icon("globe")}/${esc(n.sharing.published)}</a>` : ""}</div><div class="sharing-actions">${n.sharing.published ? `<button class="btn" data-copy-url="${esc(origin)}/${esc(n.sharing.published)}" title="Copy the public address">${icon("copy")}Copy</button>` : ""}<button class="btn" data-open-note="${n.id}">Open</button></div></div>`;
  const notesHTML = `<section class="panel sharing-section"><div class="panel-heading"><div><h2>Notes you share</h2><p>Notes in folders you edit that are shared with people, by link, or published. Open a note and use Share to change or revoke access.</p></div><div class="segmented" role="tablist" aria-label="Show">${filters.map(([id, label]) => `<button role="tab" data-sharing-filter="${id}" aria-selected="${id === sharingFilter}" class="${id === sharingFilter ? "active" : ""}">${label}</button>`).join("")}</div></div>${
    notes.length
      ? [...groups]
          .map(
            ([name, list]) =>
              `<h3 class="sharing-group">${esc(name)}</h3><div class="sharing-rows">${list.map(noteRow).join("")}</div>`,
          )
          .join("")
      : `<p class="sharing-empty">${info.managed.length ? "Nothing matches this filter." : "You are not sharing any notes yet. Open a note and choose Share."}</p>`
  }</section>`;

  // Notes shared with you.
  const receivedHTML = `<section class="panel sharing-section"><div class="panel-heading"><div><h2>Notes shared with you</h2><p>Single notes other members shared with you, outside the workspaces you belong to.</p></div></div>${
    info.shared_with_me.length
      ? `<div class="sharing-rows">${info.shared_with_me
          .map(
            (n) =>
              `<div class="sharing-row"><div class="sharing-what"><span class="sharing-icon">${esc(n.icon || "▤")}</span><div><button class="text-link" data-open-note="${n.id}">${esc(n.title)}</button><small>${esc(n.workspace || "")} · updated ${date(n.updated_at)}</small></div></div><div class="sharing-who"><span class="pill">${role(n.role)}</span></div><div class="sharing-actions"><button class="btn" data-open-note="${n.id}">Open</button></div></div>`,
          )
          .join("")}</div>`
      : '<p class="sharing-empty">Nothing has been shared with you yet.</p>'
  }</section>`;

  $("#content").innerHTML =
    `<section class="page sharing-page"><div class="eyebrow">WHO CAN SEE WHAT</div><div class="page-heading"><div><h1>Sharing</h1><p class="subtitle">Across all your workspaces. Public pages without a site are listed at <a href="${esc(origin)}/shared/" target="_blank" rel="noopener">${esc(origin)}/shared/</a>.</p></div></div>${statsHTML}${workspacesHTML}${notesHTML}${receivedHTML}${sitesPanel(origin)}</section>`;
  $$("[data-open-note]").forEach((b) =>
    action(b, "click", () => openNote(b.dataset.openNote)),
  );
  $$("[data-sharing-filter]").forEach((b) =>
    action(b, "click", () => {
      sharingFilter = b.dataset.sharingFilter;
      return renderSharing();
    }),
  );
  $$("[data-manage-workspace]").forEach((b) =>
    action(b, "click", () => shareWorkspace(b.dataset.manageWorkspace)),
  );
  $$("[data-open-workspace]").forEach((b) =>
    action(b, "click", () => {
      workspaceId = b.dataset.openWorkspace;
      view = "home";
      folderId = null;
      shell();
    }),
  );
  $$("[data-copy-url]").forEach((b) =>
    action(b, "click", async () => {
      await navigator.clipboard.writeText(b.dataset.copyUrl);
      toast("Address copied");
    }),
  );
  action($("#new-site"), "click", () => siteDialog());
  $$("[data-edit-site]").forEach((b) =>
    action(b, "click", () =>
      siteDialog(data.sites.find((x) => x.id === b.dataset.editSite)),
    ),
  );
  $$("[data-delete-site]").forEach((b) =>
    action(b, "click", async () => {
      if (!b.dataset.armed) {
        b.dataset.armed = "1";
        b.textContent = "Confirm delete";
        return;
      }
      await api(`sites/${b.dataset.deleteSite}`, "DELETE");
      await refresh();
      await renderSharing();
      toast("Site deleted");
    }),
  );
}
function workspaceSites(wid = workspaceId) {
  return (data.sites || []).filter((s) => s.workspace_id === wid);
}
/** Workspaces whose sites and folders you manage: yours, or ones you edit. */
function editableWorkspaces() {
  return data.workspaces.filter(
    (w) => w.owner_id === data.user.id || w.can_edit,
  );
}
// Public sites of all your workspaces: paths such as /blog or /wiki.
function sitesPanel(origin) {
  const editable = new Set(editableWorkspaces().map((w) => w.id));
  const sites = data.sites || [];
  const workspaceName = (id) =>
    data.workspaces.find((w) => w.id === id)?.name || "";
  const rows = sites
    .map(
      (s) =>
        `<div class="sharing-row"><div class="sharing-what"><span class="sharing-icon">${icon("globe")}</span><div><a href="${esc(origin)}/${esc(s.path)}/" target="_blank" rel="noopener"><strong>${esc(s.title || s.path)}</strong></a><small>/${esc(s.path)}/ · ${s.layout === "wiki" ? "Wiki" : "Blog"}${s.theme ? ` · ${esc(publicThemes.find((t) => t.id === s.theme)?.name || s.theme)}` : ""} · ${esc(workspaceName(s.workspace_id))}</small></div></div><div class="sharing-who"><span class="pill">${s.pages} page${s.pages === 1 ? "" : "s"}</span></div><div class="sharing-actions">${editable.has(s.workspace_id) ? `<button class="btn" data-edit-site="${s.id}">Edit</button><button class="btn danger" data-delete-site="${s.id}" ${s.pages ? 'disabled title="Move or unpublish its pages first"' : ""}>Delete</button>` : ""}</div></div>`,
    )
    .join("");
  return `<section class="panel sharing-section"><div class="panel-heading"><div><h2>Public sites</h2><p>Give published pages their own address, like /blog/my-post or /wiki/how-to. People who edit a workspace choose its site when publishing.</p></div>${editable.size ? `<button class="btn primary" id="new-site">${icon("plus")}New site</button>` : ""}</div>${rows ? `<div class="sharing-rows">${rows}</div>` : `<p class="sharing-empty">No sites yet.${editable.size ? " Create one to publish pages under their own path." : ""}</p>`}</section>`;
}
function siteDialog(site = null) {
  formDialog(
    site ? "Edit site" : "New site",
    "Pages published to this site live at /path/page-address. Blogs list pages newest first with a feed; wikis list them alphabetically.",
    `${
      site
        ? ""
        : `<label>Workspace<select name="workspace_id">${editableWorkspaces()
            .map(
              (w) =>
                `<option value="${w.id}" ${w.id === workspaceId ? "selected" : ""}>${esc(w.name)}</option>`,
            )
            .join("")}</select></label>`
    }<label>Path<span class="slug-field">${esc(serverOrigin())}/<input name="path" required maxlength="40" pattern="[a-z0-9]([a-z0-9-]*[a-z0-9])?" title="Lowercase letters, digits and inner hyphens" value="${esc(site?.path || "")}" placeholder="blog" autofocus></span></label><label>Title<input name="title" maxlength="100" value="${esc(site?.title || "")}" placeholder="My blog"></label><label>Layout<select name="layout"><option value="blog" ${site?.layout !== "wiki" ? "selected" : ""}>Blog: newest first, with a feed</option><option value="wiki" ${site?.layout === "wiki" ? "selected" : ""}>Wiki: alphabetical pages</option></select></label><label>Look<select name="theme"><option value="">Default</option>${publicThemes.map((t) => `<option value="${esc(t.id)}" ${site?.theme === t.id ? "selected" : ""}>${esc(t.name)}${t.description ? ` · ${esc(t.description)}` : ""}</option>`).join("")}${site?.theme && !publicThemes.some((t) => t.id === site.theme) ? `<option value="${esc(site.theme)}" selected>${esc(site.theme)} (not installed)</option>` : ""}</select></label><label>Description <small>(optional, shown on the index)</small><input name="description" maxlength="300" value="${esc(site?.description || "")}"></label>${site?.pages ? '<p class="hint">Changing the path changes the address of every page in this site.</p>' : ""}`,
    async (v) => {
      if (site) await api(`sites/${site.id}`, "PATCH", v);
      else
        await api("sites", "POST", {
          ...v,
          workspace_id: v.workspace_id || workspaceId,
        });
      await refresh();
      await renderSharing();
      toast(site ? "Site updated" : "Site created");
    },
    site ? "Save" : "Create site",
  );
}
async function shareDialog(newToken = null) {
  await flush();
  const targetId = note.id;
  // A note tab loads its data once: sites created since (for example on the
  // Sharing page) must be offered too.
  const [info] = await Promise.all([
    api(`notes/${targetId}/shares`),
    refresh(),
  ]);
  if (note?.id !== targetId) return;
  const origin = serverOrigin();
  const members = info.shares.filter((s) => s.kind === "member"),
    links = info.shares.filter((s) => s.kind === "link"),
    pub = info.publication;
  const noteWorkspace = data.folders.find(
    (f) => f.id === note.folder_id,
  )?.workspace_id;
  const sites = workspaceSites(noteWorkspace);
  const siteField = sites.length
    ? `<select name="site_id" aria-label="Site"><option value="">shared</option>${sites.map((s) => `<option value="${s.id}" ${pub?.site_id === s.id ? "selected" : ""}>${esc(s.path)}</option>`).join("")}</select>/`
    : '<input type="hidden" name="site_id" value="">shared/';
  const roleSelect = (name, label) =>
    `<select name="${name}" aria-label="${label}"><option value="viewer" selected>Can view</option><option value="editor">Can edit</option></select>`;
  modal.innerHTML = `<h2>Share “${esc(note.title)}”</h2><p>Read-only access is selected by default. Only people who manage this folder can change sharing.</p>
  <section class="share-section" aria-labelledby="share-people"><h3 id="share-people">People on this server</h3>
    <form id="share-member" class="share-row"><input name="email" type="email" required placeholder="name@example.com" aria-label="Member email" autocomplete="off">${roleSelect("role", "Access for this member")}<button class="btn primary">Share</button></form>
    <ul class="share-list">${members.map((m) => `<li><span>${esc(m.user?.name || "Unavailable account")} <small>${esc(m.user?.email || "")}</small></span><span class="pill">${roleLabel(m.role)}</span><button class="btn danger" data-revoke-share="${m.id}" aria-label="Remove ${esc(m.user?.name || "member")}">Remove</button></li>`).join("") || '<li class="muted">Not shared with anyone yet.</li>'}</ul>
  </section>
  <section class="share-section" aria-labelledby="share-links"><h3 id="share-links">Anyone with the link</h3>
    <form id="share-link" class="share-row">${roleSelect("role", "Access for people with the link")}<button class="btn">Create link</button></form>
    ${newToken ? `<div class="new-link" role="status"><label>Copy this link now. It is shown only once.<input id="new-link-url" readonly value="${esc(origin)}/share#${esc(newToken)}"></label><button class="btn primary" id="copy-link" type="button">Copy link</button></div>` : ""}
    <ul class="share-list">${links.map((l) => `<li><span>Link created ${date(l.created_at)}${l.created_by ? ` by ${esc(l.created_by)}` : ""}</span><span class="pill">${roleLabel(l.role)}</span><button class="btn danger" data-revoke-share="${l.id}">Revoke</button></li>`).join("") || '<li class="muted">No active links.</li>'}</ul>
    <p class="hint">Anyone holding a link can open this note without signing in until you revoke it. Guests do not see your workspace, folders, or member names.</p>
  </section>
  <section class="share-section" aria-labelledby="share-web"><h3 id="share-web">Publish to the web</h3>
    ${pub ? `<p class="public-state">Public at <a href="${esc(origin)}/${esc(pub.path)}" target="_blank" rel="noopener">${esc(origin)}/${esc(pub.path)}</a> · ${pub.mode === "pinned" ? `pinned to revision ${pub.revision} (current: ${note.revision})` : "follows live edits"}</p>` : "<p>Not published. A published page is public, read-only, and omits workspace details and editing controls.</p>"}
    <form id="publish-form" class="publish-form">
      <label>Address<span class="slug-field">${esc(origin)}/${siteField}<input name="slug" required maxlength="80" pattern="[a-z0-9]([a-z0-9-]*[a-z0-9])?" title="Lowercase letters, digits and inner hyphens" value="${esc(pub?.slug || slugify(note.title))}"></span></label>
      <label>Description <small>(optional, shown in listings)</small><input name="description" maxlength="300" value="${esc(pub?.description || "")}"></label>
      <fieldset><legend>When the note changes</legend><label class="checkbox-label"><input type="radio" name="mode" value="pinned" ${!pub || pub.mode === "pinned" ? "checked" : ""}>Keep this published version until I update it</label><label class="checkbox-label"><input type="radio" name="mode" value="live" ${pub?.mode === "live" ? "checked" : ""}>Follow live edits</label></fieldset>
      <div class="share-actions"><a class="btn" href="/preview/${esc(targetId)}" target="_blank" rel="noopener">Preview</a>${pub ? '<button type="button" class="btn danger" id="unpublish">Unpublish</button>' : ""}<button class="btn primary" type="submit">${pub ? "Update published page" : "Publish"}</button></div>
    </form>
  </section>
  <div class="form-error" role="alert"></div>
  <footer><button class="btn" id="close-modal">Done</button></footer>`;
  modal.showModal();
  const fail = (e) => ($(".form-error", modal).textContent = e.message);
  const reopen = async (token = null) => {
    await shareDialog(token);
    const fresh = await api("notes/" + targetId);
    if (note?.id === targetId) note.sharing = fresh.sharing;
    refresh().catch(() => {});
  };
  $("#close-modal").onclick = () => {
    modal.close();
    if (note?.id === targetId) renderEditor();
  };
  $("#share-member").onsubmit = (e) => {
    e.preventDefault();
    api(`notes/${targetId}/shares`, "POST", {
      kind: "member",
      ...Object.fromEntries(new FormData(e.target)),
    })
      .then(() => reopen())
      .then(() => announce("Shared"))
      .catch(fail);
  };
  $("#share-link").onsubmit = (e) => {
    e.preventDefault();
    api(`notes/${targetId}/shares`, "POST", {
      kind: "link",
      ...Object.fromEntries(new FormData(e.target)),
    })
      .then((created) => reopen(created.token))
      .then(() => $("#new-link-url")?.select())
      .catch(fail);
  };
  action($("#copy-link"), "click", async () => {
    await navigator.clipboard.writeText($("#new-link-url").value);
    toast("Link copied");
  });
  $$("[data-revoke-share]", modal).forEach((b) =>
    action(b, "click", async () => {
      await api(`notes/${targetId}/shares/${b.dataset.revokeShare}`, "DELETE");
      await reopen();
      announce("Access removed");
    }),
  );
  $("#publish-form").onsubmit = (e) => {
    e.preventDefault();
    const v = Object.fromEntries(new FormData(e.target));
    api(`notes/${targetId}/publication`, "PUT", { ...v, refresh: true })
      .then(() => reopen())
      .then(() => toast("Published page updated"))
      .catch(fail);
  };
  action($("#unpublish"), "click", async (e) => {
    if (!e.target.dataset.armed) {
      e.target.dataset.armed = "1";
      e.target.textContent = "Confirm unpublish";
      return;
    }
    await api(`notes/${targetId}/publication`, "DELETE");
    await reopen();
    toast("The page is no longer public");
  });
}
// ---------------------------------------------------------------------------
// Sidebar: collapsible sections, the folder tree, row menus, and drag and drop.

const storedSet = (key) => new Set(JSON.parse(stored(key, "[]") || "[]"));
const saveSet = (key, set) => store(key, JSON.stringify([...set]));
// Folder IDs expanded in the tree, and sidebar sections the user collapsed.
let openFolders = storedSet("minerva.tree.open"),
  collapsedSections = storedSet("minerva.sidebar.collapsed");
// Journal and Templates have their own pages, so the tree leaves them out.
function treeFolders() {
  const special = new Set([
    data.journal_folders?.[workspaceId],
    data.template_folders?.[workspaceId],
  ]);
  return folders().filter((f) => !special.has(f.id));
}
function folderChildren(list, parent) {
  const ids = new Set(list.map((f) => f.id));
  // A subfolder whose parent the user cannot see is shown at the top level.
  return list.filter((f) =>
    parent === null
      ? !f.parent_id || !ids.has(f.parent_id)
      : f.parent_id === parent,
  );
}
function folderPath(id) {
  const path = [];
  for (let f = data.folders.find((x) => x.id === id); f && path.length < 32;)
    path.unshift(f) && (f = data.folders.find((x) => x.id === f.parent_id));
  return path;
}
// Expands the folders above the open note or folder so it is visible in the tree.
function revealInTree() {
  const target = note?.folder_id || (view === "folder" ? folderId : null);
  if (!target) return;
  for (const f of folderPath(target))
    if (f.id !== target || note) openFolders.add(f.id);
  saveSet("minerva.tree.open", openFolders);
}
function sidebarSection(id, label, body, extra = "") {
  const collapsed = collapsedSections.has(id);
  return `<section class="side-section" data-section="${id}"><div class="section-label"><button class="section-toggle" aria-expanded="${!collapsed}" aria-controls="section-${id}" data-toggle-section="${id}">${icon("chevron")}${label}</button>${extra}</div><div class="section-body" id="section-${id}" ${collapsed ? "hidden" : ""}>${body}</div></section>`;
}
function noteRow(n, depth) {
  const active = note?.id === n.id;
  return `<div class="tree-row note-row ${active ? "active" : ""}" style="--depth:${depth}" ${data.roles[n.folder_id] === "editor" ? `draggable="true" data-drag-note="${n.id}"` : ""}><span class="tree-spacer"></span><button class="tree-label" data-open-note="${n.id}" ${active ? 'aria-current="page"' : ""}><span class="tree-icon note-icon" aria-hidden="true">${esc(n.icon || "▤")}</span><span class="tree-name">${esc(n.title)}</span></button><span class="tree-actions"><button data-row-menu="note:${n.id}" aria-label="More actions for ${esc(n.title)}" aria-haspopup="menu">${icon("more")}</button></span></div>`;
}
function folderNode(f, list, depth) {
  const children = folderChildren(list, f.id);
  const notes = data.notes
    .filter((n) => n.folder_id === f.id && !n.trashed)
    .sort((a, b) => a.title.localeCompare(b.title));
  const open = openFolders.has(f.id);
  const empty = !children.length && !notes.length;
  const active = view === "folder" && folderId === f.id && !note;
  const canEdit = data.roles[f.id] === "editor";
  return `<div class="tree-node" data-node="${f.id}"><div class="tree-row folder-row ${active ? "active" : ""}" style="--depth:${depth}" data-drop-folder="${f.id}" ${editsWorkspace() ? `draggable="true" data-drag-folder="${f.id}"` : ""}><button class="tree-toggle" data-toggle-folder="${f.id}" aria-expanded="${open}" aria-label="${open ? "Collapse" : "Expand"} ${esc(f.name)}" ${empty ? 'disabled tabindex="-1"' : ""}>${icon("chevron")}</button><button class="tree-label" data-folder="${f.id}" ${active ? 'aria-current="page"' : ""}><span class="tree-icon" aria-hidden="true">${icon(open && !empty ? "folderOpen" : "folder")}</span><span class="tree-name">${esc(f.name)}</span><span class="count">${notes.length || ""}</span></button><span class="tree-actions">${canEdit ? `<button data-row-new="${f.id}" aria-label="New note in ${esc(f.name)}">${icon("plus")}</button>` : ""}<button data-row-menu="folder:${f.id}" aria-label="More actions for ${esc(f.name)}" aria-haspopup="menu">${icon("more")}</button></span></div>${empty ? "" : `<div class="tree-children" role="group" ${open ? "" : "hidden"}>${children.map((c) => folderNode(c, list, depth + 1)).join("")}${notes.map((n) => noteRow(n, depth + 1)).join("")}</div>`}</div>`;
}
function sidebarSections() {
  revealInTree();
  const list = treeFolders();
  const favorites = workspaceNotes()
    .filter((n) => n.favorite && !n.trashed)
    .sort((a, b) => a.title.localeCompare(b.title));
  const shared = (data.shared_notes || []).filter((n) => !n.trashed);
  const tags = allTags().slice(0, 30);
  const others = [
    // Sharing spans every workspace, so it is not with this workspace's views.
    ...(localMode ? [] : [["sharing", "users", "Sharing"]]),
    ["trash", "trash", "Trash"],
    ["providers", "users", "AI providers"],
    // A desktop app's own server has no other accounts and no network clients.
    ...(localMode
      ? []
      : [
          ["integrations", "canvas", "MCP connections"],
          ...(data.user.admin
            ? [["admin", "users", "Server administration"]]
            : []),
        ]),
  ];
  return `${sidebarSection(
    "favorites",
    "Favorites",
    favorites.map((n) => noteRow(n, 0)).join("") ||
      `<p class="side-empty">${icon("star")}Star a note to keep it here.</p>`,
  )}${sidebarSection(
    "folders",
    "Folders",
    `<div class="tree" data-drop-root>${
      folderChildren(list, null)
        .map((f) => folderNode(f, list, 0))
        .join("") ||
      `<p class="side-empty">${editsWorkspace() ? "Create a folder to get started." : "Your administrator can give you access to this workspace."}</p>`
    }</div>`,
    editsWorkspace()
      ? `<button class="section-add" id="add-folder" aria-label="Create folder">${icon("plus")}</button>`
      : "",
  )}${
    shared.length
      ? sidebarSection(
          "shared",
          "Shared with me",
          shared.map((n) => noteRow(n, 0)).join(""),
        )
      : ""
  }${
    tags.length
      ? sidebarSection(
          "tags",
          "Tags",
          tags
            .map(
              ([tag, count]) =>
                `<div class="tree-row ${view === "tag" && tagFilter === tag ? "active" : ""}" style="--depth:0"><span class="tree-spacer"></span><button class="tree-label" data-open-tag="${esc(tag)}"><span class="tree-icon tag-hash" aria-hidden="true">#</span><span class="tree-name">${esc(tag)}</span><span class="count">${count}</span></button></div>`,
            )
            .join(""),
        )
      : ""
  }${sidebarSection(
    "others",
    "Others",
    `<nav class="nav" aria-label="More">${others
      .map(
        ([v, i, t]) =>
          `<button data-view="${v}" class="${view === v && !note ? "active" : ""}">${icon(i)}${t}</button>`,
      )
      .join("")}</nav>`,
  )}`;
}

// A small menu attached to a button; arrow keys, Home/End, and Escape work.
function rowMenu(anchor, items) {
  closeRowMenu();
  const menu = document.createElement("div");
  menu.className = "row-menu";
  menu.setAttribute("role", "menu");
  menu.innerHTML = items
    .map((item, i) =>
      item === "-"
        ? '<div class="menu-separator" role="separator"></div>'
        : `<button role="menuitem" tabindex="-1" data-item="${i}" class="${item.danger ? "danger-item" : ""}" ${item.disabled ? 'aria-disabled="true"' : ""}>${icon(item.icon)}<span>${esc(item.label)}</span></button>`,
    )
    .join("");
  document.body.append(menu);
  const r = anchor.getBoundingClientRect();
  menu.style.left = Math.min(r.left, innerWidth - menu.offsetWidth - 8) + "px";
  menu.style.top =
    (r.bottom + menu.offsetHeight + 6 > innerHeight
      ? r.top - menu.offsetHeight - 4
      : r.bottom + 4) + "px";
  anchor.setAttribute("aria-expanded", "true");
  anchor.closest(".tree-row")?.classList.add("menu-open");
  const buttons = $$("button", menu);
  const close = (refocus) => {
    closeRowMenu();
    if (refocus) anchor.focus();
  };
  menu.addEventListener("keydown", (e) => {
    const i = buttons.indexOf(document.activeElement);
    const move = {
      ArrowDown: i + 1,
      ArrowUp: i - 1,
      Home: 0,
      End: buttons.length - 1,
    }[e.key];
    if (move !== undefined) {
      e.preventDefault();
      buttons[(move + buttons.length) % buttons.length].focus();
    } else if (e.key === "Escape") {
      e.preventDefault();
      close(true);
    } else if (e.key === "Tab") close(false);
  });
  buttons.forEach((b) =>
    action(b, "click", async () => {
      const item = items[b.dataset.item];
      if (item.disabled) return;
      close(false);
      await item.run();
    }),
  );
  const outside = (e) => {
    if (!menu.contains(e.target) && !anchor.contains(e.target)) close(false);
  };
  setTimeout(() => document.addEventListener("mousedown", outside), 0);
  menu.cleanup = () => {
    document.removeEventListener("mousedown", outside);
    anchor.setAttribute("aria-expanded", "false");
    anchor.closest(".tree-row")?.classList.remove("menu-open");
  };
  buttons.find((b) => !b.getAttribute("aria-disabled"))?.focus();
}
function closeRowMenu() {
  const menu = $(".row-menu");
  menu?.cleanup?.();
  menu?.remove();
}

// Applies a change to a note through the normal save path, so the open editor,
// live collaboration, and revision checks all stay consistent.
async function updateNote(id, change) {
  await flush();
  const current = await api("notes/" + id);
  change(current);
  await api("notes/" + id, "PUT", current);
  await refresh();
  if (note?.id === id) await openNote(id);
  else shell();
}
async function moveNote(id, folder) {
  await flush();
  await api(`notes/${id}`, "PATCH", { folder_id: folder });
  openFolders.add(folder);
  saveSet("minerva.tree.open", openFolders);
  await refresh();
  if (note?.id === id) note.folder_id = folder;
  shell();
  toast(`Moved to ${data.folders.find((f) => f.id === folder)?.name}`);
}
async function moveFolder(id, parent) {
  await api(`folders/${id}`, "PATCH", { parent_id: parent });
  if (parent) openFolders.add(parent);
  saveSet("minerva.tree.open", openFolders);
  await refresh();
  shell();
}
// Folder options indented by depth, for "Move to…".
function folderTreeOptions(filter = () => true, selected = null) {
  const list = treeFolders();
  const walk = (parent, depth) =>
    folderChildren(list, parent).flatMap((f) => [
      ...(filter(f)
        ? [
            `<option value="${f.id}" ${f.id === selected ? "selected" : ""}>${"  ".repeat(depth)}${esc(f.name)}</option>`,
          ]
        : []),
      ...walk(f.id, depth + 1),
    ]);
  return walk(null, 0).join("");
}
function moveNoteDialog(n) {
  const options = folderTreeOptions(
    (f) => data.roles[f.id] === "editor" && f.id !== n.folder_id,
  );
  if (!options) return toast("There is no other folder you can move it to.");
  formDialog(
    `Move “${n.title}”`,
    "Choose a folder in this workspace.",
    `<label>Folder<select name="folder_id">${options}</select></label>`,
    (v) => moveNote(n.id, v.folder_id),
    "Move",
  );
}
function createFolder(parent = null) {
  const parentName = parent && data.folders.find((f) => f.id === parent)?.name;
  formDialog(
    parent ? `New folder in ${parentName}` : "A little more organized",
    "Folders keep related notes together.",
    `<label>Folder name<input name="name" placeholder="e.g. Product ideas" required maxlength="200" autofocus></label>`,
    async (v) => {
      await api("folders", "POST", {
        ...v,
        workspace_id: workspaceId,
        parent_id: parent,
      });
      if (parent) openFolders.add(parent);
      saveSet("minerva.tree.open", openFolders);
      await refresh();
      shell();
    },
    "Create folder",
  );
}
function renameFolder(id) {
  const f = data.folders.find((x) => x.id === id);
  formDialog(
    "Rename folder",
    "Give this space a useful name.",
    `<label>Name<input name="name" value="${esc(f.name)}" required maxlength="200" autofocus></label>`,
    async (v) => {
      await api("folders/" + id, "PATCH", v);
      await refresh();
      shell();
    },
    "Rename",
  );
}
async function newNoteIn(folder) {
  const n = await api("notes", "POST", {
    folder_id: folder,
    title: "Untitled",
  });
  openFolders.add(folder);
  saveSet("minerva.tree.open", openFolders);
  await refresh();
  await openNote(n.id);
  $("#note-title")?.select();
}
function folderMenu(anchor, id) {
  const f = data.folders.find((x) => x.id === id);
  const canEdit = data.roles[id] === "editor";
  rowMenu(anchor, [
    ...(canEdit
      ? [{ icon: "note", label: "New note", run: () => newNoteIn(id) }]
      : []),
    ...(editsWorkspace()
      ? [
          {
            icon: "folder",
            label: "New subfolder",
            run: () => createFolder(id),
          },
        ]
      : []),
    ...(editsWorkspace()
      ? [
          { icon: "edit", label: "Rename", run: () => renameFolder(id) },
          ...(f.parent_id
            ? [
                {
                  icon: "arrow",
                  label: "Move to top level",
                  run: () => moveFolder(id, null),
                },
              ]
            : []),
        ]
      : []),
    {
      icon: "star",
      label: "Ask AI about this folder",
      run: () => askAI({ kind: "folder", id }),
    },
    ...(canEdit
      ? [
          "-",
          {
            icon: "trash",
            label: "Delete folder",
            danger: true,
            run: () => deleteFolder(id),
          },
        ]
      : []),
  ]);
}
function noteMenu(anchor, id) {
  const n =
    data.notes.find((x) => x.id === id) ||
    data.shared_notes?.find((x) => x.id === id);
  const canEdit = (n.role || data.roles[n.folder_id]) === "editor";
  const folderEditor = data.roles[n.folder_id] === "editor";
  rowMenu(anchor, [
    { icon: "arrow", label: "Open", run: () => openNote(id) },
    ...(canEdit
      ? [
          {
            icon: "star",
            label: n.favorite ? "Remove from favorites" : "Add to favorites",
            run: () => updateNote(id, (x) => (x.favorite = !x.favorite)),
          },
        ]
      : []),
    ...(folderEditor
      ? [{ icon: "folder", label: "Move to…", run: () => moveNoteDialog(n) }]
      : []),
    {
      icon: "star",
      label: "Ask AI about this note",
      run: () => askAI({ kind: "note", id }),
    },
    ...(canEdit
      ? [
          "-",
          {
            icon: "trash",
            label: "Move to trash",
            danger: true,
            run: async () => {
              await updateNote(id, (x) => (x.trashed = true));
              toast("Moved to trash");
            },
          },
        ]
      : []),
  ]);
}
// Deleting a folder deletes everything in it, including subfolders.
function deleteFolder(id) {
  const f = data.folders.find((x) => x.id === id);
  const inside = new Set(
    data.folders
      .filter((x) => folderPath(x.id).some((p) => p.id === id))
      .map((x) => x.id),
  );
  const notes = data.notes.filter((n) => inside.has(n.folder_id)).length;
  const subfolders = inside.size - 1;
  const contents = [
    notes && `${notes} note${notes === 1 ? "" : "s"}`,
    subfolders && `${subfolders} subfolder${subfolders === 1 ? "" : "s"}`,
  ].filter(Boolean);
  formDialog(
    `Delete “${f.name}”?`,
    `This permanently deletes the folder${contents.length ? ` and ${contents.join(" and ")} in it (including trashed notes and databases), with their history, files, shares, and published pages` : ""}. This cannot be undone.`,
    contents.length
      ? `<label>Type the folder name to confirm<input name="confirm" required autocomplete="off"></label>`
      : "",
    async (v) => {
      if (contents.length && v.confirm !== f.name)
        throw new Error("Type the folder name exactly to confirm.");
      await api("folders/" + id, "DELETE");
      if (inside.has(folderId) || inside.has(note?.folder_id)) {
        note = null;
        folderId = null;
        view = "home";
      }
      await refresh();
      shell();
      toast("Folder deleted");
    },
    "Delete folder",
  );
}

// Expands or collapses a folder in place, without redrawing the sidebar.
function setFolderOpen(id, open) {
  const node = $(`[data-node="${id}"]`);
  const children = node && $(":scope > .tree-children", node);
  if (!children) return;
  children.hidden = !open;
  const toggle = $(`[data-toggle-folder="${id}"]`);
  const name = data.folders.find((f) => f.id === id)?.name || "";
  toggle.setAttribute("aria-expanded", String(open));
  toggle.setAttribute("aria-label", `${open ? "Collapse" : "Expand"} ${name}`);
  $(":scope > .tree-row .tree-icon", node).innerHTML = icon(
    open ? "folderOpen" : "folder",
  );
  if (open) openFolders.add(id);
  else openFolders.delete(id);
  saveSet("minerva.tree.open", openFolders);
}
// What is being dragged in the sidebar: {kind: "note" | "folder", id}.
let sidebarDrag = null;
function bindSidebar() {
  $$("[data-toggle-section]").forEach((b) =>
    action(b, "click", () => {
      const id = b.dataset.toggleSection;
      const body = $(`#section-${id}`);
      body.hidden = !body.hidden;
      b.setAttribute("aria-expanded", String(!body.hidden));
      if (body.hidden) collapsedSections.add(id);
      else collapsedSections.delete(id);
      saveSet("minerva.sidebar.collapsed", collapsedSections);
    }),
  );
  $$("[data-toggle-folder]").forEach((b) =>
    action(b, "click", () =>
      setFolderOpen(
        b.dataset.toggleFolder,
        !openFolders.has(b.dataset.toggleFolder),
      ),
    ),
  );
  $$(".sidebar [data-open-note]").forEach((b) =>
    action(b, "click", () => openNote(b.dataset.openNote)),
  );
  $$("[data-row-new]").forEach((b) =>
    action(b, "click", () => newNoteIn(b.dataset.rowNew)),
  );
  $$("[data-row-menu]").forEach((b) =>
    action(b, "click", () => {
      if (b.getAttribute("aria-expanded") === "true") return closeRowMenu();
      const [kind, id] = b.dataset.rowMenu.split(":");
      (kind === "folder" ? folderMenu : noteMenu)(b, id);
    }),
  );
  // Drag notes onto folders to move them; drag folders to nest them, or onto
  // the Folders heading to move them back to the top level.
  $$("[data-drag-note], [data-drag-folder]").forEach((row) => {
    row.addEventListener("dragstart", (e) => {
      sidebarDrag = row.dataset.dragNote
        ? { kind: "note", id: row.dataset.dragNote }
        : { kind: "folder", id: row.dataset.dragFolder };
      e.dataTransfer.effectAllowed = "move";
      e.dataTransfer.setData("text/plain", sidebarDrag.id);
      row.classList.add("sidebarDrag");
    });
    row.addEventListener("dragend", () => {
      row.classList.remove("sidebarDrag");
      $$(".drop-target").forEach((t) => t.classList.remove("drop-target"));
      sidebarDrag = null;
    });
  });
  const allowed = (target) => {
    if (!sidebarDrag) return false;
    if (target === null) return sidebarDrag.kind === "folder";
    if (sidebarDrag.kind === "note") {
      const n = data.notes.find((x) => x.id === sidebarDrag.id);
      return n?.folder_id !== target && data.roles[target] === "editor";
    }
    return !folderPath(target).some((f) => f.id === sidebarDrag.id);
  };
  const dropZones = [
    ...$$("[data-drop-folder]").map((el) => [el, el.dataset.dropFolder]),
    ...$$('[data-section="folders"] .section-label').map((el) => [el, null]),
  ];
  for (const [el, target] of dropZones) {
    el.addEventListener("dragover", (e) => {
      if (!allowed(target)) return;
      e.preventDefault();
      e.dataTransfer.dropEffect = "move";
      el.classList.add("drop-target");
    });
    el.addEventListener("dragleave", () => el.classList.remove("drop-target"));
    el.addEventListener("drop", (e) => {
      e.preventDefault();
      el.classList.remove("drop-target");
      if (!allowed(target)) return;
      const { kind, id } = sidebarDrag;
      (kind === "note" ? moveNote(id, target) : moveFolder(id, target)).catch(
        error,
      );
    });
  }
  // Hovering over a closed folder while dragging opens it, so deep targets are
  // reachable.
  $$("[data-drop-folder]").forEach((row) => {
    let timer;
    row.addEventListener("dragenter", () => {
      clearTimeout(timer);
      const id = row.dataset.dropFolder;
      if (sidebarDrag && !openFolders.has(id))
        timer = setTimeout(() => setFolderOpen(id, true), 700);
    });
    row.addEventListener("dragleave", () => clearTimeout(timer));
    row.addEventListener("drop", () => clearTimeout(timer));
  });
}

// The sidebar workspace switcher: a button that opens a menu styled to match it.
// The desktop app's servers, for the workspace menu of this computer's page;
// loaded when the menu opens.
let desktopServers = null;
// On a server's page inside the desktop app (the app tells it its origin).
const desktopRemote =
  !!document.querySelector('meta[name="minerva-server-origin"]') &&
  /\.localhost$/.test(location.hostname);
/**
 * The workspace menu, grouped like the places notes live: this computer, then
 * each server the desktop app knows with its workspaces; then adding a server,
 * creating, and importing.
 */
function workspaceMenuItems() {
  const w = currentWorkspace();
  const square = (name, extra = "") =>
    `<span class="square ${extra}">${esc(name.slice(0, 1))}</span>`;
  const row = (x) =>
    `<button role="menuitemradio" aria-checked="${x.id === workspaceId}" data-workspace="${x.id}" tabindex="-1">${square(x.name)}<span class="workspace-name">${esc(x.name)}</span>${x.id === workspaceId ? icon("check") : ""}</button>`;
  const heading = (title, detail = "") =>
    `<div class="menu-heading" role="presentation"><strong>${esc(title)}</strong>${detail ? `<small>${esc(detail)}</small>` : ""}</div>`;
  const offline = new Set(replicas.map((r) => r.workspace_id));
  let html = "";
  if (localMode) {
    html += heading("This computer", "Stored on this device");
    html += data.workspaces
      .filter((x) => !offline.has(x.id))
      .map(row)
      .join("");
    if (desktopServers === null)
      html += `<div class="menu-heading" role="presentation"><small>Loading servers…</small></div>`;
    for (const server of desktopServers || []) {
      html += `<div class="menu-separator" role="separator"></div>${heading(server.label, server.account || (server.signed_in ? "" : "Not signed in"))}`;
      if (server.workspaces)
        html += server.workspaces
          .map(
            (x) =>
              `<div class="menu-row" role="presentation"><button role="menuitemradio" aria-checked="${x.id === workspaceId}" data-remote="${esc(server.id)}|${esc(x.id)}" tabindex="-1">${square(x.name)}<span class="workspace-name">${esc(x.name)}</span>${x.id === workspaceId ? icon("check") : ""}</button><button class="menu-icon" data-offline="${esc(server.id)}|${esc(x.id)}|${x.offline ? "off" : "on"}" tabindex="-1" title="${x.offline ? "Remove the offline copy" : "Keep an offline copy on this computer"}" aria-label="${x.offline ? "Remove offline copy of" : "Keep offline copy of"} ${esc(x.name)}">${icon(x.offline ? "check" : "download")}</button></div>`,
          )
          .join("");
      else
        html += `<button role="menuitem" data-remote="${esc(server.id)}|" tabindex="-1"><span class="square new">${icon("arrow")}</span><span class="workspace-name">${server.state === "signed_out" ? "Sign in to see its workspaces" : "Can’t reach this server · open it"}</span></button>`;
    }
    html += `<div class="menu-separator" role="separator"></div><button role="menuitem" data-workspace="add-server" tabindex="-1"><span class="square new">${icon("plus")}</span><span class="workspace-name">Add server</span></button>`;
  } else {
    html += heading(desktopRemote ? "This server" : location.host);
    html += data.workspaces.map(row).join("");
    if (desktopRemote)
      html += `<div class="menu-separator" role="separator"></div><button role="menuitem" data-workspace="this-computer" tabindex="-1"><span class="square new">${icon("home")}</span><span class="workspace-name">This computer</span></button><button role="menuitem" data-workspace="all-servers" tabindex="-1"><span class="square new">${icon("list")}</span><span class="workspace-name">All servers…</span></button>`;
  }
  html += `<div class="menu-separator" role="separator"></div>`;
  if (data.user.can_create_workspace || data.user.admin || localMode)
    html += `<button role="menuitem" data-workspace="new" tabindex="-1"><span class="square new">${icon("plus")}</span><span class="workspace-name">Create workspace</span></button>`;
  html += `<button role="menuitem" data-workspace="import" tabindex="-1"><span class="square new">${icon("upload")}</span><span class="workspace-name">Import Markdown</span></button>`;
  if (w?.can_delete)
    html += `<div class="menu-separator" role="separator"></div><button role="menuitem" class="danger-item" data-workspace="delete" tabindex="-1"><span class="square danger">${icon("trash")}</span><span class="workspace-name">Delete this workspace…</span></button>`;
  return html;
}
/** Calls the desktop app (this computer's page only). */
async function desktop(path, method = "GET", body) {
  const r = await fetch("/desktop/" + path, {
    method,
    headers: { "Content-Type": "application/json", "X-Minerva-Client": "1" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const v = await r.json().catch(() => ({}));
  if (!r.ok) throw new Error(v.error || "The desktop app could not do that");
  return v;
}
function addServer() {
  formDialog(
    "Add a server",
    "The address of a Minerva server. Sign in on its page; then its workspaces appear in this menu.",
    '<label>Address<input name="url" required placeholder="https://notes.example.com" inputmode="url" spellcheck="false"></label><label>Name <small class="hint">(optional)</small><input name="label" maxlength="80" placeholder="Team notes"></label><label class="checkbox-label"><input type="checkbox" name="allow_insecure_http">Allow unencrypted HTTP on this address (trusted networks only)</label>',
    async (v) => {
      const server = await desktop("servers", "POST", {
        url: v.url,
        label: v.label,
        allow_insecure_http: v.allow_insecure_http === "on",
      });
      desktopServers = null;
      toast(`Added ${server.label}`);
    },
    "Add server",
  );
}
function workspaceMenu() {
  const button = $("#workspace"),
    menu = $("#workspace-menu");
  const items = () =>
    $$("[data-workspace], [data-remote], [data-offline]", menu);
  const close = (refocus = false) => {
    menu.hidden = true;
    button.setAttribute("aria-expanded", "false");
    document.removeEventListener("mousedown", outside);
    if (refocus) button.focus();
  };
  const outside = (e) => {
    if (!menu.contains(e.target) && !button.contains(e.target)) close();
  };
  const redraw = () => {
    const focused = document.activeElement?.dataset?.workspace;
    menu.innerHTML = workspaceMenuItems();
    bind();
    if (focused)
      menu.querySelector(`[data-workspace="${CSS.escape(focused)}"]`)?.focus();
  };
  const open = (focus = true, focusLast = false) => {
    menu.hidden = false;
    if (localMode)
      desktop("servers")
        .then((list) => {
          desktopServers = list;
          if (!menu.hidden) redraw();
        })
        .catch(() => {
          desktopServers = [];
          if (!menu.hidden) redraw();
        });
    button.setAttribute("aria-expanded", "true");
    document.addEventListener("mousedown", outside);
    // Pointer users keep focus on the button; keyboard users move into the menu.
    if (!focus) return;
    const list = items();
    (focusLast
      ? list.at(-1)
      : list.find((b) => b.getAttribute("aria-checked") === "true") || list[0]
    )?.focus();
  };
  button.addEventListener("click", (e) =>
    menu.hidden ? open(e.detail === 0) : close(),
  );
  button.addEventListener("keydown", (e) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      open(true, e.key === "ArrowUp");
    }
  });
  menu.addEventListener("keydown", (e) => {
    const list = items(),
      i = list.indexOf(document.activeElement);
    const move = {
      ArrowDown: i + 1,
      ArrowUp: i - 1,
      Home: 0,
      End: list.length - 1,
    }[e.key];
    if (move !== undefined) {
      e.preventDefault();
      list[(move + list.length) % list.length]?.focus();
    } else if (e.key === "Escape") {
      e.preventDefault();
      close(true);
    } else if (e.key === "Tab") close();
  });
  const bind = () => {
    $$("[data-offline]", menu).forEach((b) =>
      action(b, "click", async () => {
        const [server, workspace, mode] = b.dataset.offline.split("|");
        await desktop("offline", "POST", {
          id: server,
          workspace_id: workspace,
          enabled: mode === "on",
        });
        toast(
          mode === "on"
            ? "Copying to this computer… It appears here in a moment."
            : "Offline copy removed",
        );
        replicas = await api("local/replicas").catch(() => replicas);
        desktopServers = await desktop("servers").catch(() => desktopServers);
        redraw();
      }),
    );
    $$("[data-remote]", menu).forEach((b) =>
      action(b, "click", async () => {
        const [server, workspace] = b.dataset.remote.split("|");
        close();
        // An offline copy opens here; otherwise the server's own page.
        if (
          workspace &&
          replicas.some((r) => r.workspace_id === workspace) &&
          data.workspaces.some((x) => x.id === workspace)
        ) {
          await flush();
          stopLive();
          workspaceId = workspace;
          note = null;
          folderId = null;
          view = "home";
          shell();
          return;
        }
        const target = (desktopServers || []).find((x) => x.id === server);
        if (!target) return;
        await flush();
        location.href =
          target.open +
          (workspace ? `?workspace=${encodeURIComponent(workspace)}` : "");
      }),
    );
    $$("[data-workspace]", menu).forEach((b) =>
      action(b, "click", async () => {
        close();
        await flush();
        if (b.dataset.workspace === "add-server") return addServer();
        if (b.dataset.workspace === "import") return importMarkdown();
        if (b.dataset.workspace === "this-computer")
          // Built from the host: custom schemes may report a "null" origin.
          return (location.href = `${location.protocol}//${location.host.replace(/s[a-z0-9]+\.localhost$/, "local.localhost")}/`);
        if (b.dataset.workspace === "all-servers")
          return window.ipc?.postMessage('{"cmd":"show_servers"}');
        if (b.dataset.workspace === "new") return createWorkspace();
        if (b.dataset.workspace === "delete")
          return deleteWorkspace(currentWorkspace());
        if (b.dataset.workspace === workspaceId) return button.focus();
        stopLive();
        clearInterval(dbTimer);
        activeDatabase = null;
        noteTabs?.hide();
        workspaceId = b.dataset.workspace;
        note = null;
        folderId = null;
        view = "home";
        shell();
        $("#workspace")?.focus();
      }),
    );
  };
  bind();
}
function search() {
  if (!data) return;
  modal.innerHTML =
    '<h2>Find a thought</h2><p>Search titles and note content across your accessible workspaces.</p><input id="search-input" aria-label="Search notes" placeholder="Search notes…" style="width:100%" autofocus><div class="search-results" id="results"></div><footer><button class="btn" id="close-modal">Close</button></footer>';
  modal.showModal();
  const input = $("#search-input");
  let searchVersion = 0;
  async function results() {
    const version = ++searchVersion;
    const q = input.value.toLowerCase();
    const matches = await api("search", "POST", { query: q });
    if (version !== searchVersion || !modal.open) return;
    $("#results").innerHTML =
      matches
        .map(
          (n) =>
            `<button class="search-result" data-result="${n.id}"><span>${esc(n.icon)}</span>${esc(n.title)}<small>${esc(data.folders.find((f) => f.id === n.folder_id)?.name || "Shared with you")}</small></button>`,
        )
        .join("") || "<p>No matching notes.</p>";
    $$("[data-result]").forEach((b) =>
      action(b, "click", async () => {
        await flush();
        const n =
          data.notes.find((n) => n.id === b.dataset.result) ||
          data.shared_notes?.find((n) => n.id === b.dataset.result);
        workspaceId =
          data.folders.find((f) => f.id === n?.folder_id)?.workspace_id ||
          workspaceId;
        modal.close();
        await openNote(n.id);
      }),
    );
  }
  let timer;
  input.oninput = () => {
    clearTimeout(timer);
    timer = setTimeout(() => results().catch(error), 180);
  };
  results().catch(error);
  input.focus();
  $("#close-modal").onclick = () => modal.close();
}
async function renderAdmin() {
  const [info, storage] = await Promise.all([
    api("admin"),
    api("admin/storage"),
  ]);
  $("#breadcrumb").textContent = "Server administration";
  $("#content").innerHTML =
    `<section class="page"><div class="eyebrow">YOUR SERVER, YOUR RULES</div><div class="page-heading"><div><h1>People & permissions</h1><p class="subtitle">Give your team the right space to do their best work.</p></div><button class="btn primary" id="add-user">${icon("plus")}Add user</button></div><div class="admin-grid"><section class="panel"><div class="panel-heading"><div><h2>Server members</h2><p>${info.users.length} accounts on this server</p></div><span class="pill">Self-hosted</span></div><div class="table-wrap"><table class="admin-table"><thead><tr><th>NAME</th><th>ROLE</th><th>STATUS</th><th>2FA</th><th></th></tr></thead><tbody>${info.users.map((u) => `<tr><td>${esc(u.name)}<small>${esc(u.email)}</small></td><td>${u.admin ? "Administrator" : "Member"}</td><td>${u.admin ? '<span class="pill">Active</span>' : `<select data-status-user="${u.id}" aria-label="Account status for ${esc(u.name)}"><option value="active" ${!u.disabled ? "selected" : ""}>Active</option><option value="disabled" ${u.disabled ? "selected" : ""}>Disabled</option></select>`}</td><td>${u.two_factor ? `<span class="pill pill-on">On</span>${u.id !== data.user.id ? ` <button class="btn" data-reset-2fa="${u.id}" aria-label="Reset two-factor authentication for ${esc(u.name)}">Reset</button>` : ""}` : '<span class="pill">Off</span>'}</td><td class="user-actions">${u.id !== data.user.id ? `<button class="btn" data-edit-user="${u.id}" aria-label="Edit ${esc(u.name)}">Edit</button><button class="btn danger" data-delete-user="${u.id}" aria-label="Delete ${esc(u.name)}">Delete</button>` : "<small>You</small>"}</td></tr>`).join("")}</tbody></table></div></section></div>${storagePanel(storage)}</section>`;
  action($("#add-user"), "click", () =>
    formDialog(
      "Welcome someone new",
      mailEnabled
        ? "They get an email with a link to choose their own password. You can also set a temporary one to share with them yourself."
        : "Create an account and share its credentials with your teammate securely.",
      `<label>Full name<input name="name" required maxlength="200" autocomplete="off"></label><label>Email<input type="email" name="email" required autocomplete="off"></label><label>Temporary password${mailEnabled ? " (optional)" : ""}<input type="password" name="password" minlength="12" maxlength="256" autocomplete="new-password" ${mailEnabled ? "" : "required"}></label>`,
      async (v) => {
        const created = await api("users", "POST", {
          ...v,
          can_create_workspace: true,
        });
        if (created.invited) toast(`Invitation emailed to ${created.email}`);
        await renderAdmin();
      },
      "Create account",
    ),
  );
  $$("[data-create-user]").forEach((el) =>
    action(el, "change", async () => {
      try {
        await api("users/" + el.dataset.createUser, "PATCH", {
          can_create_workspace: el.checked,
        });
        toast("Workspace permission updated");
      } catch (e) {
        el.checked = !el.checked;
        throw e;
      }
    }),
  );
  $$("[data-edit-user]").forEach((el) =>
    action(el, "click", () => {
      const u = info.users.find((x) => x.id === el.dataset.editUser);
      formDialog(
        `Edit ${u.name}`,
        "Changes sign this person out of their open sessions.",
        `<label>Full name<input name="name" required maxlength="200" value="${esc(u.name)}" autocomplete="off"></label><label>Email<input type="email" name="email" required value="${esc(u.email)}" autocomplete="off"></label><label>New password <small class="hint">(leave empty to keep it)</small><input type="password" name="password" minlength="12" maxlength="256" autocomplete="new-password"></label><label class="checkbox-label"><input type="checkbox" name="admin" ${u.admin ? "checked" : ""}>Server administrator</label>`,
        async (v) => {
          const changes = {
            name: v.name,
            email: v.email,
            admin: v.admin === "on",
          };
          if (v.password) changes.password = v.password;
          await api("users/" + u.id, "PATCH", changes);
          toast("Account updated");
          await renderAdmin();
        },
        "Save",
      );
    }),
  );
  $$("[data-delete-user]").forEach((el) =>
    action(el, "click", () => {
      const u = info.users.find((x) => x.id === el.dataset.deleteUser);
      formDialog(
        `Delete ${u.name}?`,
        `${u.email} can no longer sign in. Their personal workspaces are deleted with them; shared workspaces pass to the member you select below. Their AI conversations, flashcards, and access are deleted too. To keep everything instead, set the account to Disabled. This cannot be undone.`,
        `<label>Successor for shared workspaces<select name="successor_id"><option value="">Choose if this user owns shared workspaces</option>${info.users
          .filter((x) => !x.admin && !x.disabled && x.id !== u.id)
          .map(
            (x) =>
              `<option value="${x.id}">${esc(x.name)} · ${esc(x.email)}</option>`,
          )
          .join("")}</select></label>`,
        async (values) => {
          const result = await api("users/" + u.id, "DELETE", values);
          toast(
            result.workspaces_transferred
              ? `Account deleted · ${result.workspaces_transferred} shared workspace${result.workspaces_transferred === 1 ? "" : "s"} transferred`
              : "Account deleted",
          );
          await refresh();
          await renderAdmin();
        },
        "Delete account",
      );
    }),
  );
  $$("[data-reset-2fa]").forEach((el) =>
    action(el, "click", () =>
      formDialog(
        "Reset two-factor authentication?",
        "Use this when someone lost their authenticator app. They will sign in with their password only until they set it up again.",
        "",
        async () => {
          await api("users/" + el.dataset.reset2fa, "PATCH", {
            two_factor: false,
          });
          toast("Two-factor authentication was reset");
          await renderAdmin();
        },
        "Reset",
      ),
    ),
  );
  $$("[data-status-user]").forEach((el) =>
    action(el, "change", async () => {
      try {
        await api("users/" + el.dataset.statusUser, "PATCH", {
          disabled: el.value === "disabled",
        });
        toast("Account updated");
      } catch (e) {
        await renderAdmin();
        throw e;
      }
    }),
  );
  bindStorage(storage);
  action($("#workspace-access-form"), "submit", async (e) => {
    e.preventDefault();
    await api(
      "workspace-access",
      "PUT",
      Object.fromEntries(new FormData(e.target)),
    );
    await renderAdmin();
    toast("Workspace access saved");
  });
  action($("#add-workspace"), "click", () =>
    formDialog(
      "Add a workspace",
      "A place to share with others. Add people to it in Workspace access.",
      '<label>Name<input name="name" required maxlength="200" placeholder="e.g. Research team"></label>',
      async (v) => {
        await api("workspaces", "POST", { name: v.name });
        await refresh();
        await renderAdmin();
        toast("Workspace added");
      },
      "Add workspace",
    ),
  );
  $$("[data-rename-workspace]").forEach((b) =>
    action(b, "click", () => {
      const w = data.workspaces.find((x) => x.id === b.dataset.renameWorkspace);
      formDialog(
        "Rename workspace",
        "Everyone with access sees the new name.",
        `<label>Name<input name="name" required maxlength="200" value="${esc(w.name)}"></label>`,
        async (v) => {
          await api("workspaces/" + w.id, "PATCH", { name: v.name });
          await refresh();
          await renderAdmin();
          toast("Workspace renamed");
        },
        "Rename",
      );
    }),
  );
  $$("[data-delete-workspace]").forEach((b) =>
    action(b, "click", () =>
      deleteWorkspace(
        data.workspaces.find((w) => w.id === b.dataset.deleteWorkspace),
        renderAdmin,
      ),
    ),
  );
  $$("[data-remove-access]").forEach((b) =>
    action(b, "click", async () => {
      const [workspace_id, user_id] = b.dataset.removeAccess.split(":");
      await api("workspace-access", "PUT", {
        workspace_id,
        user_id,
        role: "none",
      });
      await renderAdmin();
      toast("Workspace access removed");
    }),
  );
  $$("[data-remove-folder-grant]").forEach((b) =>
    action(b, "click", async () => {
      const [folder_id, user_id] = b.dataset.removeFolderGrant.split(":");
      await api("permissions", "PUT", { folder_id, user_id, role: "none" });
      await renderAdmin();
      toast("Folder access removed");
    }),
  );
}
// Every workspace on the server, for administrators to review and delete.
function workspacesPanel(info) {
  const name = (id) =>
    esc(info.users.find((u) => u.id === id)?.name || "Unavailable account");
  // Administrators manage only the workspaces they own.
  const mine = data.workspaces.filter((w) => w.owner_id === data.user.id);
  const rows = mine
    .map((w) => {
      const folderIds = data.folders
        .filter((f) => f.workspace_id === w.id)
        .map((f) => f.id);
      const members = new Set([
        ...info.workspace_grants
          .filter((g) => g.workspace_id === w.id)
          .map((g) => g.user_id),
        ...info.grants
          .filter((g) => folderIds.includes(g.folder_id))
          .map((g) => g.user_id),
      ]);
      members.delete(w.owner_id);
      const notes = data.notes.filter((n) =>
        folderIds.includes(n.folder_id),
      ).length;
      return `<tr><td>${esc(w.name)}</td><td>${name(w.owner_id)}</td><td>${w.personal ? '<span class="pill">Personal · private</span>' : members.size ? `${members.size} member${members.size === 1 ? "" : "s"}` : "Not shared yet"}</td><td>${folderIds.length} folder${folderIds.length === 1 ? "" : "s"} · ${notes} note${notes === 1 ? "" : "s"}</td><td class="user-actions">${w.personal ? "" : `<button class="btn" data-rename-workspace="${w.id}">Rename</button><button class="btn danger" data-delete-workspace="${w.id}">Delete</button>`}</td></tr>`;
    })
    .join("");
  return `<section class="panel"><div class="panel-heading"><div><h2>Workspaces</h2><p>Workspaces you own. Share them in Workspace access. Your My Workspace is personal and cannot be shared; other people’s workspaces stay private to them.</p></div><button class="btn primary" id="add-workspace">${icon("plus")}Add workspace</button></div><div class="table-wrap"><table class="admin-table"><thead><tr><th>WORKSPACE</th><th>OWNER</th><th>SHARED WITH</th><th>CONTENTS</th><th></th></tr></thead><tbody>${rows || '<tr><td colspan="5">No workspaces yet.</td></tr>'}</tbody></table></div></section>`;
}
// Workspace access for members, plus any older folder-level grants.
function accessPanel(info) {
  const name = (id) =>
    esc(info.users.find((u) => u.id === id)?.name || "Unavailable account");
  const workspace = (id) =>
    esc(
      data.workspaces.find((w) => w.id === id)?.name || "Unavailable workspace",
    );
  // Everyone else, administrators included: nobody sees a workspace unasked.
  const members = info.users.filter((u) => u.id !== data.user.id);
  // Administrators manage only the workspaces they own.
  const mine = data.workspaces.filter(
    (w) => w.owner_id === data.user.id && !w.personal,
  );
  const rows = mine
    .flatMap((w) => [
      ...(info.users.some((u) => u.id === w.owner_id && !u.admin)
        ? [
            `<tr><td>${name(w.owner_id)}</td><td>${esc(w.name)}</td><td><span class="pill">Owner</span></td><td></td></tr>`,
          ]
        : []),
      ...info.workspace_grants
        .filter((g) => g.workspace_id === w.id)
        .map(
          (g) =>
            `<tr><td>${name(g.user_id)}</td><td>${esc(w.name)}</td><td><span class="pill">${g.role === "editor" ? "Can edit" : "Can view"}</span></td><td><button class="btn danger" data-remove-access="${g.workspace_id}:${g.user_id}">Remove</button></td></tr>`,
        ),
    ])
    .join("");
  const folderGrants = info.grants
    .map((g) => {
      const f = data.folders.find((x) => x.id === g.folder_id);
      return `<tr><td>${name(g.user_id)}</td><td>${f ? `${workspace(f.workspace_id)} / ${esc(f.name)}` : "Unavailable folder"}</td><td><span class="pill">${g.role === "editor" ? "Can edit" : "Can view"}</span></td><td><button class="btn danger" data-remove-folder-grant="${g.folder_id}:${g.user_id}">Remove</button></td></tr>`;
    })
    .join("");
  return `<section class="panel"><div class="panel-heading"><div><h2>Workspace access</h2><p>Share the workspaces you own. Nobody, administrators included, sees a workspace until its owner adds them. Workspace members can open every folder and note in it, including folders added later.</p></div></div><form class="permission-form" id="workspace-access-form"><label>Member<select name="user_id">${members.map((u) => `<option value="${u.id}">${esc(u.name)}</option>`).join("")}</select></label><label>Workspace<select name="workspace_id">${mine.map((w) => `<option value="${w.id}">${esc(w.name)}</option>`).join("")}</select></label><label>Permission<select name="role"><option value="editor" selected>Can edit</option><option value="viewer">Can view</option><option value="none">Remove access</option></select></label><button class="btn primary" ${!members.length || !mine.length ? "disabled" : ""}>Save access</button></form><div class="table-wrap"><table class="admin-table"><thead><tr><th>MEMBER</th><th>WORKSPACE</th><th>ACCESS</th><th></th></tr></thead><tbody>${rows || '<tr><td colspan="4">No members have been added to a workspace yet.</td></tr>'}</tbody></table></div>${folderGrants ? `<h3 class="subhead">Folder-level access</h3><p class="hint">Older grants to single folders still apply. Workspace access is simpler to manage; remove these once members have workspace access.</p><div class="table-wrap"><table class="admin-table"><thead><tr><th>MEMBER</th><th>FOLDER</th><th>ACCESS</th><th></th></tr></thead><tbody>${folderGrants}</tbody></table></div>` : ""}</section>`;
}
function storagePanel(storage) {
  const percent = Math.min(100, (storage.used / storage.server_quota) * 100);
  return `<section class="panel storage-panel"><div class="panel-heading"><div><h2>Attachment storage</h2><p>${bytes(storage.used)} of ${bytes(storage.server_quota)} in ${storage.files} files · ${bytes(storage.note_quota)} per note · ${bytes(storage.file_limit)} per file</p></div></div><div class="usage-bar" role="meter" aria-label="Attachment storage used" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${percent.toFixed(0)}"><span style="width:${percent}%"></span></div><div class="table-wrap"><table class="admin-table"><thead><tr><th>NOTE</th><th>FILES</th><th>SIZE</th></tr></thead><tbody>${
    storage.notes
      .slice(0, 8)
      .map(
        (n) =>
          `<tr><td>${esc(n.title)}${n.trashed ? " <small>(in trash)</small>" : ""}</td><td>${n.files}</td><td>${bytes(n.bytes)}</td></tr>`,
      )
      .join("") || '<tr><td colspan="3">No attachments yet.</td></tr>'
  }</tbody></table></div><h3>Unreferenced files</h3><p>Files not referenced by any note, saved version (history), or published page, and older than ${Math.round(storage.grace_seconds / 3600)} hours. Deleting them is permanent.${storage.recent_unreferenced ? ` ${storage.recent_unreferenced} newer unreferenced upload${storage.recent_unreferenced === 1 ? " is" : "s are"} kept so interrupted insertions can be recovered.` : ""}</p><form id="cleanup-form"><div class="table-wrap"><table class="admin-table"><thead><tr><th><input type="checkbox" id="select-orphans" aria-label="Select all unreferenced files"></th><th>FILE</th><th>NOTE</th><th>UPLOADED</th><th>SIZE</th></tr></thead><tbody>${storage.orphans.map((o) => `<tr><td><input type="checkbox" name="orphan" value="${o.id}" aria-label="Select ${esc(o.filename)}"></td><td>${esc(o.filename)}</td><td>${esc(o.note_title || "Unavailable note")}</td><td>${date(o.created_at)}</td><td>${bytes(o.size)}</td></tr>`).join("") || '<tr><td colspan="5">Nothing to clean up.</td></tr>'}</tbody></table></div>${storage.orphans.length ? '<button class="btn danger" id="delete-orphans" disabled>Delete selected files</button>' : ""}</form></section>`;
}
function bindStorage(storage) {
  const boxes = () => $$("[name=orphan]");
  const update = () => {
    const button = $("#delete-orphans");
    if (button) button.disabled = !boxes().some((b) => b.checked);
  };
  boxes().forEach((b) => b.addEventListener("change", update));
  action($("#select-orphans"), "change", (e) => {
    boxes().forEach((b) => (b.checked = e.target.checked));
    update();
  });
  action($("#cleanup-form"), "submit", (e) => {
    e.preventDefault();
    const ids = boxes()
      .filter((b) => b.checked)
      .map((b) => b.value);
    const size = storage.orphans
      .filter((o) => ids.includes(o.id))
      .reduce((total, o) => total + o.size, 0);
    formDialog(
      `Delete ${ids.length} file${ids.length === 1 ? "" : "s"}?`,
      `This permanently frees ${bytes(size)}. Files that became referenced since this list loaded are kept.`,
      "",
      async () => {
        const result = await api("admin/storage/cleanup", "POST", { ids });
        toast(
          `Deleted ${result.deleted} file${result.deleted === 1 ? "" : "s"} (${bytes(result.bytes)})${result.kept ? ` · kept ${result.kept} still in use` : ""}`,
        );
        await renderAdmin();
      },
      "Delete permanently",
    );
  });
}
let dbWriteChain = Promise.resolve();
const dbSaves = new Set(),
  dbDrafts = new Map();
let activeDatabase = null,
  dbFilter = "",
  dbSort = null,
  dbDescending = false,
  dbView = "table",
  showArchived = false,
  dbTimer = null;
function canEditDatabase() {
  return activeDatabase && data.roles[activeDatabase.folder_id] === "editor";
}
function renderDatabaseList() {
  clearInterval(dbTimer);
  activeDatabase = null;
  $("#breadcrumb").textContent = "Databases";
  const ids = new Set(folders().map((f) => f.id));
  const databases = (data.databases || []).filter((d) => ids.has(d.folder_id));
  $("#content").innerHTML =
    `<section class="page"><div class="eyebrow">A LITTLE STRUCTURE, A LOT OF POSSIBILITY</div><div class="page-heading"><div><h1>Your databases</h1><p class="subtitle">Turn projects, reading lists, and team knowledge into something organized.</p></div><button class="btn primary" id="new-database" ${!folderOptions() ? "disabled" : ""}>${icon("plus")}New database</button></div><div class="folder-grid">${databases.map((db) => `<button class="folder-card" data-database="${db.id}"><span class="folder-icon">${icon("grid")}</span><div><strong>${esc(db.name)}</strong><small>${db.record_count} records · ${esc(data.folders.find((f) => f.id === db.folder_id)?.name)}</small></div></button>`).join("")}</div>${!databases.length ? '<div class="empty"><h3>A home for the details.</h3>Create a database with text, numbers, dates, checkboxes, links, and select properties.</div>' : ""}</section>`;
  action($("#new-database"), "click", () =>
    formDialog(
      "Create a database",
      "Start with a name and status. Add the properties your work needs.",
      `<label>Name<input name="name" required maxlength="200" placeholder="e.g. Product roadmap"></label><label>Folder<select name="folder_id">${folderOptions()}</select></label>`,
      async (v) => {
        const db = await api("databases", "POST", v);
        await refresh();
        await openDatabase(db.id);
      },
      "Create database",
    ),
  );
  $$("[data-database]").forEach((el) =>
    action(el, "click", () => openDatabase(el.dataset.database)),
  );
}
async function openDatabase(id) {
  noteTabs?.hide();
  stopAI();
  await flush();
  stopLive();
  note = null;
  view = "databases";
  activeDatabase = await api("databases/" + id);
  dbFilter = "";
  dbSort = null;
  showArchived = false;
  dbView = "table";
  renderDatabase();
  clearInterval(dbTimer);
  dbTimer = setInterval(async () => {
    if (
      !activeDatabase ||
      view !== "databases" ||
      modal.open ||
      document.activeElement?.closest(
        ".database-page input,.database-page select",
      ) ||
      dbDrafts.size ||
      dbSaves.size
    )
      return;
    try {
      const fresh = await api("databases/" + id);
      if (
        activeDatabase?.id === id &&
        view === "databases" &&
        !modal.open &&
        !dbDrafts.size &&
        !dbSaves.size &&
        JSON.stringify(activeDatabase) !== JSON.stringify(fresh)
      ) {
        activeDatabase = fresh;
        renderDatabase();
      }
    } catch (e) {
      clearInterval(dbTimer);
      error(e);
    }
  }, 2500);
}
function cellInput(col, value, key, disabled = false) {
  const attr = `data-cell="${esc(key)}" aria-label="${esc(col.name)}" ${disabled ? "disabled" : ""}`;
  if (col.kind === "checkbox")
    return `<input type="checkbox" ${attr} ${value === true ? "checked" : ""}>`;
  if (col.kind === "select")
    return `<select ${attr}><option value="">—</option>${col.options.map((o) => `<option ${value === o ? "selected" : ""}>${esc(o)}</option>`).join("")}</select>`;
  return `<input ${attr} type="${{ number: "number", date: "date", url: "url" }[col.kind] || "text"}" ${col.kind === "number" ? 'step="any"' : ""} value="${esc(value ?? "")}" placeholder="—">`;
}
function parseCell(col, el) {
  if (col.kind === "checkbox") return el.checked;
  if (el.value === "") return null;
  return col.kind === "number" ? Number(el.value) : el.value;
}
function renderDatabase() {
  const db = activeDatabase;
  if (!db) return;
  const edit = canEditDatabase();
  $("#breadcrumb").textContent = db.name;
  let rows = db.rows
    .filter((r) => r.archived === showArchived)
    .filter(
      (r) =>
        !dbFilter ||
        Object.values(r.cells).some((v) =>
          String(v ?? "")
            .toLowerCase()
            .includes(dbFilter.toLowerCase()),
        ),
    );
  if (dbSort)
    rows.sort((a, b) => {
      const av = a.cells[dbSort] ?? "",
        bv = b.cells[dbSort] ?? "";
      return (
        (typeof av === "number" && typeof bv === "number"
          ? av - bv
          : String(av).localeCompare(String(bv), undefined, {
              numeric: true,
            })) * (dbDescending ? -1 : 1)
      );
    });
  const group = db.columns.find((c) => c.kind === "select");
  $("#content").innerHTML =
    `<section class="page database-page"><div class="eyebrow">STRUCTURED KNOWLEDGE</div><div class="page-heading"><div><h1>${esc(db.name)}</h1><p class="subtitle">${esc(data.folders.find((f) => f.id === db.folder_id)?.name)} · ${rows.length} records · ${edit ? "Can edit" : "Read only"}</p></div><div class="toolbar-group"><button class="btn" id="add-property" ${!edit ? "disabled" : ""}>${icon("plus")}Property</button><button class="btn primary" id="add-record" ${!edit ? "disabled" : ""}>${icon("plus")}New record</button></div></div><div class="db-toolbar"><div class="toolbar-group"><button class="btn ${dbView === "table" ? "selected" : ""}" id="table-view">${icon("list")}Table</button><button class="btn ${dbView === "board" ? "selected" : ""}" id="board-view" ${!group ? "disabled" : ""}>${icon("grid")}Board</button></div><input id="db-filter" aria-label="Filter records" placeholder="Filter records…" value="${esc(dbFilter)}"><label class="checkbox-label"><input id="show-archived" type="checkbox" ${showArchived ? "checked" : ""}>Archived</label><button class="btn" id="db-reload">Reload records</button><button class="btn" id="db-rename" ${!edit ? "disabled" : ""}>Rename</button><button class="btn" id="db-embed" title="Copy the code that shows this database inside a note">${icon("note")}Use in a note</button><button class="btn danger" id="db-delete" ${!edit ? "disabled" : ""}>${icon("trash")}Delete</button></div>${
      dbView === "table"
        ? `<div class="panel table-wrap"><table class="records-table"><thead><tr>${db.columns.map((c) => `<th><span class="th-inner"><button data-sort="${c.id}">${esc(c.name)} <small>${esc(c.kind)}</small>${dbSort === c.id ? (dbDescending ? " ↓" : " ↑") : ""}</button><button class="th-menu" data-col-menu="${c.id}" aria-label="Options for ${esc(c.name)}" aria-haspopup="menu">${icon("more")}</button></span></th>`).join("")}<th>Actions</th></tr></thead><tbody>${rows.map((r) => `<tr data-row="${r.id}">${db.columns.map((c) => `<td>${cellInput(c, r.cells[c.id], r.id + ":" + c.id, !edit)}</td>`).join("")}<td class="record-actions"><button class="btn" data-archive="${r.id}" ${!edit ? "disabled" : ""}>${showArchived ? "Restore" : "Archive"}</button><button class="btn danger" data-delete-record="${r.id}" ${!edit ? "disabled" : ""} aria-label="Delete record">${icon("trash")}</button></td></tr>`).join("")}</tbody></table>${!rows.length ? '<div class="empty">No matching records. Add a record or change your filter.</div>' : ""}</div>`
        : `<div class="board">${["", ...group.options]
            .map(
              (option) =>
                `<section class="board-column"><h3>${esc(option || "No status")}<span>${rows.filter((r) => (r.cells[group.id] || "") === option).length}</span></h3>${rows
                  .filter((r) => (r.cells[group.id] || "") === option)
                  .map(
                    (r) =>
                      `<button class="record-card" data-edit-record="${r.id}"><strong>${esc(r.cells[db.columns[0].id] || "Untitled")}</strong>${db.columns
                        .slice(1)
                        .filter((c) => c.id !== group.id)
                        .map(
                          (c) =>
                            `<small>${esc(c.name)}: ${esc(r.cells[c.id] ?? "—")}</small>`,
                        )
                        .join("")}</button>`,
                  )
                  .join("")}</section>`,
            )
            .join("")}</div>`
    }<p class="editor-help">Changes save when you leave a cell. Other records refresh automatically. Concurrent edits to the same record are checked before saving.</p></section>`;
  action($("#db-filter"), "input", (e) => {
    const pos = e.target.selectionStart;
    dbFilter = e.target.value;
    renderDatabase();
    $("#db-filter").focus();
    $("#db-filter").setSelectionRange(pos, pos);
  });
  action($("#show-archived"), "change", (e) => {
    showArchived = e.target.checked;
    renderDatabase();
  });
  $$("[data-sort]").forEach((el) =>
    action(el, "click", () => {
      if (dbSort === el.dataset.sort) dbDescending = !dbDescending;
      else {
        dbSort = el.dataset.sort;
        dbDescending = false;
      }
      renderDatabase();
    }),
  );
  action($("#table-view"), "click", () => {
    dbView = "table";
    renderDatabase();
  });
  action($("#board-view"), "click", () => {
    dbView = "board";
    renderDatabase();
  });
  action($("#db-reload"), "click", () =>
    formDialog(
      "Reload records?",
      dbDrafts.size
        ? "Unsaved cell values will be discarded. Copy any value you need before continuing."
        : "Fetch the latest records from the server.",
      "",
      async () => {
        await Promise.all([...dbSaves]);
        dbDrafts.clear();
        activeDatabase = await api("databases/" + db.id);
        renderDatabase();
      },
      "Reload",
    ),
  );
  $$("[data-cell]").forEach((el) => {
    const [rid, cid] = el.dataset.cell.split(":");
    const col = db.columns.find((c) => c.id === cid);
    el.addEventListener("input", () =>
      dbDrafts.set(el.dataset.cell, parseCell(col, el)),
    );
    const event = ["checkbox", "select"].includes(col.kind) ? "change" : "blur";
    el.addEventListener(event, () => {
      const row = db.rows.find((r) => r.id === rid),
        value = parseCell(col, el);
      if (JSON.stringify(value) === JSON.stringify(row.cells[cid] ?? null)) {
        dbDrafts.delete(el.dataset.cell);
        return;
      }
      dbDrafts.set(el.dataset.cell, value);
      el.classList.add("saving");
      const saving = dbWriteChain
        .catch(() => {})
        .then(async () => {
          try {
            const saved = await api(`databases/${db.id}/rows/${rid}`, "PATCH", {
              revision: row.revision,
              cells: { [cid]: value },
            });
            Object.assign(row, saved);
            if (
              JSON.stringify(dbDrafts.get(el.dataset.cell)) ===
              JSON.stringify(value)
            )
              dbDrafts.delete(el.dataset.cell);
            el.classList.remove("cell-error");
            toast("Record saved");
          } catch (e) {
            toast(
              e.message +
                " Your value remains in the cell. Copy it before reloading.",
            );
            el.classList.add("cell-error");
          } finally {
            el.classList.remove("saving");
          }
        });
      dbWriteChain = saving;
      dbSaves.add(saving);
      saving.finally(() => dbSaves.delete(saving));
    });
  });
  action($("#add-record"), "click", async () => {
    const cells = {};
    for (const c of db.columns) if (c.kind === "checkbox") cells[c.id] = false;
    cells[db.columns[0].id] = db.columns[0].kind === "text" ? "Untitled" : null;
    const row = await api(`databases/${db.id}/rows`, "POST", { cells });
    db.rows.push(row);
    renderDatabase();
    $(`[data-cell="${row.id}:${db.columns[0].id}"]`)?.focus();
  });
  $$("[data-archive]").forEach((el) =>
    action(el, "click", async () => {
      const row = db.rows.find((r) => r.id === el.dataset.archive);
      const saved = await api(`databases/${db.id}/rows/${row.id}`, "PATCH", {
        revision: row.revision,
        archived: !row.archived,
      });
      Object.assign(row, saved);
      renderDatabase();
    }),
  );
  action($("#db-rename"), "click", () =>
    formDialog(
      "Rename database",
      "A useful name makes information easier to find.",
      `<label>Name<input name="name" required maxlength="200" value="${esc(db.name)}"></label>`,
      async (v) => {
        activeDatabase = await api("databases/" + db.id, "PATCH", {
          revision: db.revision,
          name: v.name,
        });
        await refresh();
        renderDatabase();
      },
      "Save",
    ),
  );
  action($("#add-property"), "click", () =>
    formDialog(
      "Add a property",
      "Choose the type of information this column will hold.",
      `<label>Name<input name="name" required maxlength="100" placeholder="e.g. Due date"></label><label>Type<select name="kind"><option value="text">Text</option><option value="number">Number</option><option value="checkbox">Checkbox</option><option value="date">Date</option><option value="select">Select</option><option value="url">URL</option></select></label><label>Select options (one per line)<textarea name="options" placeholder="Low&#10;Medium&#10;High"></textarea></label>`,
      async (v) => {
        const col = {
          id: crypto.randomUUID(),
          name: v.name,
          kind: v.kind,
          options:
            v.kind === "select"
              ? v.options
                  .split("\n")
                  .map((x) => x.trim())
                  .filter(Boolean)
              : [],
        };
        activeDatabase = await api("databases/" + db.id, "PATCH", {
          revision: db.revision,
          columns: [...db.columns, col],
        });
        renderDatabase();
      },
      "Add property",
    ),
  );
  $$("[data-edit-record]").forEach((el) =>
    action(el, "click", () =>
      editRecordDialog(
        db,
        db.rows.find((r) => r.id === el.dataset.editRecord),
        edit,
        renderDatabase,
      ),
    ),
  );
  $$("[data-delete-record]").forEach((el) =>
    action(el, "click", () => {
      const row = db.rows.find((r) => r.id === el.dataset.deleteRecord);
      formDialog(
        "Delete this record?",
        `“${embedRecordTitle(db, row)}” is removed permanently. Archive it instead to keep it out of the way.`,
        "",
        async () => {
          await api(`databases/${db.id}/rows/${row.id}`, "DELETE");
          db.rows = db.rows.filter((r) => r.id !== row.id);
          renderDatabase();
          toast("Record deleted");
        },
        "Delete record",
      );
    }),
  );
  $$("[data-col-menu]").forEach((el) =>
    action(el, "click", () => columnMenu(el, db, edit)),
  );
  action($("#db-embed"), "click", async () => {
    const code = `[[database:${db.id}]]`;
    try {
      await navigator.clipboard.writeText(code);
      toast("Copied. Paste it into a note, or use / → Database in the editor.");
    } catch {
      formDialog(
        "Use in a note",
        "Paste this into any note, or type / in the editor and choose Database.",
        `<label>Embed code<input readonly value="${esc(code)}" onfocus="this.select()"></label>`,
        async () => {},
        "Done",
      );
    }
  });
  action($("#db-delete"), "click", () =>
    formDialog(
      `Delete “${db.name}”?`,
      `The database and its ${db.rows.length} record${db.rows.length === 1 ? "" : "s"} are removed permanently. Notes that show it will say it was deleted. This cannot be undone.`,
      `<label>Type the database name to confirm<input name="confirm" required autocomplete="off"></label>`,
      async (v) => {
        if (v.confirm !== db.name)
          throw new Error("Type the database name exactly to confirm.");
        await api("databases/" + db.id, "DELETE");
        embedCache.delete(db.id);
        await refresh();
        renderDatabaseList();
        toast("Database deleted");
      },
      "Delete database",
    ),
  );
}
// Column header menu: sort, rename, edit options, change type, delete.
function columnMenu(anchor, db, edit) {
  const col = db.columns.find((c) => c.id === anchor.dataset.colMenu);
  const save = async (columns, message) => {
    activeDatabase = await api("databases/" + db.id, "PATCH", {
      revision: db.revision,
      columns,
    });
    embedCache.delete(db.id);
    renderDatabase();
    if (message) toast(message);
  };
  const sort = (descending) => {
    dbSort = col.id;
    dbDescending = descending;
    renderDatabase();
  };
  const kinds = [
    ["text", "Text"],
    ["number", "Number"],
    ["checkbox", "Checkbox"],
    ["date", "Date"],
    ["select", "Select"],
    ["url", "URL"],
  ];
  rowMenu(anchor, [
    { icon: "arrow", label: "Sort ascending", run: () => sort(false) },
    { icon: "arrow", label: "Sort descending", run: () => sort(true) },
    ...(edit
      ? [
          "-",
          {
            icon: "edit",
            label: "Edit property…",
            run: () =>
              formDialog(
                "Edit property",
                "Changing the type or removing an option clears values that no longer fit.",
                `<label>Name<input name="name" required maxlength="100" value="${esc(col.name)}"></label><label>Type<select name="kind">${kinds.map(([k, l]) => `<option value="${k}" ${k === col.kind ? "selected" : ""}>${l}</option>`).join("")}</select></label><label>Select options (one per line)<textarea name="options">${esc(col.options.join("\n"))}</textarea></label>`,
                (v) => {
                  const options =
                    v.kind === "select"
                      ? v.options
                          .split("\n")
                          .map((x) => x.trim())
                          .filter(Boolean)
                      : [];
                  if (v.kind === "select" && !options.length)
                    throw new Error("Add at least one option.");
                  return save(
                    db.columns.map((c) =>
                      c.id === col.id
                        ? { ...c, name: v.name.trim(), kind: v.kind, options }
                        : c,
                    ),
                    "Property updated",
                  );
                },
                "Save",
              ),
          },
          {
            icon: "trash",
            label: "Delete property",
            danger: true,
            disabled: db.columns.length === 1,
            run: () => {
              const used = db.rows.filter(
                (r) => r.cells[col.id] != null && r.cells[col.id] !== "",
              ).length;
              formDialog(
                `Delete “${col.name}”?`,
                used
                  ? `${used} record${used === 1 ? " has a value" : "s have values"} for this property. They are removed permanently.`
                  : "No record has a value for this property yet.",
                "",
                () =>
                  save(
                    db.columns.filter((c) => c.id !== col.id),
                    "Property deleted",
                  ),
                "Delete property",
              );
            },
          },
        ]
      : []),
  ]);
}
// ---------------------------------------------------------------------------
// Live databases inside notes: `[[database:ID]]` or `[[database:ID:board]]`.
// The server renders a slot; the app fills it for the reader with the current
// records and keeps it up to date. Readers without access see a short note.

// Starting properties for a new task list.
const TASK_COLUMNS = () => [
  { id: crypto.randomUUID(), name: "Task", kind: "text", options: [] },
  {
    id: crypto.randomUUID(),
    name: "Status",
    kind: "select",
    options: ["To do", "In progress", "Done"],
  },
  { id: crypto.randomUUID(), name: "Due", kind: "date", options: [] },
];
const embedCache = new Map();
let embedTimer = null,
  embedWrites = Promise.resolve();
async function loadEmbed(id, fresh = false) {
  const hit = embedCache.get(id);
  if (!fresh && hit && Date.now() - hit.at < 4000) return hit;
  let entry;
  try {
    entry = { at: Date.now(), db: await api("databases/" + id) };
  } catch (e) {
    entry = {
      at: Date.now(),
      error:
        e.status === 404
          ? "deleted"
          : e.status === 403
            ? "forbidden"
            : "failed",
    };
  }
  embedCache.set(id, entry);
  return entry;
}
function embedRecordTitle(db, row) {
  const first = db.columns[0];
  return String(row.cells[first.id] ?? "") || "Untitled";
}
function drawEmbed(slot, entry) {
  slot.dataset.state = "ready";
  if (entry.error) {
    slot.innerHTML = `<span class="db-embed-note">${
      {
        deleted: "▦ This database was deleted.",
        forbidden: "▦ You don't have access to this database.",
        failed: "▦ The database could not be loaded. It will retry shortly.",
      }[entry.error]
    }</span>`;
    if (entry.error === "failed") delete slot.dataset.state;
    return;
  }
  const db = entry.db;
  const edit = data.roles[db.folder_id] === "editor";
  const rows = db.rows.filter((r) => !r.archived);
  const view = slot.dataset.view;
  const group = db.columns.find((c) => c.kind === "select");
  const body =
    view === "board" && group
      ? `<div class="db-embed-board">${["", ...group.options]
          .map((option) => {
            const cards = rows.filter(
              (r) => (r.cells[group.id] || "") === option,
            );
            return `<section class="db-embed-column"><h4>${esc(option || "No " + group.name.toLowerCase())}<span>${cards.length}</span></h4>${cards
              .map(
                (r) =>
                  `<button class="db-embed-card" data-embed-record="${r.id}"><strong>${esc(embedRecordTitle(db, r))}</strong>${db.columns
                    .slice(1)
                    .filter(
                      (c) =>
                        c.id !== group.id &&
                        r.cells[c.id] != null &&
                        r.cells[c.id] !== "",
                    )
                    .map(
                      (c) =>
                        `<small>${esc(c.name)}: ${esc(c.kind === "checkbox" ? (r.cells[c.id] ? "☑" : "☐") : r.cells[c.id])}</small>`,
                    )
                    .join("")}</button>`,
              )
              .join("")}</section>`;
          })
          .join("")}</div>`
      : `<div class="db-embed-scroll"><table class="db-embed-table"><thead><tr>${db.columns.map((c) => `<th>${esc(c.name)}</th>`).join("")}</tr></thead><tbody>${rows
          .map(
            (r) =>
              `<tr>${db.columns.map((c) => `<td>${cellInput(c, r.cells[c.id], r.id + ":" + c.id, !edit)}</td>`).join("")}</tr>`,
          )
          .join(
            "",
          )}</tbody></table>${rows.length ? "" : '<p class="db-embed-empty">No records yet.</p>'}</div>`;
  slot.innerHTML = `<span class="db-embed-panel" role="group" aria-label="Database ${esc(db.name)}"><span class="db-embed-head"><span class="db-embed-title">${icon("grid")}${esc(db.name)}</span><span class="db-embed-meta">${rows.length} record${rows.length === 1 ? "" : "s"}${edit ? "" : " · read only"}</span><span class="db-embed-actions"><span class="db-embed-views" role="group" aria-label="View"><button data-embed-view="table" aria-pressed="${view !== "board"}">Table</button><button data-embed-view="board" aria-pressed="${view === "board"}" ${group ? "" : 'disabled title="Board view needs a select property"'}>Board</button></span><button data-embed-open title="Open the database">${icon("arrow")}Open</button>${edit ? `<button class="primary" data-embed-add>${icon("plus")}New</button>` : ""}</span></span>${body}</span>`;
  bindEmbed(slot, db, edit);
}
function bindEmbed(slot, db, edit) {
  const stop = (e) => e.stopPropagation();
  slot.addEventListener("click", stop);
  $$("[data-embed-view]", slot).forEach((b) =>
    b.addEventListener("click", () => setEmbedView(slot, b.dataset.embedView)),
  );
  $("[data-embed-open]", slot).addEventListener("click", () =>
    openDatabase(db.id).catch(error),
  );
  $("[data-embed-add]", slot)?.addEventListener("click", async () => {
    try {
      const cells = {};
      for (const c of db.columns)
        if (c.kind === "checkbox") cells[c.id] = false;
      cells[db.columns[0].id] =
        db.columns[0].kind === "text" ? "Untitled" : null;
      const row = await api(`databases/${db.id}/rows`, "POST", { cells });
      db.rows.push(row);
      redrawEmbeds(db.id);
      const input = $(
        `.db-embed[data-database="${db.id}"] [data-cell="${row.id}:${db.columns[0].id}"]`,
      );
      input?.focus();
      input?.select?.();
    } catch (e) {
      error(e);
    }
  });
  $$("[data-embed-record]", slot).forEach((b) =>
    b.addEventListener("click", () => {
      const row = db.rows.find((r) => r.id === b.dataset.embedRecord);
      editRecordDialog(db, row, edit, () => redrawEmbeds(db.id));
    }),
  );
  $$("[data-cell]", slot).forEach((el) => {
    const [rid, cid] = el.dataset.cell.split(":");
    const col = db.columns.find((c) => c.id === cid);
    const event = ["checkbox", "select"].includes(col.kind) ? "change" : "blur";
    el.addEventListener("keydown", (e) => {
      if (e.key === "Enter" && el.tagName === "INPUT") el.blur();
    });
    el.addEventListener(event, () => {
      const value = parseCell(col, el);
      // Writes run one after another so each uses the record's latest revision.
      embedWrites = embedWrites.catch(() => {}).then(() => saveCell(value));
    });
    const saveCell = async (value) => {
      const row = db.rows.find((r) => r.id === rid);
      if (
        !row ||
        JSON.stringify(value) === JSON.stringify(row.cells[cid] ?? null)
      )
        return;
      el.classList.add("saving");
      try {
        Object.assign(
          row,
          await api(`databases/${db.id}/rows/${rid}`, "PATCH", {
            revision: row.revision,
            cells: { [cid]: value },
          }),
        );
        embedCache.set(db.id, { at: Date.now(), db });
        el.classList.remove("cell-error");
        // Other views of the same database on this page follow the change.
        redrawEmbeds(db.id, slot);
      } catch (e) {
        el.classList.add("cell-error");
        toast(e.message);
        if (e.status === 409) {
          await loadEmbed(db.id, true);
          redrawEmbeds(db.id);
        }
      } finally {
        el.classList.remove("saving");
      }
    };
  });
}
// Redraws every slot showing a database, except `skip` (the one being edited).
function redrawEmbeds(id, skip = null) {
  const entry = embedCache.get(id);
  if (!entry) return;
  $$(`#blocks .db-embed[data-database="${id}"]`).forEach((slot) => {
    if (slot !== skip) drawEmbed(slot, entry);
  });
}
// Switches between table and board; editors save the choice in the note.
async function setEmbedView(slot, view) {
  slot.dataset.view = view;
  const entry = embedCache.get(slot.dataset.database);
  if (entry) drawEmbed(slot, entry);
  const blockId = slot.closest(".block")?.dataset.block;
  const i = note?.blocks.findIndex((b) => b.id === blockId);
  if (!editable() || i < 0) return;
  const b = note.blocks[i];
  const id = slot.dataset.database;
  const updated = b.markdown.replace(
    new RegExp(`\\[\\[database:${id}(:(table|board))?\\]\\]`),
    view === "board" ? `[[database:${id}:board]]` : `[[database:${id}]]`,
  );
  if (updated === b.markdown) return;
  live?.boundary();
  b.markdown = updated;
  note.rendered[i] = await renderMarkdown(b.markdown);
  markDirty();
  await queueSave();
}
/** Fills database slots in the open note and keeps them current. */
async function renderDatabaseEmbeds() {
  const slots = $$("#blocks .db-embed:not([data-state])");
  if (!slots.length || !note) return;
  for (const slot of slots) {
    const classes = [...slot.classList];
    slot.dataset.database =
      classes.find((c) => c.startsWith("db-id-"))?.slice(6) || "";
    slot.dataset.view = classes.includes("db-view-board") ? "board" : "table";
    slot.dataset.state = "loading";
    slot.innerHTML = `<span class="db-embed-note">${icon("grid")}Loading database…</span>`;
  }
  if (shareMode) {
    slots.forEach(
      (slot) =>
        (slot.innerHTML =
          '<span class="db-embed-note">▦ Embedded database · shown to workspace members</span>'),
    );
    return;
  }
  const ids = [...new Set(slots.map((s) => s.dataset.database))];
  const entries = new Map(
    await Promise.all(ids.map(async (id) => [id, await loadEmbed(id)])),
  );
  for (const slot of slots)
    if (slot.isConnected) drawEmbed(slot, entries.get(slot.dataset.database));
  clearInterval(embedTimer);
  embedTimer = setInterval(refreshEmbeds, 5000);
}
// Picks up changes made elsewhere: in the database itself or by other people.
async function refreshEmbeds() {
  const slots = $$("#blocks .db-embed[data-state=ready]");
  if (!slots.length || !note) return clearInterval(embedTimer);
  if (
    document.hidden ||
    modal.open ||
    document.activeElement?.closest(".db-embed")
  )
    return;
  await embedWrites.catch(() => {});
  for (const id of new Set(slots.map((s) => s.dataset.database))) {
    const before = JSON.stringify(embedCache.get(id)?.db ?? null);
    const entry = await loadEmbed(id, true);
    if (JSON.stringify(entry.db ?? null) !== before) redrawEmbeds(id);
  }
}
// Inserts a database into the note: an existing one or a new task list.
function databaseEmbedDialog(existingId) {
  const ids = new Set(folders().map((f) => f.id));
  const databases = (data.databases || []).filter((d) => ids.has(d.folder_id));
  const editorFolders = folderTreeOptions((f) => data.roles[f.id] === "editor");
  formDialog(
    "Insert a database",
    "The note shows the database's current records. Editing them here or in the database changes both.",
    `<label>Database<select name="database" id="embed-choice">${editorFolders ? '<option value="new">New task list…</option>' : ""}${databases.map((d) => `<option value="${d.id}">${esc(d.name)} · ${d.record_count} record${d.record_count === 1 ? "" : "s"}</option>`).join("")}</select></label><div id="embed-new"><label>Name<input name="name" value="Tasks" maxlength="200"></label><label>Folder<select name="folder_id">${editorFolders}</select></label></div><label>Show as<select name="view"><option value="table">Table</option><option value="board">Board (by status)</option></select></label>`,
    async (v) => {
      let id = v.database;
      if (id === "new") {
        const db = await api("databases", "POST", {
          folder_id: v.folder_id,
          name: v.name.trim() || "Tasks",
          columns: TASK_COLUMNS(),
        });
        id = db.id;
        await refresh();
      }
      if (!id) throw new Error("Choose a database.");
      const markdown =
        v.view === "board" ? `[[database:${id}:board]]` : `[[database:${id}]]`;
      let b = note.blocks.find((x) => x.id === existingId);
      if (b && !b.markdown.trim()) b.markdown = markdown;
      else {
        const at = b ? note.blocks.indexOf(b) + 1 : note.blocks.length;
        b = newBlock(markdown);
        note.blocks.splice(at, 0, b);
        note.rendered.splice(at, 0, "");
      }
      note.rendered[note.blocks.indexOf(b)] = await renderMarkdown(b.markdown);
      markDirty();
      renderBlocks();
      await queueSave();
    },
    "Insert",
  );
  const choice = $("#embed-choice", modal);
  const toggle = () =>
    ($("#embed-new", modal).hidden = choice?.value !== "new");
  choice?.addEventListener("change", toggle);
  toggle();
  if (!choice?.options.length) {
    $("[type=submit]", modal).disabled = true;
    $(".form-error", modal).textContent =
      "You need edit access to a folder to create a database.";
  }
}
// One record in a form; used by the board views.
function editRecordDialog(db, row, edit, onSaved) {
  formDialog(
    "Edit record",
    db.name,
    db.columns
      .map(
        (c) =>
          `<label>${esc(c.name)}${cellInput(c, row.cells[c.id], c.id, !edit)}</label>`,
      )
      .join(""),
    async () => {
      const cells = {};
      db.columns.forEach((c) => {
        cells[c.id] = parseCell(c, $(`[data-cell="${c.id}"]`, modal));
      });
      Object.assign(
        row,
        await api(`databases/${db.id}/rows/${row.id}`, "PATCH", {
          revision: row.revision,
          cells,
        }),
      );
      embedCache.set(db.id, { at: Date.now(), db });
      onSaved();
    },
    edit ? "Save record" : "Read only",
  );
  if (!edit) $("[type=submit]", modal).disabled = true;
}

async function renderIntegrations() {
  clearInterval(dbTimer);
  activeDatabase = null;
  const tokens = await api("mcp-credentials");
  $("#breadcrumb").textContent = "MCP connections";
  $("#content").innerHTML =
    `<section class="page"><div class="eyebrow">YOUR KNOWLEDGE, WITH CONTEXT</div><div class="page-heading"><div><h1>Connect your AI tools</h1><p class="subtitle">Let your assistant find answers in your notes and structured records.</p></div><button class="btn primary" id="create-mcp" ${!workspaceId ? "disabled" : ""}>${icon("plus")}Create credential</button></div><div class="welcome-banner"><div class="welcome-copy"><div class="eyebrow">MODEL CONTEXT PROTOCOL</div><h2>A useful connection. A clear boundary.</h2><p>Read-only access to one workspace. Your folder permissions always apply.<br>Create a separate, expiring credential for each AI tool.</p></div><div class="mcp-art" aria-hidden="true">⌘</div></div><section class="panel"><div class="panel-heading"><div><h2>Your connections</h2><p>Credentials are shown once. Revoke a connection to stop its access immediately.</p></div><span class="pill">Read only</span></div><div class="table-wrap"><table class="admin-table"><thead><tr><th>CLIENT</th><th>WORKSPACE</th><th>EXPIRES</th><th></th></tr></thead><tbody>${tokens.map((t) => `<tr><td>${esc(t.name)}</td><td>${esc(data.workspaces.find((w) => w.id === t.workspace_id)?.name || "Unavailable workspace")}</td><td>${date(t.expires_at)} ${t.expires_at < Date.now() / 1000 ? "· Expired" : ""}</td><td><button class="btn danger" data-revoke="${t.id}">Revoke</button></td></tr>`).join("") || '<tr><td colspan="4">No AI connections yet.</td></tr>'}</tbody></table></div></section><div class="integration-tools"><h2>What your assistant can do</h2><div class="folder-grid"><div class="folder-card"><div><strong>Find knowledge</strong><small>Keyword search with document and block references</small></div></div><div class="folder-card"><div><strong>Read documents</strong><small>Permission-aware, paginated note content</small></div></div><div class="folder-card"><div><strong>Query databases</strong><small>Typed records, property filters, and pagination</small></div></div></div><p class="subtitle">Supports MCP Streamable HTTP clients with bearer authentication. No AI provider receives your data until you connect a tool and use it.</p></div></section>`;
  $$("[data-revoke]").forEach((el) =>
    action(el, "click", () =>
      formDialog(
        "Revoke this connection?",
        "The AI tool will immediately lose access. You can create a new credential later.",
        "",
        async () => {
          await api("mcp-credentials/" + el.dataset.revoke, "DELETE");
          await renderIntegrations();
        },
        "Revoke",
      ),
    ),
  );
  action($("#create-mcp"), "click", () => {
    modal.innerHTML = `<form id="mcp-create-form"><h2>Connect an AI tool</h2><p>One workspace. Read only. Always subject to your permissions.</p><label>Client label<input name="name" placeholder="e.g. Claude Code" required maxlength="200"></label><label>Workspace<select name="workspace_id">${data.workspaces.map((w) => `<option value="${w.id}" ${w.id === workspaceId ? "selected" : ""}>${esc(w.name)}</option>`).join("")}</select></label><label>Expires in<select name="expires_days"><option value="7">7 days</option><option value="30" selected>30 days</option><option value="90">90 days</option></select></label><div class="form-error"></div><footer><button type="button" class="btn" id="cancel-mcp">Cancel</button><button class="btn primary">Create credential</button></footer></form>`;
    modal.showModal();
    $("#cancel-mcp").onclick = () => modal.close();
    $("#mcp-create-form").onsubmit = async (e) => {
      e.preventDefault();
      const button =
        $("button[type=submit]", modal) || $("footer button:last-child", modal);
      button.disabled = true;
      try {
        const v = Object.fromEntries(new FormData(e.target));
        const token = await api("mcp-credentials", "POST", {
          ...v,
          expires_days: Number(v.expires_days),
        });
        const config = {
          mcpServers: {
            minerva: {
              url:
                (document.querySelector('meta[name="minerva-server-origin"]')
                  ?.content || location.origin) +
                "/api/workspaces/" +
                token.workspace_id +
                "/mcp",
              headers: { Authorization: "Bearer " + token.token },
            },
          },
        };
        await renderIntegrations();
        modal.innerHTML =
          '<h2>Your connection is ready</h2><p>Copy this configuration into your MCP client. The credential is shown only once; keep it private.</p><textarea id="mcp-config" aria-label="MCP client configuration" rows="12" readonly></textarea><footer><button class="btn" id="done-mcp">Done</button><button class="btn primary" id="copy-mcp">Copy JSON</button></footer>';
        $("#mcp-config").value = JSON.stringify(config, null, 2);
        $("#done-mcp").onclick = () => {
          modal.close();
          modal.innerHTML = "";
        };
        action($("#copy-mcp"), "click", async () => {
          await navigator.clipboard.writeText($("#mcp-config").value);
          toast("Configuration copied");
        });
      } catch (err) {
        $(".form-error", modal).textContent = err.message;
        button.disabled = false;
      }
    };
  });
}

document.addEventListener("keydown", (e) => {
  // The editor handles its own shortcuts (Ctrl+K inside a block makes a link).
  if (e.defaultPrevented) return;
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "k") {
    e.preventDefault();
    if (data && !modal.open) search();
  }
  if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "s") {
    e.preventDefault();
    flush().catch(error);
  }
  const key = e.key.toLowerCase();
  if ((e.ctrlKey || e.metaKey) && key === "f" && note && !modal.open) {
    e.preventDefault();
    if (e.shiftKey) toggleFocus();
    else openFind();
    return;
  }
  if (
    e.key === "Escape" &&
    document.body.classList.contains("focus-mode") &&
    !modal.open &&
    !e.target.closest?.("input, textarea, select, .popover, .find-bar")
  )
    toggleFocus(false);
  // Note-level undo outside text fields; fields keep their native undo.
  if (
    note &&
    (e.ctrlKey || e.metaKey) &&
    (key === "z" || key === "y") &&
    !modal.open &&
    !e.target.closest?.("input, textarea, select, [contenteditable]")
  ) {
    e.preventDefault();
    undoRedo(key === "y" || e.shiftKey).catch(error);
  }
});
window.addEventListener("beforeunload", (e) => {
  if (dirty || dbDrafts.size || dbSaves.size) {
    e.preventDefault();
    e.returnValue = "";
  }
});
/** Guest view for share links: `/share#<token>`. */
async function startShared() {
  shareMode = true;
  // Opening another link while on /share only changes the fragment.
  window.addEventListener("hashchange", () => {
    if (location.hash.length > 1) location.reload();
  });
  const token = location.hash.slice(1);
  if (token) {
    await api("shared/session", "POST", { token });
    // Keep the secret out of the address bar, history, and screenshots.
    window.history.replaceState(null, "", "/share");
  }
  note = await api("shared/note");
  rememberRendered();
  data = {
    user: { name: "Guest", admin: false, can_create_workspace: false },
    roles: {},
    folders: [],
    workspaces: [],
    notes: [],
    databases: [],
  };
  live = new Collaboration(
    note.id,
    api,
    receiveLive,
    (status, pending) => {
      dirty = pending;
      paneState();
      if ($("#save-state"))
        $("#save-state").textContent = editable() ? status : "View only · live";
    },
    (e) => {
      if (e.status === 401) sharedUnavailable();
    },
    "shared/sync",
  );
  await live.start();
  $("#app").innerHTML =
    `<a class="skip-link" href="#content">Skip to content</a><div class="shared-shell"><header class="shared-top"><div class="brand"><span class="brand-mark">✳</span>minerva</div><span class="badge">${note.role === "editor" ? "Anyone with this link can edit" : "View only"}</span><span class="spacer"></span>${themeSelect()}</header><main class="main"><span id="breadcrumb" hidden></span><div id="content" tabindex="-1"></div></main></div>`;
  bindThemeSelect();
  document.title = note.title + " · Minerva";
  renderEditor();
}
function sharedUnavailable() {
  stopLive();
  note = null;
  $("#app").innerHTML =
    '<div class="loading"><span>This link is no longer available. Ask the person who shared it for a new link.</span></div>';
}
(async () => {
  if (location.pathname === "/share") {
    startShared().catch((e) =>
      e.status === 401 || e.status === 404 ? sharedUnavailable() : error(e),
    );
    return;
  }
  try {
    const health = await api("health").catch(() => ({}));
    localMode = health.mode === "local";
    mailEnabled = !!health.mail;
    const resetToken = new URLSearchParams(location.search).get("reset");
    if (resetToken) return choosePassword(resetToken);
    if (localMode) {
      replicas = await api("local/replicas").catch(() => []);
      setInterval(() => loadReplicas().catch(() => {}), 4000);
    }
    await refresh();
    // Opened from another place's workspace menu: that workspace first.
    const asked = new URLSearchParams(location.search).get("workspace");
    if (asked && data.workspaces.some((x) => x.id === asked))
      workspaceId = asked;
    if (chatPane) {
      const [kind, id] = initialChat.split(":");
      aiScope = { kind, id };
      view = "chat";
      document.documentElement.classList.add("chat-pane");
    }
    shell();
    firstSignIn();
    const requested =
      initialNote ||
      (location.hash.startsWith("#note/") ? location.hash.slice(6) : "");
    if (requested) await openNote(requested);
  } catch (e) {
    if (e.status === 401) login();
    else {
      $("#app").innerHTML =
        '<div class="loading"><span>Unable to connect to the server. Reload to try again.</span></div>';
      error(e);
    }
  }
})();
