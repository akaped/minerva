/**
 * Comments on the open note (`/api/v1/notes/{id}/comments`): a panel at the
 * side with the note's threads, a count on each block with open threads, and a
 * composer for a new thread on a block or a selection. `@person@example.com`
 * mentions are offered from the people who can open the note.
 */
import { $t, $tn } from "./i18n.js";
import { mentionQuery } from "./editor-tools.js";

/** Comment text as HTML: escaped, mentions by name, line breaks kept. */
export function commentHTML(text, esc, names = new Map()) {
  return esc(text)
    .replace(
      /(^|[\s(])@([A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,})/g,
      (_, before, email) =>
        `${before}<span class="mention" title="${email}">@${esc(names.get(email.toLowerCase()) || email)}</span>`,
    )
    .replace(/\n/g, "<br>");
}

/** $t("just now"), "5 min ago", "3 h ago", or the date. */
export function ago(at, now = Date.now() / 1000) {
  const s = Math.max(0, now - at);
  if (s < 60) return $t("just now");
  if (s < 3600) return $t("{count} min ago", { count: Math.floor(s / 60) });
  if (s < 86400) return $t("{count} h ago", { count: Math.floor(s / 3600) });
  return new Date(at * 1000).toLocaleDateString(undefined, {
    day: "numeric",
    month: "short",
  });
}

/**
 * `deps`: `api`, `esc`, `error`, `announce`, `me()` (the user's ID),
 * `canResolveAll()` (an editor of the note), `note()` (the open note) and
 * `watch(id, onChange)` (live signals; returns a function that stops them).
 */
export function createComments(deps) {
  const $ = (s, root = document) => root.querySelector(s);
  let noteId = null,
    threads = [],
    people = [],
    open = false,
    showResolved = false,
    draft = null,
    unwatch = null,
    timer = null,
    loading = null;
  const names = () =>
    new Map(people.map((p) => [p.email.toLowerCase(), p.name]));

  async function load() {
    if (!noteId) return;
    const id = noteId;
    loading = deps.api(`notes/${id}/comments`).then(
      (list) => {
        if (id !== noteId) return;
        threads = list;
        drawBadges();
        if (open) draw();
      },
      () => {},
    );
    return loading;
  }
  /** Follows a note: its threads now, and whenever it changes. */
  function attach(id) {
    if (id === noteId) return;
    detach();
    noteId = id;
    threads = [];
    people = [];
    deps
      .api(`notes/${id}/people`)
      .then((list) => id === noteId && (people = list))
      .catch(() => {});
    load();
    let pending = null;
    unwatch = deps.watch(id, () => {
      clearTimeout(pending);
      pending = setTimeout(load, 300);
    });
    // Pages without live signals (the desktop app) still catch up.
    timer = setInterval(() => !document.hidden && load(), 20000);
  }
  function detach() {
    unwatch?.();
    clearInterval(timer);
    noteId = null;
    threads = [];
    draft = null;
    panel()?.remove();
    document.body.classList.remove("comments-open");
    open = false;
  }
  const panel = () => $("#comments-panel");
  const openThreads = () => threads.filter((t) => !t.resolved);

  /** The count on each block with open threads, and on the toolbar button. */
  function drawBadges() {
    document.querySelectorAll(".block-comments").forEach((b) => b.remove());
    const counts = new Map();
    for (const t of openThreads())
      if (t.block_id) counts.set(t.block_id, (counts.get(t.block_id) || 0) + 1);
    for (const [block, n] of counts) {
      const el = $(`[data-block="${CSS.escape(block)}"]`);
      if (!el) continue;
      const badge = document.createElement("button");
      badge.className = "block-comments";
      badge.type = "button";
      badge.title = $tn(n, "{count} comment thread", "{count} comment threads");
      badge.setAttribute("aria-label", badge.title);
      badge.innerHTML = `💬 ${n}`;
      badge.onclick = (e) => {
        e.stopPropagation();
        show(block);
      };
      el.append(badge);
    }
    const button = $("#comments-toggle");
    if (button) {
      const n = openThreads().length;
      $(".comments-count", button).textContent = n ? String(n) : "";
      button.setAttribute(
        "aria-label",
        n ? `Comments, ${n} open` : $t("Comments"),
      );
      button.setAttribute("aria-pressed", String(open));
    }
  }

  function show(focusBlock = null) {
    open = true;
    draw();
    if (focusBlock) {
      const t = $(`.comment-thread[data-block="${CSS.escape(focusBlock)}"]`);
      t?.scrollIntoView({ block: "nearest" });
      t?.classList.add("flash");
    }
  }
  function toggle() {
    if (open) close();
    else show();
  }
  function close() {
    open = false;
    draft = null;
    panel()?.remove();
    document.body.classList.remove("comments-open");
    drawBadges();
  }
  /** A new thread on a block (and the text selected in it), or on the note. */
  function start(blockId = null, quote = "") {
    draft = { blockId, quote: quote.trim().slice(0, 500) };
    show();
    $(".comment-draft textarea", panel())?.focus();
  }

  function blockLabel(id) {
    const b = deps.note()?.blocks.find((x) => x.id === id);
    if (!b) return "A block that was removed";
    const text = b.markdown
      .replace(/^[\s>#*-]*(\[[ xX]\])?\s*/, "")
      .replace(/[*_`~=[\]()]/g, "");
    return text.length > 60 ? text.slice(0, 60) + "…" : text || "Empty block";
  }
  function composer(kind, id = "") {
    return `<form class="comment-compose" data-compose="${kind}" data-thread="${id}"><textarea rows="${kind === "reply" ? 1 : 3}" maxlength="4000" placeholder="${kind === "reply" ? $t("Reply…") : $t("Add a comment… (@ to mention someone)")}" aria-label="${kind === "reply" ? $t("Reply") : $t("Comment")}"></textarea><div class="mention-list" hidden></div><div class="comment-actions"><small>${$t("Ctrl+Enter to send")}</small>${kind === "new" ? `<button type="button" class="btn" data-cancel-draft>${$t("Cancel")}</button>` : ""}<button class="btn primary">${kind === "reply" ? $t("Reply") : $t("Comment")}</button></div></form>`;
  }
  function threadHTML(t) {
    const esc = deps.esc;
    const mine = t.comments[0]?.author === deps.me();
    const resolvable = deps.canResolveAll() || mine;
    return `<article class="comment-thread ${t.resolved ? "resolved" : ""}" data-thread="${t.id}" data-block="${esc(t.block_id || "")}"><header>${t.block_id ? `<button class="comment-anchor" data-goto="${esc(t.block_id)}" title="${$t("Go to the block")}">${t.quote ? `“${esc(t.quote)}”` : esc(blockLabel(t.block_id))}</button>` : `<span class="comment-anchor">${$t("On the note")}</span>`}${resolvable ? `<button class="btn" data-resolve="${t.id}" data-value="${!t.resolved}">${t.resolved ? $t("Reopen") : $t("Resolve")}</button>` : ""}</header>${t.comments
      .map(
        (c) =>
          `<div class="comment" data-comment="${c.id}"><div class="comment-meta"><strong>${esc(c.author_name)}</strong><span>${esc(ago(c.created_at))}</span>${c.author === deps.me() ? `<button class="comment-delete" data-delete="${t.id}:${c.id}" title="${c === t.comments[0] ? $t("Delete the thread") : $t("Delete")}" aria-label="${$t("Delete comment")}">×</button>` : ""}</div><div class="comment-text">${commentHTML(c.text, esc, names())}</div></div>`,
      )
      .join(
        "",
      )}${t.resolved ? `<p class="comment-resolved">${$t("Resolved{value}", { value: t.resolved_by_name ? $t(" by {name}", { name: esc(t.resolved_by_name) }) : "" })}</p>` : composer("reply", t.id)}</article>`;
  }
  function draw() {
    if (!open) return;
    let el = panel();
    if (!el) {
      el = document.createElement("aside");
      el.id = "comments-panel";
      el.className = "comments-panel";
      el.setAttribute("aria-label", $t("Comments"));
      document.body.append(el);
    }
    document.body.classList.add("comments-open");
    const order = new Map((deps.note()?.blocks || []).map((b, i) => [b.id, i]));
    const shown = threads
      .filter((t) => t.resolved === showResolved)
      .sort(
        (a, b) =>
          (order.get(a.block_id) ?? -1) - (order.get(b.block_id) ?? -1) ||
          a.created_at - b.created_at,
      );
    const esc = deps.esc;
    el.innerHTML = `<header class="comments-head"><h2>${$t("Comments")}</h2><div class="comments-tabs" role="tablist"><button role="tab" aria-selected="${!showResolved}" data-tab="open">${$t("Open {value}", { value: openThreads().length || "" })}</button><button role="tab" aria-selected="${showResolved}" data-tab="resolved">${$t("Resolved")}</button></div><button class="btn" data-new-note-thread title="${$t("Comment on the whole note")}">+</button><button class="btn" data-close aria-label="${$t("Close comments")}">×</button></header>${
      draft
        ? `<article class="comment-thread comment-draft"><header><span class="comment-anchor">${draft.quote ? `“${esc(draft.quote)}”` : draft.blockId ? esc(blockLabel(draft.blockId)) : $t("On the note")}</span></header>${composer("new")}</article>`
        : ""
    }${shown.map(threadHTML).join("") || `<p class="comments-empty">${showResolved ? $t("No resolved threads.") : $t("No comments yet. Select text or hover a block and choose 💬 to start one.")}</p>`}`;
    bind(el);
    drawBadges();
  }
  function bind(el) {
    el.querySelector("[data-close]").onclick = close;
    el.querySelector("[data-new-note-thread]").onclick = () => start();
    el.querySelectorAll("[data-tab]").forEach(
      (b) =>
        (b.onclick = () => {
          showResolved = b.dataset.tab === "resolved";
          draw();
        }),
    );
    el.querySelector("[data-cancel-draft]")?.addEventListener("click", () => {
      draft = null;
      draw();
    });
    el.querySelectorAll("[data-goto]").forEach(
      (b) =>
        (b.onclick = () => {
          const block = $(`[data-block="${CSS.escape(b.dataset.goto)}"]`);
          block?.scrollIntoView({ block: "center", behavior: "smooth" });
          block?.classList.add("comment-flash");
          setTimeout(() => block?.classList.remove("comment-flash"), 1500);
        }),
    );
    el.querySelectorAll("[data-resolve]").forEach(
      (b) =>
        (b.onclick = () =>
          act(
            deps.api(`notes/${noteId}/comments/${b.dataset.resolve}`, "PATCH", {
              resolved: b.dataset.value === "true",
            }),
            b.dataset.value === "true"
              ? $t("Thread resolved")
              : $t("Thread reopened"),
          )),
    );
    el.querySelectorAll("[data-delete]").forEach(
      (b) =>
        (b.onclick = () => {
          const [tid, cid] = b.dataset.delete.split(":");
          act(
            deps.api(`notes/${noteId}/comments/${tid}/${cid}`, "DELETE"),
            $t("Comment deleted"),
          );
        }),
    );
    el.querySelectorAll("form.comment-compose").forEach((form) => {
      const ta = form.querySelector("textarea");
      const list = form.querySelector(".mention-list");
      ta.addEventListener("input", () => {
        ta.style.height = "auto";
        ta.style.height = ta.scrollHeight + "px";
        mentions(ta, list);
      });
      ta.addEventListener("keydown", (e) => {
        if (e.key === "Enter" && (e.ctrlKey || e.metaKey)) {
          e.preventDefault();
          form.requestSubmit();
        }
      });
      form.onsubmit = (e) => {
        e.preventDefault();
        const text = ta.value.trim();
        if (!text) return;
        const body =
          form.dataset.compose === "new"
            ? deps.api(`notes/${noteId}/comments`, "POST", {
                block_id: draft?.blockId,
                quote: draft?.quote,
                text,
              })
            : deps.api(
                `notes/${noteId}/comments/${form.dataset.thread}/replies`,
                "POST",
                { text },
              );
        if (form.dataset.compose === "new") draft = null;
        act(body, $t("Comment added"));
      };
    });
  }
  /** `@` in a comment offers the people who can open the note. */
  function mentions(ta, list) {
    const q = mentionQuery(ta.value, ta.selectionStart);
    const found = q
      ? people
          .filter((p) => !p.me)
          .filter((p) =>
            `${p.name} ${p.email}`
              .toLowerCase()
              .includes(q.query.toLowerCase()),
          )
          .slice(0, 6)
      : [];
    list.hidden = !found.length;
    list.innerHTML = found
      .map(
        (p, i) =>
          `<button type="button" data-pick="${i}"><strong>${deps.esc(p.name)}</strong><small>${deps.esc(p.email)}</small></button>`,
      )
      .join("");
    list.querySelectorAll("[data-pick]").forEach((b) => {
      b.addEventListener("mousedown", (e) => e.preventDefault());
      b.onclick = () => {
        const p = found[Number(b.dataset.pick)];
        const text = `@${p.email} `;
        ta.setRangeText(text, q.start, ta.selectionStart, "end");
        list.hidden = true;
        ta.focus();
      };
    });
  }
  async function act(request, message) {
    try {
      await request;
      await load();
      deps.announce(message);
    } catch (e) {
      deps.error(e);
    }
  }
  return { attach, detach, toggle, start, drawBadges, load };
}
