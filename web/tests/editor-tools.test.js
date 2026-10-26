import test from "node:test";
import assert from "node:assert/strict";
import {
  toggleWrap,
  linkSelection,
  noteLink,
  linkQuery,
  mentionQuery,
  findMatches,
  replaceAll,
  countWords,
  normalizeTag,
  htmlToMarkdown,
} from "../editor-tools.js";

const apply = (value, r) => value.slice(0, r.from) + r.text + value.slice(r.to);

test("formatting toggles wrap and unwrap the selection", () => {
  let value = "make this bold now";
  let r = toggleWrap(value, 5, 15, "**");
  value = apply(value, r);
  assert.equal(value, "make **this bold** now");
  assert.equal(value.slice(r.selStart, r.selEnd), "this bold");
  r = toggleWrap(value, r.selStart, r.selEnd, "**");
  assert.equal(
    apply(value, r),
    "make this bold now",
    "toggling again removes markers",
  );
  // Markers inside the selection, and trailing space kept outside.
  assert.equal(apply("==hi==", toggleWrap("==hi==", 0, 6, "==")), "hi");
  assert.equal(
    apply("word next", toggleWrap("word next", 0, 5, "*")),
    "*word* next",
  );
  // Empty selection inserts markers with the caret between them.
  r = toggleWrap("ab", 1, 1, "`");
  assert.equal(apply("ab", r), "a``b");
  assert.equal(r.selStart, 2);
});

test("links select the URL placeholder and note links escape titles", () => {
  const r = linkSelection("see docs here", 4, 8);
  const value = apply("see docs here", r);
  assert.equal(value, "see [docs](https://) here");
  assert.equal(value.slice(r.selStart, r.selEnd), "https://");
  assert.equal(
    apply("https://a.example", linkSelection("https://a.example", 0, 17)),
    "[https://a.example](https://a.example)",
  );
  assert.equal(noteLink("Plan [v2]", "abc"), "[Plan \\[v2\\]](#note/abc)");
  assert.deepEqual(linkQuery("See [[Road", 10), { start: 4, query: "Road" });
  assert.equal(linkQuery("See [[Road]] x", 14), null);
  assert.equal(linkQuery("a [b] c", 7), null);
});

test("find and replace operate on Markdown, optionally case-sensitive", () => {
  const blocks = [
    { id: "a", markdown: "Alpha alpha ALPHA" },
    { id: "b", markdown: "beta" },
  ];
  assert.equal(findMatches(blocks, "alpha").length, 3);
  assert.equal(findMatches(blocks, "alpha", true).length, 1);
  assert.deepEqual(findMatches(blocks, "beta")[0], {
    id: "b",
    start: 0,
    end: 4,
  });
  assert.deepEqual(replaceAll("aaa", "a", "bb"), { text: "bbbbbb", count: 3 });
  assert.deepEqual(replaceAll("Cat cat", "cat", "dog", true), {
    text: "Cat dog",
    count: 1,
  });
  assert.equal(findMatches(blocks, "").length, 0);
});

test("word counts ignore Markdown syntax and URLs", () => {
  const stats = countWords([
    { markdown: "## Hello **world**" },
    {
      markdown:
        "[a link](https://example.com/very/long) and ![img](x.png) ünïcode words",
    },
  ]);
  assert.equal(stats.words, 7);
  assert.equal(stats.minutes, 1);
});

test("tags normalize like the server", () => {
  assert.equal(normalizeTag(" #Work-Notes "), "work-notes");
  assert.equal(normalizeTag("project/alpha"), "project/alpha");
  assert.equal(normalizeTag("#123"), null);
  assert.equal(normalizeTag("two words"), null);
});

test("pasted HTML becomes Markdown, including Google Docs styling", () => {
  assert.equal(
    htmlToMarkdown(
      "<h2>Title</h2><p>Some <strong>bold</strong>, <em>italic</em>, <mark>marked</mark> and <a href='https://x.example'>link</a>.</p><ul><li>one</li><li>two</li></ul>",
    ),
    "## Title\n\nSome **bold**, *italic*, ==marked== and [link](https://x.example).\n\n-   one\n-   two",
  );
  assert.equal(
    htmlToMarkdown(
      '<meta charset="utf-8"><b style="font-weight:normal;" id="docs-internal-guid-1"><p><span style="font-weight:700;">Strong</span> <span style="font-style:italic;">soft</span> <span style="text-decoration:line-through;">gone</span></p></b>',
    ),
    "**Strong** *soft* ~~gone~~",
  );
  assert.equal(
    htmlToMarkdown(
      "<table><thead><tr><th>A</th><th>B</th></tr></thead><tbody><tr><td>1</td><td>2</td></tr></tbody></table>",
    ),
    "| A | B |\n| --- | --- |\n| 1 | 2 |",
  );
  assert.equal(
    htmlToMarkdown("<pre><code class='language-js'>let x = 1;</code></pre>"),
    "```js\nlet x = 1;\n```",
  );
  assert.ok(
    !htmlToMarkdown("<p>ok</p><script>alert(1)</script>").includes("alert"),
  );
});

test("@ opens the mention picker after a space or at the start", () => {
  assert.deepEqual(mentionQuery("@", 1), { start: 0, query: "" });
  assert.deepEqual(mentionQuery("Ask @ha", 7), { start: 4, query: "ha" });
  assert.deepEqual(mentionQuery("(cc @gil", 8), { start: 4, query: "gil" });
  // Not inside an address, after the mention is complete, or after a space.
  assert.equal(mentionQuery("mail ana@test.it", 16), null);
  assert.equal(mentionQuery("Ask @hana@demo.local", 20), null);
  assert.equal(mentionQuery("Ask @ha now", 11), null);
});
