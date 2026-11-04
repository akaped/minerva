import { $t } from "./i18n.js";
import * as Y from "yjs";
export function encode(bytes) {
  let s = "";
  for (let i = 0; i < bytes.length; i += 8192)
    s += String.fromCharCode(...bytes.subarray(i, i + 8192));
  return btoa(s);
}
export function decode(s) {
  return Uint8Array.from(atob(s), (c) => c.charCodeAt(0));
}
// Apply the smallest replacement, preserving unaffected CRDT character identities.
export function replaceText(text, value) {
  const old = text.toString();
  if (old === value) return;
  let start = 0,
    end = 0;
  while (
    start < old.length &&
    start < value.length &&
    old[start] === value[start]
  )
    start++;
  // Never split a UTF-16 surrogate pair.
  if (start > 0 && /[\uD800-\uDBFF]/.test(old[start - 1])) start--;
  while (
    end < old.length - start &&
    end < value.length - start &&
    old[old.length - 1 - end] === value[value.length - 1 - end]
  )
    end++;
  if (end > 0 && /[\uDC00-\uDFFF]/.test(old[old.length - end])) end--;
  if (old.length - start - end) text.delete(start, old.length - start - end);
  if (value.length - start - end)
    text.insert(start, value.slice(start, value.length - end));
}
/**
 * One WebSocket per page (`/api/v1/live`) that says when a watched note changed
 * on the server, so it syncs at once. Pages served without http(s) (the desktop
 * app) and share links have none, and poll instead.
 */
class LiveSignals {
  constructor() {
    this.listeners = new Map();
    this.socket = null;
    this.connected = false;
    this.retry = 1000;
  }
  get available() {
    return (
      typeof WebSocket !== "undefined" &&
      /^https?:$/.test(globalThis.location?.protocol || "")
    );
  }
  watch(id, onSignal) {
    if (!this.available) return () => {};
    if (!this.listeners.has(id)) this.listeners.set(id, new Set());
    this.listeners.get(id).add(onSignal);
    this.connect();
    this.send();
    return () => {
      this.listeners.get(id)?.delete(onSignal);
      if (!this.listeners.get(id)?.size) this.listeners.delete(id);
      this.send();
    };
  }
  send() {
    if (this.connected)
      this.socket.send(JSON.stringify({ watch: [...this.listeners.keys()] }));
  }
  connect() {
    if (this.socket || !this.listeners.size) return;
    const url = new URL("/api/v1/live", location.href);
    url.protocol = location.protocol === "https:" ? "wss:" : "ws:";
    const socket = new WebSocket(url);
    this.socket = socket;
    socket.onopen = () => {
      this.connected = true;
      this.retry = 1000;
      this.send();
      // Changes made while disconnected arrive now.
      for (const listeners of this.listeners.values())
        for (const signal of listeners) signal();
    };
    socket.onmessage = (event) => {
      let id;
      try {
        id = JSON.parse(event.data).note;
      } catch {
        return;
      }
      for (const signal of this.listeners.get(id) || []) signal();
    };
    socket.onclose = () => {
      this.socket = null;
      this.connected = false;
      if (!this.listeners.size) return;
      setTimeout(() => this.connect(), this.retry);
      this.retry = Math.min(this.retry * 2, 30000);
    };
  }
}
const signals = new LiveSignals();
/** Calls `onChange` when note `id` changes on the server; returns a stop function. */
export function watchNote(id, onChange) {
  return signals.watch(id, onChange);
}

