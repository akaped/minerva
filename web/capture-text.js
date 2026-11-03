/**
 * Quick capture: what to save from what was typed or shared, and the captures
 * waiting on this device for a connection.
 */

/** Text shared to the app (title, text and address) as one note's Markdown. */
export function sharedText({ title = "", text = "", url = "" } = {}) {
  title = title.trim();
  text = text.trim();
  url = url.trim();
  // Some apps put the address in the text instead of the address field.
  if (!url && /^https?:\/\/\S+$/.test(text)) [url, text] = [text, ""];
  if (text.includes(url)) url = "";
  const link = url
    ? title
      ? `[${title.replace(/[[\]\\]/g, "\\$&")}](${url})`
      : url
    : title;
  return [link, text].filter(Boolean).join("\n\n");
}

/** A new note from captured text: its first line is the title. */
export function noteFrom(markdown) {
  const lines = markdown.trim().split("\n");
  const first = lines[0]
    .replace(/^#{1,6}\s+/, "")
    .replace(/^\s*(?:[-*+]|\d+[.)])\s+(?:\[[ xX]\]\s+)?/, "")
    .replace(/\[([^\]]*)\]\([^)]*\)/g, "$1")
    .replace(/[*_`~=]/g, "")
    .trim();
  const title = (first || "Quick note").slice(0, 200);
  // A first line that was only a title (or a heading) is not repeated in the
  // note; one with a list item, task or link stays, so nothing is lost.
  const rest = lines.slice(1).join("\n").trim();
  const plain = lines[0].trim().replace(/^#{1,6}\s+/, "");
  const body = first === plain ? rest : markdown.trim();
  return { title, markdown: body };
}

/** Today's date on this device, as the journal names its days. */
export function today(now = new Date()) {
  const pad = (n) => String(n).padStart(2, "0");
  return `${now.getFullYear()}-${pad(now.getMonth() + 1)}-${pad(now.getDate())}`;
}

const QUEUE = "minerva-capture-queue";

/** Captures saved on this device until they can be sent. */
export function queued(storage = globalThis.localStorage) {
  try {
    const list = JSON.parse(storage.getItem(QUEUE) || "[]");
    return Array.isArray(list) ? list : [];
  } catch {
    return [];
  }
}
export function setQueued(list, storage = globalThis.localStorage) {
  try {
    if (list.length) storage.setItem(QUEUE, JSON.stringify(list));
    else storage.removeItem(QUEUE);
    return true;
  } catch {
    return false;
  }
}
