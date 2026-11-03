import { test } from "node:test";
import assert from "node:assert/strict";
import { sourceOffset, sourceRange, objectOffset } from "../caret-map.js";

/** The Markdown before the place that rendered text `prefix` ends at. */
const before = (markdown, prefix) =>
  markdown.slice(0, sourceOffset(markdown, prefix));

test("plain text and emphasis", () => {
  const md = "The first paragraph, with **bold words** and *more*.";
  assert.equal(before(md, "The first para"), "The first para");
  assert.equal(
    before(md, "The first paragraph, with bo"),
    "The first paragraph, with **bo",
  );
  assert.equal(
    before(md, "The first paragraph, with bold words and"),
    "The first paragraph, with **bold words** and",
  );
  assert.equal(before(md, ""), "");
});

test("links skip their address", () => {
  const md = "See [the example](https://example.com/e) every day";
  assert.equal(before(md, "See the ex"), "See [the ex");
  assert.equal(
    before(md, "See the example e"),
    "See [the example](https://example.com/e) e",
  );
});

test("block markers", () => {
  assert.equal(before("# A big heading", "A big"), "# A big");
  assert.equal(before("  - [ ] Call the bank", "Call"), "  - [ ] Call");
  assert.equal(
    before("> [!NOTE]\n> Callouts here", "Callouts"),
    "> [!NOTE]\n> Callouts",
  );
  assert.equal(before("1. first\n2. second", "first\nsec"), "1. first\n2. sec");
  assert.equal(before("- a\n- b\n  - c", "a\nb\nc"), "- a\n- b\n  - c");
});

test("rendered text the Markdown does not have", () => {
  // A mention shows the person's name for their address.
  const md = "Ask @ada@example.com about it";
  assert.equal(before(md, "Ask @Ada about"), "Ask @ada@example.com about");
  // A task box or icon before the text.
  assert.equal(before("- [x] Done task", "☑Done"), "- [x] Done");
  // Cloze braces and typographic quotes.
  assert.equal(
    before("The answer is {{c1::Paris}} indeed", "The answer is Paris in"),
    "The answer is {{c1::Paris}} in",
  );
  assert.equal(
    before('She said "hi" twice', "She said “hi” tw"),
    'She said "hi" tw',
  );
});

test("code and highlights", () => {
  assert.equal(before("Run `cargo test` now", "Run cargo"), "Run `cargo");
  assert.equal(
    before("Some ==marked== text", "Some marked te"),
    "Some ==marked== te",
  );
});

test("selections keep to the text, not its markers", () => {
  const md = "With **bold words** and [a link](https://x.y) here";
  const range = (prefix, selected) => {
    const { start, end } = sourceRange(md, prefix, selected);
    return md.slice(start, end);
  };
  assert.equal(range("With ", "bold words"), "bold words");
  assert.equal(range("With bold ", "words and a"), "words** and [a");
  assert.equal(range("With bold words and ", "a link"), "a link");
});

test("pictures, formulas and diagrams", () => {
  const md =
    "Look ![a cat](/a.png) and ![dog](/d.png), $E = mc^2$ costs $5 and $10.";
  assert.equal(
    md.slice(objectOffset(md, "image", 1)),
    "![dog](/d.png), $E = mc^2$ costs $5 and $10.",
  );
  assert.equal(objectOffset(md, "image", 2), null);
  assert.equal(
    md.slice(objectOffset(md, "math", 0), objectOffset(md, "math", 0) + 3),
    "$E ",
  );
  assert.equal(
    objectOffset(md, "math", 1),
    null,
    "dollar amounts are not math",
  );
  assert.equal(objectOffset("$$\nx^2\n$$", "math", 0), 0);
  assert.equal(
    objectOffset("Before\n```mermaid\ngraph TD\n```", "diagram", 0),
    7,
  );
  // Text after a picture or formula still maps: their source is skipped.
  assert.equal(
    before(md, "Look and , costs $5 and $1"),
    "Look ![a cat](/a.png) and ![dog](/d.png), $E = mc^2$ costs $5 and $1",
  );
});
