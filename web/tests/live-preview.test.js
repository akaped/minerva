import { test } from "node:test";
import assert from "node:assert/strict";
import { previewHTML, headingLevel, frameOf } from "../live-preview.js";

const esc = (s) =>
  String(s).replace(
    /[&<>"]/g,
    (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c],
  );
// The layer must show exactly the field's text, or the two drift apart.
const text = (html) =>
  html
    .replace(/<[^>]+>/g, "")
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&amp;/g, "&")
    .replace(/​$/, "");

test("markers are dimmed and marked text styled, character for character", () => {
  for (const md of [
    "Plain **bold** and *italic* and `code` and ~~gone~~ and ==lit==",
    "## A heading with [a link](https://example.com) <b>",
    "- [ ] a task\n  - nested **item**\n> quote",
    "```js\nconst a = `x` ** 2;\n```\nafter",
    "{{c1::Rome}} is the capital, 2*3*4 is not italic, snake_case_name",
    "trailing newline\n",
  ])
    assert.equal(text(previewHTML(md, esc)), md, md);
  const html = previewHTML("Some **bold** [x](#note/1)", esc);
  assert.match(
    html,
    /<span class="lp-mark">\*\*<\/span><span class="lp-bold">bold<\/span>/,
  );
  assert.match(html, /<span class="lp-link">x<\/span>/);
  assert.doesNotMatch(
    previewHTML("2*3*4 and snake_case_name", esc),
    /lp-italic/,
  );
  assert.doesNotMatch(previewHTML("```\n**not bold**\n```", esc), /lp-bold/);
  assert.match(previewHTML("<img src=x onerror=alert(1)>", esc), /^&lt;img/);
});

test("heading blocks are edited at their size", () => {
  assert.equal(headingLevel("# Title"), 1);
  assert.equal(headingLevel("### Small"), 3);
  assert.equal(headingLevel("#### Smaller"), 0);
  assert.equal(headingLevel("#tag"), 0);
});

test("quotes and callouts are edited in their frame", () => {
  assert.equal(frameOf("> A quote\n> on two lines"), "quote");
  assert.equal(frameOf("> [!WARNING]\n> Back up first."), "warning");
  assert.equal(frameOf("> [!nonsense] text"), "quote");
  assert.equal(frameOf("> quoted\nnot quoted"), "");
  assert.equal(frameOf("Plain"), "");
  assert.match(
    previewHTML("> [!TIP]\n> Use **it**", esc),
    /<span class="lp-callout-kind">\[!TIP\]<\/span>/,
  );
});
