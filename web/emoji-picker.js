import { $t } from "./i18n.js";
// An emoji picker with search: every emoji, found by its name or tags ("heart",
// "pizza", "smile cat"), grouped like a phone keyboard, with recently used ones
// first. The data (web/dist/emoji.json) loads the first time a picker opens.

let data = null;
/** The emoji list: { groups: [name], emoji: [[char, name, search, group]] }. */
export function loadEmoji() {
  data ||= fetch("/emoji.json", { credentials: "same-origin" }).then((r) => {
    if (!r.ok) throw new Error($t("Emoji could not be loaded"));
    return r.json();
  });
  return data.catch((err) => {
    data = null;
    throw err;
  });
}

/**
 * The emoji this device can draw. An emoji newer than the system's emoji font
 * comes out as an empty box (about half as wide as an emoji), a sequence it does
 * not know as several pictures side by side (twice as wide or more), and some as
 * a glyph from a text font. Emoji exactly as wide as 😀 come from the emoji font
 * and are kept without drawing them; the others are drawn twice on a canvas, in
 * two colours, and kept only when the text colour does not change them, as with
 * a colour emoji. Without any colour emoji font, nothing is left out.
 */
let drawable = null;
export function drawableEmoji(list, font) {
  if (drawable) return drawable;
  const canvas = document.createElement("canvas");
  canvas.width = 40;
  canvas.height = 20;
  const ctx = canvas.getContext("2d", { willReadFrequently: true });
  if (!ctx) return list;
  ctx.font = `16px ${font}`;
  ctx.textBaseline = "top";
  const colour = (char) => {
    ctx.clearRect(0, 0, 40, 20);
    ctx.fillStyle = "#f00";
    ctx.fillText(char, 0, 0);
    ctx.fillStyle = "#00f";
    ctx.fillText(char, 20, 0);
    const px = ctx.getImageData(0, 0, 40, 20).data;
    let ink = false;
    for (let y = 0; y < 20; y++)
      for (let x = 0; x < 20; x++) {
        const i = (y * 40 + x) * 4,
          j = i + 80;
        if (px[i + 3]) ink = true;
        if (
          px[i] !== px[j] ||
          px[i + 2] !== px[j + 2] ||
          px[i + 3] !== px[j + 3]
        )
          return false;
      }
    return ink;
  };
  if (!colour("😀")) return (drawable = list);
  const one = ctx.measureText("😀").width;
  drawable = list.filter((e) => {
    const width = ctx.measureText(e[0]).width / one;
    if (width < 0.8 || width > 1.4) return false;
    return Math.abs(width - 1) < 0.02 || colour(e[0]);
  });
  return drawable;
}

const RECENT = "minerva.emoji.recent";
function recent() {
  try {
    return JSON.parse(localStorage.getItem(RECENT) || "[]").slice(0, 24);
  } catch {
    return [];
  }
}
function remember(char) {
  try {
    const list = [char, ...recent().filter((c) => c !== char)].slice(0, 24);
    localStorage.setItem(RECENT, JSON.stringify(list));
  } catch {
    // Private windows may refuse storage; the picker works without it.
  }
}

/**
 * Emoji whose name or tags contain every word of `query`, best first: the
 * exact name, then names that start with it, then names containing every
 * word, then matches in tags only. Ties keep the usual order.
 */
export function searchEmoji(list, query) {
  const q = query.toLowerCase().trim().replace(/\s+/g, " ");
  const words = q.split(" ").filter(Boolean);
  if (!words.length) return list;
  const rank = (e) => {
    const name = e[1].toLowerCase();
    if (name === q) return 0;
    if (name.startsWith(q)) return 1;
    if (words.every((w) => name.includes(w))) return 2;
    return 3;
  };
  return list
    .filter((e) => words.every((w) => e[2].includes(w)))
    .map((e, i) => [rank(e), i, e])
    .sort((a, b) => a[0] - b[0] || a[1] - b[1])
    .map((x) => x[2]);
}

