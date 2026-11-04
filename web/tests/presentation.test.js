import { test } from "node:test";
import assert from "node:assert/strict";
import { slideGroups } from "../presentation.js";

test("dividers split the slides, and are left out", () => {
  const blocks = [
    "# Plan",
    "Why",
    "---",
    "## Steps",
    "- one",
    "***",
    "",
    "Thanks",
  ];
  assert.deepEqual(slideGroups(blocks), {
    intro: [],
    slides: [[0, 1], [3, 4], [7]],
  });
});

test("without dividers, each top-level heading starts a slide", () => {
  const blocks = [
    "An opening line",
    "## First",
    "Text",
    "### Detail",
    "More",
    "## Second",
    "End",
  ];
  assert.deepEqual(slideGroups(blocks), {
    intro: [0],
    slides: [
      [1, 2, 3, 4],
      [5, 6],
    ],
  });
});

test("the largest heading used sets the split, even if it is not first", () => {
  const blocks = [
    "### Small",
    "a",
    "# Big",
    "b",
    "### Small again",
    "# Big again",
  ];
  assert.deepEqual(slideGroups(blocks), {
    intro: [0, 1],
    slides: [[2, 3, 4], [5]],
  });
});

test("a note without headings or dividers is all on the title slide", () => {
  assert.deepEqual(slideGroups(["one", "two", "  "]), {
    intro: [0, 1],
    slides: [],
  });
  assert.deepEqual(slideGroups([]), { intro: [], slides: [] });
});

test("empty slides between dividers are dropped, and a #tag is not a heading", () => {
  assert.deepEqual(slideGroups(["---", "---", "a", "---"]), {
    intro: [],
    slides: [[2]],
  });
  assert.deepEqual(slideGroups(["#hashtag is not a heading", "# Real"]), {
    intro: [0],
    slides: [[1]],
  });
});
