export function noteLocation(input, base) {
  const url = new URL(input, base);
  if (
    !["http:", "https:", "minerva:"].includes(url.protocol) ||
    url.username ||
    url.password
  )
    throw new Error("Use a Minerva server HTTP(S) address.");
  const id =
    url.searchParams.get("note") ||
    (url.hash.startsWith("#note/") ? url.hash.slice(6) : "");
  if (id && !/^[a-f0-9-]{36}$/i.test(id))
    throw new Error("The note link has an invalid ID.");
  return { origin: url.origin, id };
}
export function reorderTabs(tabs, from, to) {
  const result = [...tabs],
    start = result.findIndex((t) => t.key === from),
    end = result.findIndex((t) => t.key === to);
  if (start < 0 || end < 0 || start === end) return result;
  const [tab] = result.splice(start, 1);
  result.splice(end, 0, tab);
  return result;
}
export const isPane =
  typeof window !== "undefined" &&
  location.pathname === "/pane" &&
  window.parent !== window;
const params = new URLSearchParams(
  typeof location === "undefined" ? "" : location.search,
);
export const initialNote = params.get("note");
/** An AI chat tab's context, `kind:id` (for example `note:<uuid>`). */
export const initialChat = params.get("chat");
const parentOrigin = params.get("host");
export function paneMessage(type, payload = {}) {
  if (isPane && parentOrigin)
    window.parent.postMessage(
      { channel: "minerva-pane", type, ...payload },
      parentOrigin,
    );
}

