/**
 * Runs in the page being clipped (injected on a click, never on its own): reads
 * its article with Readability, as Firefox's Reader View does, or the selected
 * part, and turns it into Markdown with the converter Minerva uses for pasted
 * HTML. Links and pictures keep their full addresses; nothing is fetched.
 */
import { Readability } from "@mozilla/readability";
import { htmlToMarkdown } from "../../web/editor-tools.js";

/** A document that never runs or loads what is put in it. */
const inert = () => document.implementation.createHTMLDocument("").body;
/** Pictures embedded in the page itself are left out: they can be huge. */
const EMBEDDED = /^data:/i;

/** Makes links and pictures under `root` absolute, and drops scripts. */
function tidy(root, base) {
  root.querySelectorAll("script, style, noscript, iframe, form, button, input, select, textarea").forEach((el) =>
    el.remove(),
  );
  for (const a of root.querySelectorAll("a[href]")) {
    try {
      const url = new URL(a.getAttribute("href"), base);
      if (url.protocol === "http:" || url.protocol === "https:" || url.protocol === "mailto:")
        a.setAttribute("href", url.href);
      else a.removeAttribute("href");
    } catch {
      a.removeAttribute("href");
    }
  }
  for (const img of root.querySelectorAll("img")) {
    // Lazy-loading pages keep the real picture in a data attribute.
    const src =
      img.getAttribute("data-src") ||
      img.getAttribute("data-original") ||
      img.getAttribute("src") ||
      "";
    try {
      const url = new URL(src, base);
      if (EMBEDDED.test(src) || !/^https?:$/.test(url.protocol)) img.remove();
      else img.setAttribute("src", url.href);
    } catch {
      img.remove();
    }
    img.removeAttribute("srcset");
  }
  return root;
}

function meta(name) {
  const el = document.querySelector(`meta[name="${name}"], meta[property="${name}"]`);
  return el?.getAttribute("content")?.trim() || "";
}

/** The selected part of the page, as Markdown, or "" when nothing is selected. */
function selection() {
  const chosen = getSelection();
  if (!chosen || chosen.isCollapsed) return "";
  const box = inert();
  for (let i = 0; i < chosen.rangeCount; i++)
    box.append(box.ownerDocument.importNode(chosen.getRangeAt(i).cloneContents(), true));
  return htmlToMarkdown(tidy(box, document.baseURI).innerHTML);
}

/** The page's article, as Markdown, with its title, author and site. */
function article() {
  const copy = document.cloneNode(true);
  const found = new Readability(copy, { charThreshold: 300, keepClasses: false }).parse();
  if (!found?.content) return null;
  const box = new DOMParser().parseFromString(found.content, "text/html").body;
  return {
    title: found.title,
    byline: found.byline?.trim() || "",
    site: found.siteName?.trim() || "",
    excerpt: found.excerpt?.trim() || "",
    markdown: htmlToMarkdown(tidy(box, document.baseURI).innerHTML),
  };
}

globalThis.__minervaTake = (kind) => {
  const base = {
    url: location.href,
    title: document.title.trim() || meta("og:title"),
    site: meta("og:site_name"),
    excerpt: meta("description") || meta("og:description"),
    hasSelection: !getSelection()?.isCollapsed,
    byline: "",
    markdown: "",
  };
  if (kind === "selection") return { ...base, markdown: selection() };
  if (kind === "page") {
    const found = article();
    if (found)
      return {
        ...base,
        ...found,
        title: found.title || base.title,
        site: found.site || base.site,
        excerpt: found.excerpt || base.excerpt,
      };
    // No article in the page: its details only, as a bookmark.
    return { ...base, noArticle: true };
  }
  return base;
};
