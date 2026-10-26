// Personal study workspace. Card contents are plain text, never executable markup.
export async function renderFlashcards({ api, esc, data, error }) {
  const root = document.querySelector("#content");
  document.querySelector("#breadcrumb").textContent = "Flashcards";
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
      root.innerHTML = `<section class="page flashcards-page"><div class="eyebrow">A LITTLE PRACTICE, LASTING KNOWLEDGE</div><div class="page-heading"><div><h1>Flashcards</h1><p class="subtitle">Recall it. Check it. Give it time to stick.</p></div>${deck ? button("All decks", "fc-home") : ""}</div><div aria-live="polite" id="fc-status"></div>${body}</section>`;
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
    shell("<p>Loading your decks…</p>");
    const result = await api("flashcards/decks");
    if (!live()) return;
    shell(
      `<div class="fc-intro"><p>Your decks and study history are private to your account on this server.</p><div class="fc-actions">${button("Create a deck", "fc-create", true)}${button("Import from Anki", "fc-import")}</div></div><div class="fc-decks">${result.decks.map((d) => `<button class="fc-deck" data-deck="${esc(d.id)}"><strong>${esc(d.name)}</strong><p>${esc(d.description)}</p><span>${d.count} cards · <b>${d.due} due</b></span></button>`).join("")}</div>${!result.decks.length ? '<div class="empty"><h3>Make your first deck.</h3>Add your own questions, or turn a note, folder, or researched topic into cards with AI.</div>' : ""}`,
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
      `<form id="fc-import-form" class="fc-form"><h2>Import from Anki</h2><p>In Anki, choose <b>File → Export</b>, then <b>Anki Deck Package (.apkg)</b>, with media included to keep pictures. A <b>Notes in Plain Text</b> export or a CSV file with question and answer columns works too.</p><p>Looking for ready-made decks? Browse thousands of free shared decks on <a href="https://ankiweb.net/shared/decks" target="_blank" rel="noopener noreferrer">AnkiWeb ↗</a>, download one, and import the .apkg file here.</p><label>File<input type="file" name="file" accept=".apkg,.colpkg,.txt,.csv,.tsv" required></label><label class="checkbox-label"><input type="checkbox" name="progress" checked> Keep my review progress from Anki</label><p class="hint">Each Anki deck becomes a deck here. Cards keep their text, pictures, and links; other formatting becomes plain text, and sounds show as [audio]. Export with media to keep the pictures.</p><div class="fc-actions"><button class="btn primary" id="fc-import-button">Import</button>${button("Cancel", "fc-cancel")}</div><p role="status" id="fc-importing"></p></form>`,
    );
    bind("fc-cancel", list);
    bind(
      "fc-import-form",
      async (e) => {
        const form = e.target;
        const file = form.file.files[0];
        if (!file) throw new Error("Choose a file to import.");
        if (file.size > 100 * 1024 * 1024)
          throw new Error(
            "This file is larger than 100 MB. Export it again from Anki without media.",
          );
        const status = root.querySelector("#fc-importing");
        root.querySelector("#fc-import-button").disabled = true;
        status.textContent = "Importing… Large decks can take a moment.";
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
            throw new Error(result.error || "The file could not be imported.");
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
  function deckForm() {
    shell(
      `<form id="fc-deck-form" class="fc-form"><h2>${deck ? "Edit deck" : "Create a deck"}</h2>${field("Deck name", "name", deck?.name || "")}${field("Description", "description", deck?.description || "", true, false)}<div class="fc-actions"><button class="btn primary">Save deck</button>${button("Cancel", "fc-cancel")}</div></form>`,
    );
    bind("fc-cancel", () => (deck ? detail() : list()));
    bind(
      "fc-deck-form",
      async (e) => {
        const form = new FormData(e.target);
        const body = {
          name: form.get("name"),
          description: form.get("description"),
          revision: deck?.revision,
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
      `<div class="fc-deck-heading"><div><h2>${esc(deck.name)}</h2><p>${esc(deck.description)}</p><p>${deck.cards.length} cards · <strong>${due} due now</strong></p></div><div class="fc-actions">${button("Study due cards", "fc-study", true)}${button("Add card", "fc-add")}${button("Create with AI", "fc-ai")}${button("Edit deck", "fc-edit")}${button("Export JSON", "fc-export")}${button("Delete deck", "fc-delete")}</div></div><p class="subtitle">Reveal the answer before rating your recall. Cards return when their next review is due.</p><div class="fc-card-list">${deck.cards.map((c) => `<article class="fc-card-row"><div><strong>${rich(c.front, esc)}</strong><p>${rich(c.back, esc)}</p><small>${c.suspended ? "Paused" : c.due <= dueNow() ? "Ready to study" : "Next review: " + new Date(c.due * 1000).toLocaleString()} · ${c.reviews} reviews</small><div class="fc-sources">${sources(c.sources)}</div></div><button class="btn" data-edit="${esc(c.id)}">Edit</button></article>`).join("")}</div>${!deck.cards.length ? '<div class="empty">This deck is ready for its first question.</div>' : ""}`,
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
    shell(
      `<form class="fc-form" id="fc-card-form"><h2>${card ? "Edit card" : "Add a card"}</h2>${field("Question / front", "front", card?.front || "", true)}${field("Answer / back", "back", card?.back || "", true)}${field("Sources (one HTTP(S) URL or #note/id per line)", "sources", (card?.sources || []).join("\n"), true, false)}${card ? `<label><input type="checkbox" name="suspended" ${card.suspended ? "checked" : ""}> Pause reviews for this card</label>` : ""}<div class="fc-actions"><button class="btn primary">Save card</button>${button("Cancel", "fc-cancel")}${card ? button("Delete card", "fc-remove") : ""}</div></form>`,
    );
    bind("fc-cancel", detail);
    bind("fc-remove", async () => {
      if (!confirm("Delete this card and its review progress?")) return;
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
          sources: String(form.get("sources"))
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
        `<div class="fc-study"><h2>A little more knowledge, yours.</h2><p>You reviewed ${reviewed} cards. Cards rated Again return in ten minutes; other cards return on their scheduled day.</p>${button("Back to deck", "fc-back", true)}</div>`,
      );
      bind("fc-back", () => load(deck.id));
      return;
    }
    const c = deck.cards.find((c) => c.id === queue[0]);
    shell(
      `<div class="fc-review-meta">${reviewed} reviewed · ${queue.length} left in this session ${button("Finish for now", "fc-back")}</div><article class="fc-study"><div class="eyebrow">QUESTION</div><h2 class="fc-face">${rich(c.front, esc)}</h2>${button("Reveal answer", "fc-reveal", true)}<div id="fc-answer" hidden><hr><div class="eyebrow">ANSWER</div><div class="fc-face">${rich(c.back, esc)}</div><div class="fc-sources">${sources(c.sources)}</div><p>How well did you remember?</p><div class="fc-ratings">${["again", "hard", "good", "easy"].map((r) => `<button class="btn" data-rate="${r}">${r[0].toUpperCase() + r.slice(1)}<small>${intervalLabel(c.interval, r)}</small></button>`).join("")}</div></div></article>`,
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
    shell("<p>Loading AI providers…</p>");
    const config = await api("ai/providers");
    if (!live()) return;
    const providers = Array.isArray(config) ? config : config.providers || [];
    if (!providers.length) {
      shell(
        `<div class="empty"><h2>Connect an AI provider first.</h2><p>Your administrator can add one in AI providers. You can still make cards by hand.</p>${button("Back to deck", "fc-back")}</div>`,
      );
      bind("fc-back", detail);
      return;
    }
    const notes = [...data.notes, ...(data.shared_notes || [])].filter(
      (n) => !n.trashed,
    );
    shell(
      `<form class="fc-form" id="fc-generate"><h2>Turn understanding into practice.</h2><p>Selected source content is sent to your chosen AI provider. Review the drafts before saving. Provider charges may apply.</p><label>AI provider<select name="provider_id">${providers.map((p) => `<option value="${esc(p.id)}">${esc(p.name)}${p.web_search ? " · web search" : ""}</option>`).join("")}</select></label><label>Make cards from<select name="kind" id="fc-kind"><option value="note">A note</option><option value="folder">A folder and its subfolders</option><option value="topic">Research a topic on the web</option></select></label><label id="fc-note-label">Note<select name="note">${notes.map((n) => `<option value="${esc(n.id)}">${esc(n.title)}</option>`).join("")}</select></label><label id="fc-folder-label" hidden>Folder<select name="folder">${data.folders.map((f) => `<option value="${esc(f.id)}">${esc(f.name)}</option>`).join("")}</select></label><label id="fc-topic-label" hidden>Topic<textarea name="topic" rows="3" maxlength="1000" placeholder="For example: the causes of the French Revolution"></textarea><small>Requires an OpenAI or Anthropic profile with web search enabled.</small></label><label>Number of cards<input name="count" type="number" min="1" max="20" value="10" required></label><div class="fc-actions"><button class="btn primary" id="fc-generate-button">Generate drafts</button>${button("Cancel", "fc-cancel")}</div><p role="status" id="fc-generating"></p></form>`,
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
        root.querySelector("#fc-generating").textContent =
          "Preparing drafts… This can take a minute.";
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
      `<form id="fc-drafts"><h2>Make these cards your own.</h2><p>Check each answer against its sources, edit it, and deselect anything you don’t want to keep. Nothing has been saved yet.</p>${cards.map((c, i) => `<fieldset class="fc-draft"><legend><label><input type="checkbox" name="keep-${i}" checked> Keep card ${i + 1}</label></legend>${field("Question", "front-" + i, c.front, true)}${field("Answer", "back-" + i, c.back, true)}<div class="fc-sources">${sources(c.sources)}</div></fieldset>`).join("")}<div class="fc-actions"><button class="btn primary">Save selected cards</button>${button("Discard drafts", "fc-discard")}</div></form>`,
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
          throw new Error("Select at least one card to save.");
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
export function intervalLabel(old, rating) {
  const day = 86400;
  const seconds = Math.min(
    365 * day,
    rating === "again"
      ? 600
      : rating === "hard"
        ? old < day
          ? day
          : Math.max(day, Math.floor(old * 1.2))
        : rating === "good"
          ? old < day
            ? day
            : Math.floor(old * 2.5)
          : old < day
            ? 4 * day
            : Math.floor(old * 3.5),
  );
  return seconds < day
    ? "10 minutes"
    : `${Math.round((seconds / day) * 10) / 10} days`;
}
