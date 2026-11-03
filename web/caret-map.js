/**
 * Where a place in a rendered block is in its Markdown. A block shows rendered
 * HTML until it is edited; to put the cursor where the reader clicked (or keep
 * the text they selected), the rendered text before that place is matched
 * against the Markdown, skipping what Markdown writes but does not show: list
 * and heading markers, emphasis markers, link addresses, cloze braces.
 */

// Syntax that renders as nothing, tried at each place in the Markdown.
const HIDDEN = [
  /^\]\([^)\n]*\)/, // a link's address: `](…)`
  /^!?\[(?=[^\]\n]*\]\()/, // a link's or image's opening
  /^\{\{c\d{1,3}::/, // a cloze's opening
  /^::[^}\n]*\}\}/, // a cloze's hint and closing
  /^\}\}/,
  /^(\*\*|__|~~|==|\*|_|`)/, // emphasis and code markers
  /^<!--[\s\S]*?-->/,
];
// At the start of a line: indentation, then block markers.
const LINE_START =
  /^[ \t]*(#{1,6}[ \t]+|>[ \t]?(\[![a-z]+\][ \t]*\n?)?|[-*+][ \t]+(\[[ xX]\][ \t]+)?|\d+[.)][ \t]+)?/i;
// How far ahead a rendered character is looked for before it is taken to be
// something the Markdown does not have (a task box, a person's name).
const LOOKAHEAD = 48;

const same = (a, b) =>
  a === b ||
  a.toLowerCase() === b.toLowerCase() ||
  (/\s/.test(a) && /\s/.test(b)) ||
  // Typographic quotes and dashes the renderer may produce.
  ('“”"'.includes(a) && '“”"'.includes(b)) ||
  ("‘’'".includes(a) && "‘’'".includes(b));

/** Skips Markdown that renders as nothing, from `i`. */
function skipHidden(markdown, i) {
  for (;;) {
    const lineStart = i === 0 || markdown[i - 1] === "\n";
    const rest = markdown.slice(i);
    const marker = lineStart && rest.match(LINE_START)?.[0];
    if (marker) {
      i += marker.length;
      continue;
    }
    const hidden = HIDDEN.map((re) => rest.match(re)).find(Boolean);
    if (hidden && hidden[0].length) {
      i += hidden[0].length;
      continue;
    }
    return i;
  }
}

/**
 * The offset in `markdown` just after the text `rendered`, the rendered text of
 * the block up to a place in it.
 */
export function sourceOffset(markdown, rendered) {
  let i = 0;
  for (const c of rendered) {
    const from = skipHidden(markdown, i);
    let found = -1;
    for (let j = from; j < Math.min(markdown.length, from + LOOKAHEAD); j++) {
      if (same(markdown[j], c)) {
        found = j;
        break;
      }
      // Never look past the end of the line for ordinary text.
      if (markdown[j] === "\n" && !/\s/.test(c)) break;
    }
    // Not in the Markdown (a task box, a person's name for an address): skip it.
    if (found < 0) continue;
    i = found + 1;
  }
  return Math.min(i, markdown.length);
}

/**
 * The Markdown range of a selection in a rendered block: `prefix` is the
 * rendered text before it, `selected` the text selected. The start skips
 * markers the selection begins after, so `**bold**` selected gives `bold`.
 */
export function sourceRange(markdown, prefix, selected) {
  const start = skipHidden(markdown, sourceOffset(markdown, prefix));
  const end = sourceOffset(markdown, prefix + selected);
  return { start: Math.min(start, end), end: Math.max(start, end) };
}
