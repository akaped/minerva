import { test } from "node:test";
import assert from "node:assert/strict";
import {
  noteTarget,
  slotTarget,
  blockLabel,
  blockLink,
  embedCode,
  embedAllowed,
} from "../note-embeds.js";

const A = "0b8f5f9e-0000-4000-8000-00000000000a";
const B = "0b8f5f9e-0000-4000-8000-00000000000b";

test("note and block addresses", () => {
  assert.deepEqual(noteTarget(`#note/${A}`), { note: A, block: null });
  assert.deepEqual(noteTarget(`#note/${A}#b-1`), { note: A, block: "b-1" });
  assert.equal(noteTarget("#note/123"), null);
  assert.equal(noteTarget(`#note/${A}#`), null);
  assert.equal(noteTarget(`#note/${A}#a b`), null);
  assert.equal(noteTarget("https://example.com"), null);
  assert.deepEqual(slotTarget(["note-embed", `ne-note-${A}`, "ne-block-x"]), {
    note: A,
    block: "x",
  });
  assert.deepEqual(slotTarget(["note-embed", `ne-note-${A}`]), {
    note: A,
    block: null,
  });
  assert.equal(slotTarget(["note-embed"]), null);
});

test("labels and Markdown for links and embeds", () => {
  assert.equal(blockLabel("## The **plan**"), "The plan");
  assert.equal(blockLabel("- [ ] Call [Ada](mailto:a@b.c)"), "Call Ada");
  assert.equal(blockLabel("> [!NOTE] Mind the gap"), "Mind the gap");
  assert.equal(blockLabel("x".repeat(80), 10), "xxxxxxxxx…");
  assert.equal(blockLink("a [b]", A, "k"), `[a \\[b\\]](#note/${A}#k)`);
  assert.equal(embedCode("Plan", A), `![Plan](#note/${A})`);
  assert.equal(embedCode("Plan", A, "k"), `![Plan](#note/${A}#k)`);
});

test("embeds never show themselves or nest without end", () => {
  const host = { note: A, block: "h" };
  const ok = (target, outer = []) => embedAllowed(target, outer, host);
  assert.equal(ok({ note: B, block: null }), "ok");
  assert.equal(
    ok({ note: A, block: "other" }),
    "ok",
    "another block of the same note",
  );
  assert.equal(ok({ note: A, block: null }), "loop", "the open note itself");
  assert.equal(
    ok({ note: A, block: "h" }),
    "loop",
    "the block it is written in",
  );
  assert.equal(ok({ note: B, block: "x" }, [{ note: B, block: null }]), "loop");
  assert.equal(ok({ note: B, block: "x" }, [{ note: B, block: "x" }]), "loop");
  assert.equal(ok({ note: B, block: "y" }, [{ note: B, block: "x" }]), "ok");
  const three = [1, 2, 3].map((i) => ({ note: B, block: `b${i}` }));
  assert.equal(ok({ note: B, block: "z" }, three), "deep");
});
