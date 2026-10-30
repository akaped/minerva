import test from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import { searchEmoji } from "../emoji-picker.js";

const { emoji } = JSON.parse(fs.readFileSync("web/dist/emoji.json", "utf8"));

test("emoji are found by name and tags, every word matching", () => {
  assert.ok(emoji.length > 1800);
  assert.ok(searchEmoji(emoji, "pizza").some((e) => e[0] === "🍕"));
  // A tag, not the name: 🍕 is tagged "pepperoni".
  assert.ok(searchEmoji(emoji, "pepperoni").some((e) => e[0] === "🍕"));
  const cats = searchEmoji(emoji, "smile cat");
  assert.ok(cats.length > 0 && cats.every((e) => e[2].includes("cat")));
  assert.equal(searchEmoji(emoji, "zzznotanemoji").length, 0);
  assert.equal(searchEmoji(emoji, "  ").length, emoji.length);
});

test("the best match comes first", () => {
  assert.equal(searchEmoji(emoji, "rocket")[0][0], "🚀");
  assert.equal(searchEmoji(emoji, "red heart")[0][1], "red heart");
  assert.equal(searchEmoji(emoji, "pizza")[0][0], "🍕");
});
