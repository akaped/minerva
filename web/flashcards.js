import { $t, $tn, serverMessage } from "./i18n.js";
import { statsHTML } from "./flashcard-stats.js";
// Personal study workspace. Card contents are plain text, never executable markup.
export async function renderFlashcards({
  api,
  esc,
  data,
  error,
  icon,
  modal,
  workspaceId,
}) {
  const root = document.querySelector("#content");
  document.querySelector("#breadcrumb").textContent = $t("Flashcards");
  let deck = null,
    serverNow = Date.now() / 1000,
    receivedAt = Date.now(),
    queue = [],
    reviewed = 0;
  const live = () => root.isConnected;
  const dueNow = () => serverNow + (Date.now() - receivedAt) / 1000;
  const button = (label, id, style = "", symbol = "") =>
    `<button type="button" class="btn ${style}" id="${id}">${symbol ? icon(symbol) : ""}${label}</button>`;
  const shell = (
    body,
    { title = $t("Flashcards"), subtitle = "", actions, home = false } = {},
  ) => {
    if (!live()) return;
    const headerActions =
      (home ? button($t("All decks"), "fc-home") : "") + (actions || "");
    root.innerHTML = `<section class="page flashcards-page">
      <header class="page-heading"><div><h1>${title}</h1>${subtitle ? `<p class="subtitle">${subtitle}</p>` : ""}</div>
        ${headerActions ? `<div class="fc-actions">${headerActions}</div>` : ""}
      </header>
      <div role="status" id="fc-status" class="fc-status"></div>
      ${body}
    </section>`;
    bind("fc-home", list);
  };
  const confirmDelete = (label, message) =>
    new Promise((resolve) => {
      modal.returnValue = "";
      modal.innerHTML = `<form method="dialog"><h2>${label}</h2><p>${esc(message)}</p>
      <footer><button class="btn" value="cancel" autofocus>${$t("Cancel")}</button>
      <button class="btn danger" value="delete">${label}</button></footer></form>`;
      modal.addEventListener(
        "close",
        () => resolve(modal.returnValue === "delete"),
        { once: true },
      );
      modal.showModal();
    });
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
    `<label class="fc-field">${label}${textarea ? `<textarea name="${name}" rows="4" ${required ? "required" : ""}>${esc(value)}</textarea>` : `<input name="${name}" value="${esc(value)}" ${required ? "required" : ""}>`}</label>`;
  const formActions = (label, extra = "") =>
    `<footer class="fc-actions fc-form-actions">${extra}${button($t("Cancel"), "fc-cancel")}<button type="submit" class="btn primary">${label}</button></footer>`;
  const sources = (links = []) =>
    links
      .map(
        (url, i) =>
          `<a href="${esc(url)}" ${url.startsWith("#note/") ? "" : 'target="_blank" rel="noopener noreferrer"'}>${$t("Source {value} ↗", { value: i + 1 })}</a>`,
      )
      .join(" · ");
  const setDeck = (result) => {
    deck = result.deck;
    // On a shared deck, people who cannot edit the workspace only study.
    deck.can_edit = result.can_edit !== false;
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
      result.decks.length
        ? `<div class="fc-decks">${result.decks
            .map(
              (
                d,
              ) => `<button type="button" class="fc-deck" data-deck="${esc(d.id)}">
          <span class="fc-deck-icon">${icon("flashcards")}</span>
          <strong>${esc(d.name)}</strong>
          ${d.workspace ? `<small class="fc-shared">${esc($t("Shared · {name}", { name: d.workspace_name || "" }))}</small>` : ""}
          ${d.description ? `<span class="fc-deck-description">${esc(d.description)}</span>` : ""}
          <span class="fc-deck-count">${$t("{count} cards ·", { count: d.count })} <b>${$t("{count} due", { count: d.due })}</b></span>
        </button>`,
            )
            .join("")}</div>`
        : `<div class="empty fc-empty">${icon("flashcards")}<h2>${$t("Make your first deck.")}</h2><p>${$t("Add your own questions, or turn a note, folder, or researched topic into cards with AI.")}</p></div>`,
      {
        actions:
          button($t("Import from Anki"), "fc-import", "", "upload") +
          button($t("Statistics"), "fc-stats", "", "chart") +
          button($t("Scheduling"), "fc-settings", "", "settings") +
          button($t("Create a deck"), "fc-create", "primary", "plus"),
      },
    );
    bind("fc-create", () => deckForm());
    bind("fc-import", importForm);
    bind("fc-stats", () => statistics());
    bind("fc-settings", () => scheduling());
    root
      .querySelectorAll("[data-deck]")
      .forEach((el) => (el.onclick = () => load(el.dataset.deck).catch(error)));
  }
  // Anki packages (.apkg, .colpkg) or plain-text exports, read by the server.
  function importForm() {
    shell(
      `<form id="fc-import-form" class="fc-form">
        <label class="fc-field fc-file">${$t("File")}<input type="file" name="file" accept=".apkg,.colpkg,.txt,.csv,.tsv" required></label>
        <label class="fc-check"><input type="checkbox" name="progress" checked><span>${$t("Keep my review progress from Anki")}</span></label>
        <details class="fc-help"><summary>${$t("How to export from Anki")}</summary>
          <p>${$t("In Anki, choose")} <b>${$t("File → Export")}</b>${$t(", then")} <b>${$t("Anki Deck Package (.apkg)")}</b>${$t(", with media included to keep pictures. A")} <b>${$t("Notes in Plain Text")}</b> ${$t("export or a CSV file with question and answer columns works too.")}</p>
          <p>${$t("Looking for ready-made decks? Browse thousands of free shared decks on")} <a href="https://ankiweb.net/shared/decks" target="_blank" rel="noopener noreferrer">${$t("AnkiWeb ↗")}</a>${$t(", download one, and import the .apkg file here.")}</p>
        </details>
        <div class="fc-status" role="status" id="fc-importing"></div>
        <footer class="fc-actions fc-form-actions">${button($t("Cancel"), "fc-cancel")}<button type="submit" class="btn primary" id="fc-import-button">${$t("Import")}</button></footer>
      </form>`,
      {
        title: $t("Import from Anki"),
        subtitle: $t("Choose an Anki package or a text export up to 100 MB."),
      },
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
              result.error
                ? serverMessage(result.error)
                : $t("The file could not be imported."),
            );
          if (!live()) return;
          await list();
          const skipped = result.skipped
            ? $tn(
                result.skipped,
                " {count} card could not be read and was skipped.",
                " {count} cards could not be read and were skipped.",
              )
            : "";
          root.querySelector("#fc-status").textContent =
            `${$tn(result.cards, "Imported {count} card", "Imported {count} cards")} ${$tn(result.decks.length, "into {count} deck: {names}.", "into {count} decks: {names}.", { names: result.decks.map((d) => d.name).join(", ") })}${skipped} ${(result.warnings || []).join(" ")}`;
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
      {
        kind: "workspace",
        id: w.id,
        label: $t("Workspace · {name}", { name: w.name }),
      },
      ...data.folders
        .filter((f) => f.workspace_id === w.id)
        .map((f) => ({
          kind: "folder",
          id: f.id,
          label: $t("Folder · {workspace} / {folder}", {
            workspace: w.name,
            folder: f.name,
          }),
        })),
    ]);
  function deckForm() {
    const sources = deck ? [] : noteSources();
    const follow = sources.length
      ? `<fieldset class="fc-source"><legend>${$t("Card source")}</legend>
        <div class="fc-options">
          <label class="fc-option"><input type="radio" name="origin" value="own" checked><span>${$t("I’ll write or generate them here")}</span></label>
          <label class="fc-option"><input type="radio" name="origin" value="notes"><span>${$t("Follow the cards written in my notes")}</span></label>
        </div>
        <div class="fc-follow" hidden>
          <label class="fc-field">${$t("Notes to follow")}<select name="source" disabled>${sources.map((o, i) => `<option value="${i}">${esc(o.label)}</option>`).join("")}</select></label>
          <details class="fc-help"><summary>${$t("How cards in notes work")}</summary><p>${$t("In a note, tag a question")} <code>#card</code> ${$t("and indent its answer below it, or write")} <code>Question :: Answer #card</code>. Cloze deletions such as <code>{{c1::Rome}} is the capital of Italy</code> ${$t("become cards too. The deck keeps up with your notes each time you open it.")}</p></details>
        </div>
      </fieldset>`
      : "";
    // A deck can be shared with a workspace where you can edit (not your own).
    const shareable = data.workspaces.filter(
      (w) => !w.personal && (w.can_edit || w.owner_id === data.user.id),
    );
    const share = shareable.length
      ? `<label class="fc-field">${$t("Shared with")}<select name="workspace_id"><option value="">${$t("Only me")}</option>${shareable.map((w) => `<option value="${esc(w.id)}" ${deck?.workspace === w.id ? "selected" : ""}>${esc($t("Everyone in {name}", { name: w.name }))}</option>`).join("")}</select></label><p class="hint">${$t("Everyone in the workspace can study a shared deck, each with their own progress; people who can edit there can change its cards.")}</p>`
      : "";
    const percents = [70, 75, 80, 85, 90, 93, 95, 97];
    const retention = `<label class="fc-field">${$t("Desired retention for this deck")}<select name="retention"><option value="">${$t("My own setting (Scheduling)")}</option>${percents.map((p) => `<option value="${p}" ${Math.round((deck?.retention || 0) * 100) === p ? "selected" : ""}>${p}%</option>`).join("")}</select></label>`;
    shell(
      `<form id="fc-deck-form" class="fc-form">
        ${field($t("Deck name"), "name", deck?.name || "")}
        ${field($t("Description"), "description", deck?.description || "", true, false)}
        ${follow}
        ${retention}
        ${share}
        ${formActions($t("Save deck"))}
      </form>`,
      { title: deck ? $t("Edit deck") : $t("Create a deck") },
    );
    const form = root.querySelector("#fc-deck-form");
    form.addEventListener("change", (e) => {
      if (e.target.name !== "origin") return;
      const followsNotes = e.target.value === "notes";
      form.querySelector(".fc-follow").hidden = !followsNotes;
      form.elements.source.disabled = !followsNotes;
      if (form.elements.workspace_id)
        form.elements.workspace_id.disabled = followsNotes;
    });
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
          retention: form.get("retention")
            ? Number(form.get("retention")) / 100
            : null,
          ...(form.has("workspace_id")
            ? { workspace_id: form.get("workspace_id") }
            : {}),
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
    const deckInfo = [
      deck.description ? `<p>${esc(deck.description)}</p>` : "",
      deck.source
        ? `<p class="hint">${$t("Follows the cards written in {value}: tag a question", { value: esc(noteSources().find((o) => o.kind === deck.source.kind && o.id === deck.source.id)?.label || $t("notes you can no longer open")) })} <code>#card</code>${$t(", or write a cloze such as")} <code>{{c1::Rome}}</code>.</p>`
        : "",
      deck.workspace
        ? `<p class="hint">${esc($t("Shared with {name}: your progress is your own.", { name: data.workspaces.find((w) => w.id === deck.workspace)?.name || "" }))}</p>`
        : "",
    ].join("");
    shell(
      `<section class="fc-deck-tools">
        <div class="fc-actions">
          ${button($t("Study due cards"), "fc-study", "primary", "play")}
          ${deck.can_edit ? button($t("Add card"), "fc-add", "", "plus") + button($t("Create with AI"), "fc-ai", "", "brain") : ""}
          ${button($t("Statistics"), "fc-deck-stats", "", "chart")}
        </div>
        <div class="fc-actions">
          ${deck.can_edit ? button($t("Edit deck"), "fc-edit", "", "settings") : ""}
          ${button($t("Export JSON"), "fc-export", "", "download")}
          ${deck.can_edit ? button($t("Delete deck"), "fc-delete", "danger", "trash") : ""}
        </div>
        ${deckInfo ? `<div class="fc-deck-info">${deckInfo}</div>` : ""}
      </section>
      ${
        deck.cards.length
          ? `<div class="fc-card-list">${deck.cards
              .map(
                (c) => `<article class="fc-card-row">
        <div class="fc-card-text"><strong>${rich(c.front, esc)}</strong><p>${rich(c.back, esc)}</p>
          <div class="fc-card-meta"><small>${$t("{value} · {reviews} reviews{value2}", { value: c.suspended ? $t("Paused") : c.due <= dueNow() ? $t("Ready to study") : $t("Next review: {date}", { date: new Date(c.due * 1000).toLocaleString() }), reviews: c.reviews, value2: c.origin ? $t(" · from a note") : "" })}</small>${sources(c.sources)}</div>
        </div>
        <button type="button" class="btn" data-edit="${esc(c.id)}">${icon("edit")}${$t("Edit")}</button>
      </article>`,
              )
              .join("")}</div>`
          : `<div class="empty fc-empty">${icon("flashcards")}<p>${$t("This deck is ready for its first question.")}</p></div>`
      }`,
      {
        title: esc(deck.name),
        home: true,
        subtitle: `${$t("{count} cards ·", { count: deck.cards.length })} ${$t("{due} due now", { due })}`,
      },
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
    bind("fc-deck-stats", () => statistics(deck));
    bind("fc-ai", generateForm);
    bind("fc-edit", deckForm);
    bind("fc-delete", async () => {
      if (
        !(await confirmDelete(
          $t("Delete deck"),
          $t(
            "Delete “{name}” and all its cards and review progress? This cannot be undone.",
            { name: deck.name },
          ),
        ))
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
    // A shared deck's cards change only for those who can edit its workspace.
    const locked = fromNote || !deck.can_edit;
    shell(
      `<form class="fc-form fc-wide-form" id="fc-card-form">
        ${fromNote ? `<p class="hint">${$t("This card comes from a note:")} <a href="${esc(card.sources[0] || "#")}">${$t("open the note")}</a> ${$t("to change it.")}</p>` : !deck.can_edit ? `<p class="hint">${$t("Only people who can edit this shared deck’s workspace change its cards; you can pause one for yourself.")}</p>` : ""}
        <div class="fc-pair">
          <div class="fc-card-side">${field($t("Question / front"), "front", card?.front || "", true)}${locked ? "" : attach("front")}</div>
          <div class="fc-card-side">${field($t("Answer / back"), "back", card?.back || "", true)}${locked ? "" : attach("back")}</div>
        </div>
        ${locked ? "" : field($t("Sources (one HTTP(S) URL or #note/id per line)"), "sources", (card?.sources || []).join("\n"), true, false)}
        ${card ? `<label class="fc-check"><input type="checkbox" name="suspended" ${card.suspended ? "checked" : ""}><span>${$t("Pause reviews for this card")}</span></label>` : ""}
        ${formActions($t("Save card"), card && !locked ? button($t("Delete card"), "fc-remove", "danger", "trash") : "")}
      </form>`,
      {
        title: card ? $t("Edit card") : $t("Add a card"),
        subtitle: esc(deck.name),
      },
    );
    if (locked)
      root
        .querySelectorAll("#fc-card-form textarea")
        .forEach((el) => (el.readOnly = true));
    bind("fc-cancel", detail);
    // A picture or sound goes where the cursor is, in its side's text.
    const insertMedia = async (side, file, name) => {
      const status = root.querySelector("#fc-status");
      status.textContent = $t("Uploading {name}…", { name });
      try {
        const r = await fetch(`/api/v1/flashcards/decks/${deck.id}/media`, {
          method: "POST",
          headers: { "X-Minerva-Client": "1" },
          // Bytes rather than the File, which crashes the Linux desktop
          // app's webview.
          body: await file.arrayBuffer(),
        });
        const v = await r.json().catch(() => ({}));
        if (!r.ok)
          throw new Error(
            v.error ? serverMessage(v.error) : $t("Upload failed"),
          );
        const area = root.querySelector(
          `#fc-card-form textarea[name="${side}"]`,
        );
        const at = area.selectionStart ?? area.value.length;
        const gap = at && !/\s$/.test(area.value.slice(0, at)) ? "\n" : "";
        area.setRangeText(gap + v.markdown, at, area.selectionEnd ?? at, "end");
        area.focus();
        status.textContent = "";
      } catch (err) {
        status.textContent = "";
        error(err);
      }
    };
    root.querySelectorAll("[data-attach]").forEach((input) => {
      input.previousElementSibling.onclick = () => input.click();
      input.onchange = () => {
        const file = input.files[0];
        input.value = "";
        if (file) insertMedia(input.dataset.attach, file, file.name);
      };
    });
    // Recording a sound straight into the card, such as a pronunciation.
    root.querySelectorAll("[data-record]").forEach((button) => {
      let recorder = null;
      button.onclick = async () => {
        if (recorder?.state === "recording") return recorder.stop();
        const type = ["audio/webm;codecs=opus", "audio/webm", "audio/mp4"].find(
          (t) => globalThis.MediaRecorder?.isTypeSupported(t),
        );
        if (!navigator.mediaDevices?.getUserMedia || !type)
          return error(new Error($t("This browser cannot record audio.")));
        let stream;
        try {
          stream = await navigator.mediaDevices.getUserMedia({ audio: true });
        } catch {
          return error(
            new Error(
              $t(
                "Minerva cannot use the microphone. Allow it in your browser's site settings.",
              ),
            ),
          );
        }
        const chunks = [];
        recorder = new MediaRecorder(stream, {
          mimeType: type,
          audioBitsPerSecond: 32000,
        });
        recorder.ondataavailable = (e) => chunks.push(e.data);
        recorder.onstop = () => {
          stream.getTracks().forEach((t) => t.stop());
          button.textContent = $t("● Record sound");
          const blob = new Blob(chunks, { type: type.split(";")[0] });
          if (blob.size)
            insertMedia(button.dataset.record, blob, $t("the recording"));
        };
        recorder.start();
        button.textContent = $t("■ Stop");
      };
    });
    bind("fc-remove", async () => {
      if (
        !(await confirmDelete(
          $t("Delete card"),
          $t("Delete this card and its review progress?"),
        ))
      )
        return;
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
  const attach = (side) =>
    `<span class="fc-media-actions"><button class="btn fc-attach" type="button">${icon("upload")}${$t("Add picture or sound")}</button><input type="file" accept="image/png,image/jpeg,image/gif,image/webp,audio/*" data-attach="${side}" hidden><button type="button" class="btn fc-attach" data-record="${side}">${$t("● Record sound")}</button></span>`;
  /** Statistics of one deck, or of all decks. */
  async function statistics(of = null) {
    const shown = of;
    shell(`<p>${$t("Loading…")}</p>`);
    const offset = -new Date().getTimezoneOffset();
    const s = await api(
      `flashcards/stats?offset=${offset}${shown ? `&deck=${encodeURIComponent(shown.id)}` : ""}`,
    );
    if (!live()) return;
    const back = () => (shown ? load(shown.id) : list());
    shell(statsHTML(s, esc), {
      title: shown
        ? esc($t("Statistics · {name}", { name: shown.name }))
        : $t("Statistics · all decks"),
      actions:
        button($t("Back"), "fc-back") +
        (shown ? "" : button($t("Scheduling"), "fc-settings", "", "settings")),
    });
    bind("fc-back", back);
    bind("fc-settings", () => scheduling(() => statistics()));
  }
  /** The desired retention, and fitting the scheduler to your reviews. */
  async function scheduling(back = list) {
    const st = await api("flashcards/settings");
    if (!live()) return;
    const [lo, hi] = st.retention_range;
    const fitted = st.fitted;
    const report = fitted
      ? `<p class="hint">${esc(
          fitted.kept
            ? $t(
                "Last optimized on {date} from {count} reviews: prediction error {before} → {after}.",
                {
                  date: new Date(fitted.at * 1000).toLocaleDateString(),
                  count: fitted.reviews,
                  before: fitted.loss_before.toFixed(3),
                  after: fitted.loss_after.toFixed(3),
                },
              )
            : $t(
                "Optimizing on {date} found nothing better than the defaults; they stay.",
                {
                  date: new Date(fitted.at * 1000).toLocaleDateString(),
                },
              ),
        )}</p>`
      : "";
    shell(
      `<div class="fc-settings">
        <form class="fc-form" id="fc-settings-form">
          <h2 id="fc-retention-title">${$t("Desired retention")}</h2>
          <p>${$t("Minerva schedules each card for when you are likely to still remember it. Choose how likely: a higher target means more reviews, and less forgetting.")}</p>
          <div class="fc-retention">
            <input type="range" aria-labelledby="fc-retention-title" name="retention" min="${Math.round(lo * 100)}" max="${Math.round(hi * 100)}" step="1" value="${Math.round(st.retention * 100)}">
            <output id="fc-retention-value">${Math.round(st.retention * 100)}%</output>
          </div>
          <p class="hint" id="fc-retention-hint"></p>
          <footer class="fc-actions fc-form-actions"><button class="btn primary" type="submit">${$t("Save")}</button></footer>
        </form>
        <section class="fc-form">
          <h2>${$t("Fit to my memory")}</h2>
          <p>${esc(
            st.optimized
              ? $t("Scheduling uses weights fitted to your reviews.")
              : $t(
                  "Scheduling uses FSRS's default weights, which suit most people. With enough history, Minerva can fit them to how you remember.",
                ),
          )}</p>
          <p class="hint">${esc($tn(st.reviews, "{count} review logged · {min} needed", "{count} reviews logged · {min} needed", { min: st.min_reviews }))}</p>
          ${report}
          <footer class="fc-actions fc-form-actions">
            ${st.optimized ? button($t("Back to the defaults"), "fc-reset") : ""}
            ${button($t("Optimize for my memory"), "fc-optimize")}
          </footer>
        </section>
      </div>`,
      { title: $t("Scheduling"), actions: button($t("Back"), "fc-back") },
    );
    const range = root.querySelector('[name="retention"]');
    const explain = () => {
      const v = Number(range.value);
      root.querySelector("#fc-retention-value").textContent = `${v}%`;
      root.querySelector("#fc-retention-hint").textContent =
        v >= 95
          ? $t("Very few cards forgotten, at the cost of many more reviews.")
          : v <= 80
            ? $t("Fewer reviews; expect to forget about one card in {count}.", {
                count: Math.round(100 / (100 - v)),
              })
            : $t(
                "A balance of reviews and remembering; 90% suits most people.",
              );
    };
    range.oninput = explain;
    explain();
    root.querySelector("#fc-optimize").disabled = st.reviews < st.min_reviews;
    bind("fc-back", back);
    bind(
      "fc-settings-form",
      async () => {
        await api("flashcards/settings", "PUT", {
          retention: Number(range.value) / 100,
        });
        root.querySelector("#fc-status").textContent = $t(
          "Desired retention saved. New reviews use it.",
        );
      },
      "submit",
    );
    bind("fc-optimize", async () => {
      root.querySelector("#fc-status").textContent = $t(
        "Fitting to your reviews… This can take a minute.",
      );
      await api("flashcards/settings/optimize", "POST", {});
      await scheduling(back);
    });
    bind("fc-reset", async () => {
      await api("flashcards/settings/optimize", "DELETE");
      await scheduling(back);
    });
  }
  function study() {
    if (!queue.length) {
      shell(
        `<div class="fc-study fc-complete"><span class="fc-deck-icon">${icon("check")}</span>
          <p>${$t("You reviewed {reviewed} cards. Cards rated Again return in ten minutes; other cards return on their scheduled day.", { reviewed })}</p>
          ${button($t("Back to deck"), "fc-back", "primary")}
        </div>`,
        { title: $t("Session complete"), subtitle: esc(deck.name) },
      );
      bind("fc-back", () => load(deck.id));
      return;
    }
    const c = deck.cards.find((c) => c.id === queue[0]);
    const progress = $t("{reviewed} reviewed · {count} left", {
      reviewed,
      count: queue.length,
    });
    shell(
      `<section class="fc-review">
        <div class="fc-review-meta">
          <span id="fc-review-progress-label">${progress}</span>
          <progress aria-labelledby="fc-review-progress-label" max="${reviewed + queue.length}" value="${reviewed}"></progress>
        </div>
        <article class="fc-study">
          <div class="fc-study-side">
            <h2>${$t("Question")}</h2>
            <div class="fc-face">${rich(c.front, esc)}</div>
            ${button($t("Reveal answer"), "fc-reveal", "primary")}
          </div>
          <div id="fc-answer" hidden>
            <div class="fc-study-side"><h2>${$t("Answer")}</h2><div class="fc-face">${rich(c.back, esc)}</div><div class="fc-sources">${sources(c.sources)}</div></div>
            <div class="fc-rate-zone">
              <p>${$t("How well did you remember?")}</p>
              <div class="fc-ratings">${["again", "hard", "good", "easy"].map((r) => `<button type="button" class="btn" data-rate="${r}">${RATINGS[r]}<small>${intervalLabel(c.next?.[r])}</small></button>`).join("")}</div>
            </div>
          </div>
        </article>
      </section>`,
      {
        title: $t("Study session"),
        subtitle: esc(deck.name),
        actions: button($t("Finish for now"), "fc-back"),
      },
    );
    // How long the card took, from the question to the rating.
    const shownAt = performance.now();
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
                {
                  rating: el.dataset.rate,
                  revision: c.revision,
                  seconds: Math.min(
                    3600,
                    Math.round((performance.now() - shownAt) / 100) / 10,
                  ),
                },
              ),
            );
            queue.shift();
            reviewed++;
            study();
            root
              .querySelector(queue.length ? "#fc-reveal" : "#fc-back")
              ?.focus({ preventScroll: true });
          } catch (e) {
            error(e);
            await load(deck.id).catch(error);
          }
        }),
    );
  }
  async function generateForm() {
    shell(`<p>${$t("Loading AI providers…")}</p>`);
    const config = await api(
      `ai/providers?workspace_id=${encodeURIComponent(workspaceId || "")}`,
    );
    if (!live()) return;
    const providers = Array.isArray(config) ? config : config.providers || [];
    if (!providers.length) {
      shell(
        `<div class="empty fc-empty">${icon("brain")}<h2>${$t("Connect an AI provider first.")}</h2><p>${$t("Add a personal provider in User settings, or ask your server administrator to share one.")}</p>${button($t("Back to deck"), "fc-back")}</div>`,
        { title: $t("Create with AI"), subtitle: esc(deck.name) },
      );
      bind("fc-back", detail);
      return;
    }
    const notes = [...data.notes, ...(data.shared_notes || [])].filter(
      (n) => !n.trashed,
    );
    shell(
      `<form class="fc-form" id="fc-generate">
        <p class="hint">${$t("Selected source content is sent to your chosen AI provider. Review the drafts before saving. Provider charges may apply.")}</p>
        <div class="fc-pair">
          <label class="fc-field">${$t("AI provider")}<select name="provider_id">${providers.map((p) => `<option value="${esc(p.id)}">${esc(p.name)}${p.web_search ? $t(" · web search") : ""}</option>`).join("")}</select></label>
          <label class="fc-field">${$t("Make cards from")}<select name="kind" id="fc-kind"><option value="note">${$t("A note")}</option><option value="folder">${$t("A folder and its subfolders")}</option><option value="topic">${$t("Research a topic on the web")}</option></select></label>
        </div>
        <label class="fc-field" id="fc-note-label">${$t("Note")}<select name="note">${notes.map((n) => `<option value="${esc(n.id)}">${esc(n.title)}</option>`).join("")}</select></label>
        <label class="fc-field" id="fc-folder-label" hidden>${$t("Folder")}<select name="folder">${data.folders.map((f) => `<option value="${esc(f.id)}">${esc(f.name)}</option>`).join("")}</select></label>
        <label class="fc-field" id="fc-topic-label" hidden>${$t("Topic")}<textarea name="topic" rows="3" maxlength="1000" placeholder="${$t("For example: the causes of the French Revolution")}"></textarea><small>${$t("Requires an OpenAI or Anthropic profile with web search enabled.")}</small></label>
        <label class="fc-field fc-number">${$t("Number of cards")}<input name="count" type="number" min="1" max="20" value="10" required></label>
        <div class="fc-status" role="status" id="fc-generating"></div>
        <footer class="fc-actions fc-form-actions">${button($t("Cancel"), "fc-cancel")}<button type="submit" class="btn primary" id="fc-generate-button">${$t("Generate drafts")}</button></footer>
      </form>`,
      { title: $t("Create with AI"), subtitle: esc(deck.name) },
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
            workspace_id: workspaceId,
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
      `<form id="fc-drafts" class="fc-drafts">
        ${cards
          .map(
            (c, i) => `<div class="fc-draft">
          <label class="fc-check"><input type="checkbox" name="keep-${i}" checked><span>${$t("Keep card {value}", { value: i + 1 })}</span></label>
          <div class="fc-pair">${field($t("Question"), "front-" + i, c.front, true)}${field($t("Answer"), "back-" + i, c.back, true)}</div>
          ${c.sources?.length ? `<div class="fc-sources">${sources(c.sources)}</div>` : ""}
        </div>`,
          )
          .join("")}
        <footer class="fc-actions fc-form-actions">${button($t("Discard drafts"), "fc-discard")}<button type="submit" class="btn primary">${$t("Save selected cards")}</button></footer>
      </form>`,
      {
        title: $t("Review drafts"),
        subtitle: $t(
          "Check each answer against its sources, edit it, and deselect anything you don’t want to keep. Nothing has been saved yet.",
        ),
      },
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
      /!\[audio\]\(media:([0-9a-f-]{36})\)/g,
      '<audio class="fc-sound" controls preload="none" src="/api/v1/flashcards/media/$1"></audio>',
    )
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
  if (seconds < day) {
    const minutes = Math.max(1, Math.round(seconds / 60));
    return $tn(minutes, "{count} minute", "{count} minutes");
  }
  const days = Math.round(seconds / day);
  if (days < 31) return $tn(days, "{count} day", "{count} days");
  if (days < 365) {
    const months = Math.round((days / 30.4) * 10) / 10;
    return $tn(months, "{count} month", "{count} months");
  }
  const years = Math.round((days / 365) * 10) / 10;
  return $tn(years, "{count} year", "{count} years");
}
/** The four ratings, as the buttons say them. */
const RATINGS = {
  again: $t("Again"),
  hard: $t("Hard"),
  good: $t("Good"),
  easy: $t("Easy"),
};