export class Collaboration {
  constructor(
    noteId,
    api,
    onChange,
    onStatus,
    onFailure,
    syncPath = "notes/" + noteId + "/sync",
  ) {
    this.id = noteId;
    this.syncPath = syncPath;
    this.scoped = new Set();
    this.api = api;
    this.onChange = onChange;
    this.onStatus = onStatus;
    this.onFailure = onFailure;
    this.doc = new Y.Doc();
    this.pending = [];
    this.clientId = crypto.randomUUID();
    this.closed = false;
    this.ready = false;
    this.sending = null;
    this.doc.on("update", (update, origin) => {
      if (origin === "server") return;
      this.pending.push(update);
      this.onStatus($t("Saving…"), true);
      clearTimeout(this.debounce);
      this.debounce = setTimeout(() => this.sync().catch(() => {}), 180);
    });
  }
  /**
   * Where this user is in the note: the block they are on, and whether they
   * have it open for editing. The editor replaces this with its own view.
   */
  presence() {
    const active = document.activeElement;
    return {
      active_block: active?.closest?.("[data-block]")?.dataset.block || null,
      editing: active?.tagName === "TEXTAREA",
    };
  }
  async start() {
    await this.sync();
    // Undo only reverts this client's own changes, never collaborators' edits.
    this.undoManager = new Y.UndoManager(
      [
        this.doc.getText("title"),
        this.doc.getArray("order"),
        this.doc.getMap("meta"),
      ],
      { trackedOrigins: new Set(["local"]), captureTimeout: 600 },
    );
    this.ready = true;
    // Signals say when others changed the note; polling stays as a fallback,
    // slow while signals arrive.
    if (this.syncPath === "notes/" + this.id + "/sync")
      this.unwatch = signals.watch(this.id, () => this.sync().catch(() => {}));
    const poll = () => {
      if (this.closed) return;
      this.timer = setTimeout(
        () =>
          this.sync()
            .finally(poll)
            .catch(() => {}),
        signals.connected ? 5000 : 650,
      );
    };
    poll();
    // Moving to another block shows at once for the others.
    this.moved = () => {
      clearTimeout(this.presenceTimer);
      this.presenceTimer = setTimeout(() => {
        if (JSON.stringify(this.presence()) !== this.sentPresence)
          this.sync().catch(() => {});
      }, 120);
    };
    document.addEventListener?.("focusin", this.moved);
    document.addEventListener?.("focusout", this.moved);
    return this;
  }
  snapshot() {
    return {
      title: this.doc.getText("title").toString(),
      icon: this.doc.getMap("meta").get("icon"),
      favorite: this.doc.getMap("meta").get("favorite"),
      trashed: this.doc.getMap("meta").get("trashed"),
      archived: this.doc.getMap("meta").get("archived") ?? false,
      cover: this.doc.getMap("meta").get("cover") ?? "",
      tags: this.doc.getMap("meta").get("tags") ?? [],
      blocks: [...new Set(this.doc.getArray("order").toArray())].map((id) => ({
        id,
        markdown: this.doc.getText("block:" + id).toString(),
      })),
    };
  }
  commit(note) {
    if (!this.ready || this.closed) return;
    // Added together: Yjs copies its whole scope on each call.
    const unscoped = note.blocks.filter((b) => !this.scoped.has(b.id));
    if (unscoped.length) {
      for (const b of unscoped) this.scoped.add(b.id);
      this.undoManager.addToScope(
        unscoped.map((b) => this.doc.getText("block:" + b.id)),
      );
    }
    this.doc.transact(() => {
      replaceText(this.doc.getText("title"), note.title);
      const meta = this.doc.getMap("meta");
      for (const key of ["icon", "favorite", "trashed", "archived", "cover"])
        if (note[key] !== undefined && meta.get(key) !== note[key])
          meta.set(key, note[key]);
      if (
        note.tags &&
        JSON.stringify(meta.get("tags") ?? []) !== JSON.stringify(note.tags)
      )
        meta.set("tags", [...note.tags]);
      const order = this.doc.getArray("order"),
        desired = note.blocks.map((b) => b.id);
      // Edit only the changed portion of the order instead of replacing the
      // array, working on a copy: reading a Yjs array by index walks it.
      let current = order.toArray();
      if (
        current.length !== desired.length ||
        current.some((id, i) => id !== desired[i])
      ) {
        const wanted = new Set(desired);
        for (let i = current.length - 1; i >= 0; i--)
          if (!wanted.has(current[i])) order.delete(i, 1);
        current = current.filter((id) => wanted.has(id));
        for (let i = 0; i < desired.length; i++) {
          if (current[i] === desired[i]) continue;
          const old = current.indexOf(desired[i]);
          if (old >= 0) {
            order.delete(old, 1);
            current.splice(old, 1);
          }
          order.insert(i, [desired[i]]);
          current.splice(i, 0, desired[i]);
        }
      }
      for (const block of note.blocks)
        replaceText(this.doc.getText("block:" + block.id), block.markdown);
    }, "local");
  }
  async sync() {
    if (this.closed) return;
    if (this.sending) return this.sending;
    const updates = this.pending.splice(0);
    this.sending = (async () => {
      try {
        const presence = this.presence();
        this.sentPresence = JSON.stringify(presence);
        const result = await this.api(this.syncPath, "POST", {
          update: updates.length ? encode(Y.mergeUpdates(updates)) : "",
          // What this copy has: the server sends back only what it lacks.
          state_vector: encode(Y.encodeStateVector(this.doc)),
          client_id: this.clientId,
          ...presence,
        });
        if (this.closed) return;
        const focused = document.activeElement,
          key = focused?.dataset?.liveKey;
        let selection;
        if (key && typeof focused.selectionStart === "number") {
          const text = this.doc.getText(key);
          selection = [
            Y.createRelativePositionFromTypeIndex(text, focused.selectionStart),
            Y.createRelativePositionFromTypeIndex(text, focused.selectionEnd),
          ];
        }
        Y.applyUpdate(this.doc, decode(result.update), "server");
        this.onChange(this.snapshot(), result);
        // Never rewrite a field while an input method is composing text.
        if (key && focused.isConnected && !focused.dataset.composing) {
          focused.value = this.doc.getText(key).toString();
          if (selection) {
            const a = Y.createAbsolutePositionFromRelativePosition(
                selection[0],
                this.doc,
              ),
              b = Y.createAbsolutePositionFromRelativePosition(
                selection[1],
                this.doc,
              );
            if (a && b) focused.setSelectionRange(a.index, b.index);
          }
        }
        this.onStatus(
          this.pending.length ? $t("Saving…") : $t("Live · all changes saved"),
          this.pending.length > 0,
        );
      } catch (e) {
        this.pending.unshift(...updates);
        this.onStatus(
          this.pending.length
            ? $t("Offline · changes pending")
            : $t("Reconnecting…"),
          this.pending.length > 0,
        );
        this.onFailure(e);
        throw e;
      } finally {
        this.sending = null;
      }
    })();
    return this.sending;
  }
  /** Ends the current undo step so the next change is undone separately. */
  boundary() {
    this.undoManager?.stopCapturing();
  }
  undo() {
    return Boolean(this.undoManager?.undo());
  }
  redo() {
    return Boolean(this.undoManager?.redo());
  }
  /**
   * Starts an IME composition in text `key` at selection [start, end). Remote
   * changes may arrive while composing; positions are tracked relative to the
   * CRDT so the composed text lands in the right place.
   */
  beginComposition(key, start, end, value) {
    const text = this.doc.getText(key);
    return {
      key,
      start: Y.createRelativePositionFromTypeIndex(text, start),
      end: Y.createRelativePositionFromTypeIndex(text, end, -1),
      baseStart: start,
      baseTail: value.length - end,
    };
  }
  /** Applies the composed text and returns the merged value and caret. */
  endComposition(state, value) {
    const text = this.doc.getText(state.key);
    const inserted = value.slice(
      state.baseStart,
      value.length - state.baseTail,
    );
    const at = (rel) =>
      Y.createAbsolutePositionFromRelativePosition(rel, this.doc)?.index;
    const start = at(state.start) ?? state.baseStart,
      end = Math.max(start, at(state.end) ?? start);
    this.doc.transact(() => {
      if (end > start) text.delete(start, end - start);
      if (inserted) text.insert(start, inserted);
    }, "local");
    return { value: text.toString(), caret: start + inserted.length };
  }
  async flush(note) {
    this.commit(note);
    if (this.sending) await this.sending;
    await this.sync();
    while (this.pending.length) await this.sync();
  }
  /**
   * Tells the server this user left, so collaborators stop seeing them at
   * once instead of after a timeout. Also safe while the page unloads.
   */
  leave() {
    if (!this.ready || this.closed) return;
    fetch("/api/v1/" + this.syncPath, {
      method: "POST",
      keepalive: true,
      headers: { "Content-Type": "application/json", "X-Minerva-Client": "1" },
      body: JSON.stringify({
        update: "",
        client_id: this.clientId,
        leave: true,
      }),
    }).catch(() => {});
  }
  destroy() {
    this.leave();
    this.closed = true;
    this.unwatch?.();
    document.removeEventListener?.("focusin", this.moved);
    document.removeEventListener?.("focusout", this.moved);
    clearTimeout(this.timer);
    clearTimeout(this.presenceTimer);
    clearTimeout(this.debounce);
    this.doc.destroy();
  }
}
