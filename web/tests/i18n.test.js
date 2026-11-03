import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { $t, pickLanguage, useLanguage } from "../i18n.js";
import it from "../locales/it.json" with { type: "json" };

const web = path.join(import.meta.dirname, "..");

/** Every `$t("…")` text in the web app's code. */
function texts() {
  const found = new Set();
  // i18n.js itself only has examples in its comments.
  const code = fs
    .readdirSync(web)
    .filter((f) => f.endsWith(".js") && f !== "i18n.js");
  for (const file of code) {
    const source = fs.readFileSync(path.join(web, file), "utf8");
    for (const m of source.matchAll(/\$t\(\s*("(?:[^"\\]|\\.)*")/g))
      found.add(JSON.parse(m[1]));
  }
  return found;
}

test("the language follows the choice, then the browser", () => {
  assert.equal(pickLanguage("it", ["en-US"]), "it");
  assert.equal(pickLanguage("", ["fr-FR", "it-IT"]), "it");
  assert.equal(pickLanguage("", ["fr-FR"]), "en");
  assert.equal(pickLanguage("xx", []), "en");
});

test("texts are translated, with their values", () => {
  useLanguage("it");
  assert.equal($t("Cancel"), "Annulla");
  assert.equal($t("Not in any catalog"), "Not in any catalog");
  assert.equal($t("{n} of {m}", { n: 1, m: 2 }), "1 of 2");
  useLanguage("en");
  assert.equal($t("Cancel"), "Cancel");
});

test("the Italian catalog covers the code, and only it", () => {
  const used = texts();
  const missing = [...used].filter((t) => !(t in it));
  assert.deepEqual(missing, [], "texts without an Italian translation");
  const stale = Object.keys(it).filter((t) => !used.has(t));
  assert.deepEqual(stale, [], "translations no longer used");
});

test("translations are safe in HTML and keep their values", () => {
  for (const [english, italian] of Object.entries(it)) {
    assert.ok(italian.trim(), `empty translation for ${english}`);
    for (const c of ["<", ">", '"'])
      if (!english.includes(c))
        assert.ok(!italian.includes(c), `${c} in ${italian}`);
    const values = (s) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
    assert.deepEqual(values(italian), values(english), english);
  }
});
