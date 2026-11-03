/**
 * The interface in the reader's language.
 *
 * Interface text is written in English in the code, wrapped in `$t("…")`; each
 * language has a catalog (`web/locales/{code}.json`) from the English text to
 * its translation, and text missing from a catalog stays in English. Values
 * go in braces: `$t("Shared with {name}", { name })`, and counts choose their
 * form with `$tn(count, "{count} block", "{count} blocks")`.
 *
 * The language is the one chosen on this device (Account → This device), or
 * else the browser's, when Minerva has it. Choosing another reloads the page,
 * so text made when the app starts is in the new language too.
 */
import it from "./locales/it.json" with { type: "json" };

const CATALOGS = { it };
const KEY = "minerva.language";

/** Languages to choose from: code and name (in that language). */
export const LANGUAGES = [
  ["en", "English"],
  ["it", "Italiano"],
];

function stored() {
  try {
    return globalThis.localStorage?.getItem(KEY) || "";
  } catch {
    return "";
  }
}

/** The language for `choice` ("" for automatic) and the browser's languages. */
export function pickLanguage(choice, preferred = []) {
  const known = new Set(LANGUAGES.map(([code]) => code));
  if (known.has(choice)) return choice;
  for (const tag of preferred) {
    const code = String(tag).toLowerCase().split("-")[0];
    if (known.has(code)) return code;
  }
  return "en";
}

let current = pickLanguage(stored(), globalThis.navigator?.languages || []);
if (globalThis.document) document.documentElement.lang = current;

/** The language in use. */
export function language() {
  return current;
}
/** What was chosen on this device: a language code, or "" for automatic. */
export function chosenLanguage() {
  return stored();
}
/** Chooses a language for this device ("" follows the browser) and reloads. */
export function setLanguage(code) {
  try {
    if (code) localStorage.setItem(KEY, code);
    else localStorage.removeItem(KEY);
  } catch {}
  location.reload();
}

/** `text` in the language in use, with `{name}` replaced from `values`. */
export function $t(text, values) {
  const translated = CATALOGS[current]?.[text] || text;
  if (!values) return translated;
  return translated.replace(/\{(\w+)\}/g, (whole, name) =>
    name in values ? String(values[name]) : whole,
  );
}

// The catalog's texts with values, as patterns, for messages the server sends.
let patterns = null;
/**
 * A message the server sent (always in English), in the language in use: as
 * the catalog has it, or found by its template ("Missing {key}" for
 * "Missing workspace_id"), keeping its values.
 */
export function serverMessage(text) {
  const catalog = CATALOGS[current];
  if (!catalog || !text || catalog[text]) return $t(text);
  patterns ??= Object.keys(catalog)
    .filter((k) => /\{\w+\}/.test(k))
    .map((k) => {
      const names = [...k.matchAll(/\{(\w+)\}/g)].map((m) => m[1]);
      const source = k
        .split(/\{\w+\}/)
        .map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
        .join("(.+?)");
      const fixed = k.replace(/\{\w+\}/g, "").trim().length;
      return { key: k, names, fixed, re: new RegExp(`^${source}$`, "s") };
    })
    // Templates with more fixed words first; nearly empty ones match anything.
    .filter((p) => p.fixed >= 6)
    .sort((a, b) => b.fixed - a.fixed);
  for (const { key, names, re } of patterns) {
    const m = re.exec(text);
    if (m)
      return $t(
        key,
        Object.fromEntries(
          // An {error} inside a message is a message of its own.
          names.map((n, i) => [
            n,
            n === "error" ? serverMessage(m[i + 1]) : m[i + 1],
          ]),
        ),
      );
  }
  return text;
}

/**
 * `one` or `other` by `count` (as the language's plural rules say), then
 * translated, with `{count}` and `values` filled in:
 * `$tn(n, "{count} note", "{count} notes")`.
 */
export function $tn(count, one, other, values = {}) {
  const rule = new Intl.PluralRules(current).select(Number(count));
  return $t(rule === "one" ? one : other, { count, ...values });
}

/**
 * Marks text for translation where it is kept in a table and used as a key;
 * the places that show it translate it with `$t`.
 */
export const msg = (text) => text;

// Keys as the reader's keyboard names them.
const KEYS = [msg("Shift"), msg("Enter"), msg("Delete"), msg("Space")];
/** A shortcut such as "Ctrl+Shift+F", with its keys named for the language. */
export function keys(shortcut) {
  return shortcut
    .split("+")
    .map((key) => (KEYS.includes(key) ? $t(key) : key))
    .join("+");
}

/** For tests: use `code`'s catalog without reloading. */
export function useLanguage(code) {
  current = pickLanguage(code);
  patterns = null;
}
