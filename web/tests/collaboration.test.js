import test from "node:test";
import assert from "node:assert/strict";
import * as Y from "yjs";
import {
  replaceText,
  encode,
  decode,
  Collaboration,
} from "../collaboration.js";
test("concurrent edits in the same Unicode block converge without losing either edit", () => {
  const original = new Y.Doc();
  original.getText("body").insert(0, "Hello 🌍 world");
  const a = new Y.Doc(),
    b = new Y.Doc();
  for (const doc of [a, b]) Y.applyUpdate(doc, Y.encodeStateAsUpdate(original));
  replaceText(a.getText("body"), "Hello 🌎 world");
  replaceText(b.getText("body"), "Hello 🌍 world!");
  const ua = Y.encodeStateAsUpdate(a),
    ub = Y.encodeStateAsUpdate(b);
  Y.applyUpdate(a, ub);
  Y.applyUpdate(b, ua);
  assert.equal(a.getText("body").toString(), b.getText("body").toString());
  assert.equal(a.getText("body").toString(), "Hello 🌎 world!");
  Y.applyUpdate(a, decode(encode(ub)));
  assert.equal(a.getText("body").toString(), "Hello 🌎 world!");
});
test("text replacements preserve strings across insert, delete, emoji and line breaks", () => {
  const doc = new Y.Doc(),
    text = doc.getText("body");
  for (const value of [
    "",
    "hello",
    "hallo",
    "🌍 hello",
    "🌎 hello",
    "🌎\nhello",
    "hello",
    "",
    "a",
  ]) {
    replaceText(text, value);
    assert.equal(text.toString(), value);
  }
});

// A provider connected to an in-memory "server" document.
async function connected() {
  globalThis.document ??= { activeElement: null };
  const server = new Y.Doc();
  server.getText("title").insert(0, "Note");
  server.getArray("order").insert(0, ["a"]);
  server.getMap("meta").set("icon", "▤");
  server.getText("block:a").insert(0, "Hello");
  const api = async (_path, _method, body) => {
    if (body.update) Y.applyUpdate(server, decode(body.update));
    return { update: encode(Y.encodeStateAsUpdate(server)) };
  };
  const live = new Collaboration(
    "n",
    api,
    () => {},
    () => {},
    () => {},
  );
  await live.start();
  clearInterval(live.timer);
  return { server, live };
}
test("undo reverts only local changes and keeps a collaborator's concurrent edit", async () => {
  const { server, live } = await connected();
  const note = live.snapshot();
  note.blocks[0].markdown = "Hello local";
  live.commit(note);
  await live.sync();
  server.getText("block:a").insert(0, "Remote: ");
  await live.sync();
  assert.equal(live.snapshot().blocks[0].markdown, "Remote: Hello local");
  assert.equal(live.undo(), true);
  assert.equal(live.snapshot().blocks[0].markdown, "Remote: Hello");
  assert.equal(live.redo(), true);
  assert.equal(live.snapshot().blocks[0].markdown, "Remote: Hello local");
  // Structural changes (a new block) are undone as a separate step.
  live.boundary();
  const split = live.snapshot();
  split.blocks.push({ id: "b", markdown: "Second" });
  live.commit(split);
  assert.equal(live.undo(), true);
  assert.deepEqual(
    live.snapshot().blocks.map((b) => b.id),
    ["a"],
  );
  live.destroy();
});
test("IME composition merges with remote edits that arrive while composing", async () => {
  const { server, live } = await connected();
  const state = live.beginComposition("block:a", 5, 5, "Hello");
  server.getText("block:a").insert(0, "Hey! ");
  await live.sync();
  const { value, caret } = live.endComposition(state, "Hello世界");
  assert.equal(value, "Hey! Hello世界");
  assert.equal(caret, value.length);
  await live.sync();
  assert.equal(server.getText("block:a").toString(), "Hey! Hello世界");
  // Replacing a selection during composition.
  const replace = live.beginComposition("block:a", 0, 4, value);
  const merged = live.endComposition(replace, "やあ " + value.slice(4));
  assert.equal(merged.value, "やあ  Hello世界");
  live.destroy();
});
