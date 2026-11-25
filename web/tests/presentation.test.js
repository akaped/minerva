import { test } from "node:test";
import assert from "node:assert/strict";
import { slideGroups, slideTemplate } from "../presentation.js";
import { splitMarkdown } from "../markdown.js";

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
    layouts: ["normal", "normal", "normal"],
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
    layouts: ["normal", "normal"],
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
    layouts: ["normal", "normal"],
  });
});

test("a note without headings or dividers is all on the title slide", () => {
  assert.deepEqual(slideGroups(["one", "two", "  "]), {
    intro: [0, 1],
    slides: [],
    layouts: [],
  });
  assert.deepEqual(slideGroups([]), { intro: [], slides: [], layouts: [] });
});

test("empty slides between dividers are dropped, and a #tag is not a heading", () => {
  assert.deepEqual(slideGroups(["---", "---", "a", "---"]), {
    intro: [],
    slides: [[2]],
    layouts: ["normal"],
  });
  assert.deepEqual(slideGroups(["#hashtag is not a heading", "# Real"]), {
    intro: [0],
    slides: [[1]],
    layouts: ["normal"],
  });
});

test("slide templates keep their editable fields and layout through Markdown import", () => {
  const layouts = ["normal", "title", "section", "image-right", "image-left"];
  const templates = layouts.map(slideTemplate);
  assert.deepEqual(templates.map((blocks) => blocks.length), [3, 3, 2, 4, 4]);
  for (const blocks of [templates.flat(), splitMarkdown(templates.flat().join("\n\n"))]) {
    const groups = slideGroups(blocks);
    assert.deepEqual(groups.layouts, layouts);
    assert.deepEqual(groups.slides.map((slide) => slide.length), [2, 2, 1, 3, 3]);
  }
});
