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
    // Text kept in tables and translated where shown.
    for (const m of source.matchAll(/\bmsg\(\s*("(?:[^"\\]|\\.)*")/g))
      found.add(JSON.parse(m[1]));
    // Both forms of a plural: $tn(count, "one", "other").
    const plural =
      /\$tn\((?:[^"()]|\([^()]*\))*?,\s*("(?:[^"\\]|\\.)*")\s*,\s*("(?:[^"\\]|\\.)*")/g;
    for (const m of source.matchAll(plural))
      for (const form of [m[1], m[2]]) found.add(JSON.parse(form));
  }
  // The quick-capture page marks its text with data-t attributes.
  const page = fs.readFileSync(path.join(web, "capture.html"), "utf8");
  for (const m of page.matchAll(/data-t(?:-[a-z-]+)?="([^"]*)"/g))
    found.add(m[1]);
  return found;
}

/**
 * The server's text: its fixed error messages (`bad("…")`, `ApiError(…, "…")`),
 * which the app shows in the reader's language, and what it writes itself in
 * emails, published pages and exports (`tr`, `trv`, `msg` in `i18n.rs`).
 */
function serverMessages() {
  const dir = path.join(web, "../crates/minerva-server/src");
  const found = new Set();
  const decode = (s) =>
    s
      .replace(/\\\n\s*/g, "")
      .replace(/\\(["\\nt'])/g, (_, c) => ({ n: "\n", t: "\t" })[c] ?? c);
  for (const file of fs.readdirSync(dir).filter((f) => f.endsWith(".rs"))) {
    // Messages in tests are not shown to anyone.
    const code = fs
      .readFileSync(path.join(dir, file), "utf8")
      .split(/#\[cfg\(test\)\]/)[0];
    const patterns = [
      // The server's own text: tr(lang, "…"), trv(lang, "…", …), msg("…").
      /\b(?:trv?|msg)\(\s*(?:&?\w+,\s*)?"((?:[^"\\]|\\[\s\S])*)"/g,
      /\bbad\(\s*"((?:[^"\\]|\\[\s\S])*)"\s*,?\s*\)/g,
      /ApiError\(\s*StatusCode::\w+,\s*"((?:[^"\\]|\\[\s\S])*)"\s*\.\s*(?:into|to_string)\(\)/g,
    ];
    for (const re of patterns)
      for (const m of code.matchAll(re)) found.add(decode(m[1]));
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
  const used = new Set([...texts(), ...serverMessages()]);
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