/**
 * Fills `root` with a picker. `onPick(char)` runs when an emoji is chosen;
 * `esc` escapes text for HTML.
 */
export async function emojiPicker(root, { onPick, esc }) {
  root.classList.add("emoji-picker");
  root.innerHTML = `<input type="search" class="emoji-search" placeholder="${$t("Search emoji: heart, pizza, rocket…")}" aria-label="${$t("Search emoji")}" autocomplete="off"><p class="emoji-loading">${$t("Loading emoji…")}</p>`;
  const search = root.querySelector(".emoji-search");
  search.focus();
  let all;
  try {
    all = await loadEmoji();
  } catch (err) {
    root.querySelector(".emoji-loading").textContent = err.message;
    return;
  }
  if (!root.isConnected) return;
  all = {
    ...all,
    emoji: drawableEmoji(all.emoji, getComputedStyle(root).fontFamily),
  };
  const byChar = new Map(all.emoji.map((e) => [e[0], e]));
  const button = (e) =>
    `<button type="button" data-emoji="${esc(e[0])}" title="${esc(e[1])}" aria-label="${esc(e[1])}">${esc(e[0])}</button>`;
  root.querySelector(".emoji-loading").remove();
  const tabs = document.createElement("nav");
  tabs.className = "emoji-tabs";
  tabs.setAttribute("aria-label", $t("Emoji groups"));
  tabs.innerHTML = all.groups
    .map((name, g) => {
      const first = all.emoji.find((e) => e[3] === g);
      // A group this device draws none of (flags on Windows) is left out.
      return first
        ? `<button type="button" data-group="${g}" title="${esc(name)}" aria-label="${esc(name)}">${esc(first[0])}</button>`
        : "";
    })
    .join("");
  const grid = document.createElement("div");
  grid.className = "emoji-grid";
  grid.setAttribute("role", "listbox");
  const footer = document.createElement("p");
  footer.className = "emoji-name";
  footer.textContent = $t("Pick an emoji");
  search.after(tabs, grid, footer);

  const draw = () => {
    const query = search.value.trim();
    if (query) {
      const found = searchEmoji(all.emoji, query);
      grid.innerHTML = found.length
        ? `<div class="emoji-set">${found.slice(0, 400).map(button).join("")}</div>`
        : `<p class="emoji-empty">${$t("No emoji for “{query}”.", { query: esc(query) })}</p>`;
      tabs.hidden = true;
      return;
    }
    tabs.hidden = false;
    const used = recent()
      .map((c) => byChar.get(c))
      .filter(Boolean);
    grid.innerHTML =
      (used.length
        ? `<h4>${$t("Recently used")}</h4><div class="emoji-set">${used.map(button).join("")}</div>`
        : "") +
      all.groups
        .map((name, g) => {
          const set = all.emoji.filter((e) => e[3] === g);
          return set.length
            ? `<h4 id="emoji-group-${g}">${esc(name)}</h4><div class="emoji-set">${set.map(button).join("")}</div>`
            : "";
        })
        .join("");
  };
  const pick = (char) => {
    remember(char);
    onPick(char);
  };
  draw();
  search.addEventListener("input", draw);
  search.addEventListener("keydown", (e) => {
    if (e.key === "Enter") {
      e.preventDefault();
      const first = grid.querySelector("[data-emoji]");
      if (first) pick(first.dataset.emoji);
    }
  });
  grid.addEventListener("click", (e) => {
    const b = e.target.closest("[data-emoji]");
    if (b) pick(b.dataset.emoji);
  });
  grid.addEventListener("mouseover", (e) => {
    const b = e.target.closest("[data-emoji]");
    if (b) footer.textContent = `${b.dataset.emoji}  ${b.title}`;
  });
  tabs.addEventListener("click", (e) => {
    const b = e.target.closest("[data-group]");
    if (b)
      grid
        .querySelector(`#emoji-group-${b.dataset.group}`)
        ?.scrollIntoView({ block: "start" });
  });
}
