import { $t, $tn, serverMessage } from "./i18n.js";
import { statsHTML } from "./flashcard-stats.js";
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
      root.innerHTML = `<section class="page flashcards-page"><div class="eyebrow">${$t("A LITTLE PRACTICE, LASTING KNOWLEDGE")}</div><div class="page-heading"><div><h1>${$t("Flashcards")}</h1><p class="subtitle">${$t("Recall it. Check it. Give it time to stick.")}</p></div>${deck ? button($t("All decks"), "fc-home") : ""}</div><div aria-live="polite" id="fc-status"></div>${body}</section>`;
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
      `<div class="fc-intro"><p>${$t("Your decks and study history are private to your account on this server.")}</p><div class="fc-actions">${button($t("Create a deck"), "fc-create", true)}${button($t("Import from Anki"), "fc-import")}${button($t("Statistics"), "fc-stats")}${button($t("Scheduling"), "fc-settings")}</div></div><div class="fc-decks">${result.decks.map((d) => `<button class="fc-deck" data-deck="${esc(d.id)}"><strong>${esc(d.name)}</strong>${d.workspace ? `<small class="fc-shared">${esc($t("Shared · {name}", { name: d.workspace_name || "" }))}</small>` : ""}<p>${esc(d.description)}</p><span>${$t("{count} cards ·", { count: d.count })} <b>${$t("{count} due", { count: d.due })}</b></span></button>`).join("")}</div>${!result.decks.length ? `<div class="empty"><h3>${$t("Make your first deck.")}</h3>${$t("Add your own questions, or turn a note, folder, or researched topic into cards with AI.")}</div>` : ""}`,
    );
    bind("fc-create", () => deckForm());
    bind("fc-import", importForm);
    bind("fc-stats", () => statistics());
    bind("fc-settings", scheduling);
    root
      .querySelectorAll("[data-deck]")
      .forEach((el) => (el.onclick = () => load(el.dataset.deck).catch(error)));
  }
  // Anki packages (.apkg, .colpkg) or plain-text exports, read by the server.
  function importForm() {
    shell(
      `<form id="fc-import-form" class="fc-form"><h2>${$t("Import from Anki")}</h2><p>${$t("In Anki, choose")} <b>${$t("File → Export")}</b>${$t(", then")} <b>${$t("Anki Deck Package (.apkg)")}</b>${$t(", with media included to keep pictures. A")} <b>${$t("Notes in Plain Text")}</b> ${$t("export or a CSV file with question and answer columns works too.")}</p><p>${$t("Looking for ready-made decks? Browse thousands of free shared decks on")} <a href="https://ankiweb.net/shared/decks" target="_blank" rel="noopener noreferrer">${$t("AnkiWeb ↗")}</a>${$t(", download one, and import the .apkg file here.")}</p><label>${$t("File")}<input type="file" name="file" accept=".apkg,.colpkg,.txt,.csv,.tsv" required></label><label class="checkbox-label"><input type="checkbox" name="progress" checked> ${$t("Keep my review progress from Anki")}</label><p class="hint">${$t("Each Anki deck becomes a deck here. Cards keep their text, pictures, and links; other formatting becomes plain text, and sounds show as [audio]. Export with media to keep the pictures.")}</p><div class="fc-actions"><button class="btn primary" id="fc-import-button">${$t("Import")}</button>${button($t("Cancel"), "fc-cancel")}</div><p role="status" id="fc-importing"></p></form>`,
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
      ? `<fieldset class="fc-source"><legend>${$t("Cards")}</legend><label><input type="radio" name="origin" value="own" checked> ${$t("I’ll write or generate them here")}</label><label><input type="radio" name="origin" value="notes"> ${$t("Follow the cards written in my notes")}</label><select name="source" aria-label="${$t("Notes to follow")}" disabled>${sources.map((o, i) => `<option value="${i}">${esc(o.label)}</option>`).join("")}</select><p class="hint">${$t("In a note, tag a question")} <code>#card</code> ${$t("and indent its answer below it, or write")} <code>Question :: Answer #card</code>. Cloze deletions such as <code>{{c1::Rome}} is the capital of Italy</code> ${$t("become cards too. The deck keeps up with your notes each time you open it.")}</p></fieldset>`
      : "";
    // A deck can be shared with a workspace where you can edit (not your own).
    const shareable = data.workspaces.filter(
      (w) => !w.personal && (w.can_edit || w.owner_id === data.user.id),
    );
    const share = shareable.length
      ? `<label>${$t("Shared with")}<select name="workspace_id"><option value="">${$t("Only me")}</option>${shareable.map((w) => `<option value="${esc(w.id)}" ${deck?.workspace === w.id ? "selected" : ""}>${esc($t("Everyone in {name}", { name: w.name }))}</option>`).join("")}</select></label><p class="hint">${$t("Everyone in the workspace can study a shared deck, each with their own progress; people who can edit there can change its cards.")}</p>`
      : "";
    const percents = [70, 75, 80, 85, 90, 93, 95, 97];
    const retention = `<label>${$t("Desired retention for this deck")}<select name="retention"><option value="">${$t("My own setting (Scheduling)")}</option>${percents.map((p) => `<option value="${p}" ${Math.round((deck?.retention || 0) * 100) === p ? "selected" : ""}>${p}%</option>`).join("")}</select></label>`;
    shell(
      `<form id="fc-deck-form" class="fc-form"><h2>${deck ? $t("Edit deck") : $t("Create a deck")}</h2>${field($t("Deck name"), "name", deck?.name || "")}${field($t("Description"), "description", deck?.description || "", true, false)}${share}${retention}${follow}<div class="fc-actions"><button class="btn primary">${$t("Save deck")}</button>${button($t("Cancel"), "fc-cancel")}</div></form>`,
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
    shell(
      `<div class="fc-deck-heading"><div><h2>${esc(deck.name)}</h2><p>${esc(deck.description)}</p><p>${$t("{count} cards ·", { count: deck.cards.length })} <strong>${$t("{due} due now", { due })}</strong></p>${deck.source ? `<p class="hint">${$t("Follows the cards written in {value}: tag a question", { value: esc(noteSources().find((o) => o.kind === deck.source.kind && o.id === deck.source.id)?.label || $t("notes you can no longer open")) })} <code>#card</code>${$t(", or write a cloze such as")} <code>{{c1::Rome}}</code>.</p>` : ""}</div>${deck.workspace ? `<p class="hint">${esc($t("Shared with {name}: your progress is your own.", { name: data.workspaces.find((w) => w.id === deck.workspace)?.name || "" }))}</p>` : ""}</div><div class="fc-actions">${button($t("Study due cards"), "fc-study", true)}${deck.can_edit ? button($t("Add card"), "fc-add") : ""}${button($t("Statistics"), "fc-deck-stats")}${deck.can_edit ? button($t("Create with AI"), "fc-ai") + button($t("Edit deck"), "fc-edit") : ""}${button($t("Export JSON"), "fc-export")}${deck.can_edit ? button($t("Delete deck"), "fc-delete") : ""}</div></div><p class="subtitle">${$t("Reveal the answer before rating your recall. Cards return when their next review is due.")}</p><div class="fc-card-list">${deck.cards.map((c) => `<article class="fc-card-row"><div><strong>${rich(c.front, esc)}</strong><p>${rich(c.back, esc)}</p><small>${$t("{value} · {reviews} reviews{value2}", { value: c.suspended ? $t("Paused") : c.due <= dueNow() ? $t("Ready to study") : $t("Next review: {date}", { date: new Date(c.due * 1000).toLocaleString() }), reviews: c.reviews, value2: c.origin ? $t(" · from a note") : "" })}</small><div class="fc-sources">${sources(c.sources)}</div></div><button class="btn" data-edit="${esc(c.id)}">${$t("Edit")}</button></article>`).join("")}</div>${!deck.cards.length ? `<div class="empty">${$t("This deck is ready for its first question.")}</div>` : ""}`,
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
        !confirm(
          $t(
            "Delete “{name}” and all its cards and review progress? This cannot be undone.",
            { name: deck.name },
          ),
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
    // A shared deck's cards change only for those who can edit its workspace.
    const locked = fromNote || !deck.can_edit;
    shell(
      `<form class="fc-form" id="fc-card-form"><h2>${card ? $t("Edit card") : $t("Add a card")}</h2>${fromNote ? `<p class="hint">${$t("This card comes from a note:")} <a href="${esc(card.sources[0] || "#")}">${$t("open the note")}</a> ${$t("to change it.")}</p>` : !deck.can_edit ? `<p class="hint">${$t("Only people who can edit this shared deck’s workspace change its cards; you can pause one for yourself.")}</p>` : ""}${field($t("Question / front"), "front", card?.front || "", true)}${locked ? "" : attach("front")}${field($t("Answer / back"), "back", card?.back || "", true)}${locked ? "" : attach("back")}${locked ? "" : field($t("Sources (one HTTP(S) URL or #note/id per line)"), "sources", (card?.sources || []).join("\n"), true, false)}${card ? `<label><input type="checkbox" name="suspended" ${card.suspended ? "checked" : ""}> ${$t("Pause reviews for this card")}</label>` : ""}<div class="fc-actions"><button class="btn primary">${$t("Save card")}</button>${button($t("Cancel"), "fc-cancel")}${card && !locked ? button($t("Delete card"), "fc-remove") : ""}</div></form>`,
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
          body: file,
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
  const attach = (side) =>
    `<span class="fc-media-actions"><label class="btn fc-attach">${$t("Add picture or sound")}<input type="file" accept="image/png,image/jpeg,image/gif,image/webp,audio/*" data-attach="${side}" hidden></label><button type="button" class="btn fc-attach" data-record="${side}">${$t("● Record sound")}</button></span>`;
  /** Statistics of one deck, or of all decks. */
  async function statistics(of = null) {
    const shown = of;
    shell(`<p>${$t("Loading…")}</p>`);
    const offset = -new Date().getTimezoneOffset();
    const s = await api(
      `flashcards/stats?offset=${offset}${shown ? `&deck=${encodeURIComponent(shown.id)}` : ""}`,
    );
    if (!live()) return;
    shell(
      `<div class="fc-deck-heading"><div><h2>${shown ? esc($t("Statistics · {name}", { name: shown.name })) : $t("Statistics · all decks")}</h2></div><div class="fc-actions">${button($t("Back"), "fc-back")}${shown ? "" : button($t("Scheduling"), "fc-settings")}</div></div>${statsHTML(s, esc)}`,
    );
    bind("fc-back", () => (shown ? load(shown.id) : list()));
    bind("fc-settings", scheduling);
  }
  /** The desired retention, and fitting the scheduler to your reviews. */
  async function scheduling() {
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
      `<form class="fc-form" id="fc-settings-form"><h2>${$t("Scheduling")}</h2><p>${$t("Minerva schedules each card for when you are likely to still remember it. Choose how likely: a higher target means more reviews, and less forgetting.")}</p><label>${$t("Desired retention")}<span class="fc-retention"><input type="range" name="retention" min="${Math.round(lo * 100)}" max="${Math.round(hi * 100)}" step="1" value="${Math.round(st.retention * 100)}"><output id="fc-retention-value">${Math.round(st.retention * 100)}%</output></span></label><p class="hint" id="fc-retention-hint"></p><div class="fc-actions"><button class="btn primary">${$t("Save")}</button>${button($t("Back"), "fc-back")}</div><h3>${$t("Fit to my memory")}</h3><p>${esc(
        st.optimized
          ? $t("Scheduling uses weights fitted to your reviews.")
          : $t(
              "Scheduling uses FSRS's default weights, which suit most people. With enough history, Minerva can fit them to how you remember.",
            ),
      )}</p><p class="hint">${esc(
        $tn(
          st.reviews,
          "{count} review logged · {min} needed",
          "{count} reviews logged · {min} needed",
          { min: st.min_reviews },
        ),
      )}</p>${report}<div class="fc-actions">${button($t("Optimize for my memory"), "fc-optimize", true)}${st.optimized ? button($t("Back to the defaults"), "fc-reset") : ""}</div></form>`,
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
    bind("fc-back", list);
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
      await scheduling();
    });
    bind("fc-reset", async () => {
      await api("flashcards/settings/optimize", "DELETE");
      await scheduling();
    });
  }
  function study() {
    if (!queue.length) {
      shell(
        `<div class="fc-study"><h2>${$t("A little more knowledge, yours.")}</h2><p>${$t("You reviewed {reviewed} cards. Cards rated Again return in ten minutes; other cards return on their scheduled day.", { reviewed })}</p>${button($t("Back to deck"), "fc-back", true)}</div>`,
      );
      bind("fc-back", () => load(deck.id));
      return;
    }
    const c = deck.cards.find((c) => c.id === queue[0]);
    shell(
      `<div class="fc-review-meta">${$t("{reviewed} reviewed · {count} left in this session {value}", { reviewed, count: queue.length, value: button($t("Finish for now"), "fc-back") })}</div><article class="fc-study"><div class="eyebrow">${$t("QUESTION")}</div><h2 class="fc-face">${rich(c.front, esc)}</h2>${button($t("Reveal answer"), "fc-reveal", true)}<div id="fc-answer" hidden><hr><div class="eyebrow">${$t("ANSWER")}</div><div class="fc-face">${rich(c.back, esc)}</div><div class="fc-sources">${sources(c.sources)}</div><p>${$t("How well did you remember?")}</p><div class="fc-ratings">${["again", "hard", "good", "easy"].map((r) => `<button class="btn" data-rate="${r}">${RATINGS[r]}<small>${intervalLabel(c.next?.[r])}</small></button>`).join("")}</div></div></article>`,
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
        `<div class="empty"><h2>${$t("Connect an AI provider first.")}</h2><p>${$t("Your administrator can add one in AI providers. You can still make cards by hand.")}</p>${button($t("Back to deck"), "fc-back")}</div>`,
      );
      bind("fc-back", detail);
      return;
    }
    const notes = [...data.notes, ...(data.shared_notes || [])].filter(
      (n) => !n.trashed,
    );
    shell(
      `<form class="fc-form" id="fc-generate"><h2>${$t("Turn understanding into practice.")}</h2><p>${$t("Selected source content is sent to your chosen AI provider. Review the drafts before saving. Provider charges may apply.")}</p><label>${$t("AI provider")}<select name="provider_id">${providers.map((p) => `<option value="${esc(p.id)}">${esc(p.name)}${p.web_search ? $t(" · web search") : ""}</option>`).join("")}</select></label><label>${$t("Make cards from")}<select name="kind" id="fc-kind"><option value="note">${$t("A note")}</option><option value="folder">${$t("A folder and its subfolders")}</option><option value="topic">${$t("Research a topic on the web")}</option></select></label><label id="fc-note-label">${$t("Note")}<select name="note">${notes.map((n) => `<option value="${esc(n.id)}">${esc(n.title)}</option>`).join("")}</select></label><label id="fc-folder-label" hidden>${$t("Folder")}<select name="folder">${data.folders.map((f) => `<option value="${esc(f.id)}">${esc(f.name)}</option>`).join("")}</select></label><label id="fc-topic-label" hidden>${$t("Topic")}<textarea name="topic" rows="3" maxlength="1000" placeholder="${$t("For example: the causes of the French Revolution")}"></textarea><small>${$t("Requires an OpenAI or Anthropic profile with web search enabled.")}</small></label><label>${$t("Number of cards")}<input name="count" type="number" min="1" max="20" value="10" required></label><div class="fc-actions"><button class="btn primary" id="fc-generate-button">${$t("Generate drafts")}</button>${button($t("Cancel"), "fc-cancel")}</div><p role="status" id="fc-generating"></p></form>`,
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
      `<form id="fc-drafts"><h2>${$t("Make these cards your own.")}</h2><p>${$t("Check each answer against its sources, edit it, and deselect anything you don’t want to keep. Nothing has been saved yet.")}</p>${cards.map((c, i) => `<fieldset class="fc-draft"><legend><label><input type="checkbox" name="keep-${i}" checked> ${$t("Keep card {value}", { value: i + 1 })}</label></legend>${field($t("Question"), "front-" + i, c.front, true)}${field($t("Answer"), "back-" + i, c.back, true)}<div class="fc-sources">${sources(c.sources)}</div></fieldset>`).join("")}<div class="fc-actions"><button class="btn primary">${$t("Save selected cards")}</button>${button($t("Discard drafts"), "fc-discard")}</div></form>`,
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
