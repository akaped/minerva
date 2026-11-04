import {
  $t, $tn, msg,
  LANGUAGES,
  pickLanguage,
  language,
  chosenLanguage,
  setLanguage,
  serverMessage,
  keys as shortcutKeys,
} from "./i18n.js";
import "./pane-auth.js";
import { renderServerOverview } from "./server-overview.js";
import { renderFlashcards } from "./flashcards.js";
import { renderReminders } from "./reminders.js";
import { pluginDialog } from "./plugin-commands.js";
import { createYouTubeEmbeds } from "./youtube.js";
import {
  createNoteTabs,
  isPane,
  initialNote,
  initialBlock,
  initialChat,
  paneMessage,
} from "./note-tabs.js";
import { renderAIChat, renderAISettings, stopAI } from "./ai.js";
import { createWritingAssist } from "./ai-writing.js";
import { attachPreview, previewHTML, headingLevel } from "./live-preview.js";
import { renderDashboard } from "./dashboard.js";
import { emojiPicker } from "./emoji-picker.js";
import { splitMarkdown, blockCommands, highlightSnippet } from "./markdown.js";
import { Collaboration, watchNote, liveSignals } from "./collaboration.js";
import { createComments } from "./comments.js";
import { startNotice, serverPage, withoutNotice } from "./start-notice.js";
import {
  buildGraph,
  neighborhood,
  mountGraph,
  folderColor,
  tagColors,
} from "./graph.js";
import { sourceOffset, sourceRange, objectOffset } from "./caret-map.js";
import { recordingBlocks, timestamp } from "./recording.js";
import { present } from "./presentation.js";
import {
  startWorker,
  offlineReading,
  setOfflineReading,
  installed,
  canInstall,
  install,
  onInstallChange,
} from "./install.js";
import {
  noteTarget,
  slotTarget,
  blockLabel,
  blockLink,
  embedCode,
  embedAllowed,
} from "./note-embeds.js";
import {
  ROLLUP_FUNCTIONS,
  computedText,
  formatNumber,
  CURRENCIES,
  GROUPABLE,
  boardGroups,
  boardKey,
  rankBetween,
  moveToGroup,
  calendarExtras,
} from "./database-views.js";
import {
  subtreeEnd,
  hasChildren,
  hiddenBlocks,
  foldedAbove,
  loadFolded,
  saveFolded,
} from "./folding.js";
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
  turnInto,
  BLOCK_KINDS,
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
  ["system", $t("System theme")],
  ["light", $t("Light theme")],
  ["dark", $t("Dark theme")],
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
      /theme$/i.test(t.name) ? t.name : $t("{name} theme", { name: t.name }),
    ]),
  ]
    .map(
      ([value, label]) =>
        `<option value="${esc(value)}" ${value === current ? "selected" : ""}>${esc(label)}</option>`,
    )
    .join("");
}
function themeSelect() {
  return `<select id="theme" aria-label="${$t("Color theme")}">${themeOptions()}</select>`;
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
    // Bundled plugins' names and descriptions are in the catalog.
    // A plugin's own translation (`name.it: …`) comes first.
    const local = (list) =>
      (list || []).map((p) => {
        const own = p.translations?.[language()] || {};
        return {
          ...p,
          name: own.name || $t(p.name),
          description: own.description || (p.description && $t(p.description)),
          users: own.users || (p.users && $t(p.users)),
        };
      });
    pluginThemes = local(index.themes);
    publicThemes = local(index.public_themes);
    printTemplates = local(index.pdf_prints);
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
  comments?.detach();
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
  archive: "M3 4h18v4H3z M5 8v12h14V8 M10 12h4",
  play: "M3 4h18v12H3z M12 16v4 M8 20h8 M10 7.5l4 2.5-4 2.5z",
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
  graph:
    "M6 18a2 2 0 1 0 0 .01 M18 16a2 2 0 1 0 0 .01 M12 6a2 2 0 1 0 0 .01 M7 16.5l4-9 M13 7.5l4 7 M8 18l8-1.5",
  bell: "M6 16V11a6 6 0 0 1 12 0v5l2 2H4z M10 21h4",
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
        onGraph: (id) => navigate("graph", id).catch(error),
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
      note.archived,
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
    if (e.data.type === "show-block" && note) showBlock(String(e.data.block));
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
// Files of the open note by attachment ID: name, type, size, upload time.
const noteFiles = new Map();
let noteFilesFor = null;
async function loadNoteFiles(id = note?.id) {
  if (!id || shareMode) return;
  try {
    const { attachments } = await api(`notes/${id}/attachments`);
    for (const f of attachments) noteFiles.set(f.id, f);
    fileCards(document);
    imageBars(document);
    for (const frame of $$(".image-frame[data-image]")) drawImageBar(frame);
    // Cards drawn before the details arrived get their size and date now.
    for (const card of $$(".file-card[data-file]")) {
      const meta = noteFiles.get(card.dataset.file);
      if (meta) $("small", card).textContent = fileDetails(meta.filename, meta);
    }
  } catch {
    // Cards still show the file name from the link.
  }
}
const FILE_KINDS = [
  ["pdf", "PDF", "📕", /\.pdf$/i, /pdf/],
  [
    "image",
    "Image",
    "🖼️",
    /\.(png|jpe?g|gif|webp|svg|avif|bmp|heic)$/i,
    /^image\//,
  ],
  [
    "audio",
    "Audio",
    "🎧",
    /\.(mp3|wav|ogg|oga|opus|m4a|aac|flac|weba)$/i,
    /^audio\//,
  ],
  ["video", "Video", "🎬", /\.(mp4|mov|mkv|webm|avi)$/i, /^video\//],
  [
    "archive",
    "Archive",
    "🗜️",
    /\.(zip|tar|gz|tgz|7z|rar|bz2|xz)$/i,
    /zip|compressed/,
  ],
  [
    "sheet",
    "Spreadsheet",
    "📊",
    /\.(xlsx?|ods|csv|tsv|numbers)$/i,
    /sheet|csv/,
  ],
  ["slides", "Presentation", "📽️", /\.(pptx?|odp|key)$/i, /presentation/],
  [
    "doc",
    "Document",
    "📄",
    /\.(docx?|odt|rtf|pages)$/i,
    /word|opendocument\.text/,
  ],
  ["text", "Text", "📝", /\.(txt|md|markdown|json|xml|ya?ml|log)$/i, /^text\//],
];
/** The kind of a file, from its name or type: [id, label, icon]. */
function fileKind(name, media = "") {
  const kind = FILE_KINDS.find(
    ([, , , ext, type]) => ext.test(name) || (media && type.test(media)),
  );
  return kind ? kind.slice(0, 3) : ["file", "File", "📎"];
}
/**
 * A block that is only a link to one of the note's files shows as a card:
 * type icon, name, size and upload date, with Download, and Open for PDFs.
 */
function fileCards(root) {
  for (const link of $$('.block-body a[href*="/api/v1/attachments/"]', root)) {
    const id = link
      .getAttribute("href")
      .match(/\/api\/v1\/attachments\/([0-9a-f-]{36})$/i)?.[1];
    const paragraph = link.closest("p");
    if (
      !id ||
      link.closest(".file-card") ||
      !paragraph ||
      paragraph.textContent.trim() !== link.textContent.trim()
    )
      continue;
    // Older notes have a download card right below each uploaded picture; the
    // picture's own bar has Download now, so the card stays out of sight.
    const block = paragraph.closest(".block");
    if (
      block?.previousElementSibling?.querySelector(
        `img[src*="/api/v1/attachments/${id}"]`,
      )
    )
      block.classList.add("image-download-block");
    const meta = noteFiles.get(id);
    const name = meta?.filename || link.textContent.replace(/^Download /, "");
    const [kind, , iconChar] = fileKind(name, meta?.media_type);
    const details = fileDetails(name, meta);
    const card = document.createElement("div");
    card.className = `file-card file-${kind}`;
    card.dataset.file = id;
    card.innerHTML = `<span class="file-icon" aria-hidden="true">${iconChar}</span><div class="file-info"><strong title="${esc(name)}">${esc(name)}</strong><small>${esc(details)}</small></div><div class="file-actions">${kind === "pdf" ? `<button type="button" class="btn primary" data-open-file="${id}">${$t("Open")}</button>` : ""}<a class="btn" href="/api/v1/attachments/${id}" download>${$t("Download")}</a></div>`;
    paragraph.replaceWith(card);
    $("[data-open-file]", card)?.addEventListener("click", () =>
      openFile(id, name),
    );
  }
}
/**
 * A picture from the note's files shows a slim bar above it on hover: its name,
 * size and upload date, and Download.
 */
function imageBars(root) {
  for (const img of $$('.block-body img[src*="/api/v1/attachments/"]', root)) {
    const id = img
      .getAttribute("src")
      .match(/\/api\/v1\/attachments\/([0-9a-f-]{36})/i)?.[1];
    if (!id || img.closest(".image-frame")) continue;
    const frame = document.createElement("span");
    frame.className = "image-frame";
    frame.dataset.image = id;
    img.replaceWith(frame);
    frame.append(img);
    const bar = document.createElement("span");
    bar.className = "image-bar";
    frame.prepend(bar);
    drawImageBar(frame);
  }
}
function drawImageBar(frame) {
  const id = frame.dataset.image;
  const meta = noteFiles.get(id);
  const name = meta?.filename || $("img", frame)?.alt || "Image";
  const details = [
    meta && bytes(meta.size),
    meta?.created_at &&
      new Date(meta.created_at * 1000).toLocaleDateString(undefined, {
        day: "numeric",
        month: "short",
        year: "numeric",
      }),
  ]
    .filter(Boolean)
    .join(" · ");
  $(".image-bar", frame).innerHTML =
    `<span class="image-name" title="${esc(name)}">${esc(name)}</span>${details ? `<span class="image-meta">${esc(details)}</span>` : ""}<a href="/api/v1/attachments/${id}" download>${$t("Download")}</a>`;
}
/** "PDF · 2.4 MiB · 30 Oct 2026": a file's kind, size and upload date. */
function fileDetails(name, meta) {
  return [
    fileKind(name, meta?.media_type)[1],
    meta && bytes(meta.size),
    meta?.created_at &&
      new Date(meta.created_at * 1000).toLocaleDateString(undefined, {
        day: "numeric",
        month: "short",
        year: "numeric",
      }),
  ]
    .filter(Boolean)
    .join(" · ");
}
/** Shows a PDF beside the note: in a tab next to it, or a browser tab. */
function openFile(id, title) {
  if (isPane) return paneMessage("open-file", { file: id, title });
  if (noteTabs) return noteTabs.openFile({ file: id, title });
  window.open(`/api/v1/attachments/${id}/preview`, "_blank", "noopener");
}
if (typeof MutationObserver !== "undefined")
  new MutationObserver((changes) => {
    for (const change of changes)
      for (const node of change.addedNodes)
        if (
          node.nodeType === 1 &&
          node.querySelector?.(
            '[href*="/api/v1/attachments/"], [src*="/api/v1/attachments/"]',
          )
        ) {
          fileCards(node.closest?.("#blocks") || node);
          imageBars(node.closest?.("#blocks") || node);
        }
  }).observe(document.documentElement, { childList: true, subtree: true });
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
    announce($t("Created the journal entry for {longDate}", { longDate: longDate(date) }));
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
    script.onerror = () => reject(new Error($t("Diagrams could not be loaded")));
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
      figure.setAttribute("aria-label", $t("Diagram"));
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
    renderNoteEmbeds().catch(error);
    recordingButtons().catch(error);
    // The server writes this tooltip in English for every reader.
    $$('#blocks .reminder[title="Reminder by email"]').forEach(
      (el) => (el.title = $t("Reminder by email")),
    );
  }, 60);
}).observe(document.body, { childList: true, subtree: true });
// Links between notes and inline tags inside rendered blocks.
document.addEventListener("click", (e) => {
  const link = e.target.closest?.('a[href^="#note/"]');
  const target = link && noteTarget(link.getAttribute("href"));
  if (target && !shareMode) {
    e.preventDefault();
    openNote(target.note, { block: target.block }).catch(() =>
      toast($t("That note is not available to you.")),
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
    sharing.published ? `<span class="share-badge public">${$t("Public")}</span>` : "",
    sharing.links ? `<span class="share-badge">${$t("Link")}</span>` : "",
    sharing.members ? `<span class="share-badge">${$t("Shared")}</span>` : "",
  ].join("");
}
/**
 * Says when what is shown is this device's kept copy because the server
 * cannot be reached, and catches up once it can.
 */
let offlineShown = false;
function showOffline(on) {
  if (!on) return;
  offlineShown = true;
  // A note tab's window says it once for all its tabs.
  if (isPane || $("#offline-bar")) return;
  const bar = document.createElement("div");
  bar.id = "offline-bar";
  bar.className = "offline-bar";
  bar.setAttribute("role", "status");
  bar.textContent =
    $t("Offline · showing what this device kept. Changes cannot be saved until you are back online.");
  document.body.append(bar);
}
addEventListener("online", async () => {
  if (!offlineShown) return;
  offlineShown = false;
  $("#offline-bar")?.remove();
  if (!data || shareMode) return;
  await refresh().catch(() => {});
  if (note?.offline_copy) await openNote(note.id).catch(error);
  else shell();
});
async function api(path, method = "GET", body) {
  const r = await fetch("/api/v1/" + path, {
    method,
    headers: { "Content-Type": "application/json", "X-Minerva-Client": "1" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const v = await r.json();
  // The server could not be reached: the service worker answered.
  const kept = !!r.headers.get("x-minerva-offline");
  if (kept || !r.ok) showOffline(kept);
  if (!r.ok) {
    // The server writes its messages in English; the catalog has them too.
    const e = new Error(v.error ? serverMessage(v.error) : $t("Request failed"));
    e.status = kept ? 0 : r.status;
    throw e;
  }
  if (kept && v && typeof v === "object" && !Array.isArray(v))
    v.offline_copy = true;
  // A note tab tells the window around it, which redraws its sidebar.
  if (isPane && changesSidebar(path, method, body, v)) paneMessage("changed");
  return v;
}
/** Why the desktop app opened this computer instead of the server open last. */
function showStartNotice() {
  const notice = startNotice(location.search);
  window.history.replaceState(window.history.state, "", withoutNotice(location.href));
  if (!notice) return;
  const bar = document.createElement("div");
  bar.className = "start-notice";
  bar.setAttribute("role", "status");
  bar.innerHTML = `<span>${esc(notice.text)}</span>${notice.retry ? `<button class="btn" data-start="retry">${$t("Try again")}</button>` : ""}<button class="btn" data-start="servers">${$t("Servers")}</button><button class="icon-button" data-start="close" aria-label="${$t("Dismiss")}">×</button>`;
  bar.onclick = (e) => {
    const choice = e.target.closest("[data-start]")?.dataset.start;
    if (choice === "retry") location.href = serverPage(location, notice.server);
    else if (choice === "servers")
      window.ipc?.postMessage('{"cmd":"show_servers"}');
    if (choice) bar.remove();
  };
  document.body.append(bar);
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
  // A personal workspace keeps its English name; it shows in the reader's.
  for (const w of data.workspaces) if (w.personal) w.name = $t("My Workspace");
  // Emails and what the server makes speak the language the app shows.
  if (!shareMode && data.language !== language())
    api("account/language", "PUT", { language: language() }).catch(() => {});
  // Records' pages are listed with their records, not with the folder's notes.
  data.record_notes = data.notes.filter((n) => n.record);
  data.notes = data.notes.filter((n) => !n.record);
  // Archived notes leave the sidebar, search and overviews: only Archive lists them.
  data.archived_notes = data.notes.filter((n) => n.archived);
  data.notes = data.notes.filter((n) => !n.archived);
  // The account's default workspace first, then any other.
  if (!data.workspaces.some((w) => w.id === workspaceId)) {
    // The workspace open last time on this device, else the account's default.
    const last = lastWorkspace();
    workspaceId = [last, data.default_workspace].find((id) =>
      data.workspaces.some((w) => w.id === id),
    );
    workspaceId ??= data.workspaces[0]?.id;
  }
  if (noteTabs) {
    // Tabs of deleted or trashed notes close.
    noteTabs.prune(
      new Set(
        [
          ...data.notes,
          ...data.record_notes,
          ...data.archived_notes,
          ...(data.shared_notes || []),
        ]
          .filter((n) => !n.trashed)
          .map((n) => n.id),
      ),
    );
    redrawSidebar();
  }
}
/** The workspace this person last had open on this server, on this device. */
const lastWorkspaceKey = () =>
  `minerva.workspace.${data.server_id}.${data.user.id}`;
function lastWorkspace() {
  try {
    return localStorage.getItem(lastWorkspaceKey());
  } catch {
    return null;
  }
}
function rememberWorkspace() {
  try {
    if (workspaceId) localStorage.setItem(lastWorkspaceKey(), workspaceId);
  } catch {
    // Storage may be unavailable; the default workspace opens instead.
  }
}
/** Scrolls the sidebar to a folder and highlights it for a moment. */
function revealFolder(id) {
  const row = $(`.tree [data-folder="${CSS.escape(id)}"]`)?.closest(
    ".tree-row",
  );
  if (!row) return;
  row.scrollIntoView({ block: "nearest" });
  row.classList.add("just-added");
  setTimeout(() => row.classList.remove("just-added"), 1600);
}
/**
 * Fades the bottom of the sidebar while more of it is hidden below, so a long
 * list of folders never looks like it ends early.
 */
function sidebarOverflow() {
  const scroll = $(".sidebar-scroll");
  if (!scroll) return;
  const update = () =>
    scroll.classList.toggle(
      "more-below",
      scroll.scrollTop + scroll.clientHeight < scroll.scrollHeight - 4,
    );
  scroll.onscroll = update;
  new ResizeObserver(update).observe(scroll);
  update();
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
  sidebarOverflow();
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
      n?.archived,
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
  modal.innerHTML = `<form id="first-sign-in"><h2>${$t("Welcome to Minerva, {name}", { name: esc(data.user.name) })}</h2><p>${$t("Before you start, replace the temporary password your administrator gave you{value}.", { value: workspace ? $t(", and name your personal workspace: a private place for your own notes") : "" })}</p><label>${$t("Temporary password")}<input name="current_password" type="password" autocomplete="current-password" required></label><label>${$t("New password")}<input name="new_password" type="password" autocomplete="new-password" minlength="12" maxlength="256" required></label><label>${$t("Repeat new password")}<input name="repeat" type="password" autocomplete="new-password" required></label>${workspace ? `<label>${$t("Personal workspace")}<input name="workspace_name" value="Personal" maxlength="200" required></label>` : ""}<div class="form-error" role="alert"></div><footer><button type="button" class="btn" id="first-sign-out">${$t("Sign out")}</button><button class="btn primary" type="submit">${$t("Get started")}</button></footer></form>`;
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
        throw new Error($t("The new passwords do not match."));
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
          ? $t("You’re all set · this is your personal workspace")
          : $t("Password changed"),
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
    ? $tn(r.pending, " · {count} change waiting", " · {count} changes waiting")
    : "";
  if (r.conflicts) return `${r.conflicts} to review${waiting}`;
  if (r.revoked || state === "revoked") return "Access removed · read only";
  if (state === "offline") return `Offline${waiting}`;
  if (state === "signed_out") return $t("Sign in to sync{waiting}", { waiting });
  if (state === "synced") return waiting ? `Syncing${waiting}` : "Synced";
  if (state) return $t("Sync paused{waiting}", { waiting });
  return "Waiting for first sync";
}
function workspaceBadge() {
  const r = replicaFor();
  if (r)
    return `<button class="badge sync-badge ${r.conflicts ? "attention" : ""}" id="sync-state" title="${$t("Offline copy of a workspace on another server")}">${esc(syncLabel(r))}</button>`;
  if (localMode) return `<span class="badge">${$t("ON THIS COMPUTER")}</span>`;
  const w = currentWorkspace();
  if (!w) return "";
  // My Workspace stays private; other workspaces open their members here.
  if (w.personal)
    return `<span class="badge" title="${$t("Only you can see My Workspace")}">${$t("PRIVATE WORKSPACE")}</span>`;
  return `<button class="badge" id="share-workspace" title="${$t("Share this workspace")}">${w.shared ? $t("SHARED WORKSPACE") : $t("PRIVATE WORKSPACE")} · ${$t("Share")}</button>`;
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
          ? $t("Your change to {fields} was replaced by a newer one from the server.", { fields: c.lost.map((f) => f.replace("cells/", $t("a property"))).join(", ") })
          : `${c.incoming ? "A change from the server could not be applied here" : "The server refused this change"}: ${c.error || c.status}.`;
  const actions = (c) =>
    c.copy_id
      ? `<button class="btn" data-resolve="${c.id}" data-choice="theirs">${$t("Keep theirs")}</button><button class="btn" data-resolve="${c.id}" data-choice="both">${$t("Keep both")}</button><button class="btn primary" data-resolve="${c.id}" data-choice="mine">${$t("Use mine")}</button>`
      : `<button class="btn" data-resolve="${c.id}" data-choice="ok">${$t("OK")}</button>`;
  modal.innerHTML = `<form method="dialog" class="sync-dialog"><h2>${$t("Offline copies")}</h2><p>${$t("Workspaces from other servers kept on this computer. Changes made offline are sent when the server is reachable. Choose which workspaces to keep offline in Servers (")}<kbd>Ctrl</kbd>+<kbd>${shortcutKeys("Shift")}</kbd>+<kbd>O</kbd>).</p><ul class="sync-list">${list.map((r) => `<li><strong>${esc(r.name)}</strong><span>${esc(syncLabel(r))}${r.status?.error && r.status.state !== "synced" ? ` — ${esc(r.status.error)}` : ""}</span></li>`).join("") || `<li>${$t("No offline copies yet.")}</li>`}</ul>${conflicts.length ? `<h3>${$t("To review")}</h3><ul class="sync-list conflicts">${conflicts.map((c) => `<li><div><strong>${esc(name(c))}</strong><span>${esc(explain(c))}</span></div><div class="share-actions">${actions(c)}</div></li>`).join("")}</ul>` : ""}<footer><button class="btn">${$t("Close")}</button></footer></form>`;
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
    `<div class="login-shell"><div class="login-art"><div class="brand"><span class="brand-mark">✳</span>minerva</div><div class="eyebrow">${$t("A HOME FOR YOUR IDEAS")}</div><h1>${$t("Make room for")}<br>${$t("your next big thought.")}</h1><p>${$t("Notes, plans, and a little possibility.")}<br>${$t("A thoughtful workspace, on your own server.")}</p>${art()}</div><main class="login-main"><form class="login-form"><div class="eyebrow">${$t("YOUR SPACE, CONNECTED")}</div><h2>${$t("Welcome to Minerva")}</h2><p>${$t("Sign in to your server to pick up where you left off.")}</p><label>${$t("Email address")}<input name="email" type="email" autocomplete="username" placeholder="${$t("you@example.com")}" required></label><label>${$t("Password")}<input name="password" type="password" autocomplete="current-password" placeholder="${$t("Your password")}" required></label><div class="form-error" role="alert"></div><button class="btn primary" type="submit">${$t("Sign in")} ${icon("arrow")}</button>${mailEnabled ? `<p class="forgot"><button type="button" class="text-link" id="forgot-password">${$t("Forgot your password?")}</button></p>` : ""}<p class="footnote">${$t("Privately hosted · Powered by Rust")}<br>${$t("Ask your server administrator for an account.")}</p></form></main></div>`;
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
      if (!localMode && !isPane) startWorker();
      if (new URLSearchParams(location.search).get("next") === "capture")
        return location.assign("/capture");
      if (initialNote) await openNote(initialNote, { block: initialBlock });
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
  form.innerHTML = `<div class="eyebrow">${$t("FORGOT YOUR PASSWORD")}</div><h2>${$t("Reset your password")}</h2><p>${$t("Enter the email address of your account. We will send you a link to choose a new password.")}</p><label>${$t("Email address")}<input name="email" type="email" autocomplete="username" placeholder="${$t("you@example.com")}" required autofocus></label><div class="form-error" role="alert"></div><button class="btn primary" type="submit">${$t("Send the link")} ${icon("arrow")}</button><p class="footnote"><button type="button" class="text-link" id="back-to-login">${$t("Back to sign in")}</button></p>`;
  $("#back-to-login").onclick = login;
  form.onsubmit = async (e) => {
    e.preventDefault();
    const btn = $("button[type=submit]", form);
    btn.disabled = true;
    try {
      await api("password-reset", "POST", {
        email: form.email.value.trim(),
      });
      form.innerHTML = `<div class="eyebrow">${$t("CHECK YOUR EMAIL")}</div><h2>${$t("Look in your inbox")}</h2><p>${$t("If an account uses {value}, an email with a link to choose a new password is on its way. The link works for one hour.", { value: esc(form.email.value.trim()) })}</p><p class="footnote"><button type="button" class="text-link" id="back-to-login">${$t("Back to sign in")}</button></p>`;
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
  form.innerHTML = `<div class="eyebrow">${$t("YOUR ACCOUNT")}</div><h2>${$t("Choose a password")}</h2><p>${$t("Use at least 12 characters. You then sign in with it.")}</p><label>${$t("New password")}<input name="password" type="password" autocomplete="new-password" minlength="12" required autofocus></label><label>${$t("Repeat it")}<input name="repeat" type="password" autocomplete="new-password" minlength="12" required></label><div class="form-error" role="alert"></div><button class="btn primary" type="submit">${$t("Save password")} ${icon("arrow")}</button>`;
  form.onsubmit = async (e) => {
    e.preventDefault();
    if (form.password.value !== form.repeat.value)
      return ($(".form-error", form).textContent =
        $t("The two passwords are different."));
    try {
      const result = await api("password-reset/complete", "POST", {
        token,
        password: form.password.value,
      });
      history.replaceState(null, "", "/");
      login();
      $(".login-form").email.value = result.email || "";
      $(".login-form p").textContent =
        $t("Your password is saved. Sign in with it.");
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
    form.innerHTML = `<div class="eyebrow">${$t("TWO-STEP VERIFICATION")}</div><h2>${$t("Enter your code")}</h2><p>${recovery ? $t("Enter one of the recovery codes you saved when you set up two-factor authentication. Each code works once.") : $t("Open your authenticator app and enter the 6-digit code for Minerva.")}</p><label>${recovery ? $t("Recovery code") : $t("Authentication code")}<input name="code" ${recovery ? 'autocomplete="off" placeholder="xxxx-xxxx"' : 'inputmode="numeric" autocomplete="one-time-code" placeholder="123 456" maxlength="7"'} required autofocus></label><div class="form-error" role="alert"></div><button class="btn primary" type="submit">${$t("Verify")} ${icon("arrow")}</button><p class="footnote"><button type="button" class="text-link" id="toggle-recovery">${recovery ? $t("Use your authenticator app") : $t("Use a recovery code")}</button> · <button type="button" class="text-link" id="restart-login">${$t("Start over")}</button></p>`;
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
      if (!localMode && !isPane) startWorker();
      if (new URLSearchParams(location.search).get("next") === "capture")
        return location.assign("/capture");
      if (initialNote) await openNote(initialNote, { block: initialBlock });
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
    `<div class="app-shell"><aside class="sidebar"><div class="brand"><span class="brand-mark">✳</span>minerva</div><p class="section-label">${$t("SERVER ADMINISTRATION")}</p><nav class="nav">${[
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
      )}</nav><div class="sidebar-bottom"><div class="profile-name">${esc(data.user.name)}</div><p class="subtitle">${$t("Management account")}</p><button class="btn" id="admin-logout">${$t("Sign out")}</button></div></aside><main class="main"><header class="topbar"><button class="mobile-menu" aria-label="${$t("Toggle sidebar")}">${icon("menu")}</button><strong id="breadcrumb">${$t("Server administration")}</strong>${themeSelect()}</header><div id="content" tabindex="-1"></div></main></div>`;
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
    `<select ${attrs}><option value="viewer" ${role === "viewer" ? "selected" : ""}>${$t("Read only")}</option><option value="editor" ${role === "editor" ? "selected" : ""}>${$t("Edit")}</option></select>`;
  const draw = () => {
    const inside = info.members.filter((m) => m.role);
    const outside = info.members.filter((m) => !m.role && !m.owner);
    const rows = inside
      .sort((a, b) => b.owner - a.owner || a.name.localeCompare(b.name))
      .map(
        (m) =>
          `<li class="share-person">${avatar(m)}<div class="share-person-name"><strong>${esc(m.name)}${m.id === data.user.id ? ` <small>${$t("(you)")}</small>` : ""}</strong><small>${esc(m.email)}</small></div>${
            m.owner
              ? `<span class="pill">${$t("Owner")}</span>`
              : info.can_edit
                ? `${roleSelect(`data-member-role="${m.id}" aria-label="Access for ${esc(m.name)}"`, m.role)}<button type="button" class="share-remove" data-member-remove="${m.id}" aria-label="Remove ${esc(m.name)}" title="${$t("Remove access")}">×</button>`
                : `<span class="pill">${m.role === "editor" ? $t("Edit") : $t("Read only")}</span>`
          }</li>`,
      )
      .join("");
    modal.innerHTML = `<div class="share-workspace"><header><h2>${$t("Share {name}", { name: esc(name) })}</h2><p>${info.can_edit ? `People with <strong>${$t("Edit")}</strong> ${$t("manage folders, notes, the journal, sites and sharing, like you.")} <strong>${$t("Read only")}</strong> lets them read everything.` : $t("You can read this workspace. People who can edit it manage who has access.")}</p></header>${
      info.can_edit
        ? outside.length
          ? `<form class="share-invite" id="share-invite"><select name="user_id" aria-label="${$t("Person to invite")}" required>${outside.map((m) => `<option value="${m.id}">${esc(m.name)} · ${esc(m.email)}</option>`).join("")}</select>${roleSelect('name="role" aria-label="Access"', "editor")}<button class="btn primary" ${busy ? "disabled" : ""}>${$t("Invite")}</button></form>`
          : `<p class="share-hint">${$t("Everyone on this server already has access. Ask your administrator to add accounts.")}</p>`
        : ""
    }<h3 class="share-subtitle">${$t("People with access")} <span>${inside.length}</span></h3><ul class="share-people">${rows}</ul><div class="form-error" role="alert"></div><footer><button type="button" class="btn primary" id="share-done">${$t("Done")}</button></footer></div>`;
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
          $t("{who} can now open {name}", { who: who?.name || $t("They"), name }),
        );
      };
    $$("[data-member-role]", modal).forEach(
      (select) =>
        (select.onchange = () =>
          save(select.dataset.memberRole, select.value, $t("Access updated"))),
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
            $t("{who} no longer has access", { who: who?.name || $t("They") }),
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
  rememberWorkspace();
  // Redrawing keeps the sidebar where it was scrolled to.
  const sidebarTop = $(".sidebar-scroll")?.scrollTop || 0;
  $("#app").innerHTML =
    `<a class="skip-link" href="#content">${$t("Skip to content")}</a><div class="app-shell"><aside class="sidebar"><div class="brand"><span class="brand-mark">✳</span>minerva</div><div class="workspace-switch"><button id="workspace" class="workspace-button" aria-haspopup="menu" aria-expanded="false" aria-controls="workspace-menu" aria-label="Current workspace: ${esc(w?.name || "none")}. Switch workspace"><span class="square">${esc(w?.name.slice(0, 1) || "M")}</span><span class="workspace-name">${esc(w?.name || $t("No workspace"))}</span>${icon("chevron")}</button><div class="workspace-menu" id="workspace-menu" role="menu" aria-label="${$t("Workspaces")}" hidden>${workspaceMenuItems()}</div></div><div class="search-row"><button id="search">${icon("search")}${$t("Search")} <span class="shortcut">${$t("Ctrl K")}</span></button><button id="quick-note" aria-label="${$t("New note")}" title="${$t("New note")}">${icon("plus")}</button></div><div class="sidebar-scroll"><nav class="nav" aria-label="${$t("Main navigation")}">${[
      ["home", "home", $t("Overview")],
      ["all", "note", $t("All notes")],
      ["journal", "calendar", $t("Journal")],
      ["reminders", "bell", $t("Reminders")],
      ["templates", "template", $t("Templates")],
      ["databases", "grid", $t("Databases")],
      ["graph", "graph", $t("Graph")],
      ["flashcards", "template", $t("Flashcards")],
      ["chat", "brain", $t("AI chat")],
    ]
      .map(
        ([v, i, t]) =>
          `<button data-view="${v}" class="${view === v && !note ? "active" : ""}">${icon(i)}${t}</button>`,
      )
      .join(
        "",
      )}</nav>${sidebarSections()}</div><div class="sidebar-bottom"><div class="profile"><span class="avatar" title="${localMode ? $t("On this computer") : $t("Connected to server")} · v0.2"><i class="dot" aria-hidden="true"></i>${esc(data.user.name.slice(0, 2).toUpperCase())}</span>${localMode ? `<div class="profile-link"><div class="profile-name">${$t("On this computer")}</div><div class="profile-role">${$t("Personal notes · offline copies")}</div></div>` : `<button class="profile-link" data-view="account" title="${$t("Account security")}"><div class="profile-name">${esc(data.user.name)}</div><div class="profile-role">${$t("{value} · Security", { value: data.user.admin ? $t("Server administrator") : $t("Member") })}</div></button><button id="logout" aria-label="${$t("Sign out")}">${icon("logout")}</button>`}</div></div></aside><main class="main"><header class="topbar"><div class="crumb"><button class="mobile-menu" aria-label="${$t("Toggle sidebar")}">${icon("menu")}</button>${icon("home")}<span>${esc(w?.name || $t("Your server"))}</span><span>/</span><strong id="breadcrumb">${$t("Overview")}</strong></div><div class="top-actions">${themeSelect()}${workspaceBadge()}<span class="avatar">${esc(data.user.name[0])}</span></div></header><div id="content" tabindex="-1"></div></main></div>`;
  $$("[data-view]").forEach((b) =>
    action(b, "click", () => navigate(b.dataset.view)),
  );
  $$("[data-folder]").forEach((b) =>
    action(b, "click", () => navigate("folder", b.dataset.folder)),
  );
  $$("[data-open-tag]").forEach((b) =>
    action(b, "click", () => openTag(b.dataset.openTag)),
  );
  $(".sidebar-scroll").scrollTop = sidebarTop;
  sidebarOverflow();
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
  else if (view === "graph") renderGraph(folderId).catch(error);
  else if (view === "flashcards")
    renderFlashcards({ api, esc, data, error }).catch(error);
  else if (view === "reminders")
    renderReminders({
      api,
      esc,
      workspaceId,
      me: data.user.name,
      canAdd: folders().some((f) => data.roles[f.id] === "editor"),
      modal,
      openNote,
      toast,
      error,
    }).catch(error);
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
/** Adds blocks at the end of the open note, or below the block `after`. */
async function appendBlocks(
  markdowns,
  { after = null, source = "AI chat" } = {},
) {
  if (!note || !editable())
    throw new Error($t("You can only add to a note you can edit."));
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
  const below = note.blocks.findIndex((b) => b.id === after);
  const at = below < 0 ? note.blocks.length : below + 1;
  note.blocks.splice(at, 0, ...extra);
  note.rendered.splice(at, 0, ...rendered);
  markDirty();
  renderBlocks();
  $(`[data-block="${CSS.escape(extra[0].id)}"]`)?.scrollIntoView({
    block: "center",
    behavior: "smooth",
  });
  announce($tn(extra.length, "Added {count} block", "Added {count} blocks"));
  toast(
    $tn(extra.length, "Added {count} block from {source}", "Added {count} blocks from {source}", { source }),
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
          reject(new Error($t("The note did not respond. Nothing was added.")));
      }, 25000);
    });
  if (noteTabs) return noteTabs.insertBlocks({ id, title, blocks });
  if (note?.id === id) return appendBlocks(blocks);
  throw new Error($t("Open the note to add to it."));
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
  if (!target) throw new Error($t("You need edit access to a folder to save it."));
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
  graphView?.destroy();
  graphView = null;
  localGraph?.destroy();
  localGraph = null;
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
  $("#breadcrumb").textContent = $t("Overview");
  $("#content").innerHTML =
    `<section class="page"><div class="eyebrow">${$t("YOUR WORKSPACE, AT A GLANCE")}</div><div class="page-heading"><div><h1>${$t("A place for your ideas")}</h1><p class="subtitle">${$t("Collect your thoughts. Connect the dots. Make something great.")}</p></div><button class="btn primary" id="new-note">${icon("plus")}${$t("New note")}</button></div><div id="dashboard-root"></div></section>`;
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
  const inWorkspace = new Set(folders().map((f) => f.id));
  let notes =
    view === "archive"
      ? data.archived_notes.filter(
          (n) => !n.trashed && inWorkspace.has(n.folder_id),
        )
      : workspaceNotes().filter((n) =>
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
      ? $t("A place for your ideas")
      : view === "all"
        ? $t("All notes")
        : view === "favorites"
          ? $t("Your favorites")
          : view === "trash"
            ? $t("Trash")
            : view === "archive"
              ? $t("Archived")
              : view === "tag"
                ? "#" + tagFilter
                : folders().find((f) => f.id === folderId)?.name ||
                  $t("Folder");
  $("#breadcrumb").textContent =
    view === "home"
      ? $t("Overview")
      : view === "folder"
        ? folderPath(folderId)
            .map((f) => f.name)
            .join(" / ")
        : title;
  $("#content").innerHTML =
    `<section class="page"><div class="eyebrow">${view === "home" ? $t("YOUR WORKSPACE, AT A GLANCE") : $t("A LITTLE MORE ORGANIZED")}</div><div class="page-heading"><div><h1>${esc(title)}</h1>${view === "folder" ? `<button class="btn" id="ask-folder">${$t("Ask AI about this folder")}</button>` : ""}<p class="subtitle">${view === "home" ? $t("Collect your thoughts. Connect the dots. Make something great.") : view === "trash" ? $t("Nothing is permanently deleted. Restore a note whenever you need it.") : view === "archive" ? $t("Notes put away: out of the sidebar and search, never deleted. Open one to bring it back.") : $tn(notes.length, "{count} note · Space to think, write, and explore.", "{count} notes · Space to think, write, and explore.")}</p></div>${view !== "trash" && view !== "archive" ? `<button id="new-note" class="btn primary" ${!folders().some((f) => data.roles[f.id] === "editor") ? "disabled" : ""}>${icon("plus")}${$t("New note")}</button>` : ""}</div>${
      view === "home"
        ? `<div class="welcome-banner"><div class="welcome-copy"><div class="eyebrow">${$t("A FRESH START")}</div><h2>${$t("Good ideas start with a little space.")}</h2><p>${$t("A quick thought, a detailed plan, your next big thing.")}<br>${$t("Bring it all together in your workspace.")}</p><button class="text-link" id="start-writing">${$t("Start writing")} <span>↗</span></button></div>${art()}</div><div class="section-title"><h2>${$t("Your folders")}</h2><span>${$t("{count} spaces to keep things together", { count: folderChildren(treeFolders(), null).length })}</span></div><div class="folder-grid">${
            folderChildren(treeFolders(), null).map(folderCard).join("") ||
            `<div class="empty">${$t("Your administrator can grant you folder access.")}</div>`
          }</div>`
        : view === "folder" && folderChildren(treeFolders(), folderId).length
          ? `<div class="section-title"><h2>${$t("Subfolders")}</h2></div><div class="folder-grid">${folderChildren(treeFolders(), folderId).map(folderCard).join("")}</div>`
          : ""
    }<div class="notes-toolbar"><div class="tabs"><button class="active">${view === "home" ? $t("Recently updated") : view === "trash" ? $t("Deleted notes") : view === "archive" ? $t("Archived notes") : $t("Your notes")} <span>(${notes.length})</span></button>${view === "folder" && editsWorkspace() ? `<button id="rename-folder">${$t("Rename folder")}</button>` : ""}${view === "folder" && data.roles[folderId] === "editor" ? `<button id="delete-folder" class="danger-text">${$t("Delete folder")}</button>` : ""}</div><div style="display:flex;gap:12px;align-items:center">${view === "archive" ? "" : view !== "trash" ? `<button id="import" class="btn" title="${$t("Import notes from Markdown, Notion, Obsidian, Logseq or Evernote")}">${icon("upload")}${$t("Import")}</button>` : notes.some((n) => data.roles[n.folder_id] === "editor") ? `<button id="empty-trash" class="btn danger">${icon("trash")}${$t("Empty trash")}</button>` : ""}<div class="view-toggle"><button data-layout="grid" aria-label="${$t("Grid view")}" class="${layout === "grid" ? "active" : ""}">${icon("grid")}</button><button data-layout="list" aria-label="${$t("List view")}" class="${layout === "list" ? "active" : ""}">${icon("list")}</button></div></div></div><div class="note-${layout}">${notes.map((n) => `<button class="note-card" data-note="${n.id}"><div class="note-preview" aria-hidden="true"><span class="preview-symbol">${esc(n.icon)}</span><div class="preview-lines"><i></i><i></i><i></i></div></div><div class="note-info"><strong>${esc(n.title)}</strong><div class="note-meta">${icon("folder")}<span>${esc(data.folders.find((f) => f.id === n.folder_id)?.name)}</span><span>· ${date(n.updated_at)}</span>${n.favorite ? `<span class="star">☆</span>` : ""}${sharingBadges(n.sharing)}</div></div></button>`).join("")}</div>${!notes.length ? `<div class="empty">${icon("note")}<h3>${view === "trash" ? $t("All clear. No deleted notes.") : view === "archive" ? $t("Nothing archived.") : $t("A blank page, endless possibilities.")}</h3>${view === "trash" ? $t("Deleted notes will appear here.") : view === "archive" ? $t("Archive a note from its ⋯ menu to put it away without deleting it.") : $t("Create a note to give your ideas a home.")}</div>` : ""}<div class="page-footer">${$t("A little clarity. A little possibility. Your Minerva.")}</div></section>`;
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
      $t("Empty the trash?"),
      `${$tn(doomed.length, "{count} note is deleted permanently, with history, files, shares, and published pages.", "{count} notes are deleted permanently, with history, files, shares, and published pages.")}${doomed.length < notes.length ? $t(" Notes in folders you can only view stay.") : ""} ${$t("This cannot be undone.")}`,
      "",
      async () => {
        for (const n of doomed) await api("notes/" + n.id, "DELETE");
        await refresh();
        shell();
        toast($t("Trash emptied"));
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
  return `<button class="folder-card" data-open-folder="${f.id}"><span class="folder-icon">${icon("folder")}</span><div><strong>${esc(f.name)}</strong><small>${$tn(notes, "{count} note{value} · {value2}", "{count} notes{value} · {value2}", { value: subfolders ? ` · ${$tn(subfolders, "{count} subfolder", "{count} subfolders")}` : "", value2: data.roles[f.id] === "editor" ? $t("Can edit") : $t("Read only") })}</small></div></button>`;
}
function formDialog(title, desc, fields, onSubmit, label = "Create") {
  modal.innerHTML = `<form><h2>${esc(title)}</h2><p>${esc(desc)}</p>${fields}<div class="form-error" role="alert"></div><footer><button type="button" class="btn" id="cancel">${$t("Cancel")}</button><button class="btn primary" type="submit">${esc(label)}</button></footer></form>`;
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
    return toast($t("You need editor access to a folder first."));
  formDialog(
    $t("A new beginning"),
    $t("Give your note a name. You can always change it later."),
    `<label>${$t("Note title")}<input name="title" placeholder="${$t("Untitled idea")}" required maxlength="200" autofocus></label><label>${$t("Folder")}<select name="folder_id">${folderOptions()}</select></label>`,
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
    $t("Create a workspace"),
    $t("A separate home for a team, project, or part of your life."),
    `<label>${$t("Workspace name")}<input name="name" placeholder="${$t("e.g. Design studio")}" required maxlength="200" autofocus></label>`,
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
    $t("Delete “{name}”?", { name: w.name }),
    $t("Every folder, note, database, site, published page, and file in this workspace is deleted permanently, and members lose access. This cannot be undone."),
    `<label>${$t("Type the workspace name to confirm")}<input name="confirm" required autocomplete="off"></label>`,
    async (v) => {
      if (v.confirm !== w.name)
        throw new Error($t("Type the workspace name exactly to confirm."));
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
      toast($t("Workspace deleted"));
    },
    "Delete workspace",
  );
}
/** Opens a note; with `split`, in a tab beside the note tab that is shown. */
async function openNote(id, { split = false, block = null } = {}) {
  if (block && note?.id === id && !noteTabs && !chatPane) return showBlock(block);
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
      split,
      block: block || "",
    });
    return;
  }
  if (isPane && ((note && note.id !== id) || chatPane)) {
    paneMessage("open-note", {
      id,
      title: data.notes.find((n) => n.id === id)?.title,
      block,
    });
    return;
  }
  await flush();
  videos.stop();
  stopLive();
  localGraph?.destroy();
  localGraph = null;
  note = await api("notes/" + id);
  workspaceId = workspaceOfNote(note) || workspaceId;
  rememberRendered();
  dirty = false;
  conflict = false;
  mode = "document";
  comments.attach(id);
  if (note.offline_copy) {
    note.role = "viewer";
    live = null;
    shell();
    if (block) showBlock(block);
    return;
  }
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
        toast($t("Access changed. Export any unsaved draft before leaving."));
        stopLive();
      }
    },
  );
  live.presence = blockPresence;
  await live.start();
  shell();
  if (block) showBlock(block);
}
/** Scrolls to a block of the open note and marks it for a moment. */
function showBlock(id) {
  if (!note?.blocks.some((b) => b.id === id))
    return toast($t("That block is no longer in the note."));
  revealBlock(id);
  const el = $(`#blocks [data-block="${CSS.escape(id)}"]`);
  el?.scrollIntoView({ block: "center" });
  el?.classList.add("comment-flash");
  setTimeout(() => el?.classList.remove("comment-flash"), 1500);
  $(".block-body", el)?.focus({ preventScroll: true });
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
  for (const key of [
    "title",
    "icon",
    "favorite",
    "trashed",
    "archived",
    "cover",
    "tags",
  ])
    note[key] = snapshot[key];
  // Folding kept in the note follows other people's changes.
  if (
    JSON.stringify(result.note.folded ?? null) !==
    JSON.stringify(note.folded ?? null)
  ) {
    note.folded = result.note.folded ?? undefined;
    foldedFor = null;
    drawFolding();
  }
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
          $t("This block was removed by a collaborator. Copy any draft text before leaving it."),
        );
      }
    }
  }
  peers = result.peers.filter((p) => p.client_id !== live?.clientId);
  // Someone else opened this block a moment earlier: they keep it.
  const open = $(
    `#blocks textarea[data-live-key="block:${CSS.escape(result.denied_block || "")}"]`,
  );
  if (result.denied_block && open) {
    const holder = lockedBy(result.denied_block);
    open.blur();
    // A window in the background gets no blur event; close the block anyway.
    if (open.isConnected) open.onblur?.();
    toast($t("{value} is already editing this block.", { value: holder ? holder.name : $t("Someone") }));
  }
  if ($("#presence"))
    $("#presence").innerHTML =
      peers
        .map(
          (p) =>
            `<span class="peer" style="--peer:${peerColor(p.name)}" title="${esc(p.editing ? "Editing a block" : "Viewing this note")}">${esc(p.name)}</span>`,
        )
        .join("") || `<span class="presence-label">${$t("Only you here")}</span>`;
  drawPeers();
}
/** Collaborators in the open note, without this window. */
let peers = [];
let hoveredBlock = null;
document.addEventListener("pointerover", (e) => {
  hoveredBlock =
    e.target.closest?.("#blocks [data-block]")?.dataset.block || null;
});
/** What collaborators see of this user: the block they edit or read. */
function blockPresence() {
  const editing = $('#blocks textarea[data-live-key^="block:"]')?.closest(
    "[data-block]",
  )?.dataset.block;
  if (editing) return { active_block: editing, editing: true };
  const focused = document.activeElement?.closest?.("#blocks [data-block]")
    ?.dataset.block;
  return { active_block: focused || hoveredBlock, editing: false };
}
/** The collaborator who has block `id` open for editing, if any. */
function lockedBy(id) {
  return peers.find((p) => p.editing && p.active_block === id);
}
/** True (after telling the user) when someone else is editing block `id`. */
function blockLocked(id) {
  const holder = lockedBy(id);
  if (holder)
    toast($t("{name} is editing this block. Try again when they move on.", { name: holder.name }));
  return Boolean(holder);
}
const PEER_COLORS = [
  "#5b63d3",
  "#d0572f",
  "#1f8a5b",
  "#b0399a",
  "#2b7fb8",
  "#a8741a",
  "#7a4fc9",
  "#c23a4f",
];
function peerColor(name) {
  let h = 0;
  for (const c of name) h = (h * 31 + c.codePointAt(0)) >>> 0;
  return PEER_COLORS[h % PEER_COLORS.length];
}
/**
 * Puts a name tag left of each block a collaborator is on, solid while they
 * edit it and outlined while they read. Tags slide when the person moves.
 */
