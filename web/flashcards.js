import { $t } from "./i18n.js";
// Personal study workspace. Card contents are plain text, never executable markup.
export async function renderFlashcards({ api, esc, data, error }) {
  const root = document.querySelector("#content");
  document.querySelector("#breadcrumb").textContent = $t("Flashcards");
  let deck = null,
    serverNow = Date.now() / 1000,
    receivedAt = Date.now(),
    queue = [],
    reviewed = 0;
  const live = () => root.isConnected;
  const dueNow = () => serverNow + (Date.now() - receivedAt) / 1000;
  const button = (label, id, primary = false) =>
    `<button class="btn ${primary ? "primary" : ""}" id="${id}">${label}</button>`;
  const shell = (body) => {
    if (live())
      root.innerHTML = `<section class="page flashcards-page"><div class="eyebrow">${$t("A LITTLE PRACTICE, LASTING KNOWLEDGE")}</div><div class="page-heading"><div><h1>${$t("Flashcards")}</h1><p class="subtitle">${$t("Recall it. Check it. Give it time to stick.")}</p></div>${deck ? button("All decks", "fc-home") : ""}</div><div aria-live="polite" id="fc-status"></div>${body}</section>`;
    bind("fc-home", list);
  };
  const bind = (id, fn, event = "click") => {
    const el = root.querySelector("#" + id);
    if (el)
      el.addEventListener(event, async (e) => {
        e.preventDefault();
        if (el.dataset.busy) return;
        el.dataset.busy = "1";
        if (el.tagName === "BUTTON") el.disabled = true;
        try {
          await fn(e);
        } catch (err) {
          error(err);
        } finally {
          if (el.isConnected) {
            delete el.dataset.busy;
            if (el.tagName === "BUTTON") el.disabled = false;
          }
        }
      });
  };
  const field = (label, name, value = "", textarea = false, required = true) =>
    `<label>${label}${textarea ? `<textarea name="${name}" rows="4" ${required ? "required" : ""}>${esc(value)}</textarea>` : `<input name="${name}" value="${esc(value)}" ${required ? "required" : ""}>`}</label>`;
  const sources = (links = []) =>
    links
      .map(
        (url, i) =>
          `<a href="${esc(url)}" ${url.startsWith("#note/") ? "" : 'target="_blank" rel="noopener noreferrer"'}>Source ${i + 1} ↗</a>`,
      )
      .join(" · ");
  const setDeck = (result) => {
    deck = result.deck;
    serverNow = result.now;
    receivedAt = Date.now();
  };
  async function load(id) {
    const result = await api(`flashcards/decks/${id}`);
    if (!live()) return;
    setDeck(result);
    detail();
  }
  async function list() {
    deck = null;
    shell(`<p>${$t("Loading your decks…")}</p>`);
    const result = await api("flashcards/decks");
    if (!live()) return;
    shell(
      `<div class="fc-intro"><p>${$t("Your decks and study history are private to your account on this server.")}</p><div class="fc-actions">${button("Create a deck", "fc-create", true)}${button("Import from Anki", "fc-import")}</div></div><div class="fc-decks">${result.decks.map((d) => `<button class="fc-deck" data-deck="${esc(d.id)}"><strong>${esc(d.name)}</strong><p>${esc(d.description)}</p><span>${d.count} cards · <b>${d.due} due</b></span></button>`).join("")}</div>${!result.decks.length ? `<div class="empty"><h3>${$t("Make your first deck.")}</h3>${$t("Add your own questions, or turn a note, folder, or researched topic into cards with AI.")}</div>` : ""}`,
    );
    bind("fc-create", () => deckForm());
    bind("fc-import", importForm);
    root
      .querySelectorAll("[data-deck]")
      .forEach((el) => (el.onclick = () => load(el.dataset.deck).catch(error)));
  }
  // Anki packages (.apkg, .colpkg) or plain-text exports, read by the server.
  function importForm() {
    shell(
      `<form id="fc-import-form" class="fc-form"><h2>${$t("Import from Anki")}</h2><p>${$t("In Anki, choose")} <b>${$t("File → Export")}</b>${$t(", then")} <b>${$t("Anki Deck Package (.apkg)")}</b>${$t(", with media included to keep pictures. A")} <b>${$t("Notes in Plain Text")}</b> ${$t("export or a CSV file with question and answer columns works too.")}</p><p>${$t("Looking for ready-made decks? Browse thousands of free shared decks on")} <a href="https://ankiweb.net/shared/decks" target="_blank" rel="noopener noreferrer">${$t("AnkiWeb ↗")}</a>${$t(", download one, and import the .apkg file here.")}</p><label>${$t("File")}<input type="file" name="file" accept=".apkg,.colpkg,.txt,.csv,.tsv" required></label><label class="checkbox-label"><input type="checkbox" name="progress" checked> ${$t("Keep my review progress from Anki")}</label><p class="hint">${$t("Each Anki deck becomes a deck here. Cards keep their text, pictures, and links; other formatting becomes plain text, and sounds show as [audio]. Export with media to keep the pictures.")}</p><div class="fc-actions"><button class="btn primary" id="fc-import-button">${$t("Import")}</button>${button("Cancel", "fc-cancel")}</div><p role="status" id="fc-importing"></p></form>`,
    );
    bind("fc-cancel", list);
    bind(
      "fc-import-form",
      async (e) => {
        const form = e.target;
        const file = form.file.files[0];
        if (!file) throw new Error($t("Choose a file to import."));
        if (file.size > 100 * 1024 * 1024)
          throw new Error(
            $t(
              "This file is larger than 100 MB. Export it again from Anki without media.",
            ),
          );
        const status = root.querySelector("#fc-importing");
        root.querySelector("#fc-import-button").disabled = true;
        status.textContent = $t("Importing… Large decks can take a moment.");
        try {
          const query = new URLSearchParams({
            name: file.name,
            progress: form.progress.checked ? "1" : "0",
          });
          const response = await fetch(`/api/v1/flashcards/import?${query}`, {
            method: "POST",
            headers: { "X-Minerva-Client": "1" },
            // Bytes rather than the File, which crashes the Linux desktop
            // app's webview.
            body: await file.arrayBuffer(),
          });
          const result = await response.json().catch(() => ({}));
          if (!response.ok)
            throw new Error(
              result.error || $t("The file could not be imported."),
            );
          if (!live()) return;
          await list();
          const skipped = result.skipped
            ? ` ${result.skipped} card${result.skipped === 1 ? "" : "s"} could not be read and ${result.skipped === 1 ? "was" : "were"} skipped.`
            : "";
          root.querySelector("#fc-status").textContent =
            `Imported ${result.cards} card${result.cards === 1 ? "" : "s"} into ${result.decks.length} deck${result.decks.length === 1 ? "" : "s"}: ${result.decks.map((d) => d.name).join(", ")}.${skipped} ${(result.warnings || []).join(" ")}`;
        } finally {
          if (form.isConnected) {
            root.querySelector("#fc-import-button").disabled = false;
            status.textContent = "";
          }
        }
      },
      "submit",
    );
  }
  // Where a new deck can take its cards from: a workspace, or a folder with its
  // subfolders.
  const noteSources = () =>
    data.workspaces.flatMap((w) => [
      { kind: "workspace", id: w.id, label: `Workspace · ${w.name}` },
      ...data.folders
        .filter((f) => f.workspace_id === w.id)
        .map((f) => ({
          kind: "folder",
          id: f.id,
          label: `Folder · ${w.name} / ${f.name}`,
        })),
    ]);
  function deckForm() {
    const sources = deck ? [] : noteSources();
    const follow = sources.length
      ? `<fieldset class="fc-source"><legend>${$t("Cards")}</legend><label><input type="radio" name="origin" value="own" checked> ${$t("I’ll write or generate them here")}</label><label><input type="radio" name="origin" value="notes"> ${$t("Follow the cards written in my notes")}</label><select name="source" aria-label="${$t("Notes to follow")}" disabled>${sources.map((o, i) => `<option value="${i}">${esc(o.label)}</option>`).join("")}</select><p class="hint">${$t("In a note, tag a question")} <code>#card</code> ${$t("and indent its answer below it, or write")} <code>Question :: Answer #card</code>. Cloze deletions such as <code>{{c1::Rome}} is the capital of Italy</code> ${$t("become cards too. The deck keeps up with your notes each time you open it.")}</p></fieldset>`
      : "";
    shell(
      `<form id="fc-deck-form" class="fc-form"><h2>${deck ? $t("Edit deck") : $t("Create a deck")}</h2>${field("Deck name", "name", deck?.name || "")}${field("Description", "description", deck?.description || "", true, false)}${follow}<div class="fc-actions"><button class="btn primary">${$t("Save deck")}</button>${button("Cancel", "fc-cancel")}</div></form>`,
    );
    root
      .querySelectorAll('[name="origin"]')
      .forEach(
        (radio) =>
          (radio.onchange = () =>
            (root.querySelector('[name="source"]').disabled =
              radio.value !== "notes" || !radio.checked)),
      );
    bind("fc-cancel", () => (deck ? detail() : list()));
    bind(
      "fc-deck-form",
      async (e) => {
        const form = new FormData(e.target);
        const source =
          form.get("origin") === "notes"
            ? sources[Number(form.get("source"))]
            : null;
        const body = {
          name: form.get("name"),
          description: form.get("description"),
          revision: deck?.revision,
          ...(source ? { source: { kind: source.kind, id: source.id } } : {}),
        };
        const result = await api(
          "flashcards/decks" + (deck ? "/" + deck.id : ""),
          deck ? "PUT" : "POST",
          body,
        );
        if (deck) {
          setDeck(result);
          detail();
        } else await load(result.id);
      },
      "submit",
    );
  }
  function detail() {
    const due = deck.cards.filter(
      (c) => !c.suspended && c.due <= dueNow(),
    ).length;
    shell(
      `<div class="fc-deck-heading"><div><h2>${esc(deck.name)}</h2><p>${esc(deck.description)}</p><p>${deck.cards.length} cards · <strong>${due} due now</strong></p>${deck.source ? `<p class="hint">Follows the cards written in ${esc(noteSources().find((o) => o.kind === deck.source.kind && o.id === deck.source.id)?.label || "notes you can no longer open")}: tag a question <code>#card</code>${$t(", or write a cloze such as")} <code>{{c1::Rome}}</code>.</p>` : ""}</div><div class="fc-actions">${button("Study due cards", "fc-study", true)}${button("Add card", "fc-add")}${button("Create with AI", "fc-ai")}${button("Edit deck", "fc-edit")}${button("Export JSON", "fc-export")}${button("Delete deck", "fc-delete")}</div></div><p class="subtitle">${$t("Reveal the answer before rating your recall. Cards return when their next review is due.")}</p><div class="fc-card-list">${deck.cards.map((c) => `<article class="fc-card-row"><div><strong>${rich(c.front, esc)}</strong><p>${rich(c.back, esc)}</p><small>${c.suspended ? $t("Paused") : c.due <= dueNow() ? $t("Ready to study") : "Next review: " + new Date(c.due * 1000).toLocaleString()} · ${c.reviews} reviews${c.origin ? $t(" · from a note") : ""}</small><div class="fc-sources">${sources(c.sources)}</div></div><button class="btn" data-edit="${esc(c.id)}">${$t("Edit")}</button></article>`).join("")}</div>${!deck.cards.length ? `<div class="empty">${$t("This deck is ready for its first question.")}</div>` : ""}`,
    );
    root.querySelector("#fc-study").disabled = !due;
    bind("fc-study", () => {
      queue = deck.cards
        .filter((c) => !c.suspended && c.due <= dueNow())
        .sort((a, b) => a.due - b.due)
        .map((c) => c.id);
      reviewed = 0;
      study();
    });
    bind("fc-add", () => cardForm());
    bind("fc-ai", generateForm);
    bind("fc-edit", deckForm);
    bind("fc-delete", async () => {
      if (
        !confirm(
          `Delete “${deck.name}” and all its cards and review progress? This cannot be undone.`,
        )
      )
        return;
      await api(`flashcards/decks/${deck.id}`, "DELETE", {
        revision: deck.revision,
      });
      await list();
    });
    bind("fc-export", () => {
      const blob = new Blob(
        [JSON.stringify({ format: "minerva-flashcards-v1", deck }, null, 2)],
        { type: "application/json" },
      );
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = "minerva-flashcards.json";
      a.click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
    });
    root
      .querySelectorAll("[data-edit]")
      .forEach(
        (el) =>
          (el.onclick = () =>
            cardForm(deck.cards.find((c) => c.id === el.dataset.edit))),
      );
  }
  function cardForm(card) {
    // A card written in a note changes there; here it can only be paused.
    const fromNote = !!card?.origin;
    shell(
      `<form class="fc-form" id="fc-card-form"><h2>${card ? $t("Edit card") : $t("Add a card")}</h2>${fromNote ? `<p class="hint">${$t("This card comes from a note:")} <a href="${esc(card.sources[0] || "#")}">${$t("open the note")}</a> ${$t("to change it.")}</p>` : ""}${field("Question / front", "front", card?.front || "", true)}${field("Answer / back", "back", card?.back || "", true)}${fromNote ? "" : field("Sources (one HTTP(S) URL or #note/id per line)", "sources", (card?.sources || []).join("\n"), true, false)}${card ? `<label><input type="checkbox" name="suspended" ${card.suspended ? "checked" : ""}> ${$t("Pause reviews for this card")}</label>` : ""}<div class="fc-actions"><button class="btn primary">${$t("Save card")}</button>${button("Cancel", "fc-cancel")}${card && !fromNote ? button("Delete card", "fc-remove") : ""}</div></form>`,
    );
    if (fromNote)
      root
        .querySelectorAll("#fc-card-form textarea")
        .forEach((el) => (el.readOnly = true));
    bind("fc-cancel", detail);
    bind("fc-remove", async () => {
      if (!confirm($t("Delete this card and its review progress?"))) return;
      setDeck(
        await api(`flashcards/decks/${deck.id}/cards/${card.id}`, "DELETE", {
          revision: card.revision,
        }),
      );
      detail();
    });
    bind(
      "fc-card-form",
      async (e) => {
        const form = new FormData(e.target);
        const value = {
          front: form.get("front"),
          back: form.get("back"),
          sources: String(form.get("sources") ?? "")
            .split("\n")
            .map((s) => s.trim())
            .filter(Boolean),
          suspended: form.has("suspended"),
          revision: card?.revision,
        };
        setDeck(
          await api(
            `flashcards/decks/${deck.id}/cards${card ? "/" + card.id : ""}`,
            card ? "PUT" : "POST",
            card ? value : { cards: [value], revision: deck.revision },
          ),
        );
        detail();
      },
      "submit",
    );
  }
  function study() {
    if (!queue.length) {
      shell(
        `<div class="fc-study"><h2>${$t("A little more knowledge, yours.")}</h2><p>You reviewed ${reviewed} cards. Cards rated Again return in ten minutes; other cards return on their scheduled day.</p>${button("Back to deck", "fc-back", true)}</div>`,
      );
      bind("fc-back", () => load(deck.id));
      return;
    }
    const c = deck.cards.find((c) => c.id === queue[0]);
    shell(
      `<div class="fc-review-meta">${reviewed} reviewed · ${queue.length} left in this session ${button("Finish for now", "fc-back")}</div><article class="fc-study"><div class="eyebrow">${$t("QUESTION")}</div><h2 class="fc-face">${rich(c.front, esc)}</h2>${button("Reveal answer", "fc-reveal", true)}<div id="fc-answer" hidden><hr><div class="eyebrow">${$t("ANSWER")}</div><div class="fc-face">${rich(c.back, esc)}</div><div class="fc-sources">${sources(c.sources)}</div><p>${$t("How well did you remember?")}</p><div class="fc-ratings">${["again", "hard", "good", "easy"].map((r) => `<button class="btn" data-rate="${r}">${r[0].toUpperCase() + r.slice(1)}<small>${intervalLabel(c.next?.[r])}</small></button>`).join("")}</div></div></article>`,
    );
    bind("fc-back", () => load(deck.id));
    bind("fc-reveal", () => {
      root.querySelector("#fc-answer").hidden = false;
      root.querySelector("#fc-reveal").hidden = true;
      root.querySelector("[data-rate]").focus();
    });
    root.querySelectorAll("[data-rate]").forEach(
      (el) =>
        (el.onclick = async () => {
          root
            .querySelectorAll("[data-rate]")
            .forEach((b) => (b.disabled = true));
          try {
            setDeck(
              await api(
                `flashcards/decks/${deck.id}/cards/${c.id}/review`,
                "POST",
                { rating: el.dataset.rate, revision: c.revision },
              ),
            );
            queue.shift();
            reviewed++;
            study();
          } catch (e) {
            error(e);
            await load(deck.id).catch(error);
          }
        }),
    );
  }
  async function generateForm() {
    shell(`<p>${$t("Loading AI providers…")}</p>`);
    const config = await api("ai/providers");
    if (!live()) return;
    const providers = Array.isArray(config) ? config : config.providers || [];
    if (!providers.length) {
      shell(
        `<div class="empty"><h2>${$t("Connect an AI provider first.")}</h2><p>${$t("Your administrator can add one in AI providers. You can still make cards by hand.")}</p>${button("Back to deck", "fc-back")}</div>`,
      );
      bind("fc-back", detail);
      return;
    }
    const notes = [...data.notes, ...(data.shared_notes || [])].filter(
      (n) => !n.trashed,
    );
    shell(
      `<form class="fc-form" id="fc-generate"><h2>${$t("Turn understanding into practice.")}</h2><p>${$t("Selected source content is sent to your chosen AI provider. Review the drafts before saving. Provider charges may apply.")}</p><label>${$t("AI provider")}<select name="provider_id">${providers.map((p) => `<option value="${esc(p.id)}">${esc(p.name)}${p.web_search ? $t(" · web search") : ""}</option>`).join("")}</select></label><label>${$t("Make cards from")}<select name="kind" id="fc-kind"><option value="note">${$t("A note")}</option><option value="folder">${$t("A folder and its subfolders")}</option><option value="topic">${$t("Research a topic on the web")}</option></select></label><label id="fc-note-label">${$t("Note")}<select name="note">${notes.map((n) => `<option value="${esc(n.id)}">${esc(n.title)}</option>`).join("")}</select></label><label id="fc-folder-label" hidden>${$t("Folder")}<select name="folder">${data.folders.map((f) => `<option value="${esc(f.id)}">${esc(f.name)}</option>`).join("")}</select></label><label id="fc-topic-label" hidden>${$t("Topic")}<textarea name="topic" rows="3" maxlength="1000" placeholder="${$t("For example: the causes of the French Revolution")}"></textarea><small>${$t("Requires an OpenAI or Anthropic profile with web search enabled.")}</small></label><label>${$t("Number of cards")}<input name="count" type="number" min="1" max="20" value="10" required></label><div class="fc-actions"><button class="btn primary" id="fc-generate-button">${$t("Generate drafts")}</button>${button("Cancel", "fc-cancel")}</div><p role="status" id="fc-generating"></p></form>`,
    );
    bind("fc-cancel", detail);
    bind(
      "fc-kind",
      (e) => {
        ["note", "folder", "topic"].forEach(
          (k) =>
            (root.querySelector("#fc-" + k + "-label").hidden =
              e.target.value !== k),
        );
      },
      "change",
    );
    bind(
      "fc-generate",
      async (e) => {
        const form = new FormData(e.target);
        const kind = form.get("kind");
        const deckId = deck.id;
        const formEl = e.target;
        const btn = root.querySelector("#fc-generate-button");
        btn.disabled = true;
        root.querySelector("#fc-generating").textContent = $t(
          "Preparing drafts… This can take a minute.",
        );
        try {
          const result = await api("flashcards/generate", "POST", {
            kind,
            source_id: form.get(kind),
            topic: form.get("topic"),
            count: Number(form.get("count")),
            provider_id: form.get("provider_id"),
          });
          if (live() && formEl.isConnected && deck?.id === deckId)
            drafts(result.cards);
        } finally {
          if (formEl.isConnected) {
            btn.disabled = false;
            root.querySelector("#fc-generating").textContent = "";
          }
        }
      },
      "submit",
    );
  }
  function drafts(cards) {
    shell(
      `<form id="fc-drafts"><h2>${$t("Make these cards your own.")}</h2><p>${$t("Check each answer against its sources, edit it, and deselect anything you don’t want to keep. Nothing has been saved yet.")}</p>${cards.map((c, i) => `<fieldset class="fc-draft"><legend><label><input type="checkbox" name="keep-${i}" checked> Keep card ${i + 1}</label></legend>${field("Question", "front-" + i, c.front, true)}${field("Answer", "back-" + i, c.back, true)}<div class="fc-sources">${sources(c.sources)}</div></fieldset>`).join("")}<div class="fc-actions"><button class="btn primary">${$t("Save selected cards")}</button>${button("Discard drafts", "fc-discard")}</div></form>`,
    );
    bind("fc-discard", detail);
    bind(
      "fc-drafts",
      async (e) => {
        const form = new FormData(e.target);
        const selected = cards.flatMap((c, i) =>
          form.has("keep-" + i)
            ? [
                {
                  front: form.get("front-" + i),
                  back: form.get("back-" + i),
                  sources: c.sources,
                },
              ]
            : [],
        );
        if (!selected.length)
          throw new Error($t("Select at least one card to save."));
        setDeck(
          await api(`flashcards/decks/${deck.id}/cards`, "POST", {
            cards: selected,
            revision: deck.revision,
          }),
        );
        detail();
      },
      "submit",
    );
  }
  await list();
}
/**
 * Card text with pictures and links: the text is escaped first, then only
 * `![](media:<id>)` (an imported picture), `![](https://…)`, and
 * `[label](https://…)` become elements.
 */
export function rich(text, esc) {
  return esc(text)
    .replace(
      /!\[\]\(media:([0-9a-f-]{36})\)/g,
      '<img class="fc-picture" src="/api/v1/flashcards/media/$1" alt="" loading="lazy">',
    )
    .replace(
      /!\[\]\((https?:\/\/[^\s()<>]+)\)/g,
      '<img class="fc-picture" src="$1" alt="" loading="lazy" referrerpolicy="no-referrer">',
    )
    .replace(
      /\[([^\]\n]+)\]\((https?:\/\/[^\s()<>]+)\)/g,
      '<a href="$2" target="_blank" rel="noopener noreferrer">$1</a>',
    );
}
/** How long until a card returns, from seconds: minutes, days, months or years. */
export function intervalLabel(seconds) {
  const day = 86400;
  if (!Number.isFinite(seconds)) return "";
  if (seconds < day) return `${Math.max(1, Math.round(seconds / 60))} minutes`;
  const days = Math.round(seconds / day);
  if (days < 31) return days === 1 ? "1 day" : `${days} days`;
  if (days < 365) return `${Math.round((days / 30.4) * 10) / 10} months`;
  return `${Math.round((days / 365) * 10) / 10} years`;
}
