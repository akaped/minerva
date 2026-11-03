import { $t } from "./i18n.js";
/**
 * AI on selected text: improve, fix, shorten, lengthen, translate or continue it
 * (`POST /api/v1/ai/transform`). The result is shown first; the person replaces
 * the selection with it, adds it after the selection, inserts it below the block,
 * or discards it. Nothing changes in the note until they choose.
 */

export const WRITING_ACTIONS = [
  ["improve", "Improve writing", "Clearer and more natural"],
  ["fix", "Fix spelling & grammar", "Only corrections"],
  ["shorter", "Make shorter", "Same meaning, fewer words"],
  ["longer", "Make longer", "More detail and explanation"],
  ["translate", "Translate", "Into the language below"],
  ["continue", "Continue writing", "One or two more paragraphs"],
];

/**
 * Where `original` is now in `value`: still at `from`…`to`, or found elsewhere
 * after edits around it; `null` when it changed.
 */
export function locateSelection(value, from, to, original) {
  if (value.slice(from, to) === original) return { from, to };
  const at = value.indexOf(original);
  return at < 0 ? null : { from: at, to: at + original.length };
}

/** The text to insert at the end of a selection so a continuation reads on. */
export function continuation(before, text) {
  const body = text.trim();
  if (!before || /\s$/.test(before)) return body;
  return " " + body;
}

const remembered = (key, fallback) => {
  try {
    return localStorage.getItem(key) || fallback;
  } catch {
    return fallback;
  }
};
const remember = (key, value) => {
  try {
    localStorage.setItem(key, value);
  } catch {
    // The choice is simply not remembered.
  }
};
/** The browser's language as a name, such as "Italian", for translations. */
function defaultLanguage() {
  try {
    const code = (navigator.language || "en").split("-")[0];
    const name = new Intl.DisplayNames(["en"], { type: "language" }).of(code);
    return name === "English" ? "Italian" : name;
  } catch {
    return "English";
  }
}

/**
 * `deps`: `api`, `esc`, `error`, `announce`, `editBlock(id)`, `replaceRange(ta,
 * change)`, `insertBelow(id, markdowns)` and `splitMarkdown`.
 */
