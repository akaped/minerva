/**
 * What a clip saves: its title and Markdown, from what was taken from the page.
 * Pure functions, shared by the popup and the context menus, and tested in Node.
 */

/** The most Markdown the server takes for one note, with room to spare. */
export const MARKDOWN_LIMIT = 190_000;

/**
 * A server address as typed, as its origin: `notes.example.com` becomes
 * `https://notes.example.com`. Null when it is not a web address.
 */
export function serverOrigin(typed) {
  let text = String(typed || "").trim();
  if (!text) return null;
  if (!/^[a-z][a-z0-9+.-]*:\/\//i.test(text)) text = `https://${text}`;
  try {
    const url = new URL(text);
    if (url.protocol !== "https:" && url.protocol !== "http:") return null;
    if (url.username || url.password) return null;
    return url.origin;
  } catch {
    return null;
  }
}

/** The name of a page's site for its source line: its host without `www.`. */
export function siteName(address) {
  try {
    return new URL(address).hostname.replace(/^www\./, "");
  } catch {
    return address;
  }
}

/** Text safe inside a Markdown link's square brackets. */
function linkText(text) {
  return String(text).replace(/[\\[\]]/g, "\\$&").replace(/\s+/g, " ").trim();
}
/** An address safe inside a Markdown link's parentheses. */
function linkTarget(address) {
  return String(address).replace(
    /[()\s<>]/g,
    (c) => `%${c.charCodeAt(0).toString(16).toUpperCase().padStart(2, "0")}`,
  );
}
export function link(text, address) {
  return `[${linkText(text) || linkTarget(address)}](${linkTarget(address)})`;
}

/** Cuts Markdown that is too long at a paragraph, saying so with `notice`. */
export function fit(markdown, notice, limit = MARKDOWN_LIMIT) {
  const bytes = new TextEncoder().encode(markdown);
  if (bytes.length <= limit) return markdown;
  const room = limit - new TextEncoder().encode(notice).length - 4;
  let cut = new TextDecoder().decode(bytes.slice(0, room)).replace(/�$/, "");
  const paragraph = cut.lastIndexOf("\n\n");
  if (paragraph > room / 2) cut = cut.slice(0, paragraph);
  return `${cut.trimEnd()}\n\n*${notice}*`;
}

/** Tags as typed: separated by commas or spaces, without `#`. */
export function parseTags(typed) {
  return [
    ...new Set(
      String(typed || "")
        .split(/[\s,]+/)
        .map((t) => t.replace(/^#+/, "").toLowerCase())
        .filter(Boolean),
    ),
  ].slice(0, 20);
}

/**
 * The note for a clip. `taken` is what was read from the page (`url`, `title`,
 * `markdown`, `excerpt`, `byline`, `site`); `kind` is `page`, `selection`,
 * `link`, `image` or `bookmark`; `extra` holds a link's or picture's address
 * and text from the context menu. `t(key, ...values)` gives the words.
 */
export function compose(kind, taken, extra, t) {
  const page = taken.url;
  const site = taken.site || siteName(page);
  const source = [t("clippedFrom", link(site, page))];
  if (taken.byline && kind === "page") source.push(taken.byline);
  const lines = [source.join(" · ")];
  let title = taken.title || site;
  if (kind === "link") {
    title = extra.text?.trim() || extra.linkUrl;
    return {
      title,
      url: extra.linkUrl,
      markdown: [link(title, extra.linkUrl), t("foundOn", link(taken.title || site, page))].join("\n\n"),
    };
  }
  if (kind === "image") {
    lines.push(`![${linkText(extra.text || "")}](${linkTarget(extra.srcUrl)})`);
  } else if (kind === "bookmark") {
    if (taken.excerpt) lines.push(`> ${taken.excerpt.replace(/\s+/g, " ").trim()}`);
  } else if (taken.markdown) {
    lines.push(taken.markdown.trim());
  }
  return {
    title,
    url: page,
    markdown: fit(lines.join("\n\n"), t("cutShort")),
  };
}
