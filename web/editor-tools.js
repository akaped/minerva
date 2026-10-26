// Pure editing helpers used by the block editor. No DOM access, so they are unit-tested.
import TurndownService from "turndown";
import { gfm } from "turndown-plugin-gfm";

/**
 * Toggles inline markers (for example `**`) around a selection. Returns the
 * replacement `{from, to, text}` and the new selection `{selStart, selEnd}`.
 */
export function toggleWrap(value, start, end, before, after = before) {
  const selected = value.slice(start, end);
  // Markers just outside the selection: remove them.
  if (
    start >= before.length &&
    value.slice(start - before.length, start) === before &&
    value.slice(end, end + after.length) === after
  )
    return {
      from: start - before.length,
      to: end + after.length,
      text: selected,
      selStart: start - before.length,
      selEnd: end - before.length,
    };
  // Markers included in the selection: remove them.
  if (
    selected.length >= before.length + after.length &&
    selected.startsWith(before) &&
    selected.endsWith(after)
  ) {
    const inner = selected.slice(before.length, selected.length - after.length);
    return {
      from: start,
      to: end,
      text: inner,
      selStart: start,
      selEnd: start + inner.length,
    };
  }
  // Keep surrounding spaces outside the markers: "word " → "**word** ".
  const lead = selected.length - selected.trimStart().length;
  const trail = selected.length - selected.trimEnd().length;
  const core = selected.trim();
  const caret = start + lead + before.length;
  return {
    from: start,
    to: end,
    text:
      selected.slice(0, lead) +
      before +
      core +
      after +
      selected.slice(selected.length - trail),
    selStart: caret,
    selEnd: caret + core.length,
  };
}

/** Turns the selection into a Markdown link and selects the URL placeholder. */
export function linkSelection(value, start, end) {
  const label = value.slice(start, end).trim() || "link text";
  const url = /^https?:\/\/\S+$/.test(label) ? label : "https://";
  const text = `[${label}](${url})`;
  const urlStart = start + label.length + 3;
  return {
    from: start,
    to: end,
    text,
    selStart: urlStart,
    selEnd: urlStart + url.length,
  };
}

/** A note link that survives renames: the ID is the target, the title the label. */
export function noteLink(title, id) {
  return `[${title.replace(/[[\]\\]/g, "\\$&")}](#note/${id})`;
}

/** The `[[query` being typed just before the caret, if any. */
export function linkQuery(value, caret) {
  const match = value.slice(0, caret).match(/\[\[([^[\]\n]{0,60})$/);
  return match ? { start: caret - match[0].length, query: match[1] } : null;
}

/**
 * An `@` mention being typed before `caret`: the `@` starts the text or
 * follows a space or `(`, and is followed by up to 40 characters without
 * spaces or another `@`.
 */
export function mentionQuery(value, caret) {
  const match = value.slice(0, caret).match(/(^|[\s(])@([^\s@()[\]]{0,40})$/);
  return match ? { start: caret - match[2].length - 1, query: match[2] } : null;
}

/** Case-(in)sensitive matches of `query` in each block's Markdown. */
export function findMatches(blocks, query, caseSensitive = false) {
  if (!query) return [];
  const needle = caseSensitive ? query : query.toLowerCase();
  const matches = [];
  for (const block of blocks) {
    const text = caseSensitive ? block.markdown : block.markdown.toLowerCase();
    let at = text.indexOf(needle);
    while (at !== -1) {
      matches.push({ id: block.id, start: at, end: at + query.length });
      at = text.indexOf(needle, at + Math.max(1, query.length));
    }
  }
  return matches;
}

/** Replaces every match in a block's Markdown; returns the new text and count. */
export function replaceAll(
  markdown,
  query,
  replacement,
  caseSensitive = false,
) {
  const matches = findMatches(
    [{ id: "", markdown }],
    query,
    caseSensitive,
  ).reverse();
  let text = markdown;
  for (const m of matches)
    text = text.slice(0, m.start) + replacement + text.slice(m.end);
  return { text, count: matches.length };
}

/** Word and character counts of the readable text, and reading time at 200 wpm. */
export function countWords(blocks) {
  const text = blocks
    .map((b) =>
      b.markdown
        .replace(/^```.*$/gm, "")
        .replace(/!\[[^\]]*]\([^)]*\)/g, "")
        .replace(/\[([^\]]*)]\([^)]*\)/g, "$1")
        .replace(/[#>*_`~=|-]+/g, " "),
    )
    .join(" ");
  const words = (text.match(/[\p{L}\p{N}][\p{L}\p{N}'’]*/gu) || []).length;
  const characters = text.replace(/\s+/g, "").length;
  return { words, characters, minutes: Math.max(1, Math.round(words / 200)) };
}

/** The tag rules used by the server: letters, digits, `-`, `_`, `/`, with a letter. */
export function normalizeTag(tag) {
  const value = tag.trim().replace(/^#+/, "").toLowerCase();
  return value.length <= 40 &&
    /^[\p{L}\p{N}_/-]+$/u.test(value) &&
    /\p{L}/u.test(value)
    ? value
    : null;
}

let turndown;
function converter() {
  if (turndown) return turndown;
  turndown = new TurndownService({
    headingStyle: "atx",
    codeBlockStyle: "fenced",
    bulletListMarker: "-",
    emDelimiter: "*",
    strongDelimiter: "**",
    hr: "---",
  });
  turndown.use(gfm);
  turndown.remove(["script", "style", "meta", "title", "head", "noscript"]);
  turndown.addRule("highlight", {
    filter: "mark",
    replacement: (content) => (content.trim() ? `==${content}==` : content),
  });
  // Google Docs wraps the whole clipboard in <b style="font-weight:normal">
  // and expresses formatting with inline styles on spans.
  turndown.addRule("docs-wrapper", {
    filter: (node) =>
      node.nodeName === "B" && /font-weight:\s*normal/.test(style(node)),
    replacement: (content) => content,
  });
  turndown.addRule("styled-span", {
    filter: (node) =>
      node.nodeName === "SPAN" &&
      /font-weight:\s*(bold|[6-9]00)|font-style:\s*italic|line-through/.test(
        style(node),
      ),
    replacement: (content, node) => {
      if (!content.trim()) return content;
      const css = style(node);
      let text = content;
      if (/line-through/.test(css)) text = `~~${text}~~`;
      if (/font-style:\s*italic/.test(css)) text = `*${text}*`;
      if (/font-weight:\s*(bold|[6-9]00)/.test(css)) text = `**${text}**`;
      return text;
    },
  });
  return turndown;
}
function style(node) {
  return (node.getAttribute?.("style") || "").toLowerCase();
}

/** Converts pasted HTML (web pages, Google Docs, Word) to Markdown. */
export function htmlToMarkdown(html) {
  return converter()
    .turndown(html)
    .replace(/\u00a0/g, " ")
    .replace(/\n{3,}/g, "\n\n")
    .trim();
}
