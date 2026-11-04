import { test } from "node:test";
import assert from "node:assert/strict";
import { commentHTML, ago, findQuote } from "../comments.js";

const esc = (s) =>
  String(s).replace(
    /[&<>"]/g,
    (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" })[c],
  );

test("comments are escaped, with mentions by name and line breaks", () => {
  assert.equal(
    commentHTML(
      "Hi @ada@test.local <b>\nthanks",
      esc,
      new Map([["ada@test.local", "Ada"]]),
    ),
    'Hi <span class="mention" title="ada@test.local">@Ada</span> &lt;b&gt;<br>thanks',
  );
  assert.equal(commentHTML("mail me at x@y.com", esc), "mail me at x@y.com");
});

test("times read as just now, minutes, hours, then a date", () => {
  assert.equal(ago(1000, 1030), "just now");
  assert.equal(ago(1000, 1000 + 300), "5 min ago");
  assert.equal(ago(1000, 1000 + 3 * 3600), "3 h ago");
});

test("a quote is found in the rendered text, Markdown marks aside", () => {
  const text = "Plants turn   light into sugar, as [the book](x) says.";
  const [a, b] = findQuote(text, "turn light **into** sugar");
  assert.equal(text.slice(a, b), "turn   light into sugar");
  const [c, d] = findQuote(
    "See the book says",
    "[the book](https://x.example) says",
  );
  assert.equal("See the book says".slice(c, d), "the book says");
  assert.equal(findQuote("Nothing here", "absent"), null);
  assert.equal(findQuote("Short", "*"), null);
});
