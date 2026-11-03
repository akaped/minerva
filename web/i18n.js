/**
 * The interface in the reader's language.
 *
 * Interface text is written in English in the code, wrapped in `$t("…")`; each
 * language has a catalog (`web/locales/{code}.json`) from the English text to
 * its translation, and text missing from a catalog stays in English. Values
 * go in braces: `$t("Added {count} blocks", { count })`.
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

/** For tests: use `code`'s catalog without reloading. */
export function useLanguage(code) {
  current = pickLanguage(code);
}
