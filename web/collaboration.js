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
      this.onStatus("Saving…", true);
      clearTimeout(this.debounce);
      this.debounce = setTimeout(() => this.sync().catch(() => {}), 180);
    });
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
    this.timer = setInterval(() => this.sync().catch(() => {}), 650);
    return this;
  }
  snapshot() {
    return {
      title: this.doc.getText("title").toString(),
      icon: this.doc.getMap("meta").get("icon"),
      favorite: this.doc.getMap("meta").get("favorite"),
      trashed: this.doc.getMap("meta").get("trashed"),
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
    for (const block of note.blocks)
      if (!this.scoped.has(block.id)) {
        this.scoped.add(block.id);
        this.undoManager.addToScope(this.doc.getText("block:" + block.id));
      }
    this.doc.transact(() => {
      replaceText(this.doc.getText("title"), note.title);
      const meta = this.doc.getMap("meta");
      for (const key of ["icon", "favorite", "trashed", "cover"])
        if (note[key] !== undefined && meta.get(key) !== note[key])
          meta.set(key, note[key]);
      if (
        note.tags &&
        JSON.stringify(meta.get("tags") ?? []) !== JSON.stringify(note.tags)
      )
        meta.set("tags", [...note.tags]);
      const order = this.doc.getArray("order"),
        desired = note.blocks.map((b) => b.id);
      // Edit only the changed portion of the order instead of replacing the array.
      for (let i = order.length - 1; i >= 0; i--)
        if (!desired.includes(order.get(i))) order.delete(i, 1);
      for (let i = 0; i < desired.length; i++) {
        if (order.get(i) === desired[i]) continue;
        const old = order.toArray().indexOf(desired[i]);
        if (old >= 0) order.delete(old, 1);
        order.insert(i, [desired[i]]);
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
        const active = document.activeElement;
        const result = await this.api(this.syncPath, "POST", {
          update: updates.length ? encode(Y.mergeUpdates(updates)) : "",
          client_id: this.clientId,
          active_block:
            active?.closest?.("[data-block]")?.dataset.block || null,
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
          this.pending.length ? "Saving…" : "Live · all changes saved",
          this.pending.length > 0,
        );
      } catch (e) {
        this.pending.unshift(...updates);
        this.onStatus(
          this.pending.length ? "Offline · changes pending" : "Reconnecting…",
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
  destroy() {
    this.closed = true;
    clearInterval(this.timer);
    clearTimeout(this.debounce);
    this.doc.destroy();
  }
}
