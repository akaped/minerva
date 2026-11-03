import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import path from "node:path";
import { $t, keys, pickLanguage, serverMessage, useLanguage } from "../i18n.js";
import it from "../locales/it.json" with { type: "json" };

const web = path.join(import.meta.dirname, "..");

/** Every `$t("…")` text in the web app's code. */
function texts() {
  const found = new Set();
  for (const file of fs.readdirSync(web).filter((f) => f.endsWith(".js"))) {
    let source = fs.readFileSync(path.join(web, file), "utf8");
    // i18n.js has examples in its comments, which are not interface text.
    if (file === "i18n.js")
      source = source.replace(/\/\*[\s\S]*?\*\//g, "").replace(/\/\/.*$/gm, "");
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
  // Bundled plugins: the names, descriptions and audiences in their headers.
  const plugins = path.join(web, "../plugins");
  for (const kind of ["themes", "public-notes-themes", "pdf-prints"])
    for (const file of fs.readdirSync(path.join(plugins, kind)))
      if (file.endsWith(".css"))
        for (const m of fs
          .readFileSync(path.join(plugins, kind, file), "utf8")
          .matchAll(/^ \* (?:name|description|users): (.+)$/gm))
          found.add(m[1].trim());
  // The quick-capture page marks its text with data-t attributes.
  const page = fs.readFileSync(path.join(web, "capture.html"), "utf8");
  for (const m of page.matchAll(/data-t(?:-[a-z-]+)?="([^"]*)"/g))
    found.add(m[1]);
  return found;
}

/** Rust code without its `#[cfg(test)] mod … { … }` modules. */
function withoutTests(code) {
  const start = /#\[cfg\(test\)\]\s*mod\s+\w+\s*\{/g;
  let out = "";
  let from = 0;
  for (const m of code.matchAll(start)) {
    if (m.index < from) continue;
    out += code.slice(from, m.index);
    let depth = 0;
    let i = m.index + m[0].length - 1;
    for (; i < code.length; i++) {
      const c = code[i];
      if (c === '"') {
        // A string: skip to its end, past escaped quotes.
        for (i++; i < code.length && code[i] !== '"'; i++)
          if (code[i] === "\\") i++;
      } else if (c === "r" && /^r#+"/.test(code.slice(i, i + 4))) {
        const hashes = /^r(#+)"/.exec(code.slice(i))[1];
        i = code.indexOf(`"${hashes}`, i + hashes.length + 2) + hashes.length;
      } else if (c === "'" && code[i + 2] === "'") i += 2;
      else if (c === "'" && code[i + 1] === "\\") i = code.indexOf("'", i + 2);
      else if (c === "{") depth++;
      else if (c === "}" && --depth === 0) break;
    }
    from = i + 1;
  }
  return out + code.slice(from);
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
    const code = withoutTests(fs.readFileSync(path.join(dir, file), "utf8"));
    const patterns = [
      // The server's own text: tr(lang, "…"), trv(lang, "…", …), msg("…").
      /\b(?:trv?|msg|badv|fill)\(\s*(?:&?\w+,\s*)?"((?:[^"\\]|\\[\s\S])*)"/g,
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

test("messages from the server are found by their templates", () => {
  useLanguage("it");
  assert.equal(serverMessage("Not found"), "Non trovato");
  assert.equal(serverMessage("Missing workspace_id"), "Manca workspace_id");
  assert.equal(
    serverMessage(
      "Provider returned HTTP 429. Check its key, model and quota.",
    ),
    "Il fornitore ha restituito HTTP 429. Controlla chiave, modello e quota.",
  );
  assert.equal(
    serverMessage("Something nobody wrote"),
    "Something nobody wrote",
  );
  useLanguage("en");
  assert.equal(serverMessage("Missing workspace_id"), "Missing workspace_id");
});

test("shortcuts name their keys for the language", () => {
  useLanguage("it");
  assert.equal(keys("Ctrl+Shift+F"), "Ctrl+Maiusc+F");
  useLanguage("en");
  assert.equal(keys("Ctrl+Shift+F"), "Ctrl+Shift+F");
});