export function createWritingAssist(deps) {
  let providers = null;
  let panel = null;
  let run = 0;
  const close = () => {
    run++;
    panel?.remove();
    panel = null;
    document.removeEventListener("mousedown", outside, true);
  };
  const outside = (e) => {
    if (panel && !panel.contains(e.target)) close();
  };
  const textarea = (id) =>
    document.querySelector(`[data-block="${CSS.escape(id)}"] textarea`);
  /** The block's field, opening the block again once a closing one is drawn. */
  async function field(id) {
    for (let waited = 0; waited < 1500; waited += 30) {
      const ta = textarea(id);
      if (ta && document.activeElement === ta) return ta;
      if (!ta) {
        deps.editBlock(id);
        if (textarea(id)) return textarea(id);
      }
      await new Promise((r) => setTimeout(r, 30));
    }
    throw new Error($t("The block is busy. Try again."));
  }

  /** Opens the menu for the selection in `ta`, inside the block element `el`. */
  async function open(ta, el, id) {
    close();
    const target = {
      id,
      from: ta.selectionStart,
      to: ta.selectionEnd,
      original: ta.value.slice(ta.selectionStart, ta.selectionEnd),
      before: ta.value.slice(0, ta.selectionEnd),
    };
    if (!target.original.trim()) return;
    panel = document.createElement("div");
    panel.className = "ai-assist";
    panel.setAttribute("role", "dialog");
    panel.setAttribute("aria-label", $t("AI on selected text"));
    panel.innerHTML = `<p class="ai-assist-status">${$t("Loading models…")}</p>`;
    el.append(panel);
    document.addEventListener("mousedown", outside, true);
    panel.addEventListener("keydown", (e) => {
      if (e.key === "Escape") {
        e.preventDefault();
        close();
        deps.editBlock(id);
      }
    });
    const epoch = ++run;
    try {
      providers ??= await deps.api("ai/providers");
    } catch (e) {
      providers = null;
      if (epoch === run) close();
      return deps.error(e);
    }
    if (epoch !== run) return;
    if (!providers.length) {
      panel.innerHTML = `<p class="ai-assist-status">${$t("No AI provider is set up yet. An administrator adds one under")} <strong>${$t("AI providers")}</strong>.</p>`;
      return;
    }
    menu(target);
  }

  function menu(target) {
    const chosen = remembered("minerva.ai.writing-provider", "");
    const provider = providers.some((p) => p.id === chosen)
      ? chosen
      : providers[0].id;
    panel.innerHTML = `<div class="ai-assist-actions" role="menu">${WRITING_ACTIONS.map(
      ([action, label, hint]) =>
        `<button role="menuitem" data-action="${action}"><strong>${deps.esc(label)}</strong><small>${deps.esc(hint)}</small></button>`,
    ).join(
      "",
    )}</div><div class="ai-assist-options"><label>${$t("Language")}<input id="ai-assist-language" maxlength="40" value="${deps.esc(remembered("minerva.ai.language", defaultLanguage()))}"></label>${
      providers.length > 1
        ? `<label>${$t("Model")}<select id="ai-assist-provider">${providers
            .map(
              (p) =>
                `<option value="${p.id}" ${p.id === provider ? "selected" : ""}>${deps.esc(p.name)} · ${deps.esc(p.model)}</option>`,
            )
            .join("")}</select></label>`
        : ""
    }</div>`;
    panel.querySelector("[data-action]")?.focus();
    panel.querySelectorAll("[data-action]").forEach((b) =>
      b.addEventListener("click", () => {
        const language = panel.querySelector("#ai-assist-language").value;
        const providerId =
          panel.querySelector("#ai-assist-provider")?.value || provider;
        remember("minerva.ai.language", language);
        remember("minerva.ai.writing-provider", providerId);
        perform(target, b.dataset.action, providerId, language).catch(
          deps.error,
        );
      }),
    );
  }

  async function perform(target, action, providerId, language) {
    const epoch = ++run;
    panel.innerHTML = `<p class="ai-assist-status" role="status"><span class="chat-dots" aria-hidden="true"><i></i><i></i><i></i></span>${$t("Writing…")}</p><div class="ai-assist-buttons"><button class="btn" data-stop>${$t("Stop")}</button></div>`;
    panel.querySelector("[data-stop]").onclick = () => {
      close();
      deps.editBlock(target.id);
    };
    let reply;
    try {
      reply = await deps.api("ai/transform", "POST", {
        provider_id: providerId,
        action,
        language,
        text: action === "continue" ? target.before : target.original,
      });
    } catch (e) {
      if (epoch === run) menu(target);
      throw e;
    }
    if (epoch !== run) return;
    const text = reply.text || "";
    const replaces = action !== "continue";
    panel.innerHTML = `<div class="ai-assist-result" tabindex="0" aria-label="${$t("Suggested text")}">${deps.esc(text)}</div><div class="ai-assist-buttons">${
      replaces
        ? `<button class="btn primary" data-apply="replace">${$t("Replace selection")}</button>`
        : `<button class="btn primary" data-apply="after">${$t("Add after selection")}</button>`
    }<button class="btn" data-apply="below">${$t("Insert below")}</button><button class="btn" data-apply="copy">${$t("Copy")}</button><button class="btn" data-apply="back">${$t("Back")}</button><button class="btn" data-apply="discard">${$t("Discard")}</button></div>`;
    deps.announce($t("Suggestion ready"));
    panel.querySelector("[data-apply]").focus();
    panel
      .querySelectorAll("[data-apply]")
      .forEach((b) =>
        b.addEventListener("click", () =>
          apply(target, b.dataset.apply, text).catch(deps.error),
        ),
      );
  }

  async function apply(target, how, text) {
    if (how === "back") return menu(target);
    if (how === "copy") {
      await navigator.clipboard.writeText(text);
      return deps.announce($t("Copied"));
    }
    close();
    if (how === "discard") return deps.editBlock(target.id);
    if (how === "below") {
      return deps.insertBelow(
        target.id,
        deps.splitMarkdown(text).filter((b) => b.trim()),
      );
    }
    const ta = await field(target.id);
    const range = locateSelection(
      ta.value,
      target.from,
      target.to,
      target.original,
    );
    if (!range)
      throw new Error(
        $t("The selected text changed meanwhile. Nothing was replaced."),
      );
    const insert =
      how === "after"
        ? continuation(ta.value.slice(0, range.to), text)
        : text.trim();
    const at = how === "after" ? range.to : range.from;
    deps.replaceRange(ta, {
      from: at,
      to: range.to,
      text: insert,
      selStart: at,
      selEnd: at + insert.length,
    });
    deps.announce(
      how === "after" ? $t("Text added") : $t("Selection replaced"),
    );
  }

  return { open, close };
}
