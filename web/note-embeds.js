/**
 * Links to blocks and notes shown inside other notes.
 *
 * `[label](#note/{id}#{block})` links to a block, `![label](#note/{id})` shows a
 * note inside another and `![label](#note/{id}#{block})` one block (with the
 * blocks nested under it). The server renders an embed as an empty
 * `.note-embed.ne-note-{id}[.ne-block-{block}]` slot; the app fills it.
 */

const UUID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/i;
const BLOCK = /^[\w-]{1,64}$/;

/** The note and block of a `#note/{id}[#{block}]` address, or null. */
export function noteTarget(href) {
  const m = /^#note\/([^#]+)(?:#(.*))?$/.exec(href || "");
  if (!m || !UUID.test(m[1])) return null;
  if (m[2] !== undefined && !BLOCK.test(m[2])) return null;
  return { note: m[1], block: m[2] ?? null };
}

/** The note and block an embed slot shows, from its classes. */
export function slotTarget(classes) {
  const list = [...classes];
  const note = list.find((c) => c.startsWith("ne-note-"))?.slice(8);
  const block = list.find((c) => c.startsWith("ne-block-"))?.slice(9) ?? null;
  return note && UUID.test(note) ? { note, block } : null;
}

/** A short label for a block: its text without Markdown, up to `max` letters. */
export function blockLabel(markdown, max = 60) {
  const text = String(markdown)
    .replace(/!\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/^\s*(?:[-*+]|\d+[.)])\s+(?:\[[ xX]\]\s+)?/gm, "")
    .replace(/^\s*(?:#{1,6}|>(?:\s*\[![A-Z]+\])?)\s*/gm, "")
    .replace(/[*_`~=]/g, "")
    .replace(/\s+/g, " ")
    .trim();
  return text.length > max ? text.slice(0, max - 1).trimEnd() + "…" : text;
}

const label = (text) => text.replace(/[[\]\\]/g, "\\$&");

/** Markdown linking to a block of a note. */
export function blockLink(text, note, block) {
  return `[${label(text)}](#note/${note}#${block})`;
}

/** Markdown showing a note, or one of its blocks, inside another note. */
export function embedCode(text, note, block = null) {
  return `![${label(text)}](#note/${note}${block ? `#${block}` : ""})`;
}

/**
 * Whether a slot may show `target` ("ok"), or why not: "loop" when it would
 * show itself or what contains it, "deep" past `depth` embeds inside embeds.
 * `outer` lists the embeds around the slot, innermost first; `host` is the
 * open note and the block the outermost embed is written in.
 */
export function embedAllowed(target, outer, host, depth = 3) {
  if (outer.length >= depth) return "deep";
  const shows = (o) =>
    o.note === target.note && (!o.block || o.block === target.block);
  if (
    target.note === host.note &&
    (!target.block || target.block === host.block)
  )
    return "loop";
  return outer.some(shows) ? "loop" : "ok";
}
