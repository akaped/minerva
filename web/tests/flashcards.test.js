import { test } from "node:test";
import assert from "node:assert/strict";
import { rich } from "../flashcards.js";

const esc = (s) =>
  String(s).replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ],
  );

test("card text shows pictures and links, and nothing else as markup", () => {
  const id = "0b8f5f9e-0000-4000-8000-000000000000";
  assert.equal(
    rich(
      `See ![](media:${id}) and [Rome](https://en.wikipedia.org/wiki/Rome)`,
      esc,
    ),
    `See <img class="fc-picture" src="/api/v1/flashcards/media/${id}" alt="" loading="lazy"> and <a href="https://en.wikipedia.org/wiki/Rome" target="_blank" rel="noopener noreferrer">Rome</a>`,
  );
  assert.match(
    rich("![](https://example.com/a.png)", esc),
    /<img class="fc-picture" src="https:\/\/example.com\/a.png"/,
  );
  for (const hostile of [
    "<img src=x onerror=alert(1)>",
    "[x](javascript:alert(1))",
    '[x](https://a.example/"onmouseover="alert(1))',
    "![](media:../../etc)",
  ]) {
    const html = rich(hostile, esc);
    // Only the three allowed elements are markup; everything else stays text.
    assert.doesNotMatch(
      html,
      /<(?!img class="fc-picture" src="(\/api\/v1\/flashcards\/media\/[0-9a-f-]{36}|https:)|a href="https:|\/a>)/,
      hostile,
    );
    assert.doesNotMatch(html, /href="javascript|"\s*onmouseover/i, hostile);
  }
});
