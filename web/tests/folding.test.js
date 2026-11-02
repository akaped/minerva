import { test } from "node:test";
import assert from "node:assert/strict";
import {
  subtreeEnd,
  hasChildren,
  hiddenBlocks,
  foldedAbove,
} from "../folding.js";

const level = (md) => Math.floor((md.length - md.trimStart().length) / 2);
const blocks = ["- a", "  - b", "    - c", "  - d", "- e", "f"].map(
  (markdown, i) => ({ id: "abcdef"[i], markdown }),
);

test("a block's nested blocks are those indented further, right after it", () => {
  assert.equal(subtreeEnd(blocks, 0, level), 4);
  assert.equal(subtreeEnd(blocks, 1, level), 3);
  assert.ok(hasChildren(blocks, 0, level));
  assert.ok(!hasChildren(blocks, 3, level));
  assert.ok(!hasChildren(blocks, 5, level));
});

test("folding hides the nested blocks, and finds what hides a block", () => {
  assert.deepEqual(
    [...hiddenBlocks(blocks, new Set(["a"]), level)],
    ["b", "c", "d"],
  );
  assert.deepEqual([...hiddenBlocks(blocks, new Set(["b"]), level)], ["c"]);
  assert.deepEqual(
    [...hiddenBlocks(blocks, new Set(["b", "a"]), level)],
    ["b", "c", "d"],
  );
  assert.deepEqual(foldedAbove(blocks, "c", new Set(["a", "b"]), level), [
    "b",
    "a",
  ]);
  assert.deepEqual(foldedAbove(blocks, "e", new Set(["a"]), level), []);
});
