import { test } from "node:test";
import assert from "node:assert/strict";
import {
  sharedText,
  noteFrom,
  today,
  queued,
  setQueued,
} from "../capture-text.js";
import { offlineReading } from "../install.js";

test("shared pages and text become one note", () => {
  assert.equal(
    sharedText({ title: "A [great] read", url: "https://x.y/a" }),
    "[A \\[great\\] read](https://x.y/a)",
  );
  assert.equal(
    sharedText({
      title: "Read",
      text: "Worth it https://x.y/a",
      url: "https://x.y/a",
    }),
    "Read\n\nWorth it https://x.y/a",
    "an address already in the text is not repeated",
  );
  assert.equal(sharedText({ text: "https://x.y/b" }), "https://x.y/b");
  assert.equal(sharedText({}), "");
});

test("the first line of a capture is the note's title", () => {
  assert.deepEqual(noteFrom("Buy milk"), { title: "Buy milk", markdown: "" });
  assert.deepEqual(noteFrom("# Idea\nMore about it"), {
    title: "Idea",
    markdown: "More about it",
  });
  assert.deepEqual(noteFrom("- [ ] Call Ada\n- [ ] Book room"), {
    title: "Call Ada",
    markdown: "- [ ] Call Ada\n- [ ] Book room",
  });
  assert.deepEqual(noteFrom("[Docs](https://x.y)"), {
    title: "Docs",
    markdown: "[Docs](https://x.y)",
  });
  assert.equal(noteFrom("x".repeat(300)).title.length, 200);
  assert.equal(today(new Date(2026, 0, 5)), "2026-01-05");
});

function memory() {
  const m = new Map();
  return {
    getItem: (k) => (m.has(k) ? m.get(k) : null),
    setItem: (k, v) => m.set(k, String(v)),
    removeItem: (k) => m.delete(k),
  };
}

test("captures wait on the device", () => {
  const s = memory();
  assert.deepEqual(queued(s), []);
  assert.ok(setQueued([{ target: "folder:f", text: "hi" }], s));
  assert.deepEqual(queued(s), [{ target: "folder:f", text: "hi" }]);
  setQueued([], s);
  assert.equal(s.getItem("minerva-capture-queue"), null);
  s.setItem("minerva-capture-queue", "{broken");
  assert.deepEqual(queued(s), []);
});

test("offline reading follows the choice made on the device", () => {
  const s = memory();
  assert.equal(offlineReading(s), false, "off in a browser tab until chosen");
  s.setItem("minerva-offline-reading", "1");
  assert.equal(offlineReading(s), true);
  s.setItem("minerva-offline-reading", "0");
  assert.equal(offlineReading(s), false);
});
