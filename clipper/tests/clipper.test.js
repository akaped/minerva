import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import {
  compose,
  fit,
  link,
  parseTags,
  serverOrigin,
  siteName,
} from "../src/compose.js";
import { manifest, FIREFOX_ID } from "../manifest.js";

const words = {
  clippedFrom: (s) => `Clipped from ${s}`,
  foundOn: (s) => `Found on ${s}`,
  cutShort: () => "The rest was too long.",
};
const t = (key, ...values) => words[key](...values);
const taken = {
  url: "https://www.example.com/post?id=1",
  title: "A post",
  markdown: "# A post\n\nBody.",
  excerpt: "  About   things. ",
  byline: "Ada",
  site: "",
};

test("server addresses become origins", () => {
  assert.equal(serverOrigin("notes.example.com"), "https://notes.example.com");
  assert.equal(serverOrigin(" https://notes.example.com/app/ "), "https://notes.example.com");
  assert.equal(serverOrigin("http://localhost:3000"), "http://localhost:3000");
  assert.equal(serverOrigin("ftp://example.com"), null);
  assert.equal(serverOrigin("https://user:pw@example.com"), null);
  assert.equal(serverOrigin(""), null);
  assert.equal(siteName("https://www.example.com/a"), "example.com");
});

test("a page clip starts with its source", () => {
  const note = compose("page", taken, {}, t);
  assert.equal(note.title, "A post");
  assert.equal(note.url, taken.url);
  assert.equal(
    note.markdown,
    "Clipped from [example.com](https://www.example.com/post?id=1) · Ada\n\n# A post\n\nBody.",
  );
});

test("selections, bookmarks, links and pictures", () => {
  assert.match(compose("selection", { ...taken, markdown: "Picked." }, {}, t).markdown, /\)\n\nPicked\.$/);
  assert.match(compose("bookmark", taken, {}, t).markdown, /\n\n> About things\.$/);
  const linked = compose("link", taken, { linkUrl: "https://other.example/x", text: " Other [page] " }, t);
  assert.equal(linked.title, "Other [page]");
  assert.equal(linked.url, "https://other.example/x");
  assert.equal(
    linked.markdown,
    "[Other \\[page\\]](https://other.example/x)\n\nFound on [A post](https://www.example.com/post?id=1)",
  );
  const picture = compose("image", taken, { srcUrl: "https://img.example/a (1).png" }, t);
  assert.match(picture.markdown, /!\[\]\(https:\/\/img\.example\/a%20%281%29\.png\)$/);
});

test("links cannot break out of their Markdown", () => {
  assert.equal(link("a]b", "https://x.example/(y)"), "[a\\]b](https://x.example/%28y%29)");
});

test("long pages are cut at a paragraph, within the server's limit", () => {
  const long = Array.from({ length: 500 }, (_, i) => `Paragraph ${i} ${"é".repeat(300)}`).join("\n\n");
  const cut = fit(long, "Cut.", 50_000);
  assert.ok(new TextEncoder().encode(cut).length <= 50_000);
  assert.match(cut, /Paragraph \d+ é+\n\n\*Cut\.\*$/);
  assert.equal(fit("short", "Cut."), "short");
});

test("tags as typed", () => {
  assert.deepEqual(parseTags("#Reading, research  reading"), ["reading", "research"]);
  assert.deepEqual(parseTags(""), []);
});

test("each browser gets its manifest", () => {
  const chrome = manifest("chrome", "1.2.3");
  const firefox = manifest("firefox", "1.2.3");
  assert.equal(chrome.background.service_worker, "background.js");
  assert.equal(chrome.browser_specific_settings, undefined);
  assert.deepEqual(firefox.background.scripts, ["background.js"]);
  assert.equal(firefox.browser_specific_settings.gecko.id, FIREFOX_ID);
  for (const m of [chrome, firefox]) {
    assert.equal(m.manifest_version, 3);
    assert.equal(m.version, "1.2.3");
    // No site is readable until the person clicks, and only their server is
    // reachable, after they connect.
    assert.equal(m.host_permissions, undefined);
    assert.equal(m.content_scripts, undefined);
    assert.ok(m.permissions.includes("activeTab"));
  }
});

test("every message exists in every language, with the same placeholders", () => {
  const read = (lang) =>
    JSON.parse(readFileSync(new URL(`../_locales/${lang}/messages.json`, import.meta.url)));
  const en = read("en");
  const it = read("it");
  assert.deepEqual(Object.keys(it).sort(), Object.keys(en).sort());
  for (const [key, value] of Object.entries(en))
    assert.deepEqual(it[key].placeholders, value.placeholders, key);
  // Every key used in the code is in the catalog.
  const code = ["background.js", "popup.js", "options.js", "minerva.js", "popup.html", "options.html"]
    .map((f) => readFileSync(new URL(`../src/${f}`, import.meta.url), "utf8"))
    .join("\n");
  const used = new Set(
    [...code.matchAll(/\bt\("([A-Za-z]+)"|data-i18n(?:-placeholder)?="([A-Za-z]+)"|"(menu[A-Za-z]+)"/g)].map(
      (m) => m[1] || m[2] || m[3],
    ),
  );
  for (const key of used) assert.ok(en[key], `missing message ${key}`);
});
