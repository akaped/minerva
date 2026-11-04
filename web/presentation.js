/**
 * Presentation mode: a note shown as slides, one at a time, full screen.
 *
 * A note with divider blocks (`---`) is split at the dividers. Otherwise it is
 * split before each heading of its top level (the largest heading it uses), and
 * what comes before the first one joins the title slide. The title slide (the
 * note's icon and title) always comes first.
 */
import { $t } from "./i18n.js";

const DIVIDER = /^\s*(?:-{3,}|\*{3,}|_{3,})\s*$/;
const HEADING = /^\s*(#{1,6})\s/;

/**
 * Which blocks go on which slide, from the blocks' Markdown: `{intro, slides}`,
 * where `intro` lists the indexes shown under the title and `slides` the
 * indexes of each following slide. Dividers and empty blocks are left out, and
 * so are empty slides.
 */
export function slideGroups(markdowns) {
  const content = (i) => markdowns[i].trim() && !DIVIDER.test(markdowns[i]);
  const all = markdowns.map((_, i) => i);
  let intro = [];
  const slides = [];
  if (markdowns.some((m) => DIVIDER.test(m))) {
    let current = [];
    for (const i of all) {
      if (DIVIDER.test(markdowns[i])) {
        slides.push(current);
        current = [];
      } else if (content(i)) current.push(i);
    }
    slides.push(current);
  } else {
    const levels = markdowns
      .map((m) => m.match(HEADING)?.[1].length)
      .filter(Boolean);
    const top = levels.length ? Math.min(...levels) : 0;
    let current = intro;
    for (const i of all) {
      if (!content(i)) continue;
      if (top && markdowns[i].match(HEADING)?.[1].length === top) {
        current = [];
        slides.push(current);
      }
      current.push(i);
    }
  }
  return { intro, slides: slides.filter((s) => s.length) };
}

/**
 * Shows the slides over the page. `blocks` are the note's blocks as elements to
 * copy (in order, matching `markdowns`); `start` is a block index whose slide
 * opens first. Escape, or the close button, ends it and calls `onClose`.
 */
export function present({
  title,
  icon,
  markdowns,
  blocks,
  start = -1,
  esc,
  onClose,
}) {
  const { intro, slides: groups } = slideGroups(markdowns);
  const slides = [
    { title: true, blocks: intro },
    ...groups.map((g) => ({ blocks: g })),
  ];
  let at = Math.max(
    0,
    slides.findIndex((s) => s.blocks.includes(start)),
  );
  const previous = document.activeElement;
  const root = document.createElement("div");
  root.className = "presentation";
  root.setAttribute("role", "dialog");
  root.setAttribute("aria-modal", "true");
  root.setAttribute("aria-roledescription", $t("presentation"));
  root.setAttribute("aria-label", title);
  root.innerHTML = `<div class="slide-stage"><section class="slide" aria-live="polite"></section></div><div class="slide-bar"><button class="slide-nav" data-step="-1" aria-label="${esc($t("Previous slide"))}" title="${esc($t("Previous slide (←)"))}">‹</button><span class="slide-count" aria-live="polite"></span><button class="slide-nav" data-step="1" aria-label="${esc($t("Next slide"))}" title="${esc($t("Next slide (→ or Space)"))}">›</button><span class="slide-spacer"></span><button class="slide-nav" data-fullscreen aria-label="${esc($t("Full screen"))}" title="${esc($t("Full screen (F)"))}">⛶</button><button class="slide-nav" data-close aria-label="${esc($t("End presentation"))}" title="${esc($t("End presentation (Esc)"))}">×</button></div><div class="slide-progress" aria-hidden="true"><span></span></div>`;
  const slide = root.querySelector(".slide");
  /** A read-only copy of a block's content: no controls, ids or tab stops. */
  const copy = (i) => {
    const source = blocks[i];
    const out = document.createElement("div");
    out.className = "slide-block";
    const indent = [...(source?.classList || [])].find((c) =>
      c.startsWith("indent-"),
    );
    if (indent) out.classList.add(indent);
    const body = source?.querySelector(".block-body");
    if (!body || body.querySelector("textarea")) return null;
    const task = source.querySelector(".task-toggle");
    if (task) {
      const box = document.createElement("span");
      box.className = "slide-task";
      box.setAttribute("aria-hidden", "true");
      box.textContent = task.textContent;
      out.append(box);
      out.classList.add("is-task");
    }
    const content = body.cloneNode(true);
    content.className = "slide-content";
    content.removeAttribute("tabindex");
    content.removeAttribute("aria-describedby");
    for (const el of content.querySelectorAll(
      "[id], [tabindex], [contenteditable]",
    )) {
      el.removeAttribute("id");
      el.removeAttribute("contenteditable");
      if (el.getAttribute("tabindex") === "0") el.removeAttribute("tabindex");
    }
    out.append(content);
    return out;
  };
  /** Shrinks the slide's text until it fits on the screen (down to 55%). */
  const fit = () => {
    let scale = 1;
    slide.style.fontSize = "";
    const stage = root.querySelector(".slide-stage");
    while (slide.scrollHeight > stage.clientHeight - 8 && scale > 0.55) {
      scale -= 0.05;
      slide.style.fontSize = `${scale}em`;
    }
    slide.classList.toggle(
      "overflows",
      slide.scrollHeight > stage.clientHeight,
    );
  };
  const draw = () => {
    const s = slides[at];
    slide.innerHTML = "";
    slide.classList.toggle("title-slide", !!s.title);
    if (s.title) {
      const head = document.createElement("header");
      head.innerHTML = `${icon ? `<div class="slide-icon" aria-hidden="true">${esc(icon)}</div>` : ""}<h1>${esc(title)}</h1>`;
      slide.append(head);
    }
    for (const i of s.blocks) {
      const el = copy(i);
      if (el) slide.append(el);
    }
    root.querySelector(".slide-count").textContent = $t("{current} / {total}", {
      current: at + 1,
      total: slides.length,
    });
    root.querySelector(".slide-progress span").style.width =
      `${((at + 1) / slides.length) * 100}%`;
    root.querySelector('[data-step="-1"]').disabled = at === 0;
    root.querySelector('[data-step="1"]').disabled = at === slides.length - 1;
    slide.scrollTop = 0;
    requestAnimationFrame(fit);
  };
  const go = (to) => {
    to = Math.max(0, Math.min(slides.length - 1, to));
    if (to === at) return;
    at = to;
    draw();
  };
  // It opens full screen; leaving full screen (Escape) ends it, unless F
  // chose to stay in the window.
  let windowed = false;
  const fullscreen = () => {
    windowed = !!document.fullscreenElement;
    if (windowed) document.exitFullscreen().catch(() => {});
    else root.requestFullscreen?.().catch(() => {});
  };
  const left = () => {
    if (!document.fullscreenElement && !windowed) close();
  };
  const close = () => {
    removeEventListener("keydown", keys, true);
    removeEventListener("resize", fit);
    document.removeEventListener("fullscreenchange", left);
    if (document.fullscreenElement === root)
      document.exitFullscreen().catch(() => {});
    root.remove();
    document.body.classList.remove("presenting");
    previous?.focus?.();
    onClose?.();
  };
  const keys = (e) => {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const next = ["ArrowRight", "ArrowDown", "PageDown", " ", "Enter", "n"];
    const back = ["ArrowLeft", "ArrowUp", "PageUp", "Backspace", "p"];
    // Buttons keep Enter and Space for themselves.
    if ((e.key === "Enter" || e.key === " ") && e.target.closest?.("button"))
      return;
    if (next.includes(e.key)) go(at + 1);
    else if (back.includes(e.key)) go(at - 1);
    else if (e.key === "Home") go(0);
    else if (e.key === "End") go(slides.length - 1);
    else if (e.key === "f" || e.key === "F") fullscreen();
    else if (e.key === "Escape") {
      // The browser's own Escape leaves full screen first.
      if (!document.fullscreenElement) close();
    } else return;
    e.preventDefault();
    e.stopPropagation();
  };
  root.addEventListener("click", (e) => {
    const step = e.target.closest("[data-step]");
    if (step) go(at + Number(step.dataset.step));
    else if (e.target.closest("[data-close]")) close();
    else if (e.target.closest("[data-fullscreen]")) fullscreen();
    else if (
      e.target.closest(".slide-stage") &&
      !e.target.closest(
        "a, button, input, summary, video, audio, iframe, details",
      )
    )
      // A click on the slide moves on; on its left quarter, back.
      go(at + (e.clientX < innerWidth / 4 ? -1 : 1));
  });
  addEventListener("keydown", keys, true);
  addEventListener("resize", fit);
  document.body.append(root);
  document.body.classList.add("presenting");
  draw();
  root.tabIndex = -1;
  root.focus();
  // Browsers allow full screen only right after a click or key press.
  document.addEventListener("fullscreenchange", left);
  if (root.requestFullscreen)
    root.requestFullscreen().catch(() => (windowed = true));
  else windowed = true;
  return { close, go: (i) => go(i), slides: slides.length };
}