export function createNoteTabs({ esc, error, onBrowse, onChange }) {
  let tabs = [],
    active = null,
    secondary = null,
    shown = false,
    dragged = null;
  const root = document.createElement("div");
  root.className = "note-tabs-host";
  root.hidden = true;
  const strip = document.createElement("div");
  strip.className = "note-tab-strip";
  strip.setAttribute("aria-label", "Open notes");
  const deck = document.createElement("div");
  deck.className = "note-tab-deck";
  root.append(strip, deck);
  document.body.append(root);
  const requests = new Map();
  const group = crypto.randomUUID();
  function send(tab, type, payload = {}) {
    tab.frame.contentWindow?.postMessage(
      { channel: "minerva-host", type, ...payload },
      tab.origin,
    );
  }
  function place() {
    const main = document.querySelector(".main"),
      top = document.querySelector(".topbar");
    if (!main || !top || !tabs.length) {
      root.hidden = true;
      const content = document.querySelector("#content");
      if (content) {
        content.inert = false;
        content.style.visibility = "";
      }
      document.body.classList.remove("has-note-tabs", "note-tab-shown");
      return;
    }
    root.hidden = false;
    document.body.classList.add("has-note-tabs");
    const rect = main.getBoundingClientRect(),
      bar = top.getBoundingClientRect();
    Object.assign(root.style, {
      left: `${rect.left}px`,
      width: `${rect.width}px`,
      top: `${Math.max(0, bar.bottom)}px`,
    });
    deck.hidden = !shown;
    // The sidebar highlights no page while a note tab is in front.
    document.body.classList.toggle("note-tab-shown", shown);
    const content = document.querySelector("#content");
    if (content) {
      content.inert = shown;
      content.style.visibility = shown ? "hidden" : "";
    }
    const split = secondary && secondary !== active;
    for (const tab of tabs) {
      const visible = shown && (tab.key === active || tab.key === secondary);
      tab.frame.hidden = !visible;
      const right = split && tab.key === secondary;
      Object.assign(tab.frame.style, {
        left: right ? "50%" : "0",
        width: split ? "50%" : "100%",
      });
      send(tab, "visibility", { visible });
    }
  }
  function draw() {
    strip.innerHTML = `<button class="tab-browse" title="Browse notes" aria-label="Browse notes">☷</button><div class="note-tab-list" role="tablist">${tabs.map((t) => `<div class="note-tab ${t.key === active && shown ? "active" : ""} ${t.key === secondary && shown ? "secondary" : ""}" draggable="true" data-tab="${t.key}"><button role="tab" aria-selected="${shown && (t.key === active || t.key === secondary)}" data-activate="${t.key}" title="${esc(t.title + " · " + t.workspace + " · " + t.origin)}"><span>${esc(t.title)}</span><small>${esc([t.workspace, new URL(t.origin).host].filter(Boolean).join(" · "))}</small>${t.dirty ? ' <span aria-label="Unsaved changes">●</span>' : ""}</button><button data-split="${t.key}" title="Open side by side" aria-label="Open ${esc(t.title)} side by side">◫</button><button data-close="${t.key}" title="Close tab" aria-label="Close ${esc(t.title)}">×</button></div>`).join("")}</div><button class="tab-remote" aria-label="Open another server or note link" title="Open another server or note link">＋</button>${secondary ? '<button class="tab-unsplit" title="Single pane" aria-label="Single pane">▣</button>' : ""}`;
    strip.querySelector(".tab-browse").onclick = () => {
      shown = false;
      place();
      onBrowse();
    };
    strip.querySelector(".tab-remote").onclick = remoteDialog;
    strip.querySelector(".tab-unsplit")?.addEventListener("click", () => {
      secondary = null;
      draw();
    });
    strip.querySelectorAll("[data-activate]").forEach((b) => {
      b.onclick = () => {
        if (active !== b.dataset.activate && secondary === b.dataset.activate)
          secondary = active;
        active = b.dataset.activate;
        shown = true;
        draw();
      };
      b.onkeydown = (e) => {
        const i = tabs.findIndex((t) => t.key === b.dataset.activate);
        if (e.altKey && ["ArrowLeft", "ArrowRight"].includes(e.key)) {
          e.preventDefault();
          const target = tabs[i + (e.key === "ArrowLeft" ? -1 : 1)];
          if (target) {
            tabs = reorderTabs(tabs, b.dataset.activate, target.key);
            draw();
            strip
              .querySelector(`[data-activate="${b.dataset.activate}"]`)
              ?.focus();
          }
        }
      };
    });
    strip.querySelectorAll("[data-split]").forEach(
      (b) =>
        (b.onclick = () => {
          if (tabs.length < 2) {
            error(
              new Error("Open a second note first, then choose side by side."),
            );
            return;
          }
          secondary = b.dataset.split;
          if (active === secondary)
            active = tabs.find((t) => t.key !== secondary).key;
          shown = true;
          draw();
        }),
    );
    strip
      .querySelectorAll("[data-close]")
      .forEach((b) => (b.onclick = () => close(b.dataset.close).catch(error)));
    strip.querySelectorAll("[data-tab]").forEach((el) => {
      el.ondragstart = (e) => {
        dragged = el.dataset.tab;
        e.dataTransfer.effectAllowed = "move";
        e.dataTransfer.setData("text/plain", dragged);
      };
      el.ondragover = (e) => {
        if (dragged) {
          e.preventDefault();
          el.classList.add("drag-over");
        }
      };
      el.ondragleave = () => el.classList.remove("drag-over");
      el.ondrop = (e) => {
        e.preventDefault();
        tabs = reorderTabs(tabs, dragged, el.dataset.tab);
        dragged = null;
        draw();
      };
      el.ondragend = () => {
        dragged = null;
        strip
          .querySelectorAll(".drag-over")
          .forEach((x) => x.classList.remove("drag-over"));
      };
    });
    place();
  }
  function remoteDialog() {
    const modal = document.querySelector("#modal");
    modal.innerHTML =
      '<form id="open-server-tab"><h2>Open a server or note</h2><p>Paste a Minerva server address or a note link. Sign in separately on that server. For remote browser tabs, its administrator must allow this app’s origin in MINERVA_EMBED_ORIGINS.</p><label>Server or note URL<input name="url" type="url" placeholder="https://notes.example.com/?note=…" required></label><div class="form-error" role="alert"></div><footer><button type="button" class="btn" id="cancel-tab">Cancel</button><button class="btn primary">Open tab</button></footer></form>';
    modal.showModal();
    modal.querySelector("#cancel-tab").onclick = () => modal.close();
    modal.querySelector("form").onsubmit = (e) => {
      e.preventDefault();
      try {
        const target = noteLocation(
          new FormData(e.target).get("url"),
          location.href,
        );
        open({
          ...target,
          title: target.id ? "Remote note" : new URL(target.origin).host,
        });
        modal.close();
      } catch (err) {
        modal.querySelector(".form-error").textContent = err.message;
      }
    };
  }
  function open({
    id = "",
    origin = location.origin,
    title = "Note",
    workspace = "",
    chat = "",
  }) {
    const key = origin + "|" + (chat ? `chat:${chat}` : id);
    const existing = tabs.find((t) => t.key === key);
    if (existing) {
      active = key;
      shown = true;
      draw();
      return;
    }
    if (tabs.length >= 12)
      throw new Error("Close a tab before opening another (12 tabs maximum).");
    const frame = document.createElement("iframe");
    frame.className = "note-pane-frame";
    frame.title = title;
    const url = new URL("/pane", origin);
    url.searchParams.set("host", location.origin);
    url.searchParams.set("group", group);
    if (chat) url.searchParams.set("chat", chat);
    else if (id) url.searchParams.set("note", id);
    frame.src = url.href;
    frame.allow =
      "autoplay; fullscreen; picture-in-picture; clipboard-write; microphone";
    frame.referrerPolicy = "strict-origin-when-cross-origin";
    const tab = {
      key,
      id: chat ? "" : id,
      chat,
      queue: [],
      origin,
      title,
      workspace,
      frame,
      dirty: false,
      ready: false,
    };
    tabs.push(tab);
    deck.append(frame);
    active = key;
    shown = true;
    tab.timer = setTimeout(() => {
      if (!tab.ready && tabs.includes(tab))
        error(
          new Error(
            "The server tab has not connected. Check its address and MINERVA_EMBED_ORIGINS setting.",
          ),
        );
    }, 15000);
    draw();
  }
  /** Opens (or shows) an AI chat tab side by side with the tab `beside`. */
  function openChat({ scope, title, beside, origin = location.origin }) {
    open({ origin, chat: scope, title: `AI · ${title}` });
    const key = `${origin}|chat:${scope}`;
    if (beside && beside !== key && tabs.some((t) => t.key === beside)) {
      active = beside;
      secondary = key;
    }
    shown = true;
    draw();
  }
  /**
   * Adds Markdown blocks to a note, opening its tab when needed; resolves with
   * the number of blocks added once the note has saved them into its editor.
   */
  function insertBlocks({ origin = location.origin, id, title, blocks }) {
    let tab = tabs.find((t) => t.origin === origin && t.id === id);
    if (!tab) {
      const chat = secondary;
      open({ origin, id, title });
      tab = tabs.find((t) => t.origin === origin && t.id === id);
      // Keep the chat that sent the blocks beside the note.
      if (chat && chat !== tab.key) secondary = chat;
      draw();
    }
    return new Promise((resolve, reject) => {
      const request = crypto.randomUUID();
      const timeout = setTimeout(() => {
        requests.delete(request);
        reject(new Error("The note did not respond. Nothing was added."));
      }, 20000);
      requests.set(request, { tab, resolve, reject, timeout });
      const deliver = () => send(tab, "insert-blocks", { request, blocks });
      if (tab.ready) deliver();
      else tab.queue.push(deliver);
    });
  }
  async function flushTab(tab) {
    if (!tab.ready) {
      if (tab.dirty)
        throw new Error("This tab has not connected; it cannot safely close.");
      return;
    }
    const request = crypto.randomUUID();
    await new Promise((resolve, reject) => {
      const timeout = setTimeout(() => {
        requests.delete(request);
        reject(
          new Error("The note did not confirm its save. The tab remains open."),
        );
      }, 15000);
      requests.set(request, { tab, resolve, reject, timeout });
      send(tab, "flush", { request });
    });
  }
  async function close(key) {
    const tab = tabs.find((t) => t.key === key);
    if (!tab) return;
    await flushTab(tab);
    clearTimeout(tab.timer);
    tab.frame.remove();
    tabs = tabs.filter((t) => t !== tab);
    if (secondary === key) secondary = null;
    if (active === key) active = secondary || tabs.at(-1)?.key;
    if (active === secondary) secondary = null;
    if (!tabs.length) shown = false;
    draw();
  }
  addEventListener("message", (e) => {
    const tab = tabs.find(
      (t) => t.frame.contentWindow === e.source && t.origin === e.origin,
    );
    const message = e.data;
    if (!tab || message?.channel !== "minerva-pane") return;
    if (message.type === "ready") {
      tab.ready = true;
      clearTimeout(tab.timer);
      tab.queue.splice(0).forEach((deliver) => deliver());
      place();
    }
    // A note asks for an AI chat about itself, beside it.
    if (
      message.type === "open-chat" &&
      /^(note|folder):[a-f0-9-]{36}$/i.test(message.scope || "")
    )
      openChat({
        scope: message.scope,
        title: String(message.title || "Note").slice(0, 200),
        beside: tab.key,
      });
    // An AI chat sends reply blocks to a note of the same server.
    if (
      message.type === "insert-blocks" &&
      /^[a-f0-9-]{36}$/i.test(message.id || "") &&
      Array.isArray(message.blocks)
    )
      insertBlocks({
        origin: tab.origin,
        id: message.id,
        title: String(message.title || "Note"),
        blocks: message.blocks.map(String).slice(0, 500),
      }).then(
        (count) =>
          send(tab, "insert-result", { request: message.request, count }),
        (err) =>
          send(tab, "insert-result", {
            request: message.request,
            error: err.message,
          }),
      );
    if (message.type === "state") {
      if (/^[a-f0-9-]{36}$/i.test(message.noteId || ""))
        tab.id = message.noteId;
      if (
        tab.title === message.title &&
        tab.workspace === message.workspace &&
        tab.dirty === !!message.dirty
      )
        return;
      tab.title = String(message.title || tab.title).slice(0, 200);
      tab.workspace = String(message.workspace || "").slice(0, 200);
      tab.dirty = !!message.dirty;
      tab.frame.title = tab.title;
      draw();
    }
    // Notes or folders changed in a tab: the sidebar and other tabs catch up.
    if (message.type === "changed" && tab.origin === location.origin)
      onChange?.();
    if (
      message.type === "open-note" &&
      /^[a-f0-9-]{36}$/i.test(message.id || "")
    )
      open({
        id: message.id,
        origin: tab.origin,
        title: String(message.title || "Note"),
      });
    if (message.type === "inserted") {
      const r = requests.get(message.request);
      if (r?.tab === tab) {
        clearTimeout(r.timeout);
        requests.delete(message.request);
        message.error
          ? r.reject(new Error(String(message.error)))
          : r.resolve(message.count);
      }
    }
    if (message.type === "flushed") {
      const r = requests.get(message.request);
      if (r?.tab === tab) {
        clearTimeout(r.timeout);
        requests.delete(message.request);
        message.error
          ? r.reject(new Error(String(message.error)))
          : r.resolve();
      }
    }
  });
  addEventListener("resize", place);
  addEventListener("scroll", place, { passive: true });
  addEventListener("beforeunload", (e) => {
    if (tabs.some((t) => t.dirty)) {
      e.preventDefault();
      e.returnValue = "";
    }
  });
  return {
    open,
    openChat,
    insertBlocks,
    place,
    hide() {
      shown = false;
      place();
    },
    async flush() {
      await Promise.all(tabs.map(flushTab));
    },
    /**
     * Closes this server's note tabs whose note is gone or in the trash; `kept`
     * holds the IDs of the notes that remain. Their changes are already saved
     * or no longer matter, so nothing is flushed.
     */
    prune(kept, origin = location.origin) {
      const gone = tabs.filter(
        (t) => t.origin === origin && t.id && !t.chat && !kept.has(t.id),
      );
      if (!gone.length) return;
      for (const tab of gone) {
        clearTimeout(tab.timer);
        tab.frame.remove();
        if (secondary === tab.key) secondary = null;
      }
      tabs = tabs.filter((t) => !gone.includes(t));
      if (!tabs.some((t) => t.key === active))
        active = secondary || tabs.at(-1)?.key;
      if (active === secondary) secondary = null;
      if (!tabs.length) shown = false;
      draw();
    },
    async clear() {
      await Promise.all(tabs.map(flushTab));
      tabs.forEach((t) => {
        clearTimeout(t.timer);
        t.frame.remove();
      });
      tabs = [];
      active = secondary = null;
      shown = false;
      draw();
    },
  };
}