function drawPeers() {
  const root = $("#blocks");
  if (!root) return;
  let layer = $(".peer-layer", root);
  if (!layer) {
    layer = document.createElement("div");
    layer.className = "peer-layer";
    layer.setAttribute("aria-hidden", "true");
    root.append(layer);
  }
  $$(".block.peer-editing", root).forEach((el) => {
    el.classList.remove("peer-editing");
    el.style.removeProperty("--peer");
    $(".block-body", el)?.removeAttribute("title");
  });
  const base = root.getBoundingClientRect();
  const stacked = new Map();
  const seen = new Set();
  for (const p of peers) {
    const el =
      p.active_block &&
      $(`.block[data-block="${CSS.escape(p.active_block)}"]`, root);
    if (!el) continue;
    const color = peerColor(p.name);
    if (p.editing) {
      el.classList.add("peer-editing");
      el.style.setProperty("--peer", color);
      $(".block-body", el)?.setAttribute(
        "title",
        $t("{name} is editing this block", { name: p.name }),
      );
    }
    const rect = el.getBoundingClientRect();
    const row = stacked.get(el) || 0;
    stacked.set(el, row + 1);
    let tag = layer.querySelector(`[data-peer="${CSS.escape(p.client_id)}"]`);
    if (!tag) {
      tag = document.createElement("span");
      tag.className = "peer-tag";
      tag.dataset.peer = p.client_id;
      layer.append(tag);
    }
    tag.textContent = p.editing ? `✎ ${p.name}` : p.name;
    tag.title = p.editing ? $t("{name} is editing this block", { name: p.name }) : $t("{name} is reading this block", { name: p.name });
    tag.classList.toggle("editing", p.editing);
    tag.style.setProperty("--peer", color);
    tag.style.top = `${rect.top - base.top + 4 + row * 22}px`;
    // With no room in the margin (narrow windows), the tag sits at the right.
    const inside = rect.left < tag.offsetWidth + 44;
    tag.classList.toggle("inside", inside);
    tag.style.left = `${(inside ? rect.right : rect.left) - base.left}px`;
    seen.add(p.client_id);
  }
  for (const tag of layer.querySelectorAll("[data-peer]"))
    if (!seen.has(tag.dataset.peer)) tag.remove();
}
addEventListener("resize", () => drawPeers());
addEventListener("pagehide", () => live?.leave());
function markDirty() {
  dirty = true;
  paneState();
  updateStats();
  if (note && !isEmptyNote(note)) $(".template-fill")?.remove();
  if (live && note) live.commit(note);
  const el = $("#save-state");
  if (el) el.textContent = $t("Unsaved changes");
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
      if ($("#save-state")) $("#save-state").textContent = $t("Saving…");
      try {
        const saved = await api("notes/" + current.id, "PUT", payload);
        current.revision = saved.revision;
        current.updated_at = saved.updated_at;
        if ($("#save-state"))
          $("#save-state").textContent = dirty
            ? $t("Unsaved changes")
            : $t("All changes saved");
      } catch (e) {
        dirty = true;
        if (e.status === 409) conflict = true;
        if ($("#save-state"))
          $("#save-state").textContent = conflict
            ? $t("Conflict · export draft")
            : $t("Not saved · retry");
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
      $t("A database cell has an unsaved value. Save it or use Reload records before leaving."),
    );
  await queueSave();
  if (conflict)
    throw new Error(
      $t("Export your draft and reload this page to resolve the conflict."),
    );
}
function renderEditor() {
  // Names for mention chips, and the people the `@` picker offers.
  loadPeople().catch(() => {});
  if (noteFilesFor !== note.id) {
    noteFilesFor = note.id;
    loadNoteFiles();
  }
  const can = editable();
  $("#breadcrumb").textContent = note.title;
  const sharing = note.sharing || {};
  const shareState = sharing.published
    ? `<a class="public-indicator" href="${esc(serverOrigin())}/${esc(sharing.published)}" target="_blank" rel="noopener">${$t("Published at /{published}", { published: esc(sharing.published) })}</a>`
    : sharing.links
      ? `<span class="public-indicator">${$t("Anyone with a link can open this note")}</span>`
      : sharing.members
        ? `<span class="share-indicator">${$tn(sharing.members, "Shared with {count} member", "Shared with {count} members")}</span>`
        : "";
  const editor = data.roles?.[note.folder_id] === "editor";
  const item = (id, iconName, label, extra = "", keys = "") =>
    `<button role="menuitem" tabindex="-1" id="${id}" ${extra}>${icon(iconName)}<span>${label}</span>${keys ? `<kbd>${shortcutKeys(keys)}</kbd>` : ""}</button>`;
  const separator = '<div class="menu-separator" role="separator"></div>';
  // The note's less frequent tools, grouped in a menu.
  const menu = [
    [
      item("find-note", "search", $t("Find and replace"), "", "Ctrl+F"),
      `<button role="menuitemcheckbox" tabindex="-1" id="focus-toggle" aria-checked="${document.body.classList.contains("focus-mode")}">${icon("canvas")}<span>${$t("Focus mode")}</span><kbd>${shortcutKeys("Ctrl+Shift+F")}</kbd></button>`,
      mode === "document"
        ? item(
            "present-note",
            "play",
            $t("Present"),
            `title="${$t("Show this note as slides, split at dividers or headings")}"`,
            "Ctrl+Alt+P",
          )
        : "",
      can ? item("save-note", "check", $t("Save now"), "", "Ctrl+S") : "",
      can && !shareMode
        ? `<button role="menuitemcheckbox" tabindex="-1" id="share-folding" aria-checked="${Array.isArray(note.folded)}" title="${$t("Fold and unfold for everyone who opens the note, as Logseq does")}">${icon("list")}<span>${$t("Keep folding in the note")}</span></button>`
        : "",
    ],
    shareMode
      ? []
      : [
          item("attachments", "upload", $t("Files")),
          item(
            "history",
            "clock",
            $t("History"),
            !data.roles[note.folder_id]
              ? 'aria-disabled="true" title="Version history is available to folder members"'
              : "",
          ),
          item(
            "metadata",
            "list",
            $t("Document details"),
            'title="Details used by PDF templates"',
          ),
        ],
    [
      item(
        "copy-note-link",
        "link",
        $t("Copy link"),
        'title="Copy a link to open this note in another Minerva tab"',
      ),
      shareMode
        ? ""
        : item(
            "copy-note-embed",
            "note",
            $t("Copy as embed"),
            'title="Copy the code that shows this note inside another note"',
          ),
      item("export", "download", $t("Export…")),
    ],
    shareMode
      ? []
      : [
          note.trashed || note.record || note.journal
            ? ""
            : item(
                "archive-note",
                "archive",
                note.archived ? $t("Unarchive") : $t("Archive"),
                `title="${note.archived ? $t("Bring this note back to the sidebar and search") : $t("Put this note away: out of the sidebar and search, not deleted")}" ${!editable() ? 'aria-disabled="true"' : ""}`,
              ),
          item(
            "trash-note",
            note.trashed ? "check" : "trash",
            note.trashed ? $t("Restore") : $t("Move to trash"),
            `class="${note.trashed ? "" : "danger-item"}" ${!editable() && !(note.trashed && editor) ? 'aria-disabled="true"' : ""}`,
          ),
          note.trashed && editor
            ? item("delete-note", "trash", $t("Delete forever"), 'class="danger-item"')
            : "",
        ],
  ]
    .map((group) => group.filter(Boolean).join(""))
    .filter(Boolean)
    .join(separator);
  const tools = `${
    shareMode && !can
      ? ""
      : `<button class="btn icon-btn" id="undo" aria-label="${$t("Undo")}" title="${$t("Undo (Ctrl+Z)")}" ${!can ? "disabled" : ""}>↶</button><button class="btn icon-btn" id="redo" aria-label="${$t("Redo")}" title="${$t("Redo (Ctrl+Shift+Z)")}" ${!can ? "disabled" : ""}>↷</button>`
  }${
    shareMode
      ? guestComments()
        ? `<button class="btn" id="comments-toggle" aria-pressed="false" aria-label="${$t("Comments")}" title="${$t("Comments on this note")}">💬<span class="toolbar-label"> ${$t("Comments")}</span> <span class="comments-count"></span></button>`
        : ""
      : `<button class="btn icon-btn" id="favorite" aria-label="${$t("Favorite")}" aria-pressed="${!!note.favorite}" title="${$t("Favorite")}" ${!can ? "disabled" : ""}>${note.favorite ? "★" : "☆"}</button>${localMode ? "" : `<button class="btn" id="share-note" ${!editor || note.trashed ? `disabled title="${$t("Folder editors can share this note")}"` : ""}>${$t("Share")}${sharing.published || sharing.links ? `<span class="share-badge public">${$t("Public")}</span>` : ""}</button>`}<button class="btn" id="comments-toggle" aria-pressed="false" aria-label="${$t("Comments")}" title="${$t("Comments on this note")}">💬<span class="toolbar-label"> ${$t("Comments")}</span> <span class="comments-count"></span></button><button class="btn" id="ask-note" aria-label="${$t("Ask AI")}" title="${$t("Chat with AI about this note, side by side")}">${icon("brain")}<span class="toolbar-label">${$t("Ask AI")}</span></button>`
  }<div class="toolbar-more"><button class="btn icon-btn" id="note-more" aria-haspopup="menu" aria-expanded="false" aria-controls="note-menu" aria-label="${$t("More note tools")}" title="${$t("More")}">${icon("more")}</button><div class="row-menu note-menu" id="note-menu" role="menu" aria-label="${$t("Note tools")}" hidden>${menu}</div></div>`;
  $("#content").innerHTML =
    `<div class="editor-toolbar" role="toolbar" aria-label="${$t("Note tools")}"><span class="save-status" id="save-state" role="status">${can ? $t("All changes saved") : $t("Read only")}</span><span id="presence" class="presence"></span><span class="spacer"></span>${tools}</div>${coverHTML(note.cover, can)}<article class="document ${note.cover ? "has-cover" : ""}">${can ? `<div class="doc-adders">${note.icon ? "" : `<button class="text-link" id="add-icon">${$t("☺ Add icon")}</button>`}${note.cover ? "" : `<button class="text-link" id="add-cover">${$t("▧ Add cover")}</button>`}</div>` : ""}${note.icon ? (can ? `<button class="doc-icon" id="doc-icon" aria-label="${$t("Change icon")}" title="${$t("Change icon")}">${esc(note.icon)}</button>` : `<div class="doc-icon" aria-hidden="true">${esc(note.icon)}</div>`) : ""}${note.journal && !shareMode ? `<nav class="journal-nav" aria-label="${$t("Journal days")}"><button class="btn" data-journal-shift="-1" aria-label="${$t("Previous day")}">‹ ${esc(parseDate(shiftDate(note.journal, -1)).toLocaleDateString(undefined, { month: "short", day: "numeric" }))}</button><span>${esc(longDate(note.journal))}</span><button class="btn" data-journal-shift="1" aria-label="${$t("Next day")}">${esc(parseDate(shiftDate(note.journal, 1)).toLocaleDateString(undefined, { month: "short", day: "numeric" }))} ›</button></nav>` : ""}${!shareMode && note.archived && !note.trashed ? `<div class="archived-banner" role="note">${icon("archive")}<span>${$t("Archived · out of the sidebar and search")}</span>${editable() ? `<button class="btn" id="unarchive-banner">${$t("Unarchive")}</button>` : ""}</div>` : ""}${!shareMode && isTemplate(note) ? `<span class="template-badge">${$t("Template · used by “Fill from template”")}</span>` : ""}<input id="note-title" data-live-key="title" class="doc-title" aria-label="${$t("Note title")}" maxlength="200" value="${esc(note.title)}" ${!can || note.record ? "readonly" : ""} ${note.record ? 'title="A record\'s page is named after the record: rename it in the properties below"' : ""}><div class="doc-meta">${shareMode ? "" : `<span>${esc(data.folders.find((f) => f.id === note.folder_id)?.name || $t("Shared with you"))}</span>`}<span>${$t("Updated {date}", { date: date(note.updated_at) })}</span><span>${$t("Revision {revision}", { revision: note.revision })}</span><span id="word-count"></span>${shareState}</div>${note.record && !shareMode ? `<section class="record-panel" id="record-panel" aria-label="${$t("Record properties")}"></section>` : ""}<div class="tag-bar" id="tag-bar"></div>${can && !shareMode && isEmptyNote(note) && templateNotes(workspaceOfNote(note)).some((t) => t.id !== note.id) ? `<div class="template-fill"><button class="btn" id="fill-template">${icon("template")}${$t("Fill from template")}</button><span>${$t("or just start writing")}</span></div>` : ""}<div id="blocks" role="list" aria-label="${$t("Note blocks")}"></div><span id="block-edit-hint" hidden>${$t("Press Enter to edit.")}</span>${can ? `<button class="add-block" id="add-block">${$t("+ Add a block")}</button><p class="editor-help" id="editor-help">${$t("Click a block to edit · Enter splits it at the cursor · Backspace at the start merges with the block above · Shift+Enter adds a line break · ↑/↓ at the edges move between blocks · Ctrl+Z undoes your changes · / for block types · Ctrl+J for AI on the selection · Ctrl+Shift+P for commands on selected text")}</p>` : ""}<section class="backlinks" id="backlinks" aria-label="${$t("Linked from")}"></section></article>`;
  renderBlocks();
  renderTagBar();
  renderBacklinks();
  updateStats();
  bindToolbarMenu($("#note-more"), $("#note-menu"));
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
  action($("#present-note"), "click", presentNote);
  action($("#share-folding"), "click", async (e) => {
    const on = !Array.isArray(note.folded);
    await shareFolding(on);
    e.currentTarget?.setAttribute("aria-checked", String(on));
  });
  renderRecordPanel().catch(error);
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
    $("#favorite").setAttribute("aria-pressed", String(!!note.favorite));
  });
  action($("#export"), "click", exportNote);
  action($("#metadata"), "click", () => metadataDialog());
  action($("#copy-note-link"), "click", async () => {
    await navigator.clipboard.writeText(`${serverOrigin()}/?note=${note.id}`);
    toast($t("Note link copied"));
  });
  action($("#copy-note-embed"), "click", async () => {
    await navigator.clipboard.writeText(embedCode(note.title, note.id));
    toast($t("Embed copied. Paste it into another note to show this one there."));
  });
  action($("#history"), "click", history);
  action($("#attachments"), "click", attachmentDialog);
  action($("#ask-note"), "click", () => askAI({ kind: "note", id: note.id }));
  action($("#comments-toggle"), "click", () => comments.toggle());
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
      $t("Delete this note forever?"),
      $t("The note, its version history, files, shares, and published page are removed permanently. This cannot be undone."),
      "",
      async () => {
        await api("notes/" + note.id, "DELETE");
        note = null;
        view = "trash";
        await refresh();
        shell();
        toast($t("Note deleted"));
      },
      "Delete forever",
    ),
  );
  const toggleArchive = async () => {
    if (!editable()) return;
    note.archived = !note.archived;
    markDirty();
    await queueSave();
    const id = note.id;
    await refresh();
    toast(note.archived ? $t("Note archived") : $t("Note back from the archive"));
    await openNote(id);
  };
  action($("#archive-note"), "click", toggleArchive);
  action($("#unarchive-banner"), "click", toggleArchive);
  action($("#trash-note"), "click", () =>
    formDialog(
      note.trashed ? $t("Restore note?") : $t("Move note to trash?"),
      note.trashed
        ? $t("This note will return to its folder.")
        : $t("You can restore this note from Trash at any time."),
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
      note.title = $t("Untitled");
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
  return `<div class="note-cover ${gradient ? `cover-gradient-${gradient}` : ""}">${inner}${can ? `<button class="btn cover-change" id="change-cover">${$t("Change cover")}</button>` : ""}</div>`;
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
function setNoteField(key, value) {
  live?.boundary();
  note[key] = value;
  markDirty();
  queueSave().catch(() => {});
  renderEditor();
}
/** The note's icon: any emoji, found by searching its name. */
function iconPicker(anchor) {
  const pop = popover(
    anchor,
    `<div class="icon-emoji"></div>${note.icon ? `<button class="text-link" data-icon="">${$t("Remove icon")}</button>` : ""}`,
    "Choose an icon",
  );
  $("[data-icon]", pop)?.addEventListener("click", () => {
    closePopover();
    setNoteField("icon", "");
  });
  emojiPicker($(".icon-emoji", pop), {
    esc,
    onPick: (char) => {
      closePopover();
      setNoteField("icon", char);
    },
  }).catch(error);
}
/**
 * `/toggle`: the block becomes a heading line (its text, or "Toggle") with an
 * indented block under it to write in; the arrow beside it folds them.
 */
async function insertToggle(existingId) {
  let b = note.blocks.find((x) => x.id === existingId);
  live?.boundary();
  if (!b) {
    b = newBlock("");
    note.blocks.push(b);
    note.rendered.push("");
  }
  const level = indentLevel(b.markdown);
  if (!b.markdown.trim()) b.markdown = "  ".repeat(level) + "**Toggle**";
  const child = newBlock("  ".repeat(Math.min(level + 1, 3)));
  const i = note.blocks.indexOf(b);
  note.blocks.splice(i + 1, 0, child);
  note.rendered.splice(i + 1, 0, "");
  note.rendered[i] = await renderMarkdown(b.markdown);
  markDirty();
  renderBlocks();
  editBlock(child.id);
  await queueSave();
}
/** `/emoji`: pick an emoji and add it to the block. */
function emojiDialog(existingId) {
  modal.innerHTML =
    `<h2>${$t("Insert an emoji")}</h2><div class="emoji-dialog"></div><footer><button class="btn" id="close-emoji">${$t("Close")}</button></footer>`;
  modal.showModal();
  $("#close-emoji").onclick = () => modal.close();
  emojiPicker($(".emoji-dialog", modal), {
    esc,
    onPick: async (char) => {
      modal.close();
      await appendToBlock(existingId, char);
      await queueSave();
    },
  }).catch(error);
}
function coverPicker(anchor) {
  const pop = popover(
    anchor,
    `<div class="cover-grid">${Array.from({ length: COVER_GRADIENTS }, (_, i) => `<button class="cover-gradient-${i + 1}" data-cover="gradient:${i + 1}" aria-label="Gradient ${i + 1}"></button>`).join("")}</div><label class="btn cover-upload">${$t("Upload image")}<input type="file" accept="image/png,image/jpeg,image/gif,image/webp" hidden></label><form class="cover-url"><input name="url" type="url" placeholder="https://… image address" aria-label="${$t("Image address")}"><button class="btn">${$t("Use")}</button></form>${note.cover ? `<button class="text-link" data-cover="">${$t("Remove cover")}</button>` : ""}`,
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
      return toast($t("Use an https:// image address"));
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
      ? `<form class="tag-add"><input name="tag" list="tag-suggestions" placeholder="${$t("+ Add tag")}" aria-label="${$t("Add tag")}" maxlength="41" autocomplete="off"><datalist id="tag-suggestions">${allTags()
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
      announce($t("Removed tag {removeTag}", { removeTag: b.dataset.removeTag }));
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
          $t("Tags use letters, digits, - _ and /, up to 40 characters"),
        );
      if (own.includes(tag)) return;
      if (own.length >= 20) return toast($t("A note can have up to 20 tags"));
      setNoteField("tags", [...own, tag]);
      announce($t("Added tag {tag}", { tag }));
      $(".tag-add input")?.focus();
    };
}
function renderBacklinks() {
  const panel = $("#backlinks");
  if (!panel) return;
  const links = shareMode ? [] : note.backlinks || [];
  const linksOut = note.blocks.some((b) => b.markdown.includes("](#note/"));
  panel.hidden = !links.length && !linksOut;
  panel.innerHTML = `${links.length ? `<h2>${$tn(links.length, "Linked from {count} note", "Linked from {count} notes")}</h2>${links.map((l) => `<a class="backlink" href="#note/${esc(l.id)}">${esc(l.icon)} ${esc(l.title)}</a>`).join("")}` : ""}${shareMode ? "" : `<details class="local-graph" id="local-graph" ${localGraphOpen() ? "open" : ""}><summary>${$t("Linked notes as a graph")}</summary><div class="local-graph-body"><canvas id="local-graph-canvas" role="img" aria-label="${$t("This note and the notes it links with")}"></canvas></div></details><button class="text-link graph-link" id="show-graph">${icon("graph")}${$t("Show in the graph")}</button>`}`;
  $("#show-graph", panel)?.addEventListener("click", () => showGraph(note.id));
  const details = $("#local-graph", panel);
  if (!details) return;
  details.addEventListener("toggle", () => {
    try {
      localStorage.setItem("minerva.localGraph", details.open ? "1" : "");
    } catch {}
    if (details.open) drawLocalGraph().catch(() => {});
    else {
      localGraph?.destroy();
      localGraph = null;
    }
  });
  if (details.open) drawLocalGraph().catch(() => {});
}
// The small graph under a note: the note and the notes it links with.
let localGraph = null;
function localGraphOpen() {
  try {
    return localStorage.getItem("minerva.localGraph") === "1";
  } catch {
    return false;
  }
}
async function drawLocalGraph() {
  localGraph?.destroy();
  localGraph = null;
  const id = note?.id;
  const wid = workspaceOfNote(note);
  if (!id || !wid) return;
  const raw = await api(`workspaces/${wid}/graph`);
  const canvas = $("#local-graph-canvas");
  if (note?.id !== id || !canvas) return;
  const graph = neighborhood(buildGraph(raw), id, 1);
  const folderIds = [...new Set(raw.nodes.map((n) => n.folder_id))];
  localGraph = mountGraph(canvas, graph, {
    color: (n) => folderColor(folderIds, n.folder),
    onOpen: (n) => n.id !== id && openGraphNode(n),
  });
  // All of it in sight, this note marked.
  localGraph.select(id);
}
/** The graph around a note: in the main page when the note is in a tab. */
function showGraph(id) {
  if (isPane) return paneMessage("open-graph", { id });
  navigate("graph", id).catch(error);
}
// The graph view's choices, kept while the page is open.
const graphChoices = {
  tags: false,
  orphans: true,
  journal: true,
  depth: 2,
  folder: "",
  since: "",
  color: "folder",
};
let graphView = null;
/**
 * The graph of the workspace's notes, or of the notes around `focus` (a note
 * ID) within `graphChoices.depth` links.
 */
async function renderGraph(focus = null) {
  graphView?.destroy();
  graphView = null;
  $("#breadcrumb").textContent = $t("Graph");
  $("#content").innerHTML = `<section class="page graph-page"><p class="hint">${$t("Drawing the graph…")}</p></section>`;
  const wid = workspaceId;
  const raw = await api(`workspaces/${wid}/graph`);
  if (view !== "graph") return;
  // A folder chosen in another workspace does not apply here.
  if (!folders().some((f) => f.id === graphChoices.folder)) graphChoices.folder = "";
  // A folder filter keeps its subfolders too.
  const inFolder = graphChoices.folder
    ? new Set(
        folders()
          .filter((f) => folderPath(f.id).some((p) => p.id === graphChoices.folder))
          .map((f) => f.id),
      )
    : null;
  const since = graphChoices.since
    ? Math.floor(Date.now() / 1000) - Number(graphChoices.since) * 86400
    : 0;
  const whole = buildGraph(raw, { ...graphChoices, folders: inFolder, since });
  const around = focus && whole.nodes.find((n) => n.id === focus);
  const graph = around ? neighborhood(whole, focus, graphChoices.depth) : whole;
  const folderIds = [...new Set(raw.nodes.map((n) => n.folder_id))];
  const folderName = (id) => data.folders.find((f) => f.id === id)?.name || "";
  const byTag = graphChoices.color === "tag";
  const tagged = tagColors(graph.nodes.filter((n) => n.kind === "note"));
  const notes = graph.nodes.filter((n) => n.kind === "note").length;
  let positions = { ...(raw.positions || {}) };
  const legend = byTag
    ? tagged.tags.map(
        (t) =>
          `<span><i style="background:${tagged.colorOf(t)}"></i>#${esc(t)}</span>`,
      )
    : folderIds
        .filter((id) => graph.nodes.some((n) => n.folder === id))
        .map(
          (id) =>
            `<span><i style="background:${folderColor(folderIds, id)}"></i>${esc(folderName(id))}</span>`,
        );
  const select = (id, label, options, value) =>
    `<label class="cal-by">${label}<select id="${id}">${options.map(([v, text]) => `<option value="${esc(v)}" ${v === value ? "selected" : ""}>${esc(text)}</option>`).join("")}</select></label>`;
  $("#content").innerHTML =
    `<section class="page graph-page"><div class="page-heading"><div><h1>${around ? $t("Around “{label}”", { label: esc(around.label) }) : $t("Graph")}</h1><p class="subtitle">${$tn(notes, "{count} note", "{count} notes")} · ${$tn(graph.edges.length, "{count} link", "{count} links")}. ${$t("Drag to move, scroll to zoom, click a note to open it.")} ${$t("A note you drag stays where you put it.")}</p></div></div><div class="graph-toolbar"><div class="graph-find"><input type="search" id="graph-search" placeholder="${$t("Find a note…")}" aria-label="${$t("Find a note in the graph")}" autocomplete="off"><div class="graph-results" id="graph-results" role="listbox" aria-label="${$t("Matching notes")}"></div></div>${around ? `<label class="cal-by">${$t("Links away")}<select id="graph-depth">${[1, 2, 3].map((d) => `<option ${d === graphChoices.depth ? "selected" : ""}>${d}</option>`).join("")}</select></label><button class="btn" id="graph-whole">${$t("Whole workspace")}</button>` : ""}${select(
      "graph-folder",
      $t("Folder"),
      [
        ["", $t("All folders")],
        ...folders()
          .filter((f) => raw.nodes.some((n) => n.folder_id === f.id) || data.folders.some((c) => c.parent_id === f.id))
          .map((f) => [
            f.id,
            folderPath(f.id)
              .map((p) => p.name)
              .join(" / "),
          ]),
      ],
      graphChoices.folder,
    )}${select(
      "graph-since",
      $t("Updated"),
      [
        ["", $t("Any time")],
        ["7", $t("In the last week")],
        ["30", $t("In the last month")],
        ["365", $t("In the last year")],
      ],
      graphChoices.since,
    )}${select(
      "graph-color",
      $t("Colour by"),
      [
        ["folder", $t("Folder")],
        ["tag", $t("Tag")],
      ],
      graphChoices.color,
    )}${[
      ["tags", $t("Tags")],
      ["orphans", $t("Unlinked notes")],
      ["journal", $t("Journal")],
    ]
      .map(
        ([key, label]) =>
          `<label class="checkbox-label"><input type="checkbox" data-graph-choice="${key}" ${graphChoices[key] ? "checked" : ""}>${label}</label>`,
      )
      .join(
        "",
      )}<button class="btn" id="graph-fit">${$t("Fit")}</button><button class="btn" id="graph-reset" ${Object.keys(positions).length ? "" : "hidden"} title="${$t("Let every note you placed move freely again")}">${$t("Reset layout")}</button></div><div class="graph-wrap"><canvas id="graph-canvas" role="img" aria-label="${$tn(notes, "Graph of {count} note and its links. Use “Find a note” and the list beside it to move through it with the keyboard.", "Graph of {count} notes and their links. Use “Find a note” and the list beside it to move through it with the keyboard.")}"></canvas><aside class="graph-panel" id="graph-panel" aria-live="polite"></aside></div><div class="graph-legend">${legend.join("")}${byTag && tagged.tags.length ? `<span><i class="untagged"></i>${$t("Other tags or none")}</span>` : ""}</div>${graph.nodes.length ? "" : `<div class="empty"><h3>${raw.nodes.length ? $t("No notes match these filters.") : $t("Nothing to draw yet.")}</h3>${raw.nodes.length ? "" : `${$t("Link notes with")} <code>[[</code> ${$t("in the editor, and they appear here connected.")}`}</div>`}</section>`;
  const panel = $("#graph-panel");
  // Positions are saved shortly after the last drag.
  let unsaved = {};
  let saveTimer = 0;
  const savePositions = (changes) => {
    Object.assign(unsaved, changes);
    clearTimeout(saveTimer);
    saveTimer = setTimeout(() => {
      const body = { positions: unsaved };
      unsaved = {};
      api(`workspaces/${wid}/graph/positions`, "PUT", body)
        .then((r) => {
          positions = r.positions;
          $("#graph-reset")?.toggleAttribute(
            "hidden",
            !Object.keys(positions).length,
          );
        })
        .catch(error);
    }, 400);
  };
  const describe = (node) => {
    if (!node) {
      panel.innerHTML = `<p class="hint">${$t("Point at a note to see its links, or find one above.")}</p>`;
      return;
    }
    const linked = graphView.neighbors(node.id);
    panel.innerHTML = `<h2>${esc(node.label)}</h2>${node.kind === "note" ? `<p class="hint">${esc(folderName(node.folder))}</p>` : ""}<div class="graph-actions"><button class="btn primary" data-graph-open>${node.kind === "tag" ? $t("Show notes") : $t("Open")}</button>${node.kind === "note" && node.id !== focus ? `<button class="btn" data-graph-around>${$t("Around this note")}</button>` : ""}${graphView.isFixed(node.id) ? `<button class="btn" data-graph-release>${$t("Let it move again")}</button>` : ""}</div>${linked.length ? `<h3>${$t("Connected")}</h3><ul>${linked.map((n) => `<li><button class="text-link" data-graph-select="${esc(n.id)}">${esc(n.label)}</button></li>`).join("")}</ul>` : `<p class="hint">${$t("No links.")}</p>`}`;
    $("[data-graph-open]", panel).onclick = () => openGraphNode(node);
    if ($("[data-graph-around]", panel))
      $("[data-graph-around]", panel).onclick = () =>
        navigate("graph", node.id).catch(error);
    if ($("[data-graph-release]", panel))
      $("[data-graph-release]", panel).onclick = () => {
        graphView.release(node.id);
        savePositions({ [node.id]: null });
        describe(node);
      };
    $$("[data-graph-select]", panel).forEach(
      (b) => (b.onclick = () => graphView.center(b.dataset.graphSelect)),
    );
  };
  const rerender = () => renderGraph(focus).catch(error);
  $$("[data-graph-choice]").forEach((box) =>
    box.addEventListener("change", () => {
      graphChoices[box.dataset.graphChoice] = box.checked;
      rerender();
    }),
  );
  for (const [id, key] of [
    ["#graph-folder", "folder"],
    ["#graph-since", "since"],
    ["#graph-color", "color"],
  ])
    $(id).addEventListener("change", (e) => {
      graphChoices[key] = e.target.value;
      rerender();
    });
  $("#graph-depth")?.addEventListener("change", (e) => {
    graphChoices.depth = Number(e.target.value);
    rerender();
  });
  action($("#graph-whole"), "click", () => navigate("graph"));
  action($("#graph-reset"), "click", async () => {
    clearTimeout(saveTimer);
    unsaved = {};
    await api(`workspaces/${wid}/graph/positions`, "PUT", { reset: true });
    rerender();
  });
  if (!graph.nodes.length) return;
  graphView = mountGraph($("#graph-canvas"), graph, {
    color: byTag
      ? (n) => tagged.color(n)
      : (n) => folderColor(folderIds, n.folder),
    plain: byTag ? "#a3abb9" : null,
    positions,
    onOpen: openGraphNode,
    onSelect: describe,
    onArrange: (node, xy) => {
      savePositions({ [node.id]: xy });
      describe(node);
    },
    focus: around ? focus : null,
  });
  describe(around || null);
  action($("#graph-fit"), "click", () => graphView.fit());
  const search = $("#graph-search"),
    results = $("#graph-results");
  search.addEventListener("input", () => {
    const q = search.value.trim().toLowerCase();
    const found = q
      ? graph.nodes.filter((n) => n.label.toLowerCase().includes(q)).slice(0, 8)
      : [];
    results.innerHTML = found
      .map(
        (n) =>
          `<button role="option" data-graph-select="${esc(n.id)}">${esc(n.label)}</button>`,
      )
      .join("");
    $$("button", results).forEach(
      (b) =>
        (b.onclick = () => {
          graphView.center(b.dataset.graphSelect);
          results.innerHTML = "";
        }),
    );
    if (found[0]) graphView.center(found[0].id);
  });
  search.addEventListener("keydown", (e) => {
    const first = $("button", results);
    if (e.key === "Enter" && first) {
      e.preventDefault();
      openGraphNode(graph.nodes.find((n) => n.id === first.dataset.graphSelect));
    }
    if (e.key === "ArrowDown" && first) {
      e.preventDefault();
      first.focus();
    }
  });
}
function openGraphNode(node) {
  if (!node) return;
  if (node.kind === "tag") return openTag(node.label.slice(1));
  openNote(node.id).catch(error);
}
let statsTimer;
function updateStats() {
  clearTimeout(statsTimer);
  statsTimer = setTimeout(() => {
    const el = $("#word-count");
    if (!el || !note) return;
    const { words, characters, minutes } = countWords(note.blocks);
    el.textContent = words
      ? $tn(words, "{count} word · {minutes} min read", "{count} words · {minutes} min read", { count: words.toLocaleString(), minutes })
      : $t("No words yet");
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
    bar.innerHTML = `<input id="find-query" aria-label="${$t("Find in note")}" placeholder="${$t("Find in note")}"><span id="find-count" class="find-count" aria-live="polite"></span><button class="btn" id="find-prev" aria-label="${$t("Previous match")}" title="${$t("Previous (Shift+Enter)")}">↑</button><button class="btn" id="find-next" aria-label="${$t("Next match")}" title="${$t("Next (Enter)")}">↓</button><label class="checkbox-label" title="${$t("Match case")}"><input type="checkbox" id="find-case">${$t("Aa")}</label>${editable() ? `<input id="find-replace" aria-label="${$t("Replace with")}" placeholder="${$t("Replace with")}"><button class="btn" id="replace-one">${$t("Replace")}</button><button class="btn" id="replace-all">${$t("Replace all")}</button>` : ""}<button class="btn" id="find-close" aria-label="${$t("Close find")}" title="${$t("Close (Escape)")}">×</button>`;
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
      ? $t("No matches")
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
  announce($tn(replaced, "Replaced {count} match", "Replaced {count} matches"));
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
/**
 * A menu of a toolbar's other tools under `button`: it opens on click (or
 * Enter, Space, and ↓ unless `arrowOpens` is false; without `clickOpens`, the
 * caller opens it), moves with ↑/↓/Home/End, and closes with Escape, Tab, a
 * click elsewhere, or once an item is chosen.
 */
function bindToolbarMenu(
  button,
  menu,
  { arrowOpens = true, clickOpens = true } = {},
) {
  if (!button || !menu) return;
  const items = () =>
    $$("[role^=menuitem]", menu).filter((b) => !b.disabled);
  const escape = (e) => {
    if (e.key !== "Escape" || menu.hidden) return;
    e.preventDefault();
    e.stopPropagation();
    close(true);
  };
  const open = (focusFirst = true) => {
    menu.hidden = false;
    button.setAttribute("aria-expanded", "true");
    // Keep the menu inside the window.
    menu.classList.toggle(
      "open-left",
      button.getBoundingClientRect().right < menu.offsetWidth + 8,
    );
    if (focusFirst) items()[0]?.focus();
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("keydown", escape, true);
  };
  const close = (refocus = false) => {
    if (menu.hidden) return;
    menu.hidden = true;
    button.setAttribute("aria-expanded", "false");
    document.removeEventListener("pointerdown", outside, true);
    document.removeEventListener("keydown", escape, true);
    if (refocus) button.focus();
  };
  const outside = (e) => {
    if (!menu.contains(e.target) && e.target !== button && !button.contains(e.target))
      close();
  };
  if (clickOpens)
    button.addEventListener("click", () =>
      menu.hidden ? open(false) : close(),
    );
  button.addEventListener("keydown", (e) => {
    if (e.key === "ArrowDown" && arrowOpens) {
      e.preventDefault();
      open();
    }
  });
  menu.addEventListener("keydown", (e) => {
    const list = items();
    const i = list.indexOf(document.activeElement);
    const move = {
      ArrowDown: i + 1,
      ArrowUp: i - 1,
      Home: 0,
      End: list.length - 1,
    }[e.key];
    if (move !== undefined) {
      e.preventDefault();
      list[(move + list.length) % list.length]?.focus();
    } else if (e.key === "Tab") close();
  });
  menu.addEventListener("click", (e) => {
    const chosen = e.target.closest("[role^=menuitem]");
    if (!chosen) return;
    // A disabled item says why instead of doing anything.
    if (chosen.getAttribute("aria-disabled") === "true") {
      e.stopImmediatePropagation();
      if (chosen.title) toast(chosen.title);
      return;
    }
    close(true);
  }, true);
  return { open, close };
}
/** Shows the open note as slides (see presentation.js). */
let presenting = null;
async function presentNote() {
  if (!note || presenting || mode !== "document") return;
  await flush();
  const blocks = new Map(
    $$("#blocks .block").map((el) => [el.dataset.block, el]),
  );
  presenting = present({
    title: note.title,
    icon: note.icon,
    markdowns: note.blocks.map((b) => b.markdown),
    blocks: note.blocks.map((b) => blocks.get(b.id)),
    esc,
    onClose: () => (presenting = null),
  });
}
function toggleFocus(force) {
  const on = document.body.classList.toggle("focus-mode", force);
  $("#focus-toggle")?.setAttribute("aria-checked", String(on));
  announce(on ? $t("Focus mode on. Press Escape to leave.") : $t("Focus mode off"));
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
/** A block's Markdown shown styled, while the server renders it. */
function pendingHTML(markdown) {
  if (!markdown.trim()) return "";
  const level = headingLevel(markdown);
  return `<div class="render-pending${level ? ` lp-h${level}` : ""}">${previewHTML(markdown, esc)}</div>`;
}
/** Shows block `id`'s rendering of `markdown` when the server returns it. */
function showRendered(id, markdown) {
  const forNote = note;
  renderMarkdown(markdown)
    .then((html) => {
      const i = forNote.blocks.findIndex((b) => b.id === id);
      // Unless the block changed again (or is being edited) meanwhile.
      if (i < 0 || forNote.blocks[i].markdown !== markdown) return;
      forNote.rendered[i] = html;
      const body = note === forNote && $(`[data-block="${CSS.escape(id)}"] > .block-body`);
      if (!body || $("textarea", body)) return;
      holdHeight(body.parentElement);
      body.innerHTML = html || emptyBlockHTML();
      drawFolding();
      fillContents();
      linkCards();
    })
    .catch(() => {
      // Saved all the same; the styled Markdown stays until it is next opened.
      announce($t("This block will show formatted once the server answers."));
    });
}
function emptyBlockHTML() {
  return note?.blocks.length === 1
    ? `<span class="block-placeholder">${$t("Write something…")}</span>`
    : '<span class="block-empty" aria-hidden="true"></span>';
}
function blockHTML(b, i) {
  const level = indentLevel(b.markdown);
  return `<div class="block${level ? ` indent-${level}` : ""}" role="listitem" data-block="${esc(b.id)}"${level ? ` aria-level="${level + 1}"` : ""}>${editable() ? `<div class="block-controls"><button class="block-handle" title="${$t("Drag to move · click for options · ↑/↓ to move with the keyboard")}" aria-label="${$t("Block options (drag, or press the up and down arrows, to move)")}" aria-haspopup="menu" aria-expanded="false">⠿</button><button data-remove title="${$t("Delete block")}" aria-label="${$t("Delete block")}">×</button></div>` : shareMode ? "" : `<div class="block-controls"><button class="block-handle readonly" title="${$t("Block options")}" aria-label="${$t("Block options")}" aria-haspopup="menu" aria-expanded="false">⠿</button></div>`}${/^\s*- \[[ xX]\]/.test(b.markdown) ? `<button class="task-toggle" aria-label="${$t("Toggle task")}" ${!editable() ? "disabled" : ""}>${/\[[xX]\]/.test(b.markdown) ? "☑" : "☐"}</button>` : ""}<div class="block-body" tabindex="0" ${editable() ? 'aria-describedby="block-edit-hint"' : ""}>${note.rendered?.[i] || emptyBlockHTML()}</div>${shareMode && !guestComments() ? "" : `<button class="block-comment" type="button" data-comment-block="${esc(b.id)}" title="${$t("Comment on this block")}" aria-label="${$t("Comment on this block")}">💬</button>`}</div>`;
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
  announce($t("Block moved to position {value}", { value: j + 1 }));
  await queueSave();
}
/** Drags a block by its handle; a line shows where it will land. */
// When a block was last dropped: the click that ends a drag opens no menu.
let droppedAt = 0;
function dragBlock(e, el, id) {
  if (e.button !== 0 || !editable()) return;
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
    droppedAt = performance.now();
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
document.addEventListener("copy", (e) => {
  if (!note) return;
  if (selectedBlocks.length) copySelectedBlocks(e);
  else copyBlocks(e);
});
document.addEventListener("cut", (e) => {
  if (!note || !selectedBlocks.length || !editable()) return;
  copySelectedBlocks(e);
  deleteSelectedBlocks().catch(error);
});

/**
 * Whole blocks selected together: Shift+click, Shift+↑/↓ at the edge of a
 * block, Ctrl+A twice in a block, or a text selection dragged across blocks.
 * They are copied, cut, deleted, moved, indented or turned into another kind
 * together, each as one undo step.
 */
let selectedBlocks = [];
let selectionAnchor = null;
let lastEditedBlock = null;
const blockOrder = () => note?.blocks.map((b) => b.id) || [];
function selectBlockRange(from, to) {
  const order = blockOrder(),
    a = order.indexOf(from),
    b = order.indexOf(to);
  if (a < 0 || b < 0) return;
  selectionAnchor = from;
  selectedBlocks = order.slice(Math.min(a, b), Math.max(a, b) + 1);
  getSelection().removeAllRanges();
  drawBlockSelection();
  announce(
    $tn(selectedBlocks.length, "{count} block selected", "{count} blocks selected"),
  );
}
function clearBlockSelection() {
  if (!selectedBlocks.length) return;
  selectedBlocks = [];
  selectionAnchor = null;
  drawBlockSelection();
}
/** Extends the selection from its anchor by one block up (-1) or down (1). */
function extendBlockSelection(step) {
  const order = blockOrder();
  const ends = selectedBlocks.map((id) => order.indexOf(id));
  const anchor = order.indexOf(selectionAnchor);
  const far =
    anchor === Math.min(...ends) ? Math.max(...ends) : Math.min(...ends);
  const next = order[Math.max(0, Math.min(order.length - 1, far + step))];
  selectBlockRange(selectionAnchor, next);
  $(`[data-block="${CSS.escape(next)}"]`)?.scrollIntoView({ block: "nearest" });
}
function drawBlockSelection() {
  $$("#blocks .block").forEach((el) =>
    el.classList.toggle(
      "block-selected",
      selectedBlocks.includes(el.dataset.block),
    ),
  );
  let bar = $("#block-selection-bar");
  if (!selectedBlocks.length || !note) return bar?.remove();
  if (!bar) {
    bar = document.createElement("div");
    bar.id = "block-selection-bar";
    bar.className = "block-selection-bar";
    bar.setAttribute("role", "toolbar");
    document.body.append(bar);
  }
  const can = editable();
  bar.setAttribute("aria-label", $tn(selectedBlocks.length, "{count} block selected", "{count} blocks selected"));
  bar.innerHTML = `<strong>${$tn(selectedBlocks.length, "{count} block", "{count} blocks")}</strong>${
    can
      ? `<select data-turn aria-label="${$t("Turn into")}"><option value="">${$t("Turn into…")}</option>${BLOCK_KINDS.map(([kind, label]) => `<option value="${kind}">${label}</option>`).join("")}</select><button data-act="outdent" title="${$t("Move out (Shift+Tab)")}">⇤</button><button data-act="indent" title="${$t("Indent (Tab)")}">⇥</button><button data-act="up" title="${$t("Move up (Alt+Shift+↑)")}">↑</button><button data-act="down" title="${$t("Move down (Alt+Shift+↓)")}">↓</button>`
      : ""
  }<button data-act="copy" title="${$t("Copy (Ctrl+C)")}">${$t("Copy")}</button>${can ? `<button data-act="cut" title="${$t("Cut (Ctrl+X)")}">${$t("Cut")}</button><button data-act="delete" class="danger" title="${$t("Delete (Backspace)")}">${$t("Delete")}</button>` : ""}<button data-act="clear" title="${$t("Clear selection (Esc)")}" aria-label="${$t("Clear selection")}">×</button>`;
  $$("button", bar).forEach((b) => {
    b.setAttribute("aria-label", b.getAttribute("aria-label") || b.title);
    b.addEventListener("mousedown", (e) => e.preventDefault());
    b.onclick = () => blockSelectionAction(b.dataset.act).catch(error);
  });
  $("[data-turn]", bar)?.addEventListener("change", (e) => {
    const kind = e.target.value;
    if (kind)
      changeSelectedBlocks((md) => turnInto(md, kind), "Blocks changed").catch(
        error,
      );
  });
}
async function blockSelectionAction(act) {
  if (act === "clear") return clearBlockSelection();
  if (act === "copy" || act === "cut") {
    const text = selectedMarkdown();
    await navigator.clipboard.writeText(text);
    if (act === "cut") await deleteSelectedBlocks();
    else announce($t("Copied"));
    return;
  }
  if (act === "delete") return deleteSelectedBlocks();
  if (act === "up" || act === "down")
    return moveSelectedBlocks(act === "up" ? -1 : 1);
  if (act === "indent" || act === "outdent") {
    const step = act === "indent" ? 1 : -1;
    return changeSelectedBlocks(
      (md) => {
        const level = indentLevel(md);
        return reindent(md, level, Math.max(0, Math.min(6, level + step)));
      },
      step > 0 ? "Blocks indented" : "Blocks moved out",
    );
  }
}
const selectedMarkdown = () =>
  note.blocks
    .filter((b) => selectedBlocks.includes(b.id))
    .map((b) => b.markdown)
    .join("\n\n");
function copySelectedBlocks(e) {
  e.clipboardData.setData("text/plain", selectedMarkdown());
  e.clipboardData.setData(
    "text/html",
    note.rendered
      .filter((_, i) => selectedBlocks.includes(note.blocks[i].id))
      .join(""),
  );
  e.preventDefault();
}
/** False, with a notice, while someone else edits one of the selected blocks. */
const selectionFree = () =>
  editable() && !selectedBlocks.some((id) => blockLocked(id));
async function changeSelectedBlocks(change, message) {
  if (!selectionFree()) return;
  live?.boundary();
  await Promise.all(
    note.blocks.map(async (b, i) => {
      if (!selectedBlocks.includes(b.id)) return;
      b.markdown = change(b.markdown);
      note.rendered[i] = await renderMarkdown(b.markdown);
    }),
  );
  markDirty();
  renderBlocks();
  drawBlockSelection();
  announce(message);
  await queueSave();
}
async function deleteSelectedBlocks() {
  if (!selectionFree()) return;
  live?.boundary();
  const first = note.blocks.findIndex((b) => selectedBlocks.includes(b.id));
  const count = selectedBlocks.length;
  note.rendered = note.rendered.filter(
    (_, i) => !selectedBlocks.includes(note.blocks[i].id),
  );
  note.blocks = note.blocks.filter((b) => !selectedBlocks.includes(b.id));
  // A note always keeps one block to write in.
  if (!note.blocks.length) {
    note.blocks.push(newBlock("", 0));
    note.rendered.push("");
  }
  selectedBlocks = [];
  markDirty();
  renderBlocks();
  drawBlockSelection();
  $$(".block-body")[Math.min(first, note.blocks.length - 1)]?.focus();
  announce($t("{count} blocks deleted. Press Ctrl+Z to undo.", { count }));
  await queueSave();
}
async function moveSelectedBlocks(step) {
  if (!selectionFree()) return;
  const picked = note.blocks
    .map((b, i) => [b, note.rendered[i]])
    .filter(([b]) => selectedBlocks.includes(b.id));
  const rest = note.blocks
    .map((b, i) => [b, note.rendered[i]])
    .filter(([b]) => !selectedBlocks.includes(b.id));
  const first = note.blocks.findIndex((b) => selectedBlocks.includes(b.id));
  const at = Math.max(0, Math.min(rest.length, first + step));
  if (at === first) return;
  live?.boundary();
  rest.splice(at, 0, ...picked);
  note.blocks = rest.map((r) => r[0]);
  note.rendered = rest.map((r) => r[1]);
  markDirty();
  renderBlocks();
  drawBlockSelection();
  announce(step < 0 ? $t("Blocks moved up") : $t("Blocks moved down"));
  await queueSave();
}
// Keys while blocks are selected (and no field has the focus).
document.addEventListener("keydown", (e) => {
  if (!selectedBlocks.length || !note) return;
  if (e.target.closest?.("input, textarea, select, dialog")) return;
  const mod = e.ctrlKey || e.metaKey;
  const act = (name) => {
    e.preventDefault();
    blockSelectionAction(name).catch(error);
  };
  if (e.key === "Escape") return (e.preventDefault(), clearBlockSelection());
  if (e.key === "Backspace" || e.key === "Delete") return act("delete");
  if (e.key === "Tab") return act(e.shiftKey ? "outdent" : "indent");
  if (e.altKey && e.shiftKey && e.key === "ArrowUp") return act("up");
  if (e.altKey && e.shiftKey && e.key === "ArrowDown") return act("down");
  if (e.shiftKey && (e.key === "ArrowUp" || e.key === "ArrowDown")) {
    e.preventDefault();
    return extendBlockSelection(e.key === "ArrowUp" ? -1 : 1);
  }
  if (mod && e.key.toLowerCase() === "a") {
    e.preventDefault();
    const order = blockOrder();
    return selectBlockRange(order[0], order.at(-1));
  }
  if (e.key === "Enter" && editable()) {
    e.preventDefault();
    const id = selectedBlocks[0];
    clearBlockSelection();
    return editBlock(id);
  }
  if (!mod && !e.altKey && (e.key.length === 1 || e.key.startsWith("Arrow")))
    clearBlockSelection();
});
// A click elsewhere ends the selection; a drag across blocks selects them.
document.addEventListener("mousedown", (e) => {
  if (e.target.closest?.("#block-selection-bar")) return;
  if (!e.shiftKey) clearBlockSelection();
});
document.addEventListener("mouseup", () => {
  const sel = getSelection();
  const root = $("#blocks");
  if (!note || !root || sel.isCollapsed || !sel.rangeCount) return;
  const range = sel.getRangeAt(0);
  if (!root.contains(range.commonAncestorContainer)) return;
  const ids = $$(".block", root)
    .filter((b) => range.intersectsNode(b))
    .map((b) => b.dataset.block);
  if (ids.length >= 2) selectBlockRange(ids[0], ids.at(-1));
});
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
// Folded blocks of the open note (see folding.js), and the blocks they hide.
let folded = new Set(),
  foldedFor = null,
  foldHidden = new Set();
/**
 * Shows each block's fold arrow and hides what folded blocks hold, in place:
 * a block being edited stays open.
 */
function drawFolding() {
  if (!note) return;
  if (foldedFor !== note.id) {
    // Folding kept in the note, or else on this device.
    folded = Array.isArray(note.folded)
      ? new Set(note.folded)
      : loadFolded(note.id);
    foldedFor = note.id;
  }
  foldHidden = hiddenBlocks(note.blocks, folded, indentLevel);
  // One lookup for all blocks: one per block made long notes slow.
  const elements = new Map(
    $$("#blocks [data-block]").map((el) => [el.dataset.block, el]),
  );
  note.blocks.forEach((b, i) => {
    const el = elements.get(b.id);
    if (!el) return;
    const parent = hasChildren(note.blocks, i, indentLevel);
    const isFolded = parent && folded.has(b.id);
    el.classList.toggle("fold-hidden", foldHidden.has(b.id));
    el.classList.toggle("has-children", parent);
    el.classList.toggle("folded", isFolded);
    let arrow = $(":scope > .block-fold", el);
    if (parent && !arrow) {
      arrow = document.createElement("button");
      arrow.type = "button";
      arrow.className = "block-fold";
      arrow.dataset.fold = b.id;
      arrow.textContent = "▸";
      el.prepend(arrow);
    }
    if (!parent) arrow?.remove();
    if (arrow) {
      const label = isFolded ? "Show nested blocks" : "Hide nested blocks";
      arrow.setAttribute("aria-expanded", String(!isFolded));
      arrow.setAttribute("aria-label", label);
      arrow.title = $t("{label} (Ctrl+{value})", { label, value: isFolded ? "↓" : "↑" });
    }
    let count = $(":scope > .fold-count", el);
    if (isFolded) {
      if (!count) {
        count = document.createElement("button");
        count.type = "button";
        count.className = "fold-count";
        count.dataset.fold = b.id;
        el.append(count);
      }
      const n = subtreeEnd(note.blocks, i, indentLevel) - i - 1;
      count.textContent = `${n} hidden`;
    } else count?.remove();
  });
}
/** Folds or unfolds block `id`; `fold` forces one way. */
function toggleFold(id, fold = !folded.has(id)) {
  const i = note.blocks.findIndex((b) => b.id === id);
  if (i < 0 || !hasChildren(note.blocks, i, indentLevel)) return false;
  if (fold === folded.has(id)) return false;
  if (fold) folded.add(id);
  else folded.delete(id);
  keepFolded();
  drawFolding();
  announce(fold ? $t("Nested blocks hidden") : $t("Nested blocks shown"));
  return true;
}
/** Unfolds whatever hides block `id`. */
function revealBlock(id) {
  if (!foldHidden.has(id)) return;
  for (const above of foldedAbove(note.blocks, id, folded, indentLevel))
    folded.delete(above);
  keepFolded();
  drawFolding();
}
/**
 * Keeps what is folded: in the note for everyone when its folding is shared
 * (and the reader may edit it), else on this device.
 */
let foldSave = 0;
function keepFolded() {
  if (!Array.isArray(note?.folded) || !editable()) {
    if (!Array.isArray(note?.folded)) saveFolded(note.id, folded, note.blocks);
    return;
  }
  const ids = new Set(note.blocks.map((b) => b.id));
  note.folded = [...folded].filter((id) => ids.has(id));
  const id = note.id,
    list = note.folded;
  clearTimeout(foldSave);
  foldSave = setTimeout(
    () =>
      api(`notes/${id}/folding`, "PUT", { shared: true, folded: list }).catch(
        error,
      ),
    400,
  );
}
/** Shares the note's folding with everyone, or keeps it on each device again. */
async function shareFolding(on) {
  const ids = new Set(note.blocks.map((b) => b.id));
  const result = await api(`notes/${note.id}/folding`, "PUT", {
    shared: on,
    folded: [...folded].filter((id) => ids.has(id)),
  });
  note.folded = result.folded ?? undefined;
  if (!on) saveFolded(note.id, folded, note.blocks);
  foldedFor = null;
  drawFolding();
  toast(
    on
      ? $t("Folding is kept in the note, for everyone")
      : $t("Folding is kept on each device again"),
  );
}
// Link previews by address: a promise of the page's details, or null.
const previews = new Map();
/**
 * A block that is just a web address shows a card: the page's title,
 * description, site and picture (`/api/v1/link-preview`). Videos, pictures,
 * audio and the note's files keep their own display; without details, the
 * plain link stays.
 */
/** Changes one block's Markdown from the page, and saves. */
async function setBlockMarkdown(id, markdown) {
  const i = note.blocks.findIndex((b) => b.id === id);
  if (i < 0 || !editable()) return;
  live?.boundary();
  note.blocks[i].markdown = markdown;
  note.rendered[i] = await renderMarkdown(markdown);
  markDirty();
  renderBlocks();
  await queueSave();
}
function linkCards() {
  if (!note) return;
  for (const b of note.blocks) {
    // `<address>` is the same link, chosen to show plainly.
    const plain = b.markdown.trim().match(/^<(https?:\/\/\S+)>$/);
    if (plain && editable()) {
      const body = $(`#blocks [data-block="${CSS.escape(b.id)}"] .block-body`);
      if (body && !body.querySelector(".link-card-switch"))
        body.insertAdjacentHTML(
          "beforeend",
          `<button type="button" class="link-card-switch" data-card-switch="card" title="${$t("Show this link as a card")}">${$t("Card")}</button>`,
        );
      body
        ?.querySelector("[data-card-switch]")
        ?.addEventListener("click", (e) => {
          e.stopPropagation();
          setBlockMarkdown(b.id, plain[1]).catch(error);
        });
      continue;
    }
    const url = b.markdown.trim();
    if (!/^https?:\/\/\S+$/.test(url) || url.length > 2048) continue;
    if (
      IMAGE_LINK.test(url) ||
      AUDIO_LINK.test(url) ||
      url.includes("/api/v1/attachments/")
    )
      continue;
    const body = $(`#blocks [data-block="${CSS.escape(b.id)}"] .block-body`);
    if (
      !body ||
      body.querySelector(".link-card, .youtube-card, img, audio, iframe")
    )
      continue;
    // Guests ask through their link, only for addresses the note shows.
    if (!previews.has(url))
      previews.set(
        url,
        api(
          (shareMode ? "shared/link-preview?url=" : "link-preview?url=") +
            encodeURIComponent(url),
        ).catch(() => null),
      );
    previews.get(url).then((p) => {
      if (!p?.title || !body.isConnected || body.querySelector(".link-card"))
        return;
      if (b.markdown.trim() !== url) return;
      body.innerHTML = `<span class="link-card-wrap"><a class="link-card" href="${esc(url)}" target="_blank" rel="noopener noreferrer"><span class="link-card-text"><strong>${esc(p.title)}</strong>${p.description ? `<span>${esc(p.description)}</span>` : ""}<small>${esc(p.site || "")}</small></span>${p.image ? `<img src="${esc(p.image)}" alt="" loading="lazy" referrerpolicy="no-referrer">` : ""}</a>${editable() ? `<button type="button" class="link-card-switch" data-card-switch="link" title="${$t("Show this link as plain text")}">${$t("Link")}</button>` : ""}</span>`;
      body
        .querySelector("[data-card-switch]")
        ?.addEventListener("click", (e) => {
          e.stopPropagation();
          setBlockMarkdown(b.id, `<${url}>`).catch(error);
        });
    });
  }
}
/** `[[toc]]` blocks list the note's headings; each scrolls to its heading. */
function fillContents() {
  // A contents slot inside an embedded note is not this note's.
  const slots = $$("#blocks .note-toc").filter((s) => !s.closest(".note-embed"));
  if (!slots.length || !note) return;
  const headings = note.blocks
    .map((b) => {
      const m = b.markdown.trimStart().match(/^(#{1,3}) +(.+)/);
      if (!m) return null;
      const text = m[2]
        .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
        .replace(/[*_`~=]/g, "")
        .replace(/(^|\s)#[\w/-]+/g, "")
        .trim();
      return { id: b.id, level: m[1].length, text };
    })
    .filter((h) => h?.text);
  const html = headings.length
    ? `<p class="note-toc-title">${$t("Contents")}</p><ol>${headings.map((h) => `<li class="toc-${h.level}"><a href="#" data-toc-block="${esc(h.id)}">${esc(h.text)}</a></li>`).join("")}</ol>`
    : `<p class="note-toc-title">${$t("Contents")}</p><p class="hint">${$t("Headings (#, ##, ###) in this note will be listed here.")}</p>`;
  for (const slot of slots) slot.innerHTML = html;
}
document.addEventListener("click", (e) => {
  const entry = e.target.closest?.("#blocks [data-toc-block]");
  if (!entry || !note) return;
  e.preventDefault();
  e.stopPropagation();
  const id = entry.dataset.tocBlock;
  revealBlock(id);
  const block = $(`[data-block="${CSS.escape(id)}"]`);
  block?.scrollIntoView({ block: "start", behavior: "smooth" });
  block?.classList.add("comment-flash");
  setTimeout(() => block?.classList.remove("comment-flash"), 1500);
});
document.addEventListener("click", (e) => {
  const fold = e.target.closest?.("#blocks [data-fold]");
  if (fold && note) {
    e.stopPropagation();
    toggleFold(fold.dataset.fold);
  }
});
/** A block's own controls: editing, its task box, handle and delete button. */
function bindBlock(el) {
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
        "a, .tag, .journal-tasks button, .db-embed, .note-embed, .youtube-card, audio, .recording-actions, .file-card",
      )
    )
      return;
    // Shift+click selects whole blocks, from the one used last.
    if (e.shiftKey) {
      e.preventDefault();
      return selectBlockRange(selectionAnchor || lastEditedBlock || id, id);
    }
    // Selecting text to copy it must not open the block for editing.
    if (!getSelection().isCollapsed) return;
    if (editable()) editBlock(id, clickedOffset(e.currentTarget, id, e));
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
    if (!editable()) return;
    e.preventDefault();
    const i = note.blocks.findIndex((b) => b.id === id);
    await moveBlock(i, i + (e.key === "ArrowUp" ? -1 : 1));
    $(`[data-block="${CSS.escape(id)}"] .block-handle`)?.focus();
  });
  handle?.addEventListener("pointerdown", (e) => dragBlock(e, el, id));
  // A click on the handle (not the end of a drag) opens the block's menu.
  handle?.addEventListener("click", (e) => {
    if (performance.now() - droppedAt < 400) return;
    const menu = handleMenus.get(handle) || handleMenu(handle, id);
    handleMenus.set(handle, menu);
    if (menu.element.hidden) menu.open(e.detail === 0);
    else menu.close();
  });
  action($("[data-remove]", el), "click", async () => {
    if (blockLocked(id)) return;
    const i = note.blocks.findIndex((b) => b.id === id);
    live?.boundary();
    note.blocks.splice(i, 1);
    note.rendered.splice(i, 1);
    markDirty();
    removeBlockElement(id);
    const neighbour = $$(".block-body")[Math.min(i, note.blocks.length - 1)];
    (neighbour || $("#add-block"))?.focus();
    announce($t("Block deleted. Press Ctrl+Z to undo."));
    await queueSave();
  });
}
/**
 * The menu of a block's handle: links to the block and embeds of it, to paste
 * into any note. Made when first opened.
 */
const handleMenus = new WeakMap();
function handleMenu(handle, id) {
  const menu = document.createElement("div");
  menu.className = "row-menu note-menu block-menu";
  menu.setAttribute("role", "menu");
  menu.setAttribute("aria-label", $t("Block"));
  menu.hidden = true;
  menu.innerHTML = `<button role="menuitem" tabindex="-1" data-block-copy="link">${icon("link")}${$t("Copy link to block")}</button><button role="menuitem" tabindex="-1" data-block-copy="embed">${icon("note")}${$t("Copy as embed")}</button>`;
  handle.after(menu);
  menu.addEventListener("click", async (e) => {
    const choice = e.target.closest("[data-block-copy]")?.dataset.blockCopy;
    const b = note?.blocks.find((x) => x.id === id);
    if (!choice || !b) return;
    const label = blockLabel(b.markdown) || note.title;
    const text =
      choice === "link"
        ? blockLink(label, note.id, id)
        : embedCode(label, note.id, id);
    try {
      await navigator.clipboard.writeText(text);
      toast(
        choice === "link"
          ? $t("Link to the block copied. Paste it into any note.")
          : $t("Embed copied. Paste it into a note to show this block there."),
      );
    } catch {
      toast($t("Copying was not allowed by the browser."));
    }
  });
  return {
    element: menu,
    ...bindToolbarMenu(handle, menu, { arrowOpens: false, clickOpens: false }),
  };
}
/**
 * After blocks were added or removed in place (rather than by redrawing them
 * all, which takes long in long notes): folding, and what depends on the whole
 * note.
 */
function blocksChanged() {
  setTimeout(() => $("#find-bar") && runFind(), 0);
  setTimeout(() => (!shareMode || guestComments()) && comments.drawBadges(), 0);
  drawPeers();
  drawFolding();
  fillContents();
  linkCards();
}
/** Shows `blocks`, already in `note.blocks` from index `at`, without a redraw. */
function insertBlockElements(at, blocks) {
  const root = $("#blocks");
  if (mode !== "document" || !root) return renderBlocks();
  const next = note.blocks[at + blocks.length];
  const before =
    (next && $(`:scope > [data-block="${CSS.escape(next.id)}"]`, root)) ||
    $(":scope > .peer-layer", root);
  const holder = document.createElement("template");
  holder.innerHTML = blocks.map((b, k) => blockHTML(b, at + k)).join("");
  const added = [...holder.content.children];
  root.insertBefore(holder.content, before || null);
  added.forEach(bindBlock);
  blocksChanged();
}
/** Removes a block's element without a redraw. */
function removeBlockElement(id) {
  if (mode !== "document") return renderBlocks();
  $(`#blocks > [data-block="${CSS.escape(id)}"]`)?.remove();
  blocksChanged();
}
function renderBlocks() {
  setTimeout(() => $("#find-bar") && runFind(), 0);
  setTimeout(() => (!shareMode || guestComments()) && comments.drawBadges(), 0);
  const root = $("#blocks");
  bindMediaDrop(root);
  root.innerHTML = note.blocks
    .map((b, i) =>
      mode === "document"
        ? blockHTML(b, i)
        : `<div class="canvas-card" data-card="${esc(b.id)}" style="left:${b.x}px;top:${b.y}px"><div class="canvas-handle">${$t("⠿ &nbsp; Block {value} · drag to arrange", { value: i + 1 })}</div>${blockHTML(b, i)}</div>`,
    )
    .join("");
  drawPeers();
  drawFolding();
  fillContents();
  linkCards();
  $$(".block", root).forEach(bindBlock);
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
// Comments on the open note; on a share link, only for guests who may edit.
var comments = createComments({
  api,
  esc,
  error,
  announce,
  canResolveAll: () =>
    !!note &&
    (guestComments() || data.roles?.[note.folder_id] === "editor"),
  note: () => note,
  watch: (id, onChange) => (shareMode ? () => {} : watchNote(id, onChange)),
  get path() {
    return shareMode ? "shared/comments" : undefined;
  },
  get guest() {
    return shareMode;
  },
});
/** A share-link guest who may edit, and so comment. */
function guestComments() {
  return shareMode && note?.role === "editor";
}
const writingAssist = createWritingAssist({
  api,
  esc,
  error,
  announce,
  editBlock,
  replaceRange,
  splitMarkdown,
  insertBelow: (id, blocks) =>
    appendBlocks(blocks, { after: id, source: "AI" }),
});
/**
 * AI on the selection in a block's field; with nothing selected, on the text
 * before the cursor (to continue it) or the whole block.
 */
function openWritingAssist(ta) {
  const el = ta.closest("[data-block]");
  if (!el || !editable()) return;
  if (ta.selectionStart === ta.selectionEnd) {
    if (!ta.value.trim()) return toast($t("Write or select some text first."));
    ta.setSelectionRange(0, ta.value.length);
  }
  $(".format-bar", el)?.remove();
  writingAssist.open(ta, el, el.dataset.block).catch(error);
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
  bar.setAttribute("aria-label", $t("Format selection"));
  bar.innerHTML = formatButtons();
  $$("button", bar).forEach((b) => {
    b.setAttribute("aria-label", b.title);
    // Keep the textarea focused and its selection intact.
    b.addEventListener("mousedown", (e) => e.preventDefault());
    b.addEventListener("click", () => formatAction(ta, b.dataset));
  });
  block.append(bar);
}
function formatButtons() {
  return `<button data-format="bold" title="${$t("Bold (Ctrl+B)")}"><b>B</b></button><button data-format="italic" title="${$t("Italic (Ctrl+I)")}"><i>I</i></button><button data-format="strike" title="${$t("Strikethrough (Ctrl+Shift+X)")}"><s>S</s></button><button data-format="code" title="${$t("Inline code (Ctrl+E)")}"><code>&lt;/&gt;</code></button><button data-format="link" title="${$t("Link (Ctrl+K)")}">🔗</button><button data-format="highlight" class="format-highlight" title="${$t("Highlight (Ctrl+Shift+H)")}">🖍</button>${shareMode ? "" : `<button data-ai class="format-ai" title="${$t("AI: improve, translate, continue (Ctrl+J)")}">${$t("✦ AI")}</button>`}${shareMode && !guestComments() ? "" : `<button data-comment-selection title="${$t("Comment on the selected text")}">💬</button>`}`;
}
/** What a format bar button (or its shortcut) does to the selection in `ta`. */
function formatAction(ta, { format, ai, commentSelection }) {
  if (ai !== undefined) return openWritingAssist(ta);
  if (commentSelection !== undefined)
    return comments.start(
      ta.closest("[data-block]")?.dataset.block,
      ta.value.slice(ta.selectionStart, ta.selectionEnd),
    );
  applyFormat(ta, format);
  updateFormatBar(ta);
}

// ---------------------------------------------------------------------------
// From the rendered block to its Markdown: a click puts the cursor where it
// was, and text selected in a rendered block can be formatted (with the bar or
// the same shortcuts as in the field) or typed over; the block opens for
// editing with the same text selected.

/** Rendered text of `body` up to `node`/`offset`, as a reader sees it. */
function renderedText(body, node, offset) {
  const range = document.createRange();
  range.setStart(body, 0);
  try {
    range.setEnd(node, offset);
  } catch {
    return null;
  }
  const copy = range.cloneContents();
  // Text that is not part of the block's own words.
  $$(
    "button, [aria-hidden='true'], .image-bar, .db-embed, .note-embed, .note-toc, .link-card, math, .mermaid-diagram, .file-card",
    copy,
  ).forEach((n) => n.remove());
  return copy.textContent;
}
/** The Markdown offset under a click in a rendered block, if it hit text. */
function clickedOffset(body, id, e) {
  const b = note.blocks.find((x) => x.id === id);
  // A picture, formula or diagram: the cursor goes to the start of its Markdown.
  const object = e.target.closest?.("img, math, .mermaid-diagram");
  if (b && object && body.contains(object)) {
    const [kind, selector] = object.matches("img")
      ? ["image", "img"]
      : object.matches("math")
        ? ["math", "math"]
        : ["diagram", ".mermaid-diagram"];
    const n = $$(selector, body).indexOf(object);
    return objectOffset(b.markdown, kind, n);
  }
  const hit = document.caretPositionFromPoint
    ? document.caretPositionFromPoint(e.clientX, e.clientY)
    : document.caretRangeFromPoint?.(e.clientX, e.clientY);
  const node = hit?.offsetNode ?? hit?.startContainer;
  const offset = hit?.offset ?? hit?.startOffset;
  if (!b || !node || !body.contains(node) || node.nodeType !== 3) return null;
  const before = renderedText(body, node, offset);
  return before == null ? null : sourceOffset(b.markdown, before);
}
/** A selection inside one rendered block, as a Markdown range. */
function renderedSelection() {
  const sel = getSelection();
  if (!note || !editable() || sel.isCollapsed || !sel.rangeCount) return null;
  const range = sel.getRangeAt(0);
  const element = (n) => (n.nodeType === 1 ? n : n.parentElement);
  const body = element(range.startContainer)?.closest(".block-body");
  if (!body || !body.contains(range.endContainer) || $("textarea", body))
    return null;
  const id = body.closest("[data-block]")?.dataset.block;
  const b = note.blocks.find((x) => x.id === id);
  const before = renderedText(body, range.startContainer, range.startOffset);
  const through = renderedText(body, range.endContainer, range.endOffset);
  if (!b || before == null || through == null || blockLocked(id)) return null;
  return { id, ...sourceRange(b.markdown, before, through.slice(before.length)) };
}
/** Opens the selected block for editing with the same text selected. */
function editSelection(selection) {
  getSelection().removeAllRanges();
  $(".view-format-bar")?.remove();
  editBlock(selection.id, selection.start);
  const ta = $(`[data-block="${CSS.escape(selection.id)}"] textarea`);
  ta?.setSelectionRange(selection.start, selection.end);
  return ta;
}
/** The format bar over text selected in a rendered block. */
function updateViewFormatBar() {
  const selection = renderedSelection();
  const old = $(".view-format-bar");
  if (!selection) return old?.remove();
  if (old?.dataset.block === selection.id) return;
  old?.remove();
  const block = $(`[data-block="${CSS.escape(selection.id)}"]`);
  const bar = document.createElement("div");
  bar.className = "format-bar view-format-bar";
  bar.dataset.block = selection.id;
  bar.setAttribute("role", "toolbar");
  bar.setAttribute("aria-label", $t("Format selection"));
  bar.innerHTML = formatButtons();
  $$("button", bar).forEach((b) => {
    b.setAttribute("aria-label", b.title);
    b.addEventListener("mousedown", (e) => e.preventDefault());
    b.addEventListener("click", () => {
      const current = renderedSelection();
      const ta = current && editSelection(current);
      if (ta) formatAction(ta, b.dataset);
    });
  });
  block.append(bar);
}
document.addEventListener("selectionchange", () => {
  if (note && !document.activeElement?.matches?.("textarea, input"))
    updateViewFormatBar();
});
document.addEventListener(
  "keydown",
  (e) => {
    if (e.target.closest?.("input, textarea, select, dialog")) return;
    const selection = renderedSelection();
    if (!selection) return;
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
    if (format || (shortcut === "j" && !e.shiftKey && !shareMode)) {
      e.preventDefault();
      e.stopPropagation();
      const ta = editSelection(selection);
      if (ta) formatAction(ta, format ? { format } : { ai: "" });
      return;
    }
    // Typing over the selection replaces it, as in the field.
    const typing =
      !e.ctrlKey && !e.metaKey && !e.altKey && e.key.length === 1;
    if (typing || e.key === "Backspace" || e.key === "Delete")
      editSelection(selection);
  },
  true,
);

// While a pointer is pressed, a block that stops being edited keeps its height,
// so what the pointer pressed is still under it when released: a heading's
// field and its rendered text differ in height, and a click right below it used
// to land elsewhere (or nowhere).
let pointerPressed = false;
const heldBlocks = new Set();
document.addEventListener("pointerdown", () => (pointerPressed = true), true);
for (const event of ["pointerup", "pointercancel"])
  document.addEventListener(
    event,
    () => {
      pointerPressed = false;
      // After the click that follows the release.
      setTimeout(() => {
        for (const el of heldBlocks) el.style.minHeight = el.style.maxHeight = "";
        heldBlocks.clear();
      }, 0);
    },
    true,
  );
function holdHeight(el) {
  if (!pointerPressed || !el?.isConnected) return;
  const height = `${el.getBoundingClientRect().height}px`;
  el.style.minHeight = el.style.maxHeight = height;
  heldBlocks.add(el);
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
    // Each changed parent once: redrawing a note adds every block at once.
    const roots = new Set();
    for (const change of changes)
      for (const node of change.addedNodes)
        if (node.nodeType === 1) roots.add(node.parentNode || node);
    for (const root of roots) if (root.isConnected) nameMentions(root);
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
  showPicker(ta, query, items, $t("Mention someone"));
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
/**
 * `[[` autocomplete: pick a note and insert a link that survives renames;
 * after `![[`, the note is embedded instead.
 */
async function updateLinkPicker(ta) {
  const mention = !shareMode && mentionQuery(ta.value, ta.selectionStart);
  if (mention) return updateMentionPicker(ta, mention);
  const query = linkQuery(ta.value, ta.selectionStart);
  if (!query || shareMode) return closeLinkPicker();
  query.embed = ta.value[query.start - 1] === "!";
  // `[[Note name#words`: that note's blocks with the words.
  const hash = query.query.indexOf("#");
  if (hash > 0) return updateBlockPicker(ta, query, hash);
  const results = query.query.trim()
    ? await api("search", "POST", { query: query.query.trim() })
    : [...workspaceNotes(), ...(data.shared_notes || [])]
        .filter((n) => !n.trashed)
        .sort((a, b) => b.updated_at - a.updated_at);
  if (linkQuery(ta.value, ta.selectionStart)?.start !== query.start) return;
  const items = results.filter((n) => n.id !== note.id).slice(0, 8);
  if (query.embed) return showPicker(ta, query, items, $t("Embed a note"));
  if (
    /^(u|un|unf|unc|tas|todo|jour|open)/i.test(query.query.trim()) ||
    !query.query.trim()
  )
    items.unshift({
      special: "[[uncompleted_journal_tasks]]",
      icon: "☑",
      title: "Unfinished journal tasks (live list)",
    });
  showPicker(ta, query, items, $t("Link to a note"));
}
// Notes whose blocks the picker lists, fetched once a while.
const pickerNotes = new Map();
async function updateBlockPicker(ta, query, hash) {
  const name = query.query.slice(0, hash).trim().toLowerCase();
  const words = query.query.slice(hash + 1).trim().toLowerCase();
  const known = [...workspaceNotes(), ...(data.shared_notes || [])].filter(
    (n) => !n.trashed,
  );
  const target =
    known.find((n) => n.title.toLowerCase() === name) ||
    known.find((n) => n.title.toLowerCase().startsWith(name)) ||
    (await api("search", "POST", { query: name }).catch(() => []))[0];
  if (linkQuery(ta.value, ta.selectionStart)?.start !== query.start) return;
  if (!target) return showPicker(ta, query, [], $t("Link to a block"));
  let full = pickerNotes.get(target.id);
  if (!full || Date.now() - full.at > 10000) {
    full = {
      at: Date.now(),
      note:
        target.id === note.id
          ? note
          : await api("notes/" + target.id).catch(() => null),
    };
    pickerNotes.set(target.id, full);
  }
  if (linkQuery(ta.value, ta.selectionStart)?.start !== query.start) return;
  const blocks = (full.note?.blocks || [])
    .map((b) => ({ b, label: blockLabel(b.markdown) }))
    .filter(({ label }) => label && label.toLowerCase().includes(words))
    .slice(0, 8)
    .map(({ b, label }) => ({
      icon: "¶",
      title: label,
      block: b.id,
      note: target.id,
      noteTitle: target.title,
    }));
  showPicker(
    ta,
    query,
    blocks,
    query.embed ? $t("Embed a block") : $t("Link to a block"),
  );
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
    `<p class="muted">${linkPicker.list.getAttribute("aria-label") === $t("Mention someone") ? $t("Nobody else can open this note yet. Share it, or its workspace, first.") : linkPicker.list.getAttribute("aria-label") === $t("Link to a block") || linkPicker.list.getAttribute("aria-label") === $t("Embed a block") ? $t("No matching blocks") : $t("No matching notes")}</p>`;
  // A hint for linking a block, under note results.
  if (items.length && !items[0].block && !items[0].email)
    list.insertAdjacentHTML(
      "beforeend",
      `<p class="picker-hint">${$t("Type # after a note’s name to pick one of its blocks")}</p>`,
    );
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
    : target.block
      ? query.embed
        ? embedCode(target.title, target.note, target.block)
        : blockLink(target.title, target.note, target.block)
      : query.embed
        ? embedCode(target.title, target.id)
        : target.special || noteLink(target.title, target.id);
  const from = query.embed ? query.start - 1 : query.start;
  replaceRange(ta, {
    from,
    to: ta.selectionStart,
    text,
    selStart: from + text.length,
    selEnd: from + text.length,
  });
  closeLinkPicker();
  announce(
    target.email
      ? `Mentioned ${target.name}`
      : target.block
        ? $t("Linked to a block of {title}", { title: target.noteTitle })
        : query.embed
        ? `Embedded ${target.title}`
        : $t("Linked to {title}", { title: target.title }),
  );
}
function closeLinkPicker() {
  linkPicker?.list.remove();
  linkPicker = null;
}
function editBlock(id, caret = null) {
  revealBlock(id);
  const el = $(`[data-block="${CSS.escape(id)}"]`);
  if (!el || $("textarea", el) || blockLocked(id)) return;
  const b = note.blocks.find((b) => b.id === id);
  const body = $(".block-body", el);
  const index = () => note.blocks.findIndex((b) => b.id === id);
  body.innerHTML = `<textarea aria-label="Markdown for block ${index() + 1} of ${note.blocks.length}" aria-describedby="editor-help" spellcheck="true"></textarea>`;
  const ta = $("textarea", body);
  attachPreview(ta, esc);
  clearBlockSelection();
  lastEditedBlock = id;
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
    // Never waits for the server: what follows (the next block, a split) goes
    // on at once, with the rendered HTML when it is known, else the Markdown
    // styled as in the field until the server's rendering arrives.
    const rendered = renderedCache.get(b.markdown) ?? pendingHTML(b.markdown);
    note.rendered[i] = rendered;
    holdHeight(el);
    body.innerHTML = rendered || emptyBlockHTML();
    if (!renderedCache.has(b.markdown)) showRendered(id, b.markdown);
    // Indenting while typing may have given blocks children or taken them away,
    // and a heading may have changed.
    drawFolding();
    fillContents();
    linkCards();
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
      announce(level ? $t("Moved out to level {level}", { level }) : $t("List ended"));
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
      // After a folded block, the new one goes after what it hides.
      const at = folded.has(id)
        ? subtreeEnd(note.blocks, i, indentLevel)
        : i + 1;
      note.blocks.splice(at, 0, fresh);
      note.rendered.splice(at, 0, renderedCache.get(after) ?? "");
      markDirty();
      insertBlockElements(at, [fresh]);
      editBlock(fresh.id, prefix.length);
      if (rest) announce($t("Block split"));
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
      announce($t("Already nested as deep as allowed here"));
      return;
    }
    if (target === level) {
      announce(
        delta > 0 ? $t("Already nested as deep as allowed here") : $t("Not indented"),
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
        ? $t("Indented to level {level}", { level: target })
        : target
          ? $t("Moved out to level {level}", { level: target })
          : $t("Not indented"),
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
    if (blockLocked(previous.id)) return;
    live?.boundary();
    closed = true;
    const join = previous.markdown.length;
    previous.markdown += ta.value;
    note.blocks.splice(i, 1);
    note.rendered.splice(i, 1);
    markDirty();
    removeBlockElement(id);
    editBlock(previous.id, join);
    announce($t("Merged with the previous block"));
  }
  function mergeNext() {
    const i = index(),
      next = note.blocks[i + 1];
    if (blockLocked(next.id)) return;
    live?.boundary();
    const join = ta.value.length;
    ta.value += next.markdown;
    b.markdown = ta.value;
    note.blocks.splice(i + 1, 1);
    note.rendered.splice(i + 1, 1);
    $(`[data-block="${CSS.escape(next.id)}"]`)?.remove();
    ta.setSelectionRange(join, join);
    ta.oninput();
    announce($t("Merged with the next block"));
  }
  function moveTo(offset, toEnd) {
    // Blocks hidden in a folded block are skipped.
    let at = index() + offset;
    while (note.blocks[at] && foldHidden.has(note.blocks[at].id)) at += offset;
    const target = note.blocks[at];
    if (!target || blockLocked(target.id)) return false;
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
    // Past the edge of the text, Shift+↑/↓ selects whole blocks; so does Ctrl+A
    // once the whole text is selected.
    const atStart = ta.selectionStart === 0 && ta.selectionEnd === 0,
      atEnd = ta.selectionStart === ta.value.length;
    if (
      (e.shiftKey && !shortcut && e.key === "ArrowUp" && atStart) ||
      (e.shiftKey && !shortcut && e.key === "ArrowDown" && atEnd) ||
      (shortcut === "a" &&
        ta.selectionStart === 0 &&
        ta.selectionEnd === ta.value.length)
    ) {
      e.preventDefault();
      const order = blockOrder(),
        i = order.indexOf(id);
      const target =
        shortcut === "a"
          ? null
          : order[i + (e.key === "ArrowUp" ? -1 : 1)] || id;
      ta.blur();
      if (target) selectBlockRange(id, target);
      else selectBlockRange(order[0], order.at(-1));
      return;
    }
    if (
      (shortcut === "arrowup" || shortcut === "arrowdown") &&
      !e.shiftKey &&
      toggleFold(id, shortcut === "arrowup")
    ) {
      e.preventDefault();
      return;
    }
    if (shortcut === "j" && !e.shiftKey && !shareMode) {
      e.preventDefault();
      openWritingAssist(ta);
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
        announce($t("Changed to text"));
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
    // A web address pasted over selected text links that text to it.
    const selected = ta.value.slice(ta.selectionStart, ta.selectionEnd);
    if (
      selected.trim() &&
      !selected.includes("\n") &&
      /^https?:\/\/\S+$/.test(text.trim()) &&
      !insideCodeFence(ta.value, ta.selectionStart)
    ) {
      e.preventDefault();
      const link = `[${selected}](${text.trim().replace(/[()]/g, encodeURIComponent)})`;
      replaceRange(ta, {
        from: ta.selectionStart,
        to: ta.selectionEnd,
        text: link,
        selStart: ta.selectionStart + link.length,
        selEnd: ta.selectionStart + link.length,
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
        announce($t("Pasted {value} blocks", { value: extra.length + 1 }));
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
    return toast($t("This browser cannot record audio."));
  const type = ["audio/webm;codecs=opus", "audio/webm", "audio/mp4"].find((t) =>
    MediaRecorder.isTypeSupported(t),
  );
  if (!type) return toast($t("This browser cannot record audio."));
  const targetId = note.id;
  let stream, recorder, started, timer, blob;
  let chunks = [],
    size = 0;
  modal.innerHTML = `<form method="dialog" id="record-form"><h2>${$t("Record audio")}</h2><p>${$t("The recording is saved with this note, with a player. Up to a few hours.")}</p><div class="recorder"><button type="button" class="btn primary" id="record-toggle">${$t("● Start recording")}</button><span class="record-time" aria-live="polite">0:00</span></div><audio class="audio-player" id="record-preview" controls hidden></audio>${(await transcriptionAvailable()) ? `<label class="checkbox-label"><input type="checkbox" id="record-transcribe" checked>${$t("Then transcribe it, with a summary and action items")}</label>` : ""}<div class="form-error" role="alert"></div><footer><button class="btn" value="cancel" formnovalidate>${$t("Cancel")}</button><button class="btn primary" value="ok" id="record-save" disabled>${$t("Save to note")}</button></footer></form>`;
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
        $t("Minerva cannot use the microphone. Allow it in your browser's site settings.");
      return;
    }
    chunks = [];
    size = 0;
    // A speech bitrate: about 15 MB an hour, so long meetings can be transcribed.
    recorder = new MediaRecorder(stream, {
      mimeType: type,
      audioBitsPerSecond: 32000,
    });
    recorder.ondataavailable = (e) => {
      chunks.push(e.data);
      size += e.data.size;
      // Files hold up to 60 MiB.
      if (size > 59 * 1024 * 1024) stop();
    };
    recorder.onstop = () => {
      blob = new Blob(chunks, { type: type.split(";")[0] });
      preview.src = URL.createObjectURL(blob);
      preview.hidden = false;
      toggle.textContent = $t("● Record again");
      $("#record-save").disabled = false;
    };
    recorder.start(1000);
    started = Date.now();
    toggle.textContent = $t("■ Stop");
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
    const transcribe = $("#record-transcribe")?.checked;
    modal.close();
    if (note?.id !== targetId) return;
    await flush();
    toast($t("Saving the recording…"));
    const saved = await uploadFile(file, targetId);
    await insertAttachment(saved, targetId, afterId);
    toast($t("Recording saved"));
    if (transcribe && note?.id === targetId) {
      const block = note.blocks.find((b) => b.markdown.includes(saved.id));
      await recordingDialog(saved.id, block?.id);
    }
  };
}
// Whether this server can transcribe recordings (asked once).
let canTranscribe = null;
function transcriptionAvailable() {
  canTranscribe ??= shareMode
    ? Promise.resolve(false)
    : api("ai/recordings")
        .then((v) => !!v.available)
        .catch(() => false);
  return canTranscribe;
}
/** Recordings in a note the person can edit get a "Transcribe" button. */
async function recordingButtons() {
  const players = $$(
    '#blocks .block-body audio.audio-player[src*="/api/v1/attachments/"]',
  ).filter(
    (a) => !a.nextElementSibling?.classList.contains("recording-actions"),
  );
  if (!players.length || !editable() || !(await transcriptionAvailable()))
    return;
  for (const audio of players) {
    const id = /attachments\/([0-9a-f-]{36})/.exec(
      audio.getAttribute("src"),
    )?.[1];
    if (
      !id ||
      audio.nextElementSibling?.classList.contains("recording-actions")
    )
      continue;
    const bar = document.createElement("span");
    bar.className = "recording-actions";
    bar.innerHTML = `<button type="button" class="btn" title="${$t("Transcript, key points and action items from this recording")}">${$t("✳ Transcribe and summarize")}</button>`;
    bar.firstChild.onclick = () =>
      recordingDialog(id, audio.closest("[data-block]")?.dataset.block).catch(
        error,
      );
    audio.after(bar);
  }
}
/**
 * From a recording to notes: the server transcribes it and, with an AI
 * provider, writes key points, decisions and action items; the person keeps
 * what they want, inserted after the recording.
 */
async function recordingDialog(attachmentId, afterId) {
  const targetId = note.id;
  const providers = await api("ai/providers").catch(() => []);
  modal.innerHTML = `<form class="recording-form" id="recording-form"><h2>${$t("Transcribe and summarize")}</h2><p>${$t("The recording goes to this server’s transcription service. With notes chosen, the transcript then goes to that AI provider, which writes key points, decisions and action items for you to review.")}</p><label>${$t("Notes")}<select name="provider"><option value="">${$t("Transcript only")}</option>${providers.map((p) => `<option value="${esc(p.id)}">${esc(p.name)} · ${esc(p.model)}</option>`).join("")}</select></label><p class="hint" id="recording-status" role="status"></p><div id="recording-result"></div><div class="form-error" role="alert"></div><footer><button type="button" class="btn" id="recording-close">${$t("Cancel")}</button><button class="btn primary" id="recording-start">${$t("Start")}</button><button type="button" class="btn primary" id="recording-insert" hidden>${$t("Insert into the note")}</button></footer></form>`;
  modal.showModal();
  const form = $("#recording-form");
  if (providers.length) form.provider.selectedIndex = 1;
  const status = $("#recording-status"),
    fail = (e) => ($(".form-error", form).textContent = e.message);
  let job = null,
    stopped = false;
  const stop = () => {
    stopped = true;
    if (job && ["transcribing", "summarizing"].includes(job.state))
      api(`ai/recordings/${job.id}`, "DELETE").catch(() => {});
  };
  modal.addEventListener("close", stop, { once: true });
  $("#recording-close").onclick = () => modal.close();
  form.onsubmit = async (e) => {
    e.preventDefault();
    $(".form-error", form).textContent = "";
    $("#recording-start").disabled = true;
    try {
      job = await api("ai/recordings", "POST", {
        note_id: targetId,
        attachment_id: attachmentId,
        provider_id: form.provider.value || null,
        today: new Date().toLocaleDateString("sv"),
      });
      const started = Date.now();
      while (["transcribing", "summarizing"].includes(job.state) && !stopped) {
        const seconds = Math.round((Date.now() - started) / 1000);
        status.textContent = `${job.state === "transcribing" ? "Transcribing the recording" : "Writing the notes"}… ${timestamp(seconds)}`;
        await new Promise((r) => setTimeout(r, 1500));
        job = await api(`ai/recordings/${job.id}`);
      }
      if (stopped) return;
      if (job.state === "failed") throw new Error(serverMessage(job.error));
      status.textContent = "";
      showResult();
    } catch (err) {
      status.textContent = "";
      $("#recording-start").disabled = false;
      fail(err);
    }
  };
  function showResult() {
    const notes = job.summary;
    const list = (items) =>
      `<ul>${items.map((t) => `<li>${esc(t)}</li>`).join("")}</ul>`;
    const option = (name, label, body, checked = true) =>
      `<label class="checkbox-label recording-part"><input type="checkbox" name="${name}" ${checked ? "checked" : ""}><span><strong>${label}</strong>${body}</span></label>`;
    const actions = notes?.action_items.length
      ? `<fieldset class="recording-actions-list"><legend>${$t("Action items, as tasks")}</legend>${notes.action_items.map((a, i) => `<label class="checkbox-label"><input type="checkbox" name="action" value="${i}" checked><span>${esc(a.task)}${a.owner ? ` — ${esc(a.owner)}` : ""}${a.due ? ` <small>⏰ ${esc(a.due)}</small>` : ""}</span></label>`).join("")}</fieldset>`
      : "";
    $("#recording-result").innerHTML =
      (notes
        ? `${notes.summary.length ? option("summary", $t("Key points"), list(notes.summary)) : ""}${notes.decisions.length ? option("decisions", $t("Decisions"), list(notes.decisions)) : ""}${actions}`
        : "") +
      option(
        "transcript",
        $t("Transcript"),
        `<small> ${$tn(job.transcript.length, "· {count} part, folded under its heading", "· {count} parts, folded under its heading")}</small>`,
        !notes,
      );
    form.provider.disabled = true;
    $("#recording-start").hidden = true;
    $("#recording-insert").hidden = false;
    $("#recording-close").textContent = $t("Close");
    $("#recording-insert").onclick = async () => {
      const blocks = recordingBlocks(job, {
        summary: !!form.summary?.checked,
        decisions: !!form.decisions?.checked,
        transcript: !!form.transcript?.checked,
        actions: $$("[name=action]:checked", form).map((c) => Number(c.value)),
      });
      if (!blocks.length) return fail(new Error($t("Choose something to insert.")));
      if (note?.id !== targetId)
        return fail(new Error($t("Open the note with the recording to insert.")));
      try {
        await appendBlocks(blocks, { after: afterId, source: "the recording" });
        modal.close();
      } catch (err) {
        fail(err);
      }
    };
  }
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
  modal.innerHTML = `<form method="dialog" id="remind-form"><h2>${$t("Remind me")}</h2><p>${$t("At this time you get an email with this block and a link to the note.")}</p><label>${$t("When")}<input name="at" type="datetime-local" required min="${local(new Date())}" value="${local(soon)}"></label><footer><button class="btn" value="cancel" formnovalidate>${$t("Cancel")}</button><button class="btn primary" value="ok">${$t("Set reminder")}</button></footer></form>`;
  modal.showModal();
  const form = $("#remind-form");
  form.onsubmit = async (e) => {
    if (e.submitter?.value !== "ok") return;
    e.preventDefault();
    const when = new Date(form.at.value);
    if (!(when > new Date())) return toast($t("Choose a time in the future."));
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
    toast($t("Reminder set for {date}", { date: when.toLocaleString() }));
  };
}
/** How each `/` command shows in the menu: group, icon, description, shortcut. */
const COMMAND_GROUPS = [
  msg("Basic"),
  msg("Lists"),
  msg("Media & links"),
  msg("Advanced"),
  msg("People & time"),
  msg("Plugins"),
];
const COMMAND_LOOK = {
  Text: ["Basic", "Aa", $t("Plain writing"), ""],
  "Heading 1": ["Basic", "H1", $t("Big section title"), "#"],
  "Heading 2": ["Basic", "H2", $t("Medium section title"), "##"],
  "Heading 3": ["Basic", "H3", $t("Small section title"), "###"],
  Quote: ["Basic", "❝", $t("Quote someone"), ">"],
  Divider: ["Basic", "—", $t("Separate sections"), "---"],
  Highlight: ["Basic", "▍", $t("Mark important text"), "=="],
  Emoji: ["Basic", "☺", $t("Search every emoji"), ""],
  Task: ["Lists", "☑", $t("Track something to do"), "[]"],
  "Bullet list": ["Lists", "•", $t("A simple list"), "-"],
  "Numbered list": ["Lists", "1.", $t("A list in order"), "1."],
  "Unfinished journal tasks": ["Lists", "↻", $t("Open tasks from past days"), ""],
  "Image or file": ["Media & links", "⇪", $t("Upload, drop or pick a file"), ""],
  "Record audio": ["Media & links", "●", $t("Record from the microphone"), ""],
  Link: ["Media & links", "↗", $t("A link to a web page"), ""],
  "Link to note": ["Media & links", "⧉", $t("Link another note"), "[["],
  "Embedded note": ["Media & links", "▤", $t("Show another note here, kept current"), "![["],
  Table: ["Media & links", "▦", $t("Rows and columns"), ""],
  "Database (live)": ["Media & links", "⊞", $t("Embed a live database"), ""],
  Code: ["Advanced", "</>", $t("Code with highlighting"), "```"],
  "Math (block)": ["Advanced", "∑", $t("A formula in LaTeX"), "$$"],
  "Diagram (Mermaid)": ["Advanced", "⇄", $t("Flowcharts and more"), ""],
  "Callout · Note": ["Advanced", "ℹ", $t("A highlighted note"), ""],
  "Callout · Tip": ["Advanced", "✦", $t("A helpful tip"), ""],
  "Callout · Warning": ["Advanced", "⚠", $t("A warning to notice"), ""],
  "Assign to someone": ["People & time", "@", $t("Email a teammate"), ""],
  "Remind me": ["People & time", "⏰", $t("Get this block by email"), ""],
};
function blockMenu(existingId, capturedSelection = "") {
  if (!editable()) return;
  modal.innerHTML =
    `<div class="command-menu-search"><span aria-hidden="true">/</span><input id="command-search" aria-label="${$t("Search block commands")}" placeholder="${$t("Type to search: heading, task, file…")}" autocomplete="off"></div><div id="command-results" class="command-list" role="listbox" aria-label="${$t("Block types")}"></div><footer class="command-menu-hint"><span><kbd>↑</kbd><kbd>↓</kbd> choose</span><span><kbd>${shortcutKeys("Enter")}</kbd> insert</span><span><kbd>Esc</kbd> close</span></footer>`;
  // The menu opens by the block it inserts into, like a popover.
  modal.classList.add("command-menu");
  modal.addEventListener(
    "close",
    () => {
      modal.classList.remove("command-menu");
      modal.style.removeProperty("top");
      modal.style.removeProperty("bottom");
      modal.style.removeProperty("left");
    },
    { once: true },
  );
  modal.showModal();
  // Placed once the list is drawn: below the block, or above it near the bottom.
  const placeMenu = () => {
    const anchor = (
      $(`[data-block="${CSS.escape(existingId || "")}"]`) || $("#add-block")
    )?.getBoundingClientRect();
    if (!anchor) return;
    const width = modal.offsetWidth,
      height = modal.offsetHeight;
    const below = anchor.bottom + 6;
    // Above the block, the menu hangs from it, so it stays by the block as
    // the list gets shorter while searching.
    if (below + height < innerHeight - 8) modal.style.top = `${below}px`;
    else {
      modal.style.top = "auto";
      modal.style.bottom = `${Math.max(8, innerHeight - anchor.top + 6)}px`;
    }
    modal.style.left = `${Math.max(8, Math.min(anchor.left, innerWidth - width - 8))}px`;
  };
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
              $t("Reopen the editable source note before inserting."),
            );
          const rendered = await Promise.all(sections.map(renderMarkdown));
          if (note?.id !== noteId || !editable())
            throw new Error($t("The active note changed."));
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
          announce($t("Inserted generated knowledge"));
        },
      });
    }
    if (command[1] === null) return attachmentDialog();
    if (command[1] === "@database") return databaseEmbedDialog(existingId);
    if (command[1] === "@assign") return assignDialog(existingId);
    if (command[1] === "@record") return recordDialog(existingId);
    if (command[1] === "@emoji") return emojiDialog(existingId);
    if (command[1] === "@toggle") return insertToggle(existingId);
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
    if (command[1] === "[[" || command[1] === "![[")
      updateLinkPicker($(".block textarea")).catch(() => closeLinkPicker());
  }
  const look = (c) =>
    c[1] === "@plugin"
      ? ["Plugins", "✳", `/${c[3]?.id || ""}`, ""]
      : COMMAND_LOOK[c[0]] || ["Basic", "·", "", ""];
  function draw(reset = false) {
    const query = input.value.trim().toLowerCase();
    // Grouped in menu order, so arrow keys follow what is on screen.
    matches = commands
      .filter((c) =>
        `${c[0]} ${$t(c[0])} ${c[2]} ${look(c)[2]}`
          .toLowerCase()
          .includes(query),
      )
      .map((c, i) => [COMMAND_GROUPS.indexOf(look(c)[0]), i, c])
      .sort((a, b) => a[0] - b[0] || a[1] - b[1])
      .map((x) => x[2]);
    if (reset) selected = 0;
    let group = "";
    $("#command-results").innerHTML =
      matches
        .map((c, i) => {
          const [name, glyph, description, shortcut] = look(c);
          const heading =
            name !== group
              ? `<h3 class="command-group">${esc($t((group = name)))}</h3>`
              : "";
          const label = c[1] === "@plugin" ? c[2] : $t(c[0]);
          return `${heading}<button class="command-item ${i === selected ? "command-selected" : ""}" role="option" aria-selected="${i === selected}" data-command="${i}" ${c[3] && !c[3].available ? "disabled" : ""}><span class="command-icon" aria-hidden="true">${esc(glyph)}</span><span class="command-text"><strong>${esc(label)}</strong><small>${esc(c[3] && !c[3].available ? c[3].reason || $t("Configuration required") : description)}</small></span>${shortcut ? `<kbd>${esc(shortcut)}</kbd>` : ""}</button>`;
        })
        .join("") ||
      `<p class="command-empty">${$t("No block matches “{query}”", { query: esc(input.value) })}</p>`;
    $$("[data-command]", modal).forEach((button) => {
      action(button, "click", () => choose(Number(button.dataset.command)));
      button.onpointermove = () => {
        const i = Number(button.dataset.command);
        if (i === selected) return;
        $(".command-selected", modal)?.classList.remove("command-selected");
        button.classList.add("command-selected");
        selected = i;
      };
    });
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
  placeMenu();
  input.focus();
}
function attachmentMarkdown(file) {
  const label = file.filename.replace(/[\\[\]!*_`<>]/g, "_");
  const url = `/api/v1/attachments/${file.id}`;
  // A picture's bar (on hover) has its details and Download.
  if (file.media_type.startsWith("image/"))
    return `![${label}](${url}/preview)`;
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
  noteFiles.set(file.id, {
    ...file,
    created_at: Math.floor(Date.now() / 1000),
  });
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
    toast($t("Uploading {name}…", { name: file.name }));
    await insertAttachment(await uploadFile(file, targetId), targetId, afterId);
  }
  toast($t("Files saved. Originals are available for download."));
}
async function uploadFile(file, targetId) {
  if (!file.size || file.size > 60 * 1024 * 1024)
    throw new Error($t("{name} is too large: files hold up to 60 MiB.", { name: file.name }));
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
  if (!response.ok) throw new Error(result.error ? serverMessage(result.error) : $t("Upload failed"));
  return result;
}
async function attachmentDialog() {
  await flush();
  const targetId = note.id;
  const result = await api(`notes/${targetId}/attachments`);
  if (note?.id !== targetId) return;
  modal.innerHTML = `<h2>${$t("Images and files")}</h2>
    ${editable() ? `<label class="file-drop" id="file-drop"><input id="attachment-upload" type="file" multiple class="sr-only"><span class="file-drop-icon" aria-hidden="true">⇪</span><strong>${$t("Drop files here, or click to choose")}</strong><small>${$t("Images, PDFs, audio, documents · up to 60 MB each")}</small></label>` : ""}
    <h3 class="file-list-heading">${$t("In this note")}</h3>
    <div class="file-list">${result.attachments.map((file, i) => `<div class="attachment-row"><span class="attachment-name">${esc(file.filename)} <small>${$t("{value} KiB", { value: Math.ceil(file.size / 1024) })}</small></span><a class="btn" href="/api/v1/attachments/${file.id}" download>${$t("Download")}</a>${editable() ? `<button class="btn" data-insert-file="${i}">${$t("Insert")}</button>` : ""}</div>`).join("") || `<p class="file-list-empty">${$t("No files yet. You can also paste an image into a block, or drop files onto the note.")}</p>`}</div>
    <footer><button class="btn" id="close-modal" autofocus>${$t("Close")}</button></footer>`;
  modal.showModal();
  $("#close-modal").onclick = () => modal.close();
  const upload = async (files) => {
    if (!files.length) return;
    modal.close();
    await uploadAttachments(files);
  };
  action($("#attachment-upload"), "change", (e) => upload([...e.target.files]));
  const drop = $("#file-drop");
  if (drop) {
    drop.addEventListener("dragover", (e) => {
      e.preventDefault();
      drop.classList.add("dropping");
    });
    drop.addEventListener("dragleave", () => drop.classList.remove("dropping"));
    drop.addEventListener("drop", (e) => {
      e.preventDefault();
      drop.classList.remove("dropping");
      upload([...e.dataTransfer.files]).catch(error);
    });
  }
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
        ? $t("Automatic: {value}. Add it to use your own value.", { value: AUTOMATIC_VARIABLES[k] })
        : $t("Used by {name}", {
            name: printTemplates
              .filter((t) => (t.variables || []).includes(k))
              .map((t) => t.name)
              .join(", "),
          }),
    )}">{{${esc(k)}}}</button>`;
  const suggestions = () => {
    const free = (k) => !(k in current);
    const first = [...used].filter(free);
    const rest = known.filter((k) => free(k) && !used.has(k));
    return `${first.length ? `<p class="meta-suggest-head">${$t("Used by {name}", { name: esc(chosen.name) })}</p><div class="meta-chips">${first.map(chip).join("")}</div>` : ""}${rest.length ? `<p class="meta-suggest-head">${first.length ? $t("Used by other PDF templates") : $t("Used by PDF templates")}</p><div class="meta-chips">${rest.map(chip).join("")}</div>` : ""}<p class="meta-suggest-head">${$t("Filled in automatically")}</p><div class="meta-chips">${Object.keys(
      AUTOMATIC_VARIABLES,
    )
      .filter(free)
      .map(chip)
      .join("")}</div>`;
  };
  modal.innerHTML = `<form id="metadata-form"><h2>${$t("Metadata")}</h2><p>${$t("Your own variables for this note: name a field, give it a value, and type")} <code>{{name}}</code> ${$t("in the note to insert it when exporting. PDF templates also place some of them on covers, headers, and tables; a PDF shows the name of any it is missing, such as")} <code>{{company}}</code>.</p><div id="meta-rows" class="meta-custom">${Object.keys(
    current,
  )
    .map((k) => metaRow(k, current[k], readOnly))
    .join(
      "",
    )}</div><p class="meta-empty hint" ${Object.keys(current).length ? "hidden" : ""}>${$t("No variables yet.")}</p>${readOnly ? "" : `<button type="button" class="btn" id="meta-add">${$t("+ Add variable")}</button>`}<section class="meta-suggest" aria-label="${$t("Known variables")}"><div id="meta-suggestions">${suggestions()}</div></section><div class="form-error" role="alert"></div><footer><button type="button" class="btn" id="meta-cancel">${readOnly ? $t("Close") : $t("Cancel")}</button>${readOnly ? "" : `<button class="btn primary" type="submit">${$t("Save")}</button>`}</footer></form>`;
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
      toast($t("Metadata saved"));
      if (after) after();
    } catch (err) {
      $(".form-error", form).textContent = err.message;
    }
  };
  if (!modal.open) modal.showModal();
}
function metaRow(key, value, readOnly) {
  return `<div class="meta-row" data-key="${esc(key)}"><input name="key" placeholder="${$t("Variable name")}" aria-label="${$t("Variable name")}" value="${esc(key)}" ${readOnly ? "readonly" : ""}><input name="value" placeholder="${esc(key === "logo" || key === "photo" ? "HTTPS image address or a file link" : "Value")}" aria-label="${$t("Value")}" value="${esc(value ?? "")}" ${readOnly ? "readonly" : ""}>${readOnly ? "" : `<button type="button" class="btn" data-remove-meta aria-label="${$t("Remove field")}">✕</button>`}</div>`;
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
  modal.innerHTML = `<form id="export-form"><h2>${$t("Export “{title}”", { title: esc(note.title) })}</h2><p>${$t("Exports include the latest saved version of this note.")}</p><div class="export-formats" role="radiogroup" aria-label="${$t("Format")}">${EXPORT_FORMATS.map(([value, label, help]) => `<label class="export-format"><input type="radio" name="format" value="${value}" ${value === format ? "checked" : ""}><span><strong>${label}</strong><small>${help}</small></span></label>`).join("")}</div><fieldset class="export-options"><legend>${$t("Options")}</legend><label class="checkbox-label" data-for="pdf html"><input type="checkbox" name="cover" ${saved.cover === false ? "" : "checked"}>${$t("Include cover image")}</label><label class="checkbox-label" data-for="pdf html"><input type="checkbox" name="meta" ${saved.meta === false ? "" : "checked"}>${$t("Show date and tags under the title")}</label><label class="checkbox-label" data-for="md zip"><input type="checkbox" name="front_matter" ${saved.front_matter === false ? "" : "checked"}>${$t("Add front matter (title, tags, date)")}</label><label data-for="pdf html">${$t("Template")} <select name="template">${templateOptions(note.print_template ?? saved.template ?? "")}</select></label><p class="hint template-help" data-for="pdf html"></p><button type="button" class="btn" id="export-metadata" data-for="pdf html">${icon("list")}${$t("Edit metadata…")}</button><label data-for="pdf">${$t("Page size")} <select name="size"><option value="a4" ${saved.size !== "letter" ? "selected" : ""}>A4</option><option value="letter" ${saved.size === "letter" ? "selected" : ""}>${$t("US Letter")}</option></select></label></fieldset><div class="form-error" role="alert"></div><footer><button type="button" class="btn" id="cancel-export">${$t("Cancel")}</button><button class="btn primary" type="submit">${$t("Export")}</button></footer></form>`;
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
      ? `${$t("{description} For: {users}.", { description: t.description, users: t.users })}${t.orientation === "landscape" ? $t(" Landscape pages.") : ""}`
      : $t("Minerva's own clean layout.");
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
    toast($t("Export downloaded"));
  };
}
/** Opens the system print dialog for an export page without leaving the note. */
function printDocument(url) {
  $("#print-frame")?.remove();
  const frame = document.createElement("iframe");
  frame.id = "print-frame";
  frame.title = $t("Print preview");
  frame.setAttribute("aria-hidden", "true");
  frame.className = "print-frame";
  toast($t("Preparing PDF…"));
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
/**
 * Imports notes into a folder: Markdown files, or exports from Notion (zip),
 * Obsidian and Logseq (their folder, zipped) and Evernote (.enex). The server
 * reads each file (`POST /api/v1/import`).
 */
function importMarkdown() {
  if (!folderOptions()) return toast($t("Editor access is required."));
  formDialog(
    $t("Bring your notes along"),
    $t("Pages become notes, with their links, tags, pictures and files; folders come along when you can add folders here, and Notion tables become databases."),
    `<label>${$t("Files")}<input type="file" name="file" accept=".md,.markdown,.txt,.zip,.enex" multiple required></label><label>${$t("Into folder")}<select name="folder_id">${folderOptions()}</select></label><details class="import-help"><summary>${$t("Where to get an export")}</summary><ul><li><strong>${$t("Notion:")}</strong> ${$t("Settings → Export all workspace content (or ··· → Export on a page), format")} <em>${$t("Markdown &amp; CSV")}</em>${$t(", with subpages. Choose the downloaded zip.")}</li><li><strong>${$t("Obsidian:")}</strong> ${$t("zip your vault folder.")}</li><li><strong>${$t("Logseq:")}</strong> ${$t("zip your graph folder (with")} <code>pages</code>, <code>journals</code> and <code>assets</code>).</li><li><strong>${$t("Evernote:")}</strong> ${$t("right-click a notebook → Export notebook, as")} <em>${$t("ENEX")}</em>.</li><li><strong>${$t("Markdown:")}</strong> ${$t("choose one or more")} <code>.md</code> ${$t("files.")}</li></ul></details><p class="hint" id="import-progress" aria-live="polite"></p>`,
    async (v) => {
      const files = [...$("[name=file]", modal).files];
      const progress = $("#import-progress", modal);
      const failed = [];
      const warnings = [];
      const totals = { notes: 0, folders: 0, databases: 0, attachments: 0 };
      let first = null;
      for (const [i, file] of files.entries()) {
        progress.textContent = $t("Importing {value} of {count}: {name}…", { value: i + 1, count: files.length, name: file.name });
        try {
          if (file.size > 200 * 1024 * 1024)
            throw new Error($t("larger than 200 MB"));
          const r = await fetch(
            `/api/v1/import?folder_id=${encodeURIComponent(v.folder_id)}&name=${encodeURIComponent(file.name)}`,
            {
              method: "POST",
              headers: { "X-Minerva-Client": "1" },
              body: file,
            },
          );
          const report = await r.json().catch(() => ({}));
          if (!r.ok) throw new Error(report.error ? serverMessage(report.error) : $t("could not be read"));
          for (const key of Object.keys(totals)) totals[key] += report[key];
          warnings.push(...report.warnings);
          first ??= report.first_note;
        } catch (err) {
          failed.push(`${file.name} (${err.message})`);
        }
      }
      await refresh();
      const count = (n, word) => n && `${n} ${word}${n === 1 ? "" : "s"}`;
      const parts = [
        count(totals.notes, "note") || "0 notes",
        count(totals.folders, "folder"),
        count(totals.databases, "database"),
        count(totals.attachments, "file"),
      ].filter(Boolean);
      toast(
        [
          `Imported ${parts.join(", ")}.`,
          failed.length && $t("Not imported: {files}.", { files: failed.join(", ") }),
          ...warnings.slice(0, 3),
        ]
          .filter(Boolean)
          .join(" "),
      );
      if (totals.notes === 1 && first) await openNote(first);
      else navigate("folder", v.folder_id);
    },
    "Import",
  );
}
async function history() {
  await flush();
  const rows = await api("notes/" + note.id + "/history");
  modal.innerHTML = `<h2>${$t("Version history")}</h2><p>${$t("The last 50 saved versions of this note.")}</p><div class="search-results">${
    rows
      .reverse()
      .map(
        (n) =>
          `<div class="history-row"><span>${$t("Revision {revision}", { revision: n.revision })}<br><small>${new Date(n.updated_at * 1000).toLocaleString()}</small></span><button class="btn" data-restore="${n.revision}" ${!editable() ? "disabled" : ""}>${$t("Restore")}</button></div>`,
      )
      .join("") || `<p>${$t("No previous versions yet.")}</p>`
  }</div><footer><button class="btn" id="close-modal">${$t("Close")}</button></footer>`;
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
      toast($t("Previous version restored"));
    }),
  );
}
/** Undo or redo this client's last change to the open note. */
async function undoRedo(redo) {
  if (!note || !editable() || !live) return;
  await closing;
  if (!(redo ? live.redo() : live.undo())) {
    announce(redo ? $t("Nothing to redo") : $t("Nothing to undo"));
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
  announce(redo ? $t("Redone") : $t("Undone"));
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
      return `<button class="day ${entry ? "has-entry" : ""} ${iso === today ? "today" : ""}" data-journal-date="${iso}" aria-label="${esc(longDate(iso))}${entry ? $t(", has an entry") : ""}">${i + 1}</button>`;
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
  $("#breadcrumb").textContent = $t("Journal");
  $("#content").innerHTML =
    `<section class="page"><div class="eyebrow">${$t("ONE NOTE PER DAY")}</div><div class="page-heading"><div><h1>${$t("Journal")}</h1><p class="subtitle">${$t("Capture the day. Unfinished tasks can follow you with [[uncompleted_journal_tasks]].")}</p></div><div class="toolbar-group"><input type="date" id="journal-date" value="${today}" aria-label="${$t("Journal date")}"><button class="btn" id="journal-open">${$t("Open date")}</button><button class="btn primary" id="journal-today">${icon("calendar")}${$t("Today")}</button></div></div><div class="journal-layout"><section class="panel calendar" aria-label="${$t("Calendar")}"><div class="calendar-head"><button class="btn" id="month-prev" aria-label="${$t("Previous month")}">‹</button><h2>${esc(first.toLocaleDateString(undefined, { month: "long", year: "numeric" }))}</h2><button class="btn" id="month-next" aria-label="${$t("Next month")}">›</button></div><div class="calendar-grid">${weekdays.map((d) => `<span class="weekday">${esc(d)}</span>`).join("")}${cells}</div></section><section class="panel journal-recent"><div class="panel-heading"><div><h2>${$t("Recent entries")}</h2><p>${$tn(entries.size, "{count} entry in this workspace", "{count} entries in this workspace")}</p></div></div><div class="journal-list">${recent.map((n) => `<button class="journal-entry" data-note="${n.id}"><strong>${esc(longDate(n.journal))}</strong><small>${n.title !== n.journal ? esc(n.title) : ""}</small></button>`).join("") || `<div class="empty">${$t("No entries yet. Start with today.")}</div>`}</div>${editsWorkspace() ? `<form class="journal-settings" id="journal-settings"><label>${$t("Template for new days")}<select name="template_id"><option value="">${$t("Blank page")}</option>${templates.map((t) => `<option value="${t.id}" ${t.id === configured ? "selected" : ""}>${esc(t.title)}</option>`).join("")}</select></label><button class="btn">${$t("Save")}</button></form>` : ""}</section></div></section>`;
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
    toast($t("Journal template saved"));
  });
}
function renderTemplates() {
  const templates = templateNotes();
  $("#breadcrumb").textContent = $t("Templates");
  $("#content").innerHTML =
    `<section class="page"><div class="eyebrow">${$t("START FROM SOMETHING GOOD")}</div><div class="page-heading"><div><h1>${$t("Templates")}</h1><p class="subtitle">Empty notes offer “Fill from template”. Use {{date}}, {{weekday}}, {{long_date}} and {{title}} as placeholders.</p></div><button class="btn primary" id="new-template">${icon("plus")}${$t("New template")}</button></div><div class="note-grid">${templates.map((n) => `<button class="note-card" data-note="${n.id}"><div class="note-preview" aria-hidden="true"><span class="preview-symbol">${esc(n.icon)}</span><div class="preview-lines"><i></i><i></i><i></i></div></div><div class="note-info"><strong>${esc(n.title)}</strong><div class="note-meta"><span>${$t("Template")}</span><span>· ${date(n.updated_at)}</span></div></div></button>`).join("")}</div>${templates.length ? "" : `<div class="empty">${icon("template")}<h3>${$t("No templates yet.")}</h3>${$t("Create one for meeting notes, daily journals, or project briefs.")}</div>`}</section>`;
  $$("[data-note]").forEach((b) =>
    action(b, "click", () => openNote(b.dataset.note)),
  );
  action($("#new-template"), "click", () =>
    formDialog(
      $t("New template"),
      "Write it like a normal note. Placeholders such as {{date}} are filled in when the template is used.",
      `<label>${$t("Template name")}<input name="title" required maxlength="200" placeholder="${$t("e.g. Meeting notes")}" autofocus></label>`,
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
        announce($t("Template applied"));
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
          `<span class="journal-tasks-note">${$t("Unfinished journal tasks are shown to signed-in members.")}</span>`),
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
  const html = `<span class="journal-tasks-panel" role="group" aria-label="${$t("Unfinished tasks from earlier journal entries")}"><span class="journal-tasks-title">${$t("☑ Unfinished tasks from earlier entries")} <small>${tasks.length}</small></span>${tasks.length ? `<span class="journal-tasks-list">${tasks.map((t, i) => `<span class="journal-task"><button class="task-toggle" data-complete-task="${i}" aria-label="Mark “${esc(t.text)}” done">☐</button><span class="journal-task-text">${t.html}</span><a class="journal-task-origin" href="#note/${esc(t.note_id)}" title="${$t("Open the entry")}">${esc(parseDate(t.date).toLocaleDateString(undefined, { month: "short", day: "numeric" }))}</a></span>`).join("")}</span>` : `<span class="journal-tasks-note">${$t("Nothing left over. Nice.")}</span>`}</span>`;
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
            $tn(result.completed, "Marked done in {count} entry", "Marked done in {count} entries"),
          );
        } catch (err) {
          button.disabled = false;
          error(err);
        }
      }),
    );
  }
}
/** Account → This device: installing the app and reading offline. */
function deviceHTML() {
  const worker = "serviceWorker" in navigator && isSecureContext;
  const app = installed()
    ? `<p>${$t("Minerva is installed on this device.")}</p>`
    : canInstall()
      ? `<p>${$t("Install Minerva to open it in its own window, from your home screen or dock, with")} <strong>${$t("Quick note")}</strong> ${$t("and sharing from other apps on phones.")}</p><button class="btn primary" id="install-app">${$t("Install Minerva")}</button>`
      : `<p>${$t("To install Minerva, use your browser’s menu:")} <strong>${$t("Install app")}</strong> or <strong>${$t("Add to Home Screen")}</strong> ${$t("(Safari: Share → Add to Home Screen). Then share pages and text to it, or write a")} <a href=\"/capture\">${$t("quick note")}</a>.</p>`;
  // What "automatic" means here: the browser's language, when Minerva has it.
  const browser =
    LANGUAGES.find(
      ([code]) => code === pickLanguage("", navigator.languages || []),
    )?.[1] || "English";
  const languages = `<label class="device-language">${$t("Language")}<select id="interface-language"><option value="">${$t("Automatic")} (${esc(browser)})</option>${LANGUAGES.map(([code, name]) => `<option value="${code}" ${chosenLanguage() === code ? "selected" : ""}>${esc(name)}</option>`).join("")}</select></label>`;
  return `<section class="panel security-panel" id="device-panel"><div class="panel-heading"><div><h2>${$t("This device")}</h2><p>${$t("Settings for this browser only.")}</p></div></div><div class="security-body">${languages}${app}${
    worker
      ? `<label class="checkbox-label device-offline"><input type="checkbox" id="offline-reading" ${offlineReading() ? "checked" : ""}><span><strong>${$t("Keep notes I open for reading offline")}</strong><br><small>${$t("Notes and pictures you open are kept in this browser and shown read-only when the server cannot be reached. They are deleted when you sign out. Leave this off on a shared computer.")}</small></span></label>`
      : `<p class=\"hint\">${$t("Reading offline needs a secure (https) connection to the server.")}</p>`
  }</div></section>`;
}
function bindDevice() {
  action($("#interface-language"), "change", (e) => setLanguage(e.target.value));
  action($("#offline-reading"), "change", (e) => {
    setOfflineReading(e.target.checked);
    toast(
      e.target.checked
        ? $t("Notes you open are now kept on this device for reading offline.")
        : $t("Notes kept on this device were deleted."),
    );
  });
  action($("#install-app"), "click", async () => {
    if (await install()) toast($t("Minerva is installed."));
  });
}
onInstallChange(() => {
  const panel = $("#device-panel");
  if (panel) {
    panel.outerHTML = deviceHTML();
    bindDevice();
  }
});
async function renderAccount() {
  const status = await api("account/2fa");
  $("#breadcrumb").textContent = $t("Account security");
  $("#content").innerHTML =
    `<section class="page"><div class="eyebrow">${$t("YOUR ACCOUNT")}</div><div class="page-heading"><div><h1>${$t("Account security")}</h1><p class="subtitle">${esc(data.user.name)} · ${esc(data.user.email)}</p></div></div><section class="panel security-panel"><div class="panel-heading"><div><h2>${$t("Password")}</h2><p>${$t("Changing it signs you out everywhere else.")}</p></div></div><button class="btn" id="change-password">${$t("Change password")}</button></section><section class="panel security-panel"><div class="panel-heading"><div><h2>${$t("Two-factor authentication")}</h2><p>${$t("Sign in with your password and a code from an authenticator app such as Google Authenticator, Microsoft Authenticator, 1Password, or Aegis.")}</p></div><span class="pill ${status.enabled ? "pill-on" : ""}">${status.enabled ? $t("On") : $t("Off")}</span></div><div class="security-body">${
      status.enabled
        ? `<p>${$t("On since {date}. {recovery_remaining} of 10 recovery codes left.", { date: esc(new Date(status.enabled_at * 1000).toLocaleDateString()), recovery_remaining: status.recovery_remaining })}</p><div class="toolbar-group"><button class="btn" id="new-recovery">${$t("New recovery codes")}</button><button class="btn danger" id="disable-2fa">${$t("Turn off")}</button></div>`
        : `<p>${$t("Two-factor authentication protects your notes even if your password is stolen.")}</p><button class="btn primary" id="setup-2fa">${$t("Set up two-factor authentication")}</button>`
    }</div></section>${deviceHTML()}</section>`;
  bindDevice();
  action($("#setup-2fa"), "click", setupTwoFactor);
  action($("#change-password"), "click", () =>
    formDialog(
      $t("Change your password"),
      $t("Use 12 characters or more. Your other devices will need to sign in again."),
      `<label>${$t("Current password")}<input name="current_password" type="password" autocomplete="current-password" required></label><label>${$t("New password")}<input name="new_password" type="password" autocomplete="new-password" minlength="12" maxlength="256" required></label><label>${$t("Repeat new password")}<input name="repeat" type="password" autocomplete="new-password" required></label>`,
      async (v) => {
        if (v.new_password !== v.repeat)
          throw new Error($t("The new passwords do not match."));
        await api("account/password", "POST", {
          current_password: v.current_password,
          new_password: v.new_password,
        });
        toast($t("Password changed"));
      },
      "Change password",
    ),
  );
  action($("#new-recovery"), "click", () =>
    formDialog(
      $t("New recovery codes"),
      $t("Your current recovery codes will stop working. Enter a code from your authenticator app to continue."),
      `<label>${$t("Authentication code")}<input name="code" inputmode="numeric" autocomplete="one-time-code" required maxlength="7"></label>`,
      async (v) => {
        const result = await api("account/2fa/recovery", "POST", v);
        showRecoveryCodes(result.recovery_codes);
      },
      "Create new codes",
    ),
  );
  action($("#disable-2fa"), "click", () =>
    formDialog(
      $t("Turn off two-factor authentication?"),
      $t("Your account will be protected by your password only."),
      `<label>${$t("Password")}<input name="password" type="password" autocomplete="current-password" required></label><label>${$t("Authentication or recovery code")}<input name="code" autocomplete="one-time-code" required></label>`,
      async (v) => {
        await api("account/2fa/disable", "POST", v);
        toast($t("Two-factor authentication is off"));
        await renderAccount();
      },
      "Turn off",
    ),
  );
}
async function setupTwoFactor() {
  const setup = await api("account/2fa/setup", "POST", {});
  modal.innerHTML = `<form id="totp-form"><h2>${$t("Set up two-factor authentication")}</h2><ol class="totp-steps"><li><strong>${$t("Scan this QR code")}</strong> ${$t("with your authenticator app.")}<div class="totp-qr" aria-label="${$t("QR code for your authenticator app")}">${setup.qr_svg}</div><details><summary>${$t("Can't scan? Enter the key manually")}</summary><code class="totp-secret">${esc(setup.secret)}</code><button type="button" class="btn" id="copy-secret">${$t("Copy key")}</button><p>${$t("Account: {email} · Time-based · 6 digits", { email: esc(data.user.email) })}</p></details></li><li><strong>${$t("Enter the 6-digit code")}</strong> ${$t("the app shows for Minerva.")}<input name="code" inputmode="numeric" autocomplete="one-time-code" placeholder="123 456" maxlength="7" required aria-label="${$t("Authentication code")}"></li></ol><div class="form-error" role="alert"></div><footer><button type="button" class="btn" id="cancel-totp">${$t("Cancel")}</button><button class="btn primary" type="submit">${$t("Turn on")}</button></footer></form>`;
  modal.showModal();
  $("#cancel-totp").onclick = () => modal.close();
  action($("#copy-secret"), "click", async () => {
    await navigator.clipboard.writeText(setup.secret.replace(/ /g, ""));
    toast($t("Key copied"));
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
  modal.innerHTML = `<h2>${justEnabled ? $t("Two-factor authentication is on") : $t("Your new recovery codes")}</h2><p>${$t("Save these recovery codes somewhere safe. Each one signs you in once if you lose your phone. They will not be shown again.{value}", { value: justEnabled ? $t(" Your other sessions were signed out.") : "" })}</p><div class="recovery-codes">${codes.map((c) => `<code>${esc(c)}</code>`).join("")}</div><footer><button class="btn" id="copy-codes">${$t("Copy")}</button><button class="btn" id="download-codes">${$t("Download")}</button><button class="btn primary" id="done-codes">${$t("I saved them")}</button></footer>`;
  if (!modal.open) modal.showModal();
  action($("#copy-codes"), "click", async () => {
    await navigator.clipboard.writeText(text);
    toast($t("Recovery codes copied"));
  });
  action($("#download-codes"), "click", () => {
    const a = document.createElement("a");
    a.href = URL.createObjectURL(
      new Blob([`${$t("Minerva recovery codes for {email}", { email: data.user.email })}\n\n${text}\n`], {
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
  $("#breadcrumb").textContent = $t("Sharing");
  const origin = serverOrigin();
  const role = (r) => (r === "editor" ? "Edit" : "Read only");
  const people = (list) =>
    list
      .map(
        (p) =>
          `<span class="person-chip" title="${esc(role(p.role))}">${esc(p.name)}<small>${p.role === "editor" ? $t("Edit") : $t("View")}</small></span>`,
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
  const workspacesHTML = `<section class="panel sharing-section"><div class="panel-heading"><div><h2>${$t("Shared workspaces")}</h2><p>${$t("Everyone in a workspace sees all of its folders and notes. Share one from its badge at the top of the page, or here.")}</p></div></div>${
    info.workspaces_shared.length || info.workspaces_received.length
      ? `<div class="sharing-rows">${info.workspaces_shared
          .map(
            (w) =>
              `<div class="sharing-row"><div class="sharing-what"><span class="square">${esc(w.name.slice(0, 1))}</span><div><strong>${esc(w.name)}</strong><small>${$tn(w.members.length, "Yours · shared with {count} person", "Yours · shared with {count} people")}</small></div></div><div class="sharing-who">${people(w.members)}</div><div class="sharing-actions"><button class="btn" data-manage-workspace="${w.id}">${$t("Manage")}</button><button class="btn" data-open-workspace="${w.id}">${$t("Open")}</button></div></div>`,
          )
          .join("")}${info.workspaces_received
          .map(
            (w) =>
              `<div class="sharing-row"><div class="sharing-what"><span class="square">${esc(w.name.slice(0, 1))}</span><div><strong>${esc(w.name)}</strong><small>${$t("Shared with you by {owner}", { owner: esc(w.owner) })}</small></div></div><div class="sharing-who"><span class="pill">${role(w.role)}</span></div><div class="sharing-actions"><button class="btn" data-open-workspace="${w.id}">${$t("Open")}</button></div></div>`,
          )
          .join("")}</div>`
      : `<p class="sharing-empty">${$t("No shared workspaces yet. Open a workspace and use the badge at the top right to invite people.")}</p>`
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
    `<div class="sharing-row"><div class="sharing-what"><span class="sharing-icon">${esc(n.icon || "▤")}</span><div><button class="text-link" data-open-note="${n.id}">${esc(n.title)}</button>${n.trashed ? `<small>${$t("In the trash")}</small>` : `<small>${$t("Updated {date}", { date: date(n.updated_at) })}</small>`}</div></div><div class="sharing-who">${people(n.people)}${n.sharing.links ? `<span class="pill">${icon("link")}${$tn(n.sharing.links, "{count} link", "{count} links")}</span>` : ""}${n.sharing.published ? `<a class="pill pill-public" href="${esc(origin)}/${esc(n.sharing.published)}" target="_blank" rel="noopener">${icon("globe")}/${esc(n.sharing.published)}</a>` : ""}</div><div class="sharing-actions">${n.sharing.published ? `<button class="btn" data-copy-url="${esc(origin)}/${esc(n.sharing.published)}" title="${$t("Copy the public address")}">${icon("copy")}${$t("Copy")}</button>` : ""}<button class="btn" data-open-note="${n.id}">${$t("Open")}</button></div></div>`;
  const notesHTML = `<section class="panel sharing-section"><div class="panel-heading"><div><h2>${$t("Notes you share")}</h2><p>${$t("Notes in folders you edit that are shared with people, by link, or published. Open a note and use Share to change or revoke access.")}</p></div><div class="segmented" role="tablist" aria-label="${$t("Show")}">${filters.map(([id, label]) => `<button role="tab" data-sharing-filter="${id}" aria-selected="${id === sharingFilter}" class="${id === sharingFilter ? "active" : ""}">${label}</button>`).join("")}</div></div>${
    notes.length
      ? [...groups]
          .map(
            ([name, list]) =>
              `<h3 class="sharing-group">${esc(name)}</h3><div class="sharing-rows">${list.map(noteRow).join("")}</div>`,
          )
          .join("")
      : `<p class="sharing-empty">${info.managed.length ? $t("Nothing matches this filter.") : $t("You are not sharing any notes yet. Open a note and choose Share.")}</p>`
  }</section>`;

  // Notes shared with you.
  const receivedHTML = `<section class="panel sharing-section"><div class="panel-heading"><div><h2>${$t("Notes shared with you")}</h2><p>${$t("Single notes other members shared with you, outside the workspaces you belong to.")}</p></div></div>${
    info.shared_with_me.length
      ? `<div class="sharing-rows">${info.shared_with_me
          .map(
            (n) =>
              `<div class="sharing-row"><div class="sharing-what"><span class="sharing-icon">${esc(n.icon || "▤")}</span><div><button class="text-link" data-open-note="${n.id}">${esc(n.title)}</button><small>${$t("{value} · updated {date}", { value: esc(n.workspace || ""), date: date(n.updated_at) })}</small></div></div><div class="sharing-who"><span class="pill">${role(n.role)}</span></div><div class="sharing-actions"><button class="btn" data-open-note="${n.id}">${$t("Open")}</button></div></div>`,
          )
          .join("")}</div>`
      : `<p class="sharing-empty">${$t("Nothing has been shared with you yet.")}</p>`
  }</section>`;

  $("#content").innerHTML =
    `<section class="page sharing-page"><div class="eyebrow">${$t("WHO CAN SEE WHAT")}</div><div class="page-heading"><div><h1>${$t("Sharing")}</h1><p class="subtitle">${$t("Across all your workspaces. Public pages without a site are listed at")} <a href="${esc(origin)}/shared/" target="_blank" rel="noopener">${esc(origin)}/shared/</a>.</p></div></div>${statsHTML}${workspacesHTML}${notesHTML}${receivedHTML}${sitesPanel(origin)}</section>`;
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
      toast($t("Address copied"));
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
        b.textContent = $t("Confirm delete");
        return;
      }
      await api(`sites/${b.dataset.deleteSite}`, "DELETE");
      await refresh();
      await renderSharing();
      toast($t("Site deleted"));
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
        `<div class="sharing-row"><div class="sharing-what"><span class="sharing-icon">${icon("globe")}</span><div><a href="${esc(origin)}/${esc(s.path)}/" target="_blank" rel="noopener"><strong>${esc(s.title || s.path)}</strong></a><small>/${esc(s.path)}/ · ${s.layout === "wiki" ? $t("Wiki") : $t("Blog")}${s.theme ? ` · ${esc(publicThemes.find((t) => t.id === s.theme)?.name || s.theme)}` : ""} · ${esc(workspaceName(s.workspace_id))}</small></div></div><div class="sharing-who"><span class="pill">${$tn(s.pages, "{count} page", "{count} pages")}</span></div><div class="sharing-actions">${editable.has(s.workspace_id) ? `<button class="btn" data-edit-site="${s.id}">${$t("Edit")}</button><button class="btn danger" data-delete-site="${s.id}" ${s.pages ? 'disabled title="Move or unpublish its pages first"' : ""}>${$t("Delete")}</button>` : ""}</div></div>`,
    )
    .join("");
  return `<section class="panel sharing-section"><div class="panel-heading"><div><h2>${$t("Public sites")}</h2><p>${$t("Give published pages their own address, like /blog/my-post or /wiki/how-to. People who edit a workspace choose its site when publishing.")}</p></div>${editable.size ? `<button class="btn primary" id="new-site">${icon("plus")}${$t("New site")}</button>` : ""}</div>${rows ? `<div class="sharing-rows">${rows}</div>` : `<p class="sharing-empty">${$t("No sites yet.{value}", { value: editable.size ? $t(" Create one to publish pages under their own path.") : "" })}</p>`}</section>`;
}
function siteDialog(site = null) {
  formDialog(
    site ? $t("Edit site") : $t("New site"),
    $t("Pages published to this site live at /path/page-address. Blogs list pages newest first with a feed; wikis list them alphabetically."),
    `${
      site
        ? ""
        : `<label>${$t("Workspace")}<select name="workspace_id">${editableWorkspaces()
            .map(
              (w) =>
                `<option value="${w.id}" ${w.id === workspaceId ? "selected" : ""}>${esc(w.name)}</option>`,
            )
            .join("")}</select></label>`
    }<label>${$t("Path")}<span class="slug-field">${esc(serverOrigin())}/<input name="path" required maxlength="40" pattern="[a-z0-9]([a-z0-9-]*[a-z0-9])?" title="${$t("Lowercase letters, digits and inner hyphens")}" value="${esc(site?.path || "")}" placeholder="blog" autofocus></span></label><label>${$t("Title")}<input name="title" maxlength="100" value="${esc(site?.title || "")}" placeholder="${$t("My blog")}"></label><label>${$t("Layout")}<select name="layout"><option value="blog" ${site?.layout !== "wiki" ? "selected" : ""}>${$t("Blog: newest first, with a feed")}</option><option value="wiki" ${site?.layout === "wiki" ? "selected" : ""}>${$t("Wiki: alphabetical pages")}</option></select></label><label>${$t("Look")}<select name="theme"><option value="">${$t("Default")}</option>${publicThemes.map((t) => `<option value="${esc(t.id)}" ${site?.theme === t.id ? "selected" : ""}>${esc(t.name)}${t.description ? ` · ${esc(t.description)}` : ""}</option>`).join("")}${site?.theme && !publicThemes.some((t) => t.id === site.theme) ? `<option value="${esc(site.theme)}" selected>${$t("{theme} (not installed)", { theme: esc(site.theme) })}</option>` : ""}</select></label><label>${$t("Description")} <small>${$t("(optional, shown on the index)")}</small><input name="description" maxlength="300" value="${esc(site?.description || "")}"></label>${site?.pages ? `<p class="hint">${$t("Changing the path changes the address of every page in this site.")}</p>` : ""}`,
    async (v) => {
      if (site) await api(`sites/${site.id}`, "PATCH", v);
      else
        await api("sites", "POST", {
          ...v,
          workspace_id: v.workspace_id || workspaceId,
        });
      await refresh();
      await renderSharing();
      toast(site ? $t("Site updated") : $t("Site created"));
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
    ? `<select name="site_id" aria-label="${$t("Site")}"><option value="">shared</option>${sites.map((s) => `<option value="${s.id}" ${pub?.site_id === s.id ? "selected" : ""}>${esc(s.path)}</option>`).join("")}</select>/`
    : '<input type="hidden" name="site_id" value="">shared/';
  const roleSelect = (name, label) =>
    `<select name="${name}" aria-label="${label}"><option value="viewer" selected>${$t("Can view")}</option><option value="editor">${$t("Can edit")}</option></select>`;
  modal.innerHTML = `<h2>${$t("Share “{title}”", { title: esc(note.title) })}</h2><p>${$t("Read-only access is selected by default. Only people who manage this folder can change sharing.")}</p>
  <section class="share-section" aria-labelledby="share-people"><h3 id="share-people">${$t("People on this server")}</h3>
    <form id="share-member" class="share-row"><input name="email" type="email" required placeholder="${$t("name@example.com")}" aria-label="${$t("Member email")}" autocomplete="off">${roleSelect("role", $t("Access for this member"))}<button class="btn primary">${$t("Share")}</button></form>
    <ul class="share-list">${members.map((m) => `<li><span>${esc(m.user?.name || $t("Unavailable account"))} <small>${esc(m.user?.email || "")}</small></span><span class="pill">${roleLabel(m.role)}</span><button class="btn danger" data-revoke-share="${m.id}" aria-label="Remove ${esc(m.user?.name || "member")}">${$t("Remove")}</button></li>`).join("") || `<li class="muted">${$t("Not shared with anyone yet.")}</li>`}</ul>
  </section>
  <section class="share-section" aria-labelledby="share-links"><h3 id="share-links">${$t("Anyone with the link")}</h3>
    <form id="share-link" class="share-row">${roleSelect("role", $t("Access for people with the link"))}<button class="btn">${$t("Create link")}</button></form>
    ${newToken ? `<div class="new-link" role="status"><label>${$t("Copy this link now. It is shown only once.")}<input id="new-link-url" readonly value="${esc(origin)}/share#${esc(newToken)}"></label><button class="btn primary" id="copy-link" type="button">${$t("Copy link")}</button></div>` : ""}
    <ul class="share-list">${links.map((l) => `<li><span>${$t("Link created {date}{value}", { date: date(l.created_at), value: l.created_by ? $t(" by {name}", { name: esc(l.created_by) }) : "" })}</span><span class="pill">${roleLabel(l.role)}</span><button class="btn danger" data-revoke-share="${l.id}">${$t("Revoke")}</button></li>`).join("") || `<li class="muted">${$t("No active links.")}</li>`}</ul>
    <p class="hint">${$t("Anyone holding a link can open this note without signing in until you revoke it. Guests do not see your workspace, folders, or member names.")}</p>
  </section>
  <section class="share-section" aria-labelledby="share-web"><h3 id="share-web">${$t("Publish to the web")}</h3>
    ${pub ? `<p class="public-state">${$t("Public at")} <a href="${esc(origin)}/${esc(pub.path)}" target="_blank" rel="noopener">${esc(origin)}/${esc(pub.path)}</a> · ${pub.mode === "pinned" ? $t("pinned to revision {revision} (current: {current})", { revision: pub.revision, current: note.revision }) : $t("follows live edits")}</p>` : `<p>${$t("Not published. A published page is public, read-only, and omits workspace details and editing controls.")}</p>`}
    <form id="publish-form" class="publish-form">
      <label>${$t("Address")}<span class="slug-field">${esc(origin)}/${siteField}<input name="slug" required maxlength="80" pattern="[a-z0-9]([a-z0-9-]*[a-z0-9])?" title="${$t("Lowercase letters, digits and inner hyphens")}" value="${esc(pub?.slug || slugify(note.title))}"></span></label>
      <label>${$t("Description")} <small>${$t("(optional, shown in listings)")}</small><input name="description" maxlength="300" value="${esc(pub?.description || "")}"></label>
      <fieldset><legend>${$t("When the note changes")}</legend><label class="checkbox-label"><input type="radio" name="mode" value="pinned" ${!pub || pub.mode === "pinned" ? "checked" : ""}>${$t("Keep this published version until I update it")}</label><label class="checkbox-label"><input type="radio" name="mode" value="live" ${pub?.mode === "live" ? "checked" : ""}>${$t("Follow live edits")}</label></fieldset>
      <div class="share-actions"><a class="btn" href="/preview/${esc(targetId)}" target="_blank" rel="noopener">${$t("Preview")}</a>${pub ? `<button type="button" class="btn danger" id="unpublish">${$t("Unpublish")}</button>` : ""}<button class="btn primary" type="submit">${pub ? $t("Update published page") : $t("Publish")}</button></div>
    </form>
  </section>
  <div class="form-error" role="alert"></div>
  <footer><button class="btn" id="close-modal">${$t("Done")}</button></footer>`;
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
      .then(() => announce($t("Shared")))
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
    toast($t("Link copied"));
  });
  $$("[data-revoke-share]", modal).forEach((b) =>
    action(b, "click", async () => {
      await api(`notes/${targetId}/shares/${b.dataset.revokeShare}`, "DELETE");
      await reopen();
      announce($t("Access removed"));
    }),
  );
  $("#publish-form").onsubmit = (e) => {
    e.preventDefault();
    const v = Object.fromEntries(new FormData(e.target));
    api(`notes/${targetId}/publication`, "PUT", { ...v, refresh: true })
      .then(() => reopen())
      .then(() => toast($t("Published page updated")))
      .catch(fail);
  };
  action($("#unpublish"), "click", async (e) => {
    if (!e.target.dataset.armed) {
      e.target.dataset.armed = "1";
      e.target.textContent = $t("Confirm unpublish");
      return;
    }
    await api(`notes/${targetId}/publication`, "DELETE");
    await reopen();
    toast($t("The page is no longer public"));
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
  return `<div class="tree-node" data-node="${f.id}"><div class="tree-row folder-row ${active ? "active" : ""}" style="--depth:${depth}" data-drop-folder="${f.id}" ${editsWorkspace() ? `draggable="true" data-drag-folder="${f.id}"` : ""}><button class="tree-toggle" data-toggle-folder="${f.id}" aria-expanded="${open}" aria-label="${open ? $t("Collapse") : $t("Expand")} ${esc(f.name)}" ${empty ? 'disabled tabindex="-1"' : ""}>${icon("chevron")}</button><button class="tree-label" data-folder="${f.id}" ${active ? 'aria-current="page"' : ""}><span class="tree-icon" aria-hidden="true">${icon(open && !empty ? "folderOpen" : "folder")}</span><span class="tree-name">${esc(f.name)}</span><span class="count">${notes.length || ""}</span></button><span class="tree-actions">${canEdit ? `<button data-row-new="${f.id}" aria-label="New note in ${esc(f.name)}">${icon("plus")}</button>` : ""}<button data-row-menu="folder:${f.id}" aria-label="More actions for ${esc(f.name)}" aria-haspopup="menu">${icon("more")}</button></span></div>${empty ? "" : `<div class="tree-children" role="group" ${open ? "" : "hidden"}>${children.map((c) => folderNode(c, list, depth + 1)).join("")}${notes.map((n) => noteRow(n, depth + 1)).join("")}</div>`}</div>`;
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
    ...(localMode ? [] : [["sharing", "users", $t("Sharing")]]),
    ["archive", "archive", $t("Archived")],
    ["trash", "trash", $t("Trash")],
    ["providers", "users", $t("AI providers")],
    // A desktop app's own server has no other accounts and no network clients.
    ...(localMode
      ? []
      : [
          ["integrations", "canvas", $t("MCP connections")],
          ...(data.user.admin
            ? [["admin", "users", $t("Server administration")]]
            : []),
        ]),
  ];
  return `${sidebarSection(
    "favorites",
    $t("Favorites"),
    favorites.map((n) => noteRow(n, 0)).join("") ||
      `<p class="side-empty">${icon("star")}${$t("Star a note to keep it here.")}</p>`,
  )}${sidebarSection(
    "folders",
    $t("Folders"),
    `<div class="tree" data-drop-root>${
      folderChildren(list, null)
        .map((f) => folderNode(f, list, 0))
        .join("") ||
      `<p class="side-empty">${editsWorkspace() ? $t("Create a folder to get started.") : $t("Your administrator can give you access to this workspace.")}</p>`
    }</div>`,
    editsWorkspace()
      ? `<button class="section-add" id="add-folder" aria-label="${$t("Create folder")}">${icon("plus")}</button>`
      : "",
  )}${
    shared.length
      ? sidebarSection(
          "shared",
          $t("Shared with me"),
          shared.map((n) => noteRow(n, 0)).join(""),
        )
      : ""
  }${
    tags.length
      ? sidebarSection(
          "tags",
          $t("Tags"),
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
    $t("Others"),
    `<nav class="nav" aria-label="${$t("More")}">${others
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
  toast($t("Moved to {value}", { value: data.folders.find((f) => f.id === folder)?.name }));
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
  if (!options) return toast($t("There is no other folder you can move it to."));
  formDialog(
    $t("Move “{title}”", { title: n.title }),
    $t("Choose a folder in this workspace."),
    `<label>${$t("Folder")}<select name="folder_id">${options}</select></label>`,
    (v) => moveNote(n.id, v.folder_id),
    "Move",
  );
}
function createFolder(parent = null) {
  const parentName = parent && data.folders.find((f) => f.id === parent)?.name;
  formDialog(
    parent ? $t("New folder in {name}", { name: parentName }) : $t("A little more organized"),
    $t("Folders keep related notes together."),
    `<label>${$t("Folder name")}<input name="name" placeholder="${$t("e.g. Product ideas")}" required maxlength="200" autofocus></label>`,
    async (v) => {
      const folder = await api("folders", "POST", {
        ...v,
        workspace_id: workspaceId,
        parent_id: parent,
      });
      if (parent) openFolders.add(parent);
      saveSet("minerva.tree.open", openFolders);
      collapsedSections.delete("folders");
      saveSet("minerva.sidebar.collapsed", collapsedSections);
      await refresh();
      shell();
      revealFolder(folder.id);
    },
    "Create folder",
  );
}
function renameFolder(id) {
  const f = data.folders.find((x) => x.id === id);
  formDialog(
    $t("Rename folder"),
    $t("Give this space a useful name."),
    `<label>${$t("Name")}<input name="name" value="${esc(f.name)}" required maxlength="200" autofocus></label>`,
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
              toast($t("Moved to trash"));
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
    notes && `${$tn(notes, "{count} note", "{count} notes")}`,
    subfolders && `${$tn(subfolders, "{count} subfolder", "{count} subfolders")}`,
  ].filter(Boolean);
  formDialog(
    $t("Delete “{name}”?", { name: f.name }),
    contents.length
        ? $t("This permanently deletes the folder and {contents} in it (including trashed notes and databases), with their history, files, shares, and published pages. This cannot be undone.", { contents: contents.join($t(" and ")) })
        : $t("This permanently deletes the folder. This cannot be undone."),
    contents.length
      ? `<label>${$t("Type the folder name to confirm")}<input name="confirm" required autocomplete="off"></label>`
      : "",
    async (v) => {
      if (contents.length && v.confirm !== f.name)
        throw new Error($t("Type the folder name exactly to confirm."));
      await api("folders/" + id, "DELETE");
      if (inside.has(folderId) || inside.has(note?.folder_id)) {
        note = null;
        folderId = null;
        view = "home";
      }
      await refresh();
      shell();
      toast($t("Folder deleted"));
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
      html += `<div class="menu-heading" role="presentation"><small>${$t("Loading servers…")}</small></div>`;
    for (const server of desktopServers || []) {
      html += `<div class="menu-separator" role="separator"></div>${heading(server.label, server.account || (server.signed_in ? "" : "Not signed in"))}`;
      if (server.workspaces)
        html += server.workspaces
          .map(
            (x) =>
              `<div class="menu-row" role="presentation"><button role="menuitemradio" aria-checked="${x.id === workspaceId}" data-remote="${esc(server.id)}|${esc(x.id)}" tabindex="-1">${square(x.name)}<span class="workspace-name">${esc(x.name)}</span>${x.id === workspaceId ? icon("check") : ""}</button><button class="menu-icon" data-offline="${esc(server.id)}|${esc(x.id)}|${x.offline ? "off" : "on"}" tabindex="-1" title="${x.offline ? $t("Remove the offline copy") : $t("Keep an offline copy on this computer")}" aria-label="${x.offline ? $t("Remove offline copy of") : $t("Keep offline copy of")} ${esc(x.name)}">${icon(x.offline ? "check" : "download")}</button></div>`,
          )
          .join("");
      else
        html += `<button role="menuitem" data-remote="${esc(server.id)}|" tabindex="-1"><span class="square new">${icon("arrow")}</span><span class="workspace-name">${server.state === "signed_out" ? $t("Sign in to see its workspaces") : $t("Can’t reach this server · open it")}</span></button>`;
    }
    html += `<div class="menu-separator" role="separator"></div><button role="menuitem" data-workspace="add-server" tabindex="-1"><span class="square new">${icon("plus")}</span><span class="workspace-name">${$t("Add server")}</span></button>`;
  } else {
    html += heading(desktopRemote ? "This server" : location.host);
    html += data.workspaces.map(row).join("");
    if (desktopRemote)
      html += `<div class="menu-separator" role="separator"></div><button role="menuitem" data-workspace="this-computer" tabindex="-1"><span class="square new">${icon("home")}</span><span class="workspace-name">${$t("This computer")}</span></button><button role="menuitem" data-workspace="all-servers" tabindex="-1"><span class="square new">${icon("list")}</span><span class="workspace-name">${$t("All servers…")}</span></button>`;
  }
  html += `<div class="menu-separator" role="separator"></div>`;
  if (data.user.can_create_workspace || data.user.admin || localMode)
    html += `<button role="menuitem" data-workspace="new" tabindex="-1"><span class="square new">${icon("plus")}</span><span class="workspace-name">${$t("Create workspace")}</span></button>`;
  html += `<button role="menuitem" data-workspace="import" tabindex="-1"><span class="square new">${icon("upload")}</span><span class="workspace-name">${$t("Import notes")}</span></button>`;
  if (w?.can_delete)
    html += `<div class="menu-separator" role="separator"></div><button role="menuitem" class="danger-item" data-workspace="delete" tabindex="-1"><span class="square danger">${icon("trash")}</span><span class="workspace-name">${$t("Delete this workspace…")}</span></button>`;
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
  if (!r.ok) throw new Error(v.error || $t("The desktop app could not do that"));
  return v;
}
function addServer() {
  formDialog(
    $t("Add a server"),
    $t("The address of a Minerva server. Sign in on its page; then its workspaces appear in this menu."),
    `<label>${$t("Address")}<input name="url" required placeholder="https://notes.example.com" inputmode="url" spellcheck="false"></label><label>${$t("Name")} <small class="hint">${$t("(optional)")}</small><input name="label" maxlength="80" placeholder="${$t("Team notes")}"></label><label class="checkbox-label"><input type="checkbox" name="allow_insecure_http">${$t("Allow unencrypted HTTP on this address (trusted networks only)")}</label>`,
    async (v) => {
      const server = await desktop("servers", "POST", {
        url: v.url,
        label: v.label,
        allow_insecure_http: v.allow_insecure_http === "on",
      });
      desktopServers = null;
      toast($t("Added {label}", { label: server.label }));
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
            ? $t("Copying to this computer… It appears here in a moment.")
            : $t("Offline copy removed"),
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
// Ctrl+K's filters, kept while the page is open.
const searchFilters = { folder: "", tag: "", since: "" };
/**
 * Ctrl+K: find a note by its title, text or tags, a database record, or a
 * file's text (PDFs and text files), and open it. Filters narrow it to a
 * folder (with its subfolders), a tag, or a recent change.
 */
function search() {
  if (!data) return;
  if (
    searchFilters.folder &&
    !data.folders.some((f) => f.id === searchFilters.folder)
  )
    searchFilters.folder = "";
  const filterSelect = (id, label, options, value) =>
    `<label class="search-filter"><span>${label}</span><select id="${id}">${options.map(([v, text]) => `<option value="${esc(v)}" ${v === value ? "selected" : ""}>${esc(text)}</option>`).join("")}</select></label>`;
  const filters = `<div class="search-filters">${filterSelect(
    "search-folder",
    $t("Folder"),
    [
      ["", $t("All folders")],
      ...folders().map((f) => [
        f.id,
        folderPath(f.id)
          .map((p) => p.name)
          .join(" / "),
      ]),
    ],
    searchFilters.folder,
  )}${filterSelect(
    "search-tag",
    $t("Tag"),
    [["", $t("Any tag")], ...allTags().map(([t]) => [t, "#" + t])],
    searchFilters.tag,
  )}${filterSelect(
    "search-since",
    $t("Updated"),
    [
      ["", $t("Any time")],
      ["7", $t("In the last week")],
      ["30", $t("In the last month")],
      ["365", $t("In the last year")],
    ],
    searchFilters.since,
  )}</div>`;
  modal.innerHTML =
    `<h2>${$t("Find a thought")}</h2><p>${$t("Search titles, text and tags, database records, and the text of attached PDFs.")}</p><input id="search-input" role="combobox" aria-expanded="true" aria-controls="results" aria-autocomplete="list" aria-label="${$t("Search notes")}" placeholder="${$t("Search notes…")}" style="width:100%" autocomplete="off" autofocus>${filters}<div class="search-results" id="results" role="listbox" aria-label="${$t("Notes")}"></div><footer><span class="search-hint"><span><kbd>↑</kbd><kbd>↓</kbd> choose</span><span><kbd>${shortcutKeys("Enter")}</kbd> open</span>` +
    (noteTabs
      ? `<span><kbd>${shortcutKeys("Shift")}</kbd>+<kbd>${shortcutKeys("Enter")}</kbd> ${$t("side by side")}</span>`
      : "") +
    `</span><button class="btn" id="close-modal">${$t("Close")}</button></footer>`;
  modal.showModal();
  const input = $("#search-input");
  let searchVersion = 0,
    matches = [],
    selected = 0;
  const place = (n) =>
    [
      n.folder_id && !data.folders.some((f) => f.id === n.folder_id)
        ? "Shared with you"
        : folderPath(n.folder_id)
            .map((f) => f.name)
            .join(" / "),
      data.workspaces.length > 1 &&
        data.workspaces.find((w) => w.id === workspaceOfNote(n))?.name,
    ]
      .filter(Boolean)
      .join(" · ");
  async function choose(i, split = false) {
    const n = matches[i];
    if (!n) return;
    await flush();
    workspaceId = workspaceOfNote(n) || workspaceId;
    modal.close();
    if (n.kind === "record") {
      const base = await api("databases/" + n.database_id);
      const row = base.rows.find((r) => r.id === n.id);
      if (row) return openRecordPage(base, row);
      return;
    }
    if (n.kind === "file") {
      await openNote(n.note_id, { split });
      return openFile(n.id, n.title);
    }
    await openNote(n.id, { split });
  }
  function select(i) {
    selected = Math.max(0, Math.min(i, matches.length - 1));
    $$("[data-result]").forEach((b, j) =>
      b.setAttribute("aria-selected", j === selected),
    );
    const current = $(`#search-result-${selected}`);
    input.setAttribute("aria-activedescendant", current?.id || "");
    current?.scrollIntoView({ block: "nearest" });
  }
  function draw(heading) {
    $("#results").innerHTML =
      (matches.length ? heading : "") +
      (matches
        .map(
          (n, i) =>
            `${n.kind && n.kind !== matches[i - 1]?.kind && n.kind !== "note" ? `<p class="search-heading">${n.kind === "record" ? $t("Database records") : $t("In attached files")}</p>` : ""}<button class="search-result" id="search-result-${i}" role="option" tabindex="-1" data-result="${i}"><span class="search-icon">${n.kind === "record" ? icon("grid") : n.kind === "file" ? icon("upload") : esc(n.icon || "📄")}</span><span class="search-main"><span class="search-title">${esc(n.title || $t("Untitled"))}</span>${n.snippet ? `<span class="search-snippet">${highlightSnippet(n.snippet, esc)}</span>` : ""}</span><small>${esc(n.kind === "record" ? [n.database_name, place(n)].filter(Boolean).join(" · ") : n.kind === "file" ? [n.note_title, place(n)].filter(Boolean).join(" · ") : place(n))}</small></button>`,
        )
        .join("") || `<p>${$t("No matching notes.")}</p>`);
    $$("[data-result]").forEach((b) =>
      action(b, "click", (e) => choose(+b.dataset.result, e.shiftKey)),
    );
    select(0);
  }
  async function results() {
    const version = ++searchVersion;
    const q = input.value.trim();
    const filtered =
      searchFilters.folder || searchFilters.tag || searchFilters.since;
    // Before typing, without filters: the notes changed most recently.
    if (!q && !filtered) {
      matches = [...data.notes, ...(data.shared_notes || [])]
        .filter((n) => !n.trashed)
        .sort((a, b) => b.updated_at - a.updated_at)
        .slice(0, 8);
      return draw(`<p class="search-heading">${$t("Recently updated")}</p>`);
    }
    const found = await api("search", "POST", {
      query: q,
      include: ["records", "files"],
      ...(searchFilters.folder ? { folder_id: searchFilters.folder } : {}),
      ...(searchFilters.tag ? { tag: searchFilters.tag } : {}),
      ...(searchFilters.since
        ? {
            updated_after:
              Math.floor(Date.now() / 1000) -
              Number(searchFilters.since) * 86400,
          }
        : {}),
    });
    if (version !== searchVersion || !modal.open) return;
    matches = found;
    draw("");
  }
  for (const [id, key] of [
    ["#search-folder", "folder"],
    ["#search-tag", "tag"],
    ["#search-since", "since"],
  ])
    $(id).addEventListener("change", (e) => {
      searchFilters[key] = e.target.value;
      results().catch(error);
      input.focus();
    });
  let timer;
  input.oninput = () => {
    clearTimeout(timer);
    timer = setTimeout(() => results().catch(error), 150);
  };
  input.onkeydown = (e) => {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      select(selected + (e.key === "ArrowDown" ? 1 : -1));
    } else if (e.key === "Enter" && !e.isComposing) {
      e.preventDefault();
      choose(selected, e.shiftKey && !!noteTabs).catch(error);
    }
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
  $("#breadcrumb").textContent = $t("Server administration");
  $("#content").innerHTML =
    `<section class="page"><div class="eyebrow">${$t("YOUR SERVER, YOUR RULES")}</div><div class="page-heading"><div><h1>${$t("People & permissions")}</h1><p class="subtitle">${$t("Give your team the right space to do their best work.")}</p></div><button class="btn primary" id="add-user">${icon("plus")}${$t("Add user")}</button></div><div class="admin-grid"><section class="panel"><div class="panel-heading"><div><h2>${$t("Server members")}</h2><p>${$t("{count} accounts on this server", { count: info.users.length })}</p></div><span class="pill">${$t("Self-hosted")}</span></div><div class="table-wrap"><table class="admin-table"><thead><tr><th>${$t("NAME")}</th><th>${$t("ROLE")}</th><th>${$t("STATUS")}</th><th>${$t("2FA")}</th><th></th></tr></thead><tbody>${info.users.map((u) => `<tr><td>${esc(u.name)}<small>${esc(u.email)}</small></td><td>${u.admin ? $t("Administrator") : $t("Member")}</td><td>${u.admin ? `<span class="pill">${$t("Active")}</span>` : `<select data-status-user="${u.id}" aria-label="Account status for ${esc(u.name)}"><option value="active" ${!u.disabled ? "selected" : ""}>${$t("Active")}</option><option value="disabled" ${u.disabled ? "selected" : ""}>${$t("Disabled")}</option></select>`}</td><td>${u.two_factor ? `<span class="pill pill-on">${$t("On")}</span>${u.id !== data.user.id ? ` <button class="btn" data-reset-2fa="${u.id}" aria-label="Reset two-factor authentication for ${esc(u.name)}">${$t("Reset")}</button>` : ""}` : `<span class="pill">${$t("Off")}</span>`}</td><td class="user-actions">${u.id !== data.user.id ? `<button class="btn" data-edit-user="${u.id}" aria-label="Edit ${esc(u.name)}">${$t("Edit")}</button><button class="btn danger" data-delete-user="${u.id}" aria-label="Delete ${esc(u.name)}">${$t("Delete")}</button>` : `<small>${$t("You")}</small>`}</td></tr>`).join("")}</tbody></table></div></section></div>${storagePanel(storage)}</section>`;
  action($("#add-user"), "click", () =>
    formDialog(
      $t("Welcome someone new"),
      mailEnabled
        ? $t("They get an email with a link to choose their own password. You can also set a temporary one to share with them yourself.")
        : $t("Create an account and share its credentials with your teammate securely."),
      `<label>${$t("Full name")}<input name="name" required maxlength="200" autocomplete="off"></label><label>${$t("Email")}<input type="email" name="email" required autocomplete="off"></label><label>${$t("Temporary password{value}", { value: mailEnabled ? $t(" (optional)") : "" })}<input type="password" name="password" minlength="12" maxlength="256" autocomplete="new-password" ${mailEnabled ? "" : "required"}></label>`,
      async (v) => {
        const created = await api("users", "POST", {
          ...v,
          can_create_workspace: true,
        });
        if (created.invited) toast($t("Invitation emailed to {email}", { email: created.email }));
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
        toast($t("Workspace permission updated"));
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
        $t("Edit {name}", { name: u.name }),
        $t("Changes sign this person out of their open sessions."),
        `<label>${$t("Full name")}<input name="name" required maxlength="200" value="${esc(u.name)}" autocomplete="off"></label><label>${$t("Email")}<input type="email" name="email" required value="${esc(u.email)}" autocomplete="off"></label><label>${$t("New password")} <small class="hint">${$t("(leave empty to keep it)")}</small><input type="password" name="password" minlength="12" maxlength="256" autocomplete="new-password"></label><label class="checkbox-label"><input type="checkbox" name="admin" ${u.admin ? "checked" : ""}>${$t("Server administrator")}</label>`,
        async (v) => {
          const changes = {
            name: v.name,
            email: v.email,
            admin: v.admin === "on",
          };
          if (v.password) changes.password = v.password;
          await api("users/" + u.id, "PATCH", changes);
          toast($t("Account updated"));
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
        $t("Delete {name}?", { name: u.name }),
        $t("{email} can no longer sign in. Their personal workspaces are deleted with them; shared workspaces pass to the member you select below. Their AI conversations, flashcards, and access are deleted too. To keep everything instead, set the account to Disabled. This cannot be undone.", { email: u.email }),
        `<label>${$t("Successor for shared workspaces")}<select name="successor_id"><option value="">${$t("Choose if this user owns shared workspaces")}</option>${info.users
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
              ? $tn(result.workspaces_transferred, "Account deleted · {count} shared workspace transferred", "Account deleted · {count} shared workspaces transferred")
              : $t("Account deleted"),
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
        $t("Reset two-factor authentication?"),
        $t("Use this when someone lost their authenticator app. They will sign in with their password only until they set it up again."),
        "",
        async () => {
          await api("users/" + el.dataset.reset2fa, "PATCH", {
            two_factor: false,
          });
          toast($t("Two-factor authentication was reset"));
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
        toast($t("Account updated"));
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
    toast($t("Workspace access saved"));
  });
  action($("#add-workspace"), "click", () =>
    formDialog(
      $t("Add a workspace"),
      $t("A place to share with others. Add people to it in Workspace access."),
      `<label>${$t("Name")}<input name="name" required maxlength="200" placeholder="${$t("e.g. Research team")}"></label>`,
      async (v) => {
        await api("workspaces", "POST", { name: v.name });
        await refresh();
        await renderAdmin();
        toast($t("Workspace added"));
      },
      "Add workspace",
    ),
  );
  $$("[data-rename-workspace]").forEach((b) =>
    action(b, "click", () => {
      const w = data.workspaces.find((x) => x.id === b.dataset.renameWorkspace);
      formDialog(
        $t("Rename workspace"),
        $t("Everyone with access sees the new name."),
        `<label>${$t("Name")}<input name="name" required maxlength="200" value="${esc(w.name)}"></label>`,
        async (v) => {
          await api("workspaces/" + w.id, "PATCH", { name: v.name });
          await refresh();
          await renderAdmin();
          toast($t("Workspace renamed"));
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
      toast($t("Workspace access removed"));
    }),
  );
  $$("[data-remove-folder-grant]").forEach((b) =>
    action(b, "click", async () => {
      const [folder_id, user_id] = b.dataset.removeFolderGrant.split(":");
      await api("permissions", "PUT", { folder_id, user_id, role: "none" });
      await renderAdmin();
      toast($t("Folder access removed"));
    }),
  );
}
// Every workspace on the server, for administrators to review and delete.
function workspacesPanel(info) {
  const name = (id) =>
    esc(info.users.find((u) => u.id === id)?.name || $t("Unavailable account"));
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
      return `<tr><td>${esc(w.name)}</td><td>${name(w.owner_id)}</td><td>${w.personal ? `<span class="pill">${$t("Personal · private")}</span>` : members.size ? `${$tn(members.size, "{count} member", "{count} members")}` : $t("Not shared yet")}</td><td>${$tn(folderIds.length, "{count} folder", "{count} folders")} · ${$tn(notes, "{count} note", "{count} notes")}</td><td class="user-actions">${w.personal ? "" : `<button class="btn" data-rename-workspace="${w.id}">${$t("Rename")}</button><button class="btn danger" data-delete-workspace="${w.id}">${$t("Delete")}</button>`}</td></tr>`;
    })
    .join("");
  return `<section class="panel"><div class="panel-heading"><div><h2>${$t("Workspaces")}</h2><p>${$t("Workspaces you own. Share them in Workspace access. Your My Workspace is personal and cannot be shared; other people’s workspaces stay private to them.")}</p></div><button class="btn primary" id="add-workspace">${icon("plus")}${$t("Add workspace")}</button></div><div class="table-wrap"><table class="admin-table"><thead><tr><th>${$t("WORKSPACE")}</th><th>${$t("OWNER")}</th><th>${$t("SHARED WITH")}</th><th>${$t("CONTENTS")}</th><th></th></tr></thead><tbody>${rows || `<tr><td colspan="5">${$t("No workspaces yet.")}</td></tr>`}</tbody></table></div></section>`;
}
// Workspace access for members, plus any older folder-level grants.
function accessPanel(info) {
  const name = (id) =>
    esc(info.users.find((u) => u.id === id)?.name || $t("Unavailable account"));
  const workspace = (id) =>
    esc(
      data.workspaces.find((w) => w.id === id)?.name || $t("Unavailable workspace"),
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
            `<tr><td>${name(w.owner_id)}</td><td>${esc(w.name)}</td><td><span class="pill">${$t("Owner")}</span></td><td></td></tr>`,
          ]
        : []),
      ...info.workspace_grants
        .filter((g) => g.workspace_id === w.id)
        .map(
          (g) =>
            `<tr><td>${name(g.user_id)}</td><td>${esc(w.name)}</td><td><span class="pill">${g.role === "editor" ? $t("Can edit") : $t("Can view")}</span></td><td><button class="btn danger" data-remove-access="${g.workspace_id}:${g.user_id}">${$t("Remove")}</button></td></tr>`,
        ),
    ])
    .join("");
  const folderGrants = info.grants
    .map((g) => {
      const f = data.folders.find((x) => x.id === g.folder_id);
      return `<tr><td>${name(g.user_id)}</td><td>${f ? `${workspace(f.workspace_id)} / ${esc(f.name)}` : $t("Unavailable folder")}</td><td><span class="pill">${g.role === "editor" ? $t("Can edit") : $t("Can view")}</span></td><td><button class="btn danger" data-remove-folder-grant="${g.folder_id}:${g.user_id}">${$t("Remove")}</button></td></tr>`;
    })
    .join("");
  return `<section class="panel"><div class="panel-heading"><div><h2>${$t("Workspace access")}</h2><p>${$t("Share the workspaces you own. Nobody, administrators included, sees a workspace until its owner adds them. Workspace members can open every folder and note in it, including folders added later.")}</p></div></div><form class="permission-form" id="workspace-access-form"><label>${$t("Member")}<select name="user_id">${members.map((u) => `<option value="${u.id}">${esc(u.name)}</option>`).join("")}</select></label><label>${$t("Workspace")}<select name="workspace_id">${mine.map((w) => `<option value="${w.id}">${esc(w.name)}</option>`).join("")}</select></label><label>${$t("Permission")}<select name="role"><option value="editor" selected>${$t("Can edit")}</option><option value="viewer">${$t("Can view")}</option><option value="none">${$t("Remove access")}</option></select></label><button class="btn primary" ${!members.length || !mine.length ? "disabled" : ""}>${$t("Save access")}</button></form><div class="table-wrap"><table class="admin-table"><thead><tr><th>${$t("MEMBER")}</th><th>${$t("WORKSPACE")}</th><th>${$t("ACCESS")}</th><th></th></tr></thead><tbody>${rows || `<tr><td colspan="4">${$t("No members have been added to a workspace yet.")}</td></tr>`}</tbody></table></div>${folderGrants ? `<h3 class="subhead">${$t("Folder-level access")}</h3><p class="hint">${$t("Older grants to single folders still apply. Workspace access is simpler to manage; remove these once members have workspace access.")}</p><div class="table-wrap"><table class="admin-table"><thead><tr><th>${$t("MEMBER")}</th><th>${$t("FOLDER")}</th><th>${$t("ACCESS")}</th><th></th></tr></thead><tbody>${folderGrants}</tbody></table></div>` : ""}</section>`;
}
function storagePanel(storage) {
  const percent = Math.min(100, (storage.used / storage.server_quota) * 100);
  return `<section class="panel storage-panel"><div class="panel-heading"><div><h2>${$t("Attachment storage")}</h2><p>${$t("{size} of {size2} in {files} files · {size3} per note · {size4} per file", { size: bytes(storage.used), size2: bytes(storage.server_quota), files: storage.files, size3: bytes(storage.note_quota), size4: bytes(storage.file_limit) })}</p></div></div><div class="usage-bar" role="meter" aria-label="${$t("Attachment storage used")}" aria-valuemin="0" aria-valuemax="100" aria-valuenow="${percent.toFixed(0)}"><span style="width:${percent}%"></span></div><div class="table-wrap"><table class="admin-table"><thead><tr><th>${$t("NOTE")}</th><th>${$t("FILES")}</th><th>${$t("SIZE")}</th></tr></thead><tbody>${
    storage.notes
      .slice(0, 8)
      .map(
        (n) =>
          `<tr><td>${esc(n.title)}${n.trashed ? ` <small>${$t("(in trash)")}</small>` : ""}</td><td>${n.files}</td><td>${bytes(n.bytes)}</td></tr>`,
      )
      .join("") || `<tr><td colspan="3">${$t("No attachments yet.")}</td></tr>`
  }</tbody></table></div><h3>${$t("Unreferenced files")}</h3><p>${$t("Files not referenced by any note, saved version (history), or published page, and older than {value} hours. Deleting them is permanent.{value2}", { value: Math.round(storage.grace_seconds / 3600), value2: storage.recent_unreferenced ? $tn(storage.recent_unreferenced, " {count} newer unreferenced upload is kept so interrupted insertions can be recovered.", " {count} newer unreferenced uploads are kept so interrupted insertions can be recovered.") : "" })}</p><form id="cleanup-form"><div class="table-wrap"><table class="admin-table"><thead><tr><th><input type="checkbox" id="select-orphans" aria-label="${$t("Select all unreferenced files")}"></th><th>${$t("FILE")}</th><th>${$t("NOTE")}</th><th>${$t("UPLOADED")}</th><th>${$t("SIZE")}</th></tr></thead><tbody>${storage.orphans.map((o) => `<tr><td><input type="checkbox" name="orphan" value="${o.id}" aria-label="${$t("Select {name}", { name: esc(o.filename) })}"></td><td>${esc(o.filename)}</td><td>${esc(o.note_title || $t("Unavailable note"))}</td><td>${date(o.created_at)}</td><td>${bytes(o.size)}</td></tr>`).join("") || `<tr><td colspan="5">${$t("Nothing to clean up.")}</td></tr>`}</tbody></table></div>${storage.orphans.length ? `<button class="btn danger" id="delete-orphans" disabled>${$t("Delete selected files")}</button>` : ""}</form></section>`;
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
      `Delete ${$tn(ids.length, "{count} file", "{count} files")}?`,
      $t("This permanently frees {size}. Files that became referenced since this list loaded are kept.", { size: bytes(size) }),
      "",
      async () => {
        const result = await api("admin/storage/cleanup", "POST", { ids });
        toast(
          $tn(result.deleted, "Deleted {count} file ({size}){value}", "Deleted {count} files ({size}){value}", { size: bytes(result.bytes), value: result.kept ? $t(" · kept {count} still in use", { count: result.kept }) : "" }),
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
  dbGroupColumn = null,
  showArchived = false,
  dbTimer = null;
function canEditDatabase() {
  return activeDatabase && data.roles[activeDatabase.folder_id] === "editor";
}
function renderDatabaseList() {
  clearInterval(dbTimer);
  activeDatabase = null;
  $("#breadcrumb").textContent = $t("Databases");
  const ids = new Set(folders().map((f) => f.id));
  const databases = (data.databases || []).filter((d) => ids.has(d.folder_id));
  $("#content").innerHTML =
    `<section class="page"><div class="eyebrow">${$t("A LITTLE STRUCTURE, A LOT OF POSSIBILITY")}</div><div class="page-heading"><div><h1>${$t("Your databases")}</h1><p class="subtitle">${$t("Turn projects, reading lists, and team knowledge into something organized.")}</p></div><button class="btn primary" id="new-database" ${!folderOptions() ? "disabled" : ""}>${icon("plus")}${$t("New database")}</button></div><div class="folder-grid">${databases.map((db) => `<button class="folder-card" data-database="${db.id}"><span class="folder-icon">${icon("grid")}</span><div><strong>${esc(db.name)}</strong><small>${$t("{record_count} records · {value}", { record_count: db.record_count, value: esc(data.folders.find((f) => f.id === db.folder_id)?.name) })}</small></div></button>`).join("")}</div>${!databases.length ? `<div class="empty"><h3>${$t("A home for the details.")}</h3>${$t("Create a database with text, numbers, dates, checkboxes, links, and select properties.")}</div>` : ""}</section>`;
  action($("#new-database"), "click", () =>
    formDialog(
      $t("Create a database"),
      $t("Start with a name and status. Add the properties your work needs."),
      `<label>${$t("Name")}<input name="name" required maxlength="200" placeholder="${$t("e.g. Product roadmap")}"></label><label>${$t("Folder")}<select name="folder_id">${folderOptions()}</select></label>`,
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
  await loadDatabaseContext(activeDatabase);
  dbFilter = "";
  dbSort = null;
  showArchived = false;
  dbView = "table";
  dbGroupColumn = null;
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
        await loadDatabaseContext(fresh);
        activeDatabase = fresh;
        renderDatabase();
      }
    } catch (e) {
      clearInterval(dbTimer);
      error(e);
    }
  }, 2500);
}
// Properties a database can have, in the property dialogs.
const DB_KINDS = [
  ["text", $t("Text")],
  ["number", $t("Number")],
  ["checkbox", $t("Checkbox")],
  ["date", $t("Date")],
  ["select", $t("Select")],
  ["multi_select", $t("Multi-select")],
  ["url", $t("URL")],
  ["person", $t("Person")],
  ["relation", $t("Relation")],
  ["created", $t("Created time")],
  ["edited", $t("Last edited time")],
  ["rollup", $t("Rollup")],
  ["formula", $t("Formula")],
];
// Kept or computed by the server: shown, never written.
const COMPUTED = new Set(["created", "edited", "rollup", "formula"]);
// The people and related databases a database's cells refer to, by its ID.
const dbContext = new Map();
async function loadDatabaseContext(db) {
  const targets = [
    ...new Set(
      db.columns.filter((c) => c.kind === "relation").map((c) => c.target),
    ),
  ];
  const [people, related] = await Promise.all([
    db.columns.some((c) => c.kind === "person")
      ? api(`databases/${db.id}/people`).catch(() => [])
      : [],
    Promise.all(targets.map((id) => api("databases/" + id).catch(() => null))),
  ]);
  dbContext.set(db.id, {
    people,
    targets: new Map(targets.map((id, i) => [id, related[i]])),
  });
}
const contextOf = (db) =>
  dbContext.get(db?.id) || { people: [], targets: new Map() };
/** A database that is on screen, by ID: the open one or one shown in a note. */
function shownDatabase(id) {
  return activeDatabase?.id === id ? activeDatabase : embedCache.get(id)?.db;
}
function dayLabel(at) {
  return at
    ? new Date(at * 1000).toLocaleDateString(undefined, {
        day: "numeric",
        month: "short",
        year: "numeric",
      })
    : "";
}
/** A cell as readable text: names of people and related records, dates. */
function cellText(col, row, db) {
  const v = row.cells[col.id];
  const ctx = contextOf(db);
  switch (col.kind) {
    case "created":
      return dayLabel(row.created_at || row.updated_at);
    case "edited":
      return dayLabel(row.updated_at);
    case "checkbox":
      return v ? "☑" : "☐";
    case "multi_select":
      return (v || []).join(", ");
    case "person":
      return v ? ctx.people.find((p) => p.id === v)?.name || "Someone" : "";
    case "relation":
      return relatedTitles(col, v, db).join(", ");
    case "rollup":
    case "formula":
      return computedText(row.computed?.[col.id], col.format);
    case "number":
      return v == null ? "" : col.format ? formatNumber(v, col.format) : String(v);
    default:
      return v == null ? "" : String(v);
  }
}
function relatedTitles(col, ids, db) {
  const target = contextOf(db).targets.get(col.target);
  return (ids || [])
    .map((id) => {
      const r = target?.rows.find((x) => x.id === id);
      return r ? embedRecordTitle(target, r) : null;
    })
    .filter(Boolean);
}
function pickerChips(col, value, db) {
  const labels =
    col.kind === "relation" ? relatedTitles(col, value, db) : value || [];
  return labels.length
    ? labels.map((l) => `<span class="chip">${esc(l)}</span>`).join("")
    : '<span class="chip-empty">—</span>';
}
function cellInput(col, value, key, disabled = false, { db, row } = {}) {
  if (COMPUTED.has(col.kind))
    return `<span class="cell-computed">${esc(row ? cellText(col, row, db) : "") || "—"}</span>`;
  const attr = `data-cell="${esc(key)}" aria-label="${esc(col.name)}" ${disabled ? "disabled" : ""}`;
  if (col.kind === "checkbox")
    return `<input type="checkbox" ${attr} ${value === true ? "checked" : ""}>`;
  if (col.kind === "select")
    return `<select ${attr}><option value="">—</option>${col.options.map((o) => `<option ${value === o ? "selected" : ""}>${esc(o)}</option>`).join("")}</select>`;
  if (col.kind === "person") {
    const people = contextOf(db).people;
    const unknown =
      value && !people.some((p) => p.id === value)
        ? `<option value="${esc(value)}" selected>${$t("Someone")}</option>`
        : "";
    return `<select ${attr}><option value="">—</option>${unknown}${people.map((p) => `<option value="${esc(p.id)}" ${value === p.id ? "selected" : ""}>${esc(p.name)}</option>`).join("")}</select>`;
  }
  if (col.kind === "multi_select" || col.kind === "relation")
    return `<button type="button" class="cell-picker" ${attr} data-db="${esc(db?.id || "")}" data-value="${esc(JSON.stringify(value || []))}">${pickerChips(col, value, db)}</button>`;
  // A formatted number is typed plainly beside its symbol; a percentage as
  // 25 for 0.25.
  if (col.kind === "number" && col.format) {
    const percent = col.format === "percent";
    const shown =
      value == null || value === ""
        ? ""
        : percent
          ? Math.round(value * 100 * 1e8) / 1e8
          : value;
    const symbol = percent
      ? "%"
      : formatNumber(0, col.format)
          .replace(/[\d\s.,\u00a0\u202f-]/g, "")
          .trim() || col.format.slice(9);
    const input = `<input ${attr} type="number" step="any" value="${esc(shown)}" placeholder="—" title="${esc(value == null ? "" : formatNumber(value, col.format))}">`;
    return `<span class="number-cell ${percent ? "suffix" : "prefix"}"><span class="number-symbol" aria-hidden="true">${esc(symbol)}</span>${input}</span>`;
  }
  return `<input ${attr} type="${{ number: "number", date: "date", url: "url" }[col.kind] || "text"}" ${col.kind === "number" ? 'step="any"' : ""} value="${esc(value ?? "")}" placeholder="—">`;
}
function parseCell(col, el) {
  if (col.kind === "checkbox") return el.checked;
  if (col.kind === "multi_select" || col.kind === "relation") {
    const list = JSON.parse(el.dataset.value || "[]");
    return list.length ? list : null;
  }
  if (el.value === "") return null;
  if (col.kind === "number" && col.format === "percent")
    return Math.round((Number(el.value) / 100) * 1e10) / 1e10;
  return col.kind === "number" ? Number(el.value) : el.value;
}
/** When a cell's editor saves: at once for choices, on leaving for typing. */
function cellEvent(col) {
  return ["checkbox", "select", "person", "multi_select", "relation"].includes(
    col.kind,
  )
    ? "change"
    : "blur";
}
/**
 * Choosing several options, or records of the related database (with search):
 * a small checklist below the cell. Each change updates the cell and saves it
 * like any other cell (`change`).
 */
function openCellPicker(el) {
  $(".cell-picker-pop")?.remove();
  const db = shownDatabase(el.dataset.db);
  const col = db?.columns.find(
    (c) => c.id === el.dataset.cell.split(":").pop(),
  );
  if (!col) return;
  const target = contextOf(db).targets.get(col.target);
  if (col.kind === "relation" && !target)
    return toast($t("The related database cannot be opened."));
  const choices =
    col.kind === "relation"
      ? target.rows
          .filter((r) => !r.archived)
          .map((r) => ({ value: r.id, label: embedRecordTitle(target, r) }))
      : col.options.map((o) => ({ value: o, label: o }));
  const pop = document.createElement("div");
  pop.className = "cell-picker-pop";
  pop.setAttribute("role", "dialog");
  pop.setAttribute("aria-label", col.name);
  pop.innerHTML = `${choices.length > 8 ? '<input type="search" class="cell-picker-search" placeholder="Search…" aria-label="Search">' : ""}<div class="cell-picker-list"></div>`;
  (el.closest("dialog") || document.body).append(pop);
  const r = el.getBoundingClientRect();
  pop.style.left = Math.max(8, Math.min(r.left, innerWidth - 300)) + "px";
  pop.style.top = Math.min(r.bottom + 4, innerHeight - 320) + "px";
  const chosen = () => JSON.parse(el.dataset.value || "[]");
  const draw = (query = "") => {
    const q = query.toLowerCase();
    $(".cell-picker-list", pop).innerHTML =
      choices
        .filter((c) => c.label.toLowerCase().includes(q))
        .slice(0, 200)
        .map(
          (c) =>
            `<label><input type="checkbox" value="${esc(c.value)}" ${chosen().includes(c.value) ? "checked" : ""}>${esc(c.label)}</label>`,
        )
        .join("") || `<p class="hint">${$t("Nothing to choose.")}</p>`;
    $$("input[type=checkbox]", pop).forEach((box) =>
      box.addEventListener("change", () => {
        const now = chosen().filter((v) => v !== box.value);
        if (box.checked) now.push(box.value);
        // Options keep their order; related records the order they were picked.
        const value =
          col.kind === "multi_select"
            ? col.options.filter((o) => now.includes(o))
            : now;
        el.dataset.value = JSON.stringify(value);
        el.innerHTML = pickerChips(col, value, db);
        el.dispatchEvent(new Event("change"));
      }),
    );
  };
  draw();
  $(".cell-picker-search", pop)?.addEventListener("input", (e) =>
    draw(e.target.value),
  );
  ($(".cell-picker-search", pop) || $("input", pop))?.focus();
  const close = (e) => {
    if (e.type === "keydown" && e.key !== "Escape") return;
    if (e.type === "mousedown" && (pop.contains(e.target) || e.target === el))
      return;
    if (e.type === "keydown") e.stopPropagation();
    pop.remove();
    document.removeEventListener("mousedown", close, true);
    document.removeEventListener("keydown", close, true);
    el.focus();
  };
  document.addEventListener("mousedown", close, true);
  document.addEventListener("keydown", close, true);
}
document.addEventListener("click", (e) => {
  const block = e.target.closest?.("[data-comment-block]");
  if (block && note) comments.start(block.dataset.commentBlock);
});
document.addEventListener("click", (e) => {
  const picker = e.target.closest?.(".cell-picker");
  if (picker && !picker.disabled) openCellPicker(picker);
});
/** A property's settings: options for choices, the database for a relation. */
function propertyFields(db, col = {}) {
  const ws = data.folders.find((f) => f.id === db.folder_id)?.workspace_id;
  const related = (data.databases || []).filter(
    (d) => data.folders.find((f) => f.id === d.folder_id)?.workspace_id === ws,
  );
  return `<label>${$t("Name")}<input name="name" required maxlength="100" value="${esc(col.name || "")}" placeholder="${$t("e.g. Due date")}"></label><label>${$t("Type")}<select name="kind" id="property-kind">${DB_KINDS.map(([k, l]) => `<option value="${k}" ${k === (col.kind || "text") ? "selected" : ""}>${l}</option>`).join("")}</select></label><label data-for="select multi_select">${$t("Options (one per line)")}<textarea name="options" placeholder="${$t("Low&#10;Medium&#10;High")}">${esc((col.options || []).join("\n"))}</textarea></label><label data-for="relation">${$t("Links to records of")}<select name="target">${related.map((d) => `<option value="${d.id}" ${d.id === col.target ? "selected" : ""}>${esc(d.name)}</option>`).join("")}</select></label>${rollupFields(db, col)}${formatFields(col)}<label data-for="formula">${$t("Formula")}<textarea name="formula" rows="3" spellcheck="false" placeholder='prop("Price") * prop("Quantity")'>${esc(col.formula || "")}</textarea><small class="hint">${$t("Use properties as")} <code>prop("Name")</code>${$t(", with + − * / %, comparisons,")} <code>and</code>, <code>or</code>, <code>not</code>. Functions: if, concat, join, length, contains, lower, upper, empty, round, floor, ceil, abs, min, max, sum, average, first, last, unique, today, days(from, to), add_days(date, n), year, month. ${$t("A relation's records:")} <code>related("Tasks", "Hours")</code> ${$t("gives the Hours of every linked task, as a list.")}</small></label>`;
}
/** How a number shows: plain, a percentage, or an amount in a currency. */
function formatFields(col) {
  const format = col.format || "";
  const code = format.startsWith("currency:") ? format.slice(9) : "EUR";
  const kind = format.startsWith("currency:") ? "currency" : format;
  return `<label data-for="number rollup formula">${$t("Number format")}<select name="number_format" id="number-format"><option value="" ${kind === "" ? "selected" : ""}>${$t("Number")}</option><option value="percent" ${kind === "percent" ? "selected" : ""}>${$t("Percent (0.25 shows as 25%)")}</option><option value="currency" ${kind === "currency" ? "selected" : ""}>${$t("Currency")}</option></select></label><label data-for="number rollup formula" id="currency-label">${$t("Currency")}<select name="currency">${CURRENCIES.map((c) => `<option ${c === code ? "selected" : ""}>${c}</option>`).join("")}</select></label>`;
}
/** A rollup's settings: the relation it follows, what it computes, of which property. */
function rollupFields(db, col) {
  const relations = db.columns.filter(
    (c) => c.kind === "relation" && c.id !== col.id,
  );
  if (!relations.length)
    return `<p class="hint" data-for="rollup">${$t("A rollup combines the records a relation links to. Add a relation property first.")}</p>`;
  const spec = col.rollup || {};
  const properties = relations.flatMap((r) =>
    (contextOf(db).targets.get(r.target)?.columns || [])
      // Rollups and formulas there too: a rollup of a rollup.
      .filter((p) => p.kind !== "relation")
      .map(
        (p) =>
          `<option value="${esc(p.id)}" data-relation="${esc(r.id)}" ${spec.relation === r.id && spec.property === p.id ? "selected" : ""}>${esc(p.name)}</option>`,
      ),
  );
  return `<label data-for="rollup">${$t("Through")}<select name="rollup_relation" id="rollup-relation">${relations.map((r) => `<option value="${esc(r.id)}" ${spec.relation === r.id ? "selected" : ""}>${esc(r.name)}</option>`).join("")}</select></label><label data-for="rollup">${$t("Calculate")}<select name="rollup_function" id="rollup-function">${ROLLUP_FUNCTIONS.map(([f, label]) => `<option value="${f}" ${(spec.function || "count") === f ? "selected" : ""}>${label}</option>`).join("")}</select></label><label data-for="rollup" id="rollup-property-label">${$t("Of")}<select name="rollup_property" id="rollup-property"><option value="">—</option>${properties.join("")}</select></label>`;
}
function bindPropertyFields() {
  const kind = $("#property-kind", modal);
  const relation = $("#rollup-relation", modal),
    fn = $("#rollup-function", modal),
    property = $("#rollup-property", modal);
  const show = () => {
    $$("[data-for]", modal).forEach(
      (el) => (el.hidden = !el.dataset.for.split(" ").includes(kind.value)),
    );
    const currency = $("#currency-label", modal);
    if (currency && !currency.hidden)
      currency.hidden = $("#number-format", modal).value !== "currency";
    if (!relation) return;
    // Only the related database's properties, and none for a plain count.
    $$("option[data-relation]", property).forEach(
      (o) => (o.hidden = o.dataset.relation !== relation.value),
    );
    if (property.selectedOptions[0]?.hidden) property.value = "";
    $("#rollup-property-label", modal).hidden =
      kind.value !== "rollup" || fn.value === "count";
  };
  for (const el of [kind, relation, fn, $("#number-format", modal)])
    el?.addEventListener("change", show);
  show();
}
/** The column a property dialog describes. */
function propertyColumn(v, base = {}) {
  const options = ["select", "multi_select"].includes(v.kind)
    ? v.options
        .split("\n")
        .map((x) => x.trim())
        .filter(Boolean)
    : [];
  if (["select", "multi_select"].includes(v.kind) && !options.length)
    throw new Error($t("Add at least one option."));
  if (v.kind === "relation" && !v.target)
    throw new Error($t("Choose the database this property links to."));
  const col = {
    id: base.id || crypto.randomUUID(),
    name: v.name.trim(),
    kind: v.kind,
    options,
  };
  if (v.kind === "relation") col.target = v.target;
  if (v.kind === "rollup") {
    if (!v.rollup_relation)
      throw new Error($t("Add a relation property first: a rollup combines the records it links to."));
    if (v.rollup_function !== "count" && !v.rollup_property)
      throw new Error($t("Choose the property to combine."));
    col.rollup = {
      relation: v.rollup_relation,
      property: v.rollup_function === "count" ? "" : v.rollup_property,
      function: v.rollup_function,
    };
  }
  if (v.kind === "formula") {
    if (!v.formula?.trim()) throw new Error($t("Write the formula."));
    col.formula = v.formula.trim();
  }
  if (["number", "rollup", "formula"].includes(v.kind) && v.number_format)
    col.format =
      v.number_format === "currency"
        ? `currency:${v.currency || "EUR"}`
        : v.number_format;
  return col;
}
// Calendar view on the database page: the month shown, the date property it
// places records by, and whether reminders and journal entries show too.
// Embedded calendars keep the same on their slot (`calendarState`).
let dbMonth = null,
  dbCalendarColumn = null,
  dbCalendarExtras = false;
function isoDay(at) {
  const d = new Date(at * 1000);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}
const DATED = ["date", "created", "edited"];
// Reminders and journal entries of the workspace, kept for a minute.
let extrasCache = null;
async function loadCalendarExtras() {
  if (extrasCache && Date.now() - extrasCache.at < 60000) return;
  const all = await api("reminders").catch(() => ({
    reminders: [],
    from_notes: [],
  }));
  const local = (at) => {
    const d = new Date(at * 1000);
    return `${isoDay(at)} ${String(d.getHours()).padStart(2, "0")}:${String(d.getMinutes()).padStart(2, "0")}`;
  };
  extrasCache = {
    at: Date.now(),
    reminders: [
      ...(all.reminders || []).map((r) => ({
        stamp: local(r.at),
        text: r.title,
        note_id: r.note_id || null,
        workspace_id: r.workspace_id,
      })),
      ...(all.from_notes || []),
    ],
  };
}
function calendarItems(db, month) {
  const ws = data.folders.find((f) => f.id === db.folder_id)?.workspace_id;
  return calendarExtras(month, {
    reminders: (extrasCache?.reminders || []).filter(
      (r) => !r.workspace_id || r.workspace_id === ws,
    ),
    journal: journalNotes(ws),
  });
}
/** `state`: `{month, column, extras}`. */
function calendarHTML(db, rows, edit, state) {
  const cols = db.columns.filter((c) => DATED.includes(c.kind));
  const col = cols.find((c) => c.id === state.column) || cols[0];
  const today = isoDay(Date.now() / 1000);
  const month = state.month || today.slice(0, 7);
  const [y, m] = month.split("-").map(Number);
  const first = new Date(y, m - 1, 1);
  const offset = (first.getDay() + 6) % 7;
  const days = new Date(y, m, 0).getDate();
  const dateOf = (r) =>
    col.kind === "date"
      ? r.cells[col.id]
      : isoDay(
          col.kind === "created" ? r.created_at || r.updated_at : r.updated_at,
        );
  const byDay = new Map();
  let undated = 0;
  for (const r of rows) {
    const day = dateOf(r);
    if (!day) undated++;
    else byDay.set(day, [...(byDay.get(day) || []), r]);
  }
  const extras = state.extras ? calendarItems(db, month) : new Map();
  const weekdays = Array.from({ length: 7 }, (_, i) =>
    new Date(2024, 0, 1 + i).toLocaleDateString(undefined, {
      weekday: "short",
    }),
  );
  const cells = [];
  for (let i = 0; i < offset; i++)
    cells.push('<div class="cal-day blank"></div>');
  for (let d = 1; d <= days; d++) {
    const iso = `${month}-${String(d).padStart(2, "0")}`;
    const records = byDay.get(iso) || [];
    const more = extras.get(iso) || [];
    cells.push(
      `<div class="cal-day ${iso === today ? "today" : ""}" data-day="${iso}"><span class="cal-num">${d}</span>${records
        .slice(0, 6)
        .map(
          (r) =>
            `<button class="cal-record" data-edit-record="${r.id}" title="${esc(embedRecordTitle(db, r))}">${esc(embedRecordTitle(db, r))}</button>`,
        )
        .join(
          "",
        )}${records.length > 6 ? `<small>${$t("+{value} more", { value: records.length - 6 })}</small>` : ""}${more
        .map(
          (x) =>
            `<button class="cal-extra cal-${x.kind}" ${x.note_id ? `data-cal-note="${esc(x.note_id)}"` : "disabled"} title="${esc(x.label)}">${x.kind === "reminder" ? "⏰" : "📓"} ${esc(x.label)}</button>`,
        )
        .join(
          "",
        )}${edit && col.kind === "date" ? `<button class="cal-add" data-cal-add="${iso}" aria-label="Add a record on ${iso}">+</button>` : ""}</div>`,
    );
  }
  return `<div class="panel calendar"><div class="cal-head"><div class="toolbar-group"><button class="btn" data-cal-month="-1" aria-label="${$t("Previous month")}">‹</button><button class="btn" data-cal-month="0">${$t("Today")}</button><button class="btn" data-cal-month="1" aria-label="${$t("Next month")}">›</button></div><h2>${esc(first.toLocaleDateString(undefined, { month: "long", year: "numeric" }))}</h2>${
    cols.length > 1
      ? `<label class="cal-by">${$t("By")}<select data-cal-column>${cols.map((c) => `<option value="${c.id}" ${c.id === col.id ? "selected" : ""}>${esc(c.name)}</option>`).join("")}</select></label>`
      : `<span class="cal-by">By ${esc(col.name)}</span>`
  }<label class="checkbox-label cal-extras"><input type="checkbox" data-cal-extras ${state.extras ? "checked" : ""}>${$t("Reminders &amp; journal")}</label></div><div class="cal-grid">${weekdays.map((w) => `<div class="cal-weekday">${esc(w)}</div>`).join("")}${cells.join("")}</div>${undated ? `<p class="hint">${$tn(undated, "{count} record without {name}.", "{count} records without {name}.", { name: esc(col.name) })}</p>` : ""}</div>`;
}
/**
 * Calendar controls inside `root`: `state` is changed in place, then `redraw`.
 * `onAdded(row)` follows a record added on a day.
 */
function bindCalendar(root, db, state, redraw, onAdded) {
  $$("[data-cal-month]", root).forEach((el) =>
    action(el, "click", () => {
      const step = Number(el.dataset.calMonth);
      const [y, m] = (state.month || isoDay(Date.now() / 1000).slice(0, 7))
        .split("-")
        .map(Number);
      const d = step ? new Date(y, m - 1 + step, 1) : new Date();
      state.month = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}`;
      redraw();
    }),
  );
  const column = $("[data-cal-column]", root);
  if (column)
    action(column, "change", () => {
      state.column = column.value;
      redraw();
    });
  action($("[data-cal-extras]", root), "change", async (e) => {
    state.extras = e.target.checked;
    if (state.extras) await loadCalendarExtras();
    redraw();
  });
  $$("[data-cal-note]", root).forEach((el) =>
    action(el, "click", () => openNote(el.dataset.calNote)),
  );
  $$("[data-cal-add]", root).forEach((el) =>
    action(el, "click", async () => {
      const col =
        db.columns.find((c) => c.id === state.column && c.kind === "date") ||
        db.columns.find((c) => c.kind === "date");
      const cells = { [col.id]: el.dataset.calAdd };
      for (const c of db.columns)
        if (c.kind === "checkbox") cells[c.id] = false;
      if (db.columns[0].kind === "text") cells[db.columns[0].id] = "Untitled";
      const row = await api(`databases/${db.id}/rows`, "POST", { cells });
      db.rows.push(row);
      redraw();
      onAdded(row);
    }),
  );
}
/** The property a board groups by: the chosen one, else the first that can. */
function boardColumn(db, wanted) {
  const cols = db.columns.filter((c) => GROUPABLE.includes(c.kind));
  return cols.find((c) => c.id === wanted) || cols[0] || null;
}
/** Board columns of `rows` by `col`; cards can be dragged between columns. */
function boardHTML(db, rows, col, { edit, embed = false }) {
  const groups = boardGroups(col, rows, contextOf(db).people, db.rows);
  const [wrap, column, heading, card, open] = embed
    ? ["db-embed-board", "db-embed-column", "h4", "db-embed-card", "data-embed-record"]
    : ["board", "board-column", "h3", "record-card", "data-edit-record"];
  return `<div class="${wrap}">${groups
    .map(
      (g) =>
        `<section class="${column}" data-drop-group="${esc(g.key)}"><${heading}>${esc(g.label || "No " + col.name.toLowerCase())}<span>${g.rows.length}</span></${heading}>${g.rows
          .map(
            (r) =>
              `<button class="${card}" ${open}="${r.id}" ${edit ? `draggable="true" data-drag-record="${r.id}" data-from="${esc(g.key)}"` : ""}><strong>${esc(embedRecordTitle(db, r))}</strong>${db.columns
                .slice(1)
                .filter((c) => c.id !== col.id && cellText(c, r, db) !== "")
                .map(
                  (c) =>
                    `<small>${esc(c.name)}: ${esc(cellText(c, r, db))}</small>`,
                )
                .join("")}</button>`,
          )
          .join("")}</section>`,
    )
    .join("")}</div>`;
}
/** Dragging cards between board columns changes the grouped property. */
function bindBoardDrag(root, db, col, redraw) {
  let dragged = null;
  $$("[data-drag-record]", root).forEach((el) =>
    el.addEventListener("dragstart", (e) => {
      dragged = { id: el.dataset.dragRecord, from: el.dataset.from };
      e.dataTransfer.setData("text/plain", el.dataset.dragRecord);
      e.dataTransfer.effectAllowed = "move";
    }),
  );
  $$("[data-drop-group]", root).forEach((zone) => {
    zone.addEventListener("dragover", (e) => {
      if (!dragged) return;
      e.preventDefault();
      zone.classList.add("drop-target");
    });
    zone.addEventListener("dragleave", () =>
      zone.classList.remove("drop-target"),
    );
    zone.addEventListener("drop", async (e) => {
      e.preventDefault();
      zone.classList.remove("drop-target");
      const move = dragged;
      dragged = null;
      if (!move) return;
      const row = db.rows.find((r) => r.id === move.id);
      // The card goes before the first card whose middle is below the pointer.
      const cards = $$("[data-drag-record]", zone).filter(
        (c) => c.dataset.dragRecord !== move.id,
      );
      const at = cards.findIndex((c) => {
        const box = c.getBoundingClientRect();
        return e.clientY < box.top + box.height / 2;
      });
      const key = boardKey(db.rows);
      const rowOf = (c) =>
        c && db.rows.find((r) => r.id === c.dataset.dragRecord);
      const before = rowOf(at < 0 ? cards.at(-1) : cards[at - 1]);
      const after = rowOf(at < 0 ? null : cards[at]);
      const rank = rankBetween(
        before ? key(before) : null,
        after ? key(after) : null,
      );
      const moved = move.from !== zone.dataset.dropGroup;
      // Dropped where it already was: nothing to save.
      const all = $$("[data-drag-record]", zone);
      const here = all.findIndex((c) => c.dataset.dragRecord === move.id);
      if (
        !moved &&
        here >= 0 &&
        before === rowOf(all[here - 1]) &&
        after === rowOf(all[here + 1])
      )
        return;
      try {
        Object.assign(
          row,
          await api(`databases/${db.id}/rows/${row.id}`, "PATCH", {
            revision: row.revision,
            rank,
            ...(moved
              ? {
                  cells: moveToGroup(
                    col,
                    row,
                    move.from,
                    zone.dataset.dropGroup,
                  ),
                }
              : {}),
          }),
        );
        embedCache.set(db.id, { at: Date.now(), db });
      } catch (err) {
        error(err);
      }
      redraw();
    });
  });
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
        db.columns.some((c) =>
          cellText(c, r, db).toLowerCase().includes(dbFilter.toLowerCase()),
        ),
    );
  // Sorts by what a cell holds: times for created/edited, names for people and
  // related records.
  const sortColumn = db.columns.find((c) => c.id === dbSort);
  const sortValue = (r) =>
    sortColumn.kind === "created"
      ? r.created_at || r.updated_at
      : sortColumn.kind === "edited"
        ? r.updated_at
        : ["person", "relation", "multi_select"].includes(sortColumn.kind)
          ? cellText(sortColumn, r, db)
          : ["rollup", "formula"].includes(sortColumn.kind)
            ? (r.computed?.[dbSort] ?? "")
            : (r.cells[dbSort] ?? "");
  if (sortColumn)
    rows.sort((a, b) => {
      const av = sortValue(a),
        bv = sortValue(b);
      return (
        (typeof av === "number" && typeof bv === "number"
          ? av - bv
          : String(av).localeCompare(String(bv), undefined, {
              numeric: true,
            })) * (dbDescending ? -1 : 1)
      );
    });
  const group = boardColumn(db, dbGroupColumn);
  const groupable = db.columns.filter((c) => GROUPABLE.includes(c.kind));
  const dated = db.columns.some((c) => DATED.includes(c.kind));
  const calendar = {
    month: dbMonth,
    column: dbCalendarColumn,
    extras: dbCalendarExtras,
  };
  if ((dbView === "calendar" && !dated) || (dbView === "board" && !group))
    dbView = "table";
  $("#content").innerHTML =
    `<section class="page database-page"><div class="eyebrow">${$t("STRUCTURED KNOWLEDGE")}</div><div class="page-heading"><div><h1>${esc(db.name)}</h1><p class="subtitle">${$t("{value} · {count} records · {value2}", { value: esc(data.folders.find((f) => f.id === db.folder_id)?.name), count: rows.length, value2: edit ? $t("Can edit") : $t("Read only") })}</p></div><div class="toolbar-group"><button class="btn" id="add-property" ${!edit ? "disabled" : ""}>${icon("plus")}${$t("Property")}</button><button class="btn primary" id="add-record" ${!edit ? "disabled" : ""}>${icon("plus")}${$t("New record")}</button></div></div><div class="db-toolbar"><div class="toolbar-group"><button class="btn ${dbView === "table" ? "selected" : ""}" id="table-view">${icon("list")}${$t("Table")}</button><button class="btn ${dbView === "board" ? "selected" : ""}" id="board-view" ${!group ? "disabled" : ""}>${icon("grid")}${$t("Board")}</button><button class="btn ${dbView === "calendar" ? "selected" : ""}" id="calendar-view" ${!dated ? "disabled" : ""} title="${dated ? $t("Records by date") : $t("Add a date property to see records on a calendar")}">${icon("calendar")}${$t("Calendar")}</button></div>${dbView === "board" && groupable.length > 1 ? `<label class="cal-by">${$t("Group by")}<select id="board-column">${groupable.map((c) => `<option value="${c.id}" ${c.id === group.id ? "selected" : ""}>${esc(c.name)}</option>`).join("")}</select></label>` : ""}<input id="db-filter" aria-label="${$t("Filter records")}" placeholder="${$t("Filter records…")}" value="${esc(dbFilter)}"><label class="checkbox-label"><input id="show-archived" type="checkbox" ${showArchived ? "checked" : ""}>${$t("Archived")}</label><button class="btn" id="db-reload">${$t("Reload records")}</button><button class="btn" id="db-rename" ${!edit ? "disabled" : ""}>${$t("Rename")}</button><button class="btn" id="db-embed" title="${$t("Copy the code that shows this database inside a note")}">${icon("note")}${$t("Use in a note")}</button><button class="btn danger" id="db-delete" ${!edit ? "disabled" : ""}>${icon("trash")}${$t("Delete")}</button></div>${
      dbView === "calendar"
        ? calendarHTML(db, rows, edit, calendar)
        : dbView === "table"
          ? `<div class="panel table-wrap"><table class="records-table"><thead><tr>${db.columns.map((c) => `<th><span class="th-inner"><button data-sort="${c.id}">${esc(c.name)} <small>${esc((DB_KINDS.find((k) => k[0] === c.kind)?.[1] || c.kind).toLowerCase())}</small>${dbSort === c.id ? (dbDescending ? " ↓" : " ↑") : ""}</button><button class="th-menu" data-col-menu="${c.id}" aria-label="Options for ${esc(c.name)}" aria-haspopup="menu">${icon("more")}</button></span></th>`).join("")}<th>${$t("Actions")}</th></tr></thead><tbody>${rows.map((r) => `<tr data-row="${r.id}">${db.columns.map((c) => `<td>${cellInput(c, r.cells[c.id], r.id + ":" + c.id, !edit, { db, row: r })}</td>`).join("")}<td class="record-actions"><button class="btn" data-record-page="${r.id}" ${!edit && !r.page ? "disabled" : ""} title="${r.page ? $t("Open the record's page") : $t("Open as a page, with room for notes")}" aria-label="${$t("Open as page")}">${icon("note")}</button><button class="btn" data-archive="${r.id}" ${!edit ? "disabled" : ""}>${showArchived ? $t("Restore") : $t("Archive")}</button><button class="btn danger" data-delete-record="${r.id}" ${!edit ? "disabled" : ""} aria-label="${$t("Delete record")}">${icon("trash")}</button></td></tr>`).join("")}</tbody></table>${!rows.length ? `<div class="empty">${$t("No matching records. Add a record or change your filter.")}</div>` : ""}</div>`
          : boardHTML(db, rows, group, { edit })
    }<p class="editor-help">${$t("Changes save when you leave a cell. Other records refresh automatically. Concurrent edits to the same record are checked before saving.")}</p></section>`;
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
  action($("#calendar-view"), "click", () => {
    dbView = "calendar";
    renderDatabase();
  });
  if (dbView === "calendar")
    bindCalendar(
      $("#content"),
      db,
      calendar,
      () => {
        ({ month: dbMonth, column: dbCalendarColumn, extras: dbCalendarExtras } =
          calendar);
        renderDatabase();
      },
      (row) => editRecordDialog(db, row, true, renderDatabase),
    );
  if (dbView === "board" && edit)
    bindBoardDrag($("#content"), db, group, renderDatabase);
  action($("#board-column"), "change", (e) => {
    dbGroupColumn = e.target.value;
    renderDatabase();
  });
  action($("#db-reload"), "click", () =>
    formDialog(
      $t("Reload records?"),
      dbDrafts.size
        ? $t("Unsaved cell values will be discarded. Copy any value you need before continuing.")
        : $t("Fetch the latest records from the server."),
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
  // Only the page's own cells: a record dialog may still be open over it.
  $$("[data-cell]", $("#content")).forEach((el) => {
    const [rid, cid] = el.dataset.cell.split(":");
    const col = db.columns.find((c) => c.id === cid);
    el.addEventListener("input", () =>
      dbDrafts.set(el.dataset.cell, parseCell(col, el)),
    );
    const event = cellEvent(col);
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
            toast($t("Record saved"));
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
  $$("[data-record-page]").forEach((el) =>
    action(el, "click", () =>
      openRecordPage(
        db,
        db.rows.find((r) => r.id === el.dataset.recordPage),
      ),
    ),
  );
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
      $t("Rename database"),
      $t("A useful name makes information easier to find."),
      `<label>${$t("Name")}<input name="name" required maxlength="200" value="${esc(db.name)}"></label>`,
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
  action($("#add-property"), "click", () => {
    formDialog(
      $t("Add a property"),
      $t("Choose the type of information this column will hold."),
      propertyFields(db),
      async (v) => {
        activeDatabase = await api("databases/" + db.id, "PATCH", {
          revision: db.revision,
          columns: [...db.columns, propertyColumn(v)],
        });
        await loadDatabaseContext(activeDatabase);
        renderDatabase();
      },
      "Add property",
    );
    bindPropertyFields();
  });
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
        $t("Delete this record?"),
        $t("“{title}” is removed permanently. Archive it instead to keep it out of the way.", { title: embedRecordTitle(db, row) }),
        "",
        async () => {
          await api(`databases/${db.id}/rows/${row.id}`, "DELETE");
          db.rows = db.rows.filter((r) => r.id !== row.id);
          renderDatabase();
          toast($t("Record deleted"));
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
      toast($t("Copied. Paste it into a note, or use / → Database in the editor."));
    } catch {
      formDialog(
        $t("Use in a note"),
        $t("Paste this into any note, or type / in the editor and choose Database."),
        `<label>${$t("Embed code")}<input readonly value="${esc(code)}" onfocus="this.select()"></label>`,
        async () => {},
        "Done",
      );
    }
  });
  action($("#db-delete"), "click", () =>
    formDialog(
      $t("Delete “{name}”?", { name: db.name }),
      $tn(db.rows.length, "The database and its {count} record are removed permanently. Notes that show it will say it was deleted. This cannot be undone.", "The database and its {count} records are removed permanently. Notes that show it will say it was deleted. This cannot be undone."),
      `<label>${$t("Type the database name to confirm")}<input name="confirm" required autocomplete="off"></label>`,
      async (v) => {
        if (v.confirm !== db.name)
          throw new Error($t("Type the database name exactly to confirm."));
        await api("databases/" + db.id, "DELETE");
        embedCache.delete(db.id);
        await refresh();
        renderDatabaseList();
        toast($t("Database deleted"));
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
    await loadDatabaseContext(activeDatabase);
    renderDatabase();
    if (message) toast(message);
  };
  const sort = (descending) => {
    dbSort = col.id;
    dbDescending = descending;
    renderDatabase();
  };
  rowMenu(anchor, [
    { icon: "arrow", label: "Sort ascending", run: () => sort(false) },
    { icon: "arrow", label: "Sort descending", run: () => sort(true) },
    ...(edit
      ? [
          "-",
          {
            icon: "edit",
            label: "Edit property…",
            run: () => {
              formDialog(
                $t("Edit property"),
                $t("Changing the type or removing an option clears values that no longer fit."),
                propertyFields(db, col),
                (v) =>
                  save(
                    db.columns.map((c) =>
                      c.id === col.id ? propertyColumn(v, col) : c,
                    ),
                    "Property updated",
                  ),
                "Save",
              );
              bindPropertyFields();
            },
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
                $t("Delete “{name}”?", { name: col.name }),
                used
                  ? $tn(used, "{count} record has a value for this property. They are removed permanently.", "{count} records have values for this property. They are removed permanently.")
                  : $t("No record has a value for this property yet."),
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
    if (entry.db.columns.some((c) => ["person", "relation"].includes(c.kind)))
      await loadDatabaseContext(entry.db);
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
  const chosen = slot.dataset.column || null;
  const group = boardColumn(
    db,
    slot.dataset.view === "board" ? chosen : null,
  );
  const groupable = db.columns.filter((c) => GROUPABLE.includes(c.kind));
  const dated = db.columns.some((c) => DATED.includes(c.kind));
  let view = slot.dataset.view;
  if ((view === "board" && !group) || (view === "calendar" && !dated))
    view = "table";
  slot.calendar ||= {
    month: null,
    column: view === "calendar" ? chosen : null,
    extras: false,
  };
  const body =
    view === "board"
      ? `${groupable.length > 1 ? `<label class="cal-by db-embed-group">${$t("Group by")}<select data-embed-group>${groupable.map((c) => `<option value="${c.id}" ${c.id === group.id ? "selected" : ""}>${esc(c.name)}</option>`).join("")}</select></label>` : ""}${boardHTML(db, rows, group, { edit, embed: true })}`
      : view === "calendar"
        ? calendarHTML(db, rows, edit, slot.calendar)
        : `<div class="db-embed-scroll"><table class="db-embed-table"><thead><tr>${db.columns.map((c) => `<th>${esc(c.name)}</th>`).join("")}</tr></thead><tbody>${rows
            .map(
              (r) =>
                `<tr>${db.columns.map((c) => `<td>${cellInput(c, r.cells[c.id], r.id + ":" + c.id, !edit, { db, row: r })}</td>`).join("")}</tr>`,
            )
            .join(
              "",
            )}</tbody></table>${rows.length ? "" : `<p class="db-embed-empty">${$t("No records yet.")}</p>`}</div>`;
  const viewButton = (name, label, enabled, why) =>
    `<button data-embed-view="${name}" aria-pressed="${view === name}" ${enabled ? "" : `disabled title="${why}"`}>${label}</button>`;
  slot.innerHTML = `<span class="db-embed-panel" role="group" aria-label="Database ${esc(db.name)}"><span class="db-embed-head"><span class="db-embed-title">${icon("grid")}${esc(db.name)}</span><span class="db-embed-meta">${$tn(rows.length, "{count} record", "{count} records")}${edit ? "" : $t(" · read only")}</span><span class="db-embed-actions"><span class="db-embed-views" role="group" aria-label="${$t("View")}">${viewButton("table", $t("Table"), true)}${viewButton("board", $t("Board"), group, $t("Board view needs a select, multi-select or person property"))}${viewButton("calendar", $t("Calendar"), dated, $t("Calendar view needs a date property"))}</span><button data-embed-open title="${$t("Open the database")}">${icon("arrow")}${$t("Open")}</button>${edit ? `<button class="primary" data-embed-add>${icon("plus")}${$t("New")}</button>` : ""}</span></span>${body}</span>`;
  if (view === "board" && edit)
    bindBoardDrag(slot, db, group, () => redrawEmbeds(db.id));
  if (view === "calendar")
    bindCalendar(
      slot,
      db,
      slot.calendar,
      () => {
        // A different date property is kept in the note.
        if ((slot.calendar.column || "") !== slot.dataset.column)
          return setEmbedView(slot, "calendar", slot.calendar.column).catch(
            error,
          );
        drawEmbed(slot, embedCache.get(db.id) || { db });
      },
      (row) => editRecordDialog(db, row, true, () => redrawEmbeds(db.id)),
    );
  const groupBy = $("[data-embed-group]", slot);
  if (groupBy)
    groupBy.addEventListener("change", () =>
      setEmbedView(slot, "board", groupBy.value).catch(error),
    );
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
  $$("[data-embed-record], [data-edit-record]", slot).forEach((b) =>
    b.addEventListener("click", () => {
      const row = db.rows.find(
        (r) => r.id === (b.dataset.embedRecord || b.dataset.editRecord),
      );
      editRecordDialog(db, row, edit, () => redrawEmbeds(db.id));
    }),
  );
  $$("[data-cell]", slot).forEach((el) => {
    const [rid, cid] = el.dataset.cell.split(":");
    const col = db.columns.find((c) => c.id === cid);
    const event = cellEvent(col);
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
/**
 * Shows an embed as `view`, grouped or placed by `column` (a board's or a
 * calendar's property), and keeps both in the note's `[[database:…]]` code.
 */
async function setEmbedView(slot, view, column = null) {
  // A view of its own starts from its own property.
  if (slot.dataset.view !== view) slot.calendar = null;
  slot.dataset.view = view;
  slot.dataset.column = view === "table" ? "" : column || "";
  const entry = embedCache.get(slot.dataset.database);
  if (entry) drawEmbed(slot, entry);
  const blockId = slot.closest(".block")?.dataset.block;
  const i = note?.blocks.findIndex((b) => b.id === blockId);
  if (!editable() || i < 0) return;
  const b = note.blocks[i];
  const id = slot.dataset.database;
  const updated = b.markdown.replace(
    new RegExp(
      `\\[\\[database:${id}(:(table|board|calendar)(:[A-Za-z0-9_-]{1,80})?)?\\]\\]`,
    ),
    view === "table"
      ? `[[database:${id}]]`
      : `[[database:${id}:${view}${column ? ":" + column : ""}]]`,
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
    slot.dataset.view = classes.includes("db-view-board")
      ? "board"
      : classes.includes("db-view-calendar")
        ? "calendar"
        : "table";
    // The property the board groups by, or the calendar places records by.
    slot.dataset.column =
      classes.find((c) => c.startsWith("db-col-"))?.slice(7) || "";
    slot.dataset.state = "loading";
    slot.innerHTML = `<span class="db-embed-note">${icon("grid")}${$t("Loading database…")}</span>`;
  }
  if (shareMode) {
    slots.forEach(
      (slot) =>
        (slot.innerHTML =
          `<span class="db-embed-note">${$t("▦ Embedded database · shown to workspace members")}</span>`),
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
// Embedded notes and blocks the server sent, by "note#block", for redraws.
const noteEmbeds = new Map();
// What each slot shows now, so unchanged content is not redrawn.
const drawnEmbeds = new WeakMap();
let noteEmbedTimer;
const embedKey = (t) => `${t.note}#${t.block || ""}`;
const EMBED_PROBLEMS = {
  unavailable: "This note is not available to you, or was deleted.",
  trashed: "This note is in the trash.",
  "no-block": "That block is no longer in its note.",
  loop: "Already shown here.",
  deep: "Embeds inside embeds stop here.",
};
/** The embeds around a slot (innermost first), and the block it is written in. */
function embedContext(slot) {
  const outer = [];
  for (
    let p = slot.parentElement?.closest(".note-embed");
    p;
    p = p.parentElement?.closest(".note-embed")
  )
    outer.push(slotTarget(p.classList));
  const block = slot.closest("#blocks > [data-block]")?.dataset.block;
  return { outer, host: { note: note?.id, block } };
}
/** A block of the open note, and the blocks nested under it, as an embed. */
function localEmbed(target) {
  const i = note.blocks.findIndex((b) => b.id === target.block);
  if (i < 0) return { state: "no-block" };
  const level = indentLevel(note.blocks[i].markdown);
  let end = i + 1;
  while (
    end < note.blocks.length &&
    note.blocks[end].markdown.trim() &&
    indentLevel(note.blocks[end].markdown) > level
  )
    end++;
  return {
    state: "ok",
    title: note.title,
    icon: note.icon,
    blocks: note.blocks.slice(i, end).map((b, j) => ({
      id: b.id,
      html: note.rendered?.[i + j] ?? esc(b.markdown),
    })),
  };
}
function drawNoteEmbed(slot, target, entry) {
  slot.dataset.state = "ready";
  // Never redrawn while its block is being edited here.
  if (slot.dataset.editing) return;
  const href = `#note/${target.note}${target.block ? `#${target.block}` : ""}`;
  const problem = EMBED_PROBLEMS[entry?.state] || (!entry?.blocks && "This note could not be loaded.");
  const html = problem
    ? `<span class="note-embed-note">${icon("note")}${esc(problem)}${entry?.title && entry.state !== "trashed" ? ` <a href="${esc(href)}">${esc(entry.title)}</a>` : ""}</span>`
    : `<span class="note-embed-panel" role="group" aria-label="${target.block ? $t("Block") : $t("Note")} from ${esc(entry.title)}"><a class="note-embed-source" href="${esc(href)}" title="${target.block ? $t("Open the block in its note") : $t("Open the note")}"><span aria-hidden="true">${esc(entry.icon || "▤")}</span>${esc(entry.title)}</a>${target.block && entry.can_edit && !shareMode && target.note !== note?.id ? `<button type="button" class="note-embed-edit" data-embed-edit title="${$t("Edit this block here; it changes in its note too")}" aria-label="${$t("Edit the embedded block")}">✎</button>` : ""}<span class="note-embed-body">${entry.blocks.map((b) => `<span class="note-embed-block">${b.html}</span>`).join("")}${entry.more ? `<span class="note-embed-more">${$t("The rest is in the note.")}</span>` : ""}</span></span>`;
  // Unchanged content is left alone, so a reader's selection survives.
  if (drawnEmbeds.get(slot) === html) return;
  drawnEmbeds.set(slot, html);
  slot.innerHTML = html;
  const edit = $("[data-embed-edit]", slot);
  if (edit)
    edit.addEventListener("click", (e) => {
      e.stopPropagation();
      editEmbeddedBlock(slot, target, entry);
    });
}
/** Edits an embedded block in place; it saves to its own note. */
function editEmbeddedBlock(slot, target, entry) {
  const base = entry.blocks?.[0]?.markdown;
  const body = $(".note-embed-body", slot);
  if (base == null || !body) return;
  slot.dataset.editing = "1";
  body.innerHTML = `<span class="note-embed-editor"><textarea aria-label="${$t("Edit the embedded block")}" spellcheck="true"></textarea><span class="note-embed-actions"><small>${$t("Ctrl+Enter to save · Escape to cancel")}</small><button type="button" class="btn" data-embed-cancel>${$t("Cancel")}</button><button type="button" class="btn primary" data-embed-save>${$t("Save")}</button></span></span>`;
  const ta = $("textarea", body);
  ta.value = base;
  ta.style.height = Math.max(60, ta.scrollHeight) + "px";
  ta.focus();
  for (const kind of ["click", "mousedown", "keydown"])
    body.addEventListener(kind, (e) => e.stopPropagation());
  const done = () => {
    delete slot.dataset.editing;
    drawnEmbeds.delete(slot);
    drawNoteEmbed(slot, target, noteEmbeds.get(embedKey(target)) || entry);
  };
  const save = async () => {
    const markdown = ta.value;
    if (markdown === base) return done();
    try {
      await api(`notes/${target.note}/blocks/${target.block}`, "PUT", {
        markdown,
        base,
      });
      noteEmbeds.delete(embedKey(target));
      const [fresh] = await api("embeds", "POST", { items: [target] });
      if (fresh) noteEmbeds.set(embedKey(fresh), fresh);
      toast($t("Saved in “{title}”", { title: entry.title }));
      done();
    } catch (e) {
      error(e);
    }
  };
  ta.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
      e.preventDefault();
      save();
    } else if (e.key === "Escape") {
      e.preventDefault();
      done();
    }
  });
  $("[data-embed-save]", body).onclick = save;
  $("[data-embed-cancel]", body).onclick = done;
}
/** Fills the slots of embedded notes and blocks, then keeps them current. */
async function renderNoteEmbeds() {
  const slots = $$("#blocks .note-embed:not([data-state])");
  if (!slots.length || !note) return;
  const waiting = [];
  const wanted = new Map();
  for (const slot of slots) {
    const target = slotTarget(slot.classList);
    if (!target) {
      slot.dataset.state = "ready";
      continue;
    }
    slot.dataset.state = "loading";
    const { outer, host } = embedContext(slot);
    const allowed = embedAllowed(target, outer, host);
    const key = embedKey(target);
    if (allowed !== "ok") drawNoteEmbed(slot, target, { state: allowed });
    else if (target.note === note.id) drawNoteEmbed(slot, target, localEmbed(target));
    else if (noteEmbeds.has(key)) drawNoteEmbed(slot, target, noteEmbeds.get(key));
    else {
      slot.innerHTML = `<span class="note-embed-note">${icon("note")}${$t("Loading…")}</span>`;
      wanted.set(key, target);
      waiting.push([slot, target]);
    }
  }
  if (wanted.size) {
    const items = [...wanted.values()];
    const answers = [];
    for (let i = 0; i < items.length; i += 50)
      answers.push(
        ...(await api("embeds", "POST", { items: items.slice(i, i + 50) }).catch(
          () => [],
        )),
      );
    for (const a of answers) noteEmbeds.set(embedKey(a), a);
    for (const [slot, target] of waiting)
      if (slot.isConnected)
        drawNoteEmbed(slot, target, noteEmbeds.get(embedKey(target)));
  }
  watchEmbeds();
  // Live signals bring changes at once; polling stays as a fallback, often
  // where there are no signals (the desktop app, share links).
  clearInterval(noteEmbedTimer);
  noteEmbedTimer = setInterval(
    () => refreshNoteEmbeds().catch(error),
    liveSignals() ? 60000 : 5000,
  );
}
// Live signals of the notes embedded in the open note: one per note.
const embedWatches = new Map();
let embedSignal = 0;
function watchEmbeds() {
  const ids = new Set();
  for (const slot of $$("#blocks .note-embed[data-state=ready]")) {
    const target = slotTarget(slot.classList);
    if (target && note && target.note !== note.id) ids.add(target.note);
  }
  for (const [id, stop] of embedWatches)
    if (!ids.has(id)) {
      stop();
      embedWatches.delete(id);
    }
  for (const id of ids)
    if (!embedWatches.has(id))
      embedWatches.set(
        id,
        watchNote(id, () => {
          clearTimeout(embedSignal);
          embedSignal = setTimeout(
            () => refreshNoteEmbeds().catch(() => {}),
            250,
          );
        }),
      );
}
// Picks up edits to embedded notes, made here or by other people.
async function refreshNoteEmbeds() {
  const slots = $$("#blocks .note-embed[data-state=ready]");
  if (!slots.length || !note) {
    watchEmbeds();
    return clearInterval(noteEmbedTimer);
  }
  if (document.hidden) return;
  const known = new Map();
  for (const slot of slots) {
    const target = slotTarget(slot.classList);
    if (target && target.note !== note.id && noteEmbeds.has(embedKey(target)))
      known.set(embedKey(target), target);
  }
  const items = [...known.values()].slice(0, 50).map((t) => ({
    ...t,
    revision: noteEmbeds.get(embedKey(t)).revision,
  }));
  const answers = items.length
    ? await api("embeds", "POST", { items }).catch(() => [])
    : [];
  for (const a of answers)
    if (!a.unchanged) noteEmbeds.set(embedKey(a), a);
  for (const slot of $$("#blocks .note-embed[data-state=ready]")) {
    const target = slotTarget(slot.classList);
    if (!target) continue;
    const { outer, host } = embedContext(slot);
    if (embedAllowed(target, outer, host) !== "ok") continue;
    const entry =
      target.note === note.id
        ? localEmbed(target)
        : noteEmbeds.get(embedKey(target));
    if (entry) drawNoteEmbed(slot, target, entry);
  }
}
// Inserts a database into the note: an existing one or a new task list.
function databaseEmbedDialog(existingId) {
  const ids = new Set(folders().map((f) => f.id));
  const databases = (data.databases || []).filter((d) => ids.has(d.folder_id));
  const editorFolders = folderTreeOptions((f) => data.roles[f.id] === "editor");
  formDialog(
    $t("Insert a database"),
    $t("The note shows the database's current records. Editing them here or in the database changes both."),
    `<label>${$t("Database")}<select name="database" id="embed-choice">${editorFolders ? `<option value="new">${$t("New task list…")}</option>` : ""}${databases.map((d) => `<option value="${d.id}">${$tn(d.record_count, "{name} · {count} record", "{name} · {count} records", { name: esc(d.name) })}</option>`).join("")}</select></label><div id="embed-new"><label>${$t("Name")}<input name="name" value="Tasks" maxlength="200"></label><label>${$t("Folder")}<select name="folder_id">${editorFolders}</select></label></div><label>${$t("Show as")}<select name="view"><option value="table">${$t("Table")}</option><option value="board">${$t("Board (by status)")}</option><option value="calendar">${$t("Calendar (by date)")}</option></select></label>`,
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
      if (!id) throw new Error($t("Choose a database."));
      const markdown =
        v.view === "table" ? `[[database:${id}]]` : `[[database:${id}:${v.view}]]`;
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
      $t("You need edit access to a folder to create a database.");
  }
}
// One record in a form; used by the board views.
function editRecordDialog(db, row, edit, onSaved) {
  formDialog(
    $t("Edit record"),
    db.name,
    db.columns
      .map(
        (c) =>
          `<label>${esc(c.name)}${cellInput(c, row.cells[c.id], c.id, !edit, { db, row })}</label>`,
      )
      .join(""),
    async () => {
      const cells = {};
      db.columns
        .filter((c) => !COMPUTED.has(c.kind))
        .forEach((c) => {
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
  if (edit || row.page) {
    const page = document.createElement("button");
    page.type = "button";
    page.className = "btn";
    page.innerHTML = `${icon("note")}${$t("Open as page")}`;
    page.onclick = () => openRecordPage(db, row).catch(error);
    $("footer", modal)?.prepend(page);
  }
}
/** Opens a record's page, making it the first time (editors only). */
async function openRecordPage(db, row) {
  const id = row.page || (await api(`databases/${db.id}/rows/${row.id}/page`, "POST")).id;
  row.page = id;
  if (modal.open) modal.close();
  await refresh();
  await openNote(id);
}
/** On a record's page: the record's properties, which editors change here. */
async function renderRecordPanel() {
  const panel = $("#record-panel");
  if (!panel || !note?.record) return;
  const { database_id: did, record_id: rid } = note.record;
  let db;
  try {
    db = await api("databases/" + did);
    await loadDatabaseContext(db);
  } catch {
    panel.innerHTML = `<p class="hint">${$t("The database of this record cannot be opened.")}</p>`;
    return;
  }
  const row = db.rows.find((r) => r.id === rid);
  if ($("#record-panel") !== panel) return;
  if (!row) {
    panel.innerHTML = `<p class="hint">${$t("This record was deleted.")}</p>`;
    return;
  }
  // The cell pickers find the database among those on screen.
  embedCache.set(db.id, { at: Date.now(), db });
  const edit = data.roles[db.folder_id] === "editor";
  panel.innerHTML = `<header><button class="text-link" id="record-db">${icon("grid")}${esc(db.name)}</button><span>${$t("Record properties")}</span></header><div class="record-fields">${db.columns
    .map(
      (c) =>
        `<label data-col="${esc(c.id)}"><span>${esc(c.name)}</span>${cellInput(c, row.cells[c.id], "page:" + c.id, !edit, { db, row })}</label>`,
    )
    .join("")}</div>`;
  action($("#record-db", panel), "click", () => openDatabase(db.id));
  $$("[data-cell]", panel).forEach((el) => {
    const cid = el.dataset.cell.slice(5);
    const col = db.columns.find((c) => c.id === cid);
    el.addEventListener(cellEvent(col), async () => {
      const value = parseCell(col, el);
      if (JSON.stringify(value) === JSON.stringify(row.cells[cid] ?? null)) return;
      el.classList.add("saving");
      try {
        Object.assign(
          row,
          await api(`databases/${db.id}/rows/${rid}`, "PATCH", {
            revision: row.revision,
            cells: { [cid]: value },
          }),
        );
        el.classList.remove("cell-error");
        // Computed properties follow; the title follows the record's name.
        for (const c of db.columns.filter((c) => COMPUTED.has(c.kind))) {
          const span = $(`[data-col="${c.id}"] .cell-computed`, panel);
          if (span) span.textContent = cellText(c, row, db) || "—";
        }
        if (cid === db.columns[0].id) {
          note.title = embedRecordTitle(db, row);
          $("#note-title").value = note.title;
          $("#breadcrumb").textContent = note.title;
        }
      } catch (e) {
        el.classList.add("cell-error");
        toast(e.message);
      } finally {
        el.classList.remove("saving");
      }
    });
  });
}

async function renderIntegrations() {
  clearInterval(dbTimer);
  activeDatabase = null;
  const tokens = await api("mcp-credentials");
  // Server administrators have no personal tokens (they manage, not write).
  const personal = data.user.admin ? null : await api("api-tokens");
  $("#breadcrumb").textContent = $t("MCP connections");
  $("#content").innerHTML =
    `<section class="page"><div class="eyebrow">${$t("YOUR KNOWLEDGE, WITH CONTEXT")}</div><div class="page-heading"><div><h1>${$t("Connect your AI tools")}</h1><p class="subtitle">${$t("Let your assistant find answers in your notes and structured records.")}</p></div><button class="btn primary" id="create-mcp" ${!workspaceId ? "disabled" : ""}>${icon("plus")}${$t("Create credential")}</button></div><div class="welcome-banner"><div class="welcome-copy"><div class="eyebrow">${$t("MODEL CONTEXT PROTOCOL")}</div><h2>${$t("A useful connection. A clear boundary.")}</h2><p>${$t("Access to one workspace: read only, or read and write when you choose. Your folder permissions always apply.")}<br>${$t("Create a separate, expiring credential for each AI tool.")}</p></div><div class="mcp-art" aria-hidden="true">⌘</div></div><section class="panel"><div class="panel-heading"><div><h2>${$t("Your connections")}</h2><p>${$t("Credentials are shown once. Revoke a connection to stop its access immediately.")}</p></div></div><div class="table-wrap"><table class="admin-table"><thead><tr><th>${$t("CLIENT")}</th><th>${$t("WORKSPACE")}</th><th>${$t("ACCESS")}</th><th>${$t("LAST USED")}</th><th>${$t("EXPIRES")}</th><th></th></tr></thead><tbody>${tokens.map((t) => `<tr><td>${esc(t.name)}</td><td>${esc(data.workspaces.find((w) => w.id === t.workspace_id)?.name || $t("Unavailable workspace"))}</td><td><span class="pill">${t.access === "read_write" ? $t("Read and write") : $t("Read only")}</span></td><td>${t.last_used_at ? date(t.last_used_at) : $t("Never")}</td><td>${date(t.expires_at)} ${t.expires_at < Date.now() / 1000 ? $t("· Expired") : ""}</td><td><button class="btn danger" data-revoke="${t.id}">${$t("Revoke")}</button></td></tr>`).join("") || `<tr><td colspan="6">${$t("No AI connections yet.")}</td></tr>`}</tbody></table></div></section>${
      personal
        ? `<section class="panel"><div class="panel-heading"><div><h2>${$t("Personal API tokens")}</h2><p>${$t("For your scripts and the")} <code>minerva-cli</code> ${$t("command: a token acts as you, with your permissions, but cannot change your password, second factor or tokens.")}</p></div><button class="btn" id="create-api-token">${icon("plus")}${$t("New token")}</button></div><div class="table-wrap"><table class="admin-table"><thead><tr><th>${$t("NAME")}</th><th>${$t("ACCESS")}</th><th>${$t("CREATED")}</th><th>${$t("LAST USED")}</th><th>${$t("EXPIRES")}</th><th></th></tr></thead><tbody>${personal.map((t) => `<tr><td>${esc(t.name)}</td><td><span class="pill">${t.access === "read_only" ? $t("Read only") : $t("Read and write")}</span></td><td>${date(t.created_at)}</td><td>${t.last_used_at ? date(t.last_used_at) : $t("Never")}</td><td>${date(t.expires_at)} ${t.expires_at < Date.now() / 1000 ? $t("· Expired") : ""}</td><td><button class="btn danger" data-revoke-token="${t.id}">${$t("Revoke")}</button></td></tr>`).join("") || `<tr><td colspan="6">${$t("No API tokens yet.")}</td></tr>`}</tbody></table></div></section>`
        : ""
    }<div class="integration-tools"><h2>${$t("What your assistant can do")}</h2><div class="folder-grid"><div class="folder-card"><div><strong>${$t("Find knowledge")}</strong><small>${$t("Keyword search with document and block references")}</small></div></div><div class="folder-card"><div><strong>${$t("Read documents")}</strong><small>${$t("Permission-aware, paginated note content")}</small></div></div><div class="folder-card"><div><strong>${$t("Query databases")}</strong><small>${$t("Typed records, property filters, and pagination")}</small></div></div><div class="folder-card"><div><strong>${$t("Write, if you allow it")}</strong><small>${$t("Create notes, add blocks, add and update records. Nothing is deleted, and earlier versions stay in history")}</small></div></div></div><p class="subtitle">${$t("Supports MCP Streamable HTTP clients with bearer authentication. No AI provider receives your data until you connect a tool and use it.")}</p></div></section>`;
  $$("[data-revoke]").forEach((el) =>
    action(el, "click", () =>
      formDialog(
        $t("Revoke this connection?"),
        $t("The AI tool will immediately lose access. You can create a new credential later."),
        "",
        async () => {
          await api("mcp-credentials/" + el.dataset.revoke, "DELETE");
          await renderIntegrations();
        },
        "Revoke",
      ),
    ),
  );
  $$("[data-revoke-token]").forEach((el) =>
    action(el, "click", () =>
      formDialog(
        $t("Revoke this token?"),
        $t("Scripts using it stop working at once."),
        "",
        async () => {
          await api("api-tokens/" + el.dataset.revokeToken, "DELETE");
          await renderIntegrations();
        },
        "Revoke",
      ),
    ),
  );
  if ($("#create-api-token"))
    action($("#create-api-token"), "click", () => {
      modal.innerHTML = `<form id="api-token-form"><h2>${$t("New API token")}</h2><p>${$t("It acts as you. Give each script or computer its own token.")}</p><label>${$t("Name")}<input name="name" placeholder="${$t("e.g. Laptop backup")}" required maxlength="80"></label><label>${$t("Expires in")}<select name="expires_days"><option value="30">${$t("30 days")}</option><option value="90" selected>${$t("90 days")}</option><option value="365">${$t("1 year")}</option></select></label><label>${$t("Access")}<select name="access"><option value="read_write">${$t("Read and write")}</option><option value="read_only">${$t("Read only (for backups and reports)")}</option></select></label><div class="form-error"></div><footer><button type="button" class="btn" id="cancel-token">${$t("Cancel")}</button><button class="btn primary">${$t("Create token")}</button></footer></form>`;
      modal.showModal();
      $("#cancel-token").onclick = () => modal.close();
      $("#api-token-form").onsubmit = async (e) => {
        e.preventDefault();
        const button = $("footer button:last-child", modal);
        button.disabled = true;
        try {
          const v = Object.fromEntries(new FormData(e.target));
          const created = await api("api-tokens", "POST", {
            name: v.name,
            expires_days: Number(v.expires_days),
            access: v.access,
          });
          await renderIntegrations();
          const origin =
            document.querySelector('meta[name="minerva-server-origin"]')
              ?.content || location.origin;
          modal.innerHTML =
            `<h2>${$t("Your token is ready")}</h2><p>${$t("It is shown only once; keep it private. For")} <code>minerva-cli</code>:</p><textarea id="token-env" aria-label="${$t("API token")}" rows="3" readonly></textarea><footer><button class="btn" id="done-token">${$t("Done")}</button><button class="btn primary" id="copy-token">${$t("Copy")}</button></footer>`;
          $("#token-env").value =
            `export MINERVA_URL=${origin}\nexport MINERVA_TOKEN=${created.token}`;
          $("#done-token").onclick = () => {
            modal.close();
            modal.innerHTML = "";
          };
          action($("#copy-token"), "click", async () => {
            await navigator.clipboard.writeText($("#token-env").value);
            toast($t("Copied"));
          });
        } catch (err) {
          $(".form-error", modal).textContent = err.message;
          button.disabled = false;
        }
      };
    });
  action($("#create-mcp"), "click", () => {
    modal.innerHTML = `<form id="mcp-create-form"><h2>${$t("Connect an AI tool")}</h2><p>${$t("One workspace. Always subject to your permissions.")}</p><label>${$t("Client label")}<input name="name" placeholder="${$t("e.g. Claude Code")}" required maxlength="200"></label><label>${$t("Workspace")}<select name="workspace_id">${data.workspaces.map((w) => `<option value="${w.id}" ${w.id === workspaceId ? "selected" : ""}>${esc(w.name)}</option>`).join("")}</select></label><label>${$t("Expires in")}<select name="expires_days"><option value="7">${$t("7 days")}</option><option value="30" selected>${$t("30 days")}</option><option value="90">${$t("90 days")}</option></select></label><label>${$t("Access")}<select name="access"><option value="read_only" selected>${$t("Read only")}</option><option value="read_write">${$t("Read and write: create notes, add blocks and records")}</option></select></label><div class="form-error"></div><footer><button type="button" class="btn" id="cancel-mcp">${$t("Cancel")}</button><button class="btn primary">${$t("Create credential")}</button></footer></form>`;
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
          `<h2>${$t("Your connection is ready")}</h2><p>${$t("Copy this configuration into your MCP client. The credential is shown only once; keep it private.")}</p><textarea id="mcp-config" aria-label="${$t("MCP client configuration")}" rows="12" readonly></textarea><footer><button class="btn" id="done-mcp">${$t("Done")}</button><button class="btn primary" id="copy-mcp">${$t("Copy JSON")}</button></footer>`;
        $("#mcp-config").value = JSON.stringify(config, null, 2);
        $("#done-mcp").onclick = () => {
          modal.close();
          modal.innerHTML = "";
        };
        action($("#copy-mcp"), "click", async () => {
          await navigator.clipboard.writeText($("#mcp-config").value);
          toast($t("Configuration copied"));
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
  if (
    (e.ctrlKey || e.metaKey) &&
    e.altKey &&
    key === "p" &&
    note &&
    !modal.open
  ) {
    e.preventDefault();
    presentNote().catch(error);
    return;
  }
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
        $("#save-state").textContent = editable() ? status : $t("View only · live");
    },
    (e) => {
      if (e.status === 401) sharedUnavailable();
    },
    "shared/sync",
  );
  live.presence = blockPresence;
  await live.start();
  if (guestComments()) comments.attach(note.id);
  $("#app").innerHTML =
    `<a class="skip-link" href="#content">${$t("Skip to content")}</a><div class="shared-shell"><header class="shared-top"><div class="brand"><span class="brand-mark">✳</span>minerva</div><span class="badge">${note.role === "editor" ? $t("Anyone with this link can edit") : $t("View only")}</span><span class="spacer"></span>${themeSelect()}</header><main class="main"><span id="breadcrumb" hidden></span><div id="content" tabindex="-1"></div></main></div>`;
  bindThemeSelect();
  document.title = note.title + " · Minerva";
  renderEditor();
}
function sharedUnavailable() {
  stopLive();
  note = null;
  $("#app").innerHTML =
    `<div class="loading"><span>${$t("This link is no longer available. Ask the person who shared it for a new link.")}</span></div>`;
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
    if (!localMode && !isPane) startWorker();
    // Opened from another place's workspace menu: that workspace first.
    const asked = new URLSearchParams(location.search).get("workspace");
    if (asked && data.workspaces.some((x) => x.id === asked))
      workspaceId = asked;
    // Links in reminder emails open the Reminders page.
    if (new URLSearchParams(location.search).get("view") === "reminders")
      view = "reminders";
    if (chatPane) {
      const [kind, id] = initialChat.split(":");
      aiScope = { kind, id };
      view = "chat";
      document.documentElement.classList.add("chat-pane");
    }
    shell();
    if (localMode) showStartNotice();
    firstSignIn();
    const requested = initialNote
      ? { note: initialNote, block: initialBlock }
      : noteTarget(location.hash);
    if (requested)
      await openNote(requested.note, { block: requested.block });
    // The window around this tab shows it once its note is drawn.
    if (isPane) paneMessage("loaded");
  } catch (e) {
    if (isPane) paneMessage("loaded");
    if (e.status === 401) login();
    else {
      // Offline (status 0): the service worker says what is missing.
      $("#app").innerHTML =
        `<div class="loading"><span>${e.status === 0 ? esc(e.message) : $t("Unable to connect to the server. Reload to try again.")}</span></div>`;
      if (e.status !== 0) error(e);
    }
  }
})();
