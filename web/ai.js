import { splitMarkdown } from "./markdown.js";
const $ = (selector, root = document) => root.querySelector(selector);
const $$ = (selector, root = document) => [...root.querySelectorAll(selector)];
let controller = null;
let generation = 0;
export function stopAI() {
  generation++;
  controller?.abort();
  controller = null;
}
const presets = {
  openai: { label: "OpenAI", endpoint: "https://api.openai.com/v1" },
  deepseek: { label: "DeepSeek", endpoint: "https://api.deepseek.com" },
  anthropic: {
    label: "Anthropic Claude",
    endpoint: "https://api.anthropic.com/v1",
  },
  compatible: { label: "OpenAI-compatible API", endpoint: "" },
};

export async function renderAISettings({ api, esc, data, error }) {
  const epoch = generation;
  const providers = await api("ai/providers");
  if (epoch !== generation) return;
  $("#breadcrumb").textContent = "AI providers";
  $("#content").innerHTML =
    `<section class="page"><div class="eyebrow">IN-APP INTELLIGENCE</div><div class="page-heading"><div><h1>AI providers</h1><p class="subtitle">Choose the models your server can use for contextual chat.</p></div>${data.user.admin ? '<button class="btn primary" id="add-provider">+ Add provider</button>' : ""}</div>
    <div class="panel ai-notice"><h2>Your context, your choice</h2><p>Chat sends the question, recent conversation, and permitted notes and records from the selected scope to your chosen provider. Uploaded image and file contents are not sent. Provider usage may incur API charges.</p><p>API keys are encrypted on the server and never returned to this page. ChatGPT subscriptions do not supply OpenAI API credits. These settings are separate from external MCP connections.</p></div>
    <div class="ai-provider-grid">${providers.map((p) => `<section class="panel ai-provider"><span class="pill">${esc(presets[p.kind]?.label || p.kind)}</span><h2>${esc(p.name)}</h2><p>${esc(p.model)}</p><small>${esc(p.endpoint)}</small><p>Available to server members · key stored${p.web_search ? " · web search on" : ""} · answers up to ${Number(p.max_tokens || 4096).toLocaleString()} tokens</p>${data.user.admin ? `<div class="ai-provider-actions"><button class="btn" data-edit-provider="${p.id}">Edit</button><button class="btn danger" data-remove-provider="${p.id}">Remove provider</button></div>` : ""}</section>`).join("") || '<div class="empty"><h3>No providers configured</h3><p>A server administrator can add a model and API key to enable chat.</p></div>'}</div></section>`;
  // Adds a provider, or edits `existing`. The stored key is never shown: an
  // empty key field keeps it, and pointing it at another server needs it again.
  const providerForm = (existing = null) => {
    const modal = $("#modal");
    const p = existing || { kind: "openai", endpoint: presets.openai.endpoint };
    modal.innerHTML = `<form id="provider-form"><h2>${existing ? "Edit AI provider" : "Add an AI provider"}</h2><p>This profile is available to server members. Requests use this API key and its billing account.</p><label>Provider<select name="kind">${Object.entries(
      presets,
    )
      .map(
        ([k, preset]) =>
          `<option value="${k}" ${k === p.kind ? "selected" : ""}>${esc(preset.label)}</option>`,
      )
      .join(
        "",
      )}</select></label><label>Profile name<input name="name" maxlength="200" placeholder="e.g. Team assistant" value="${esc(p.name || "")}" required></label><label>API base URL<input name="endpoint" type="url" value="${esc(p.endpoint || "")}" required></label><label>Model ID<input name="model" placeholder="Exact model ID from your provider" maxlength="200" value="${esc(p.model || "")}" required></label><label class="checkbox-label"><input type="checkbox" name="web_search" ${p.web_search ? "checked" : ""}>Allow web search</label><p class="hint web-search-hint"></p><label>Max answer length (tokens)<input name="max_tokens" type="number" min="256" max="131072" step="256" value="${esc(String(p.max_tokens || 4096))}" required></label><p class="hint">Thinking models (Qwen, DeepSeek R1…) also spend these tokens reasoning: give them 16384 or more, within the model's context window. 4096 suits most cloud models and keeps costs predictable.</p><label>API key<input name="api_key" type="password" autocomplete="new-password" maxlength="4096" ${existing ? 'placeholder="Leave empty to keep the stored key"' : "required"}></label>${existing ? "<p>The stored key is kept unless you enter a new one. Changing the API base URL to another server requires entering the key again.</p>" : ""}<p>Use the provider’s API base URL, or the address of a self-hosted or compatible server. Questions and permitted note content are sent there, so use HTTPS unless the server is on a trusted network. Keep a backup of the server’s .ai-key file alongside the database.</p><div class="form-error" role="alert"></div><footer><button type="button" class="btn" id="cancel-provider">Cancel</button><button class="btn primary" type="submit">Save provider</button></footer></form>`;
    modal.showModal();
    const clear = () => {
      modal.close();
      modal.innerHTML = "";
    };
    $("#cancel-provider").onclick = clear;
    modal.addEventListener(
      "close",
      () => {
        const key = $("input[name=api_key]", modal);
        if (key) key.value = "";
      },
      { once: true },
    );
    // Only OpenAI and Anthropic have a web search tool of their own.
    const searchable = () => {
      const kind = $("select[name=kind]", modal).value;
      const box = $("input[name=web_search]", modal);
      const supported = kind === "openai" || kind === "anthropic";
      box.disabled = !supported;
      if (!supported) box.checked = false;
      $(".web-search-hint", modal).textContent = supported
        ? "The model may search the web while answering. Searches run at the provider, are billed per search, and receive search terms based on the question and notes."
        : "Web search is available for OpenAI and Anthropic providers.";
    };
    searchable();
    $("select[name=kind]", modal).onchange = (e) => {
      $("input[name=endpoint]", modal).value = presets[e.target.value].endpoint;
      searchable();
    };
    $("#provider-form").onsubmit = async (e) => {
      e.preventDefault();
      const button = $("button[type=submit]", modal);
      button.disabled = true;
      try {
        const values = Object.fromEntries(new FormData(e.target));
        values.web_search = $("input[name=web_search]", modal).checked;
        values.max_tokens = Number(values.max_tokens);
        if (existing) await api(`ai/providers/${existing.id}`, "PATCH", values);
        else await api("ai/providers", "POST", values);
        clear();
        await renderAISettings({ api, esc, data, error });
      } catch (err) {
        $(".form-error", modal).textContent = err.message;
        button.disabled = false;
      }
    };
  };
  $("#add-provider")?.addEventListener("click", () => providerForm());
  $$("[data-edit-provider]").forEach(
    (button) =>
      (button.onclick = () =>
        providerForm(
          providers.find((p) => p.id === button.dataset.editProvider),
        )),
  );
  $$("[data-remove-provider]").forEach(
    (button) =>
      (button.onclick = async () => {
        const modal = $("#modal");
        modal.innerHTML =
          '<h2>Remove this provider?</h2><p>New requests will stop using this profile. Existing conversation history stays available.</p><footer><button class="btn" id="cancel-remove">Cancel</button><button class="btn danger" id="confirm-remove">Remove</button></footer>';
        modal.showModal();
        $("#cancel-remove").onclick = () => modal.close();
        $("#confirm-remove").onclick = async () => {
          try {
            await api(
              `ai/providers/${button.dataset.removeProvider}`,
              "DELETE",
            );
            modal.close();
            await renderAISettings({ api, esc, data, error });
          } catch (err) {
            error(err);
          }
        };
      }),
  );
}

export async function renderAIChat({
  api,
  esc,
  data,
  workspaceId,
  initialScope,
  docked = false,
  dockedTitle = "",
  openNote,
  openDatabase,
  sendToNote,
  noteFromReply,
  canEditNote,
  configure,
  error,
}) {
  const epoch = generation;
  const [providers, history] = await Promise.all([
    api("ai/providers"),
    api("ai/chats"),
  ]);
  if (epoch !== generation) return;
  let chat = null,
    running = false;
  // A docked chat (a tab beside a note) stays on its one context.
  const scopeOptions = docked
    ? [
        {
          kind: initialScope.kind,
          id: initialScope.id,
          label: `${initialScope.kind === "note" ? "Note" : "Folder"} · ${dockedTitle}`,
        },
      ]
    : [
        {
          kind: "workspace",
          id: workspaceId,
          label:
            "Workspace · " +
            (data.workspaces.find((w) => w.id === workspaceId)?.name || ""),
        },
        ...data.folders
          .filter((f) => f.workspace_id === workspaceId)
          .map((f) => ({
            kind: "folder",
            id: f.id,
            label: "Folder · " + f.name,
          })),
        ...data.notes
          .filter(
            (n) =>
              !n.trashed &&
              data.folders.some(
                (f) => f.id === n.folder_id && f.workspace_id === workspaceId,
              ),
          )
          .map((n) => ({ kind: "note", id: n.id, label: "Note · " + n.title })),
      ];
  const chosen = scopeOptions.findIndex(
    (s) => s.kind === initialScope?.kind && s.id === initialScope?.id,
  );
  $("#breadcrumb").textContent = "AI chat";
  $("#content").innerHTML =
    `<section class="page ai-page ${docked ? "docked" : ""}">${docked ? `<div class="page-heading ai-docked-heading"><h1><span class="chat-avatar" aria-hidden="true">✳</span>${esc(dockedTitle)}</h1><div class="ai-heading-actions"><button class="btn" id="ai-history-toggle" aria-expanded="false" aria-controls="ai-history">History</button><button class="btn" id="ai-settings">Providers</button></div></div>` : '<div class="eyebrow">ANSWERS FROM YOUR KNOWLEDGE</div><div class="page-heading"><div><h1>Ask your workspace</h1><p class="subtitle">A focused conversation, with sources you can open.</p></div><button class="btn" id="ai-settings">Provider settings</button></div>'}<div class="ai-layout"><aside class="panel ai-history" id="ai-history" ${docked ? "hidden" : ""}><button class="btn" id="new-chat">+ New conversation</button><h3>Your conversations</h3><div id="chat-history"></div></aside><section class="panel ai-conversation"><div class="ai-controls"><label>Context<select id="chat-scope">${scopeOptions.map((s, i) => `<option value="${i}" ${i === Math.max(0, chosen) ? "selected" : ""}>${esc(s.label)}</option>`).join("")}</select></label><label>Model<select id="chat-provider">${providers.map((p) => `<option value="${p.id}">${esc(p.name)} · ${esc(p.model)}</option>`).join("") || '<option value="">No provider configured</option>'}</select></label></div><details class="ai-disclosure"><summary>What is sent to the model?</summary><p>Only permitted text from this context is sent to the selected provider, together with recent conversation. Context is capped; source coverage appears with each answer. Replies may be inaccurate.</p></details><div id="chat-messages" class="chat-messages" aria-label="Conversation"><div class="empty"><h3>What would you like to understand?</h3><p>Summarize a project, compare ideas, or ask about a note.</p></div></div><form id="chat-form" class="chat-composer"><label for="chat-question" class="sr-only">Your question</label><div class="chat-input"><textarea id="chat-question" rows="1" maxlength="8000" placeholder="${docked ? "Ask about this note…" : "Ask a question about this context…"}" required ${providers.length ? "" : "disabled"}></textarea><button class="btn primary chat-send" id="send-chat" title="Send (Enter)" aria-label="Send" ${providers.length ? "" : "disabled"}>↑</button></div><div class="chat-compose-actions"><span id="chat-status" role="status">${providers.length ? "Enter to send · Shift+Enter for a new line" : "Configure a provider to begin."}</span><button class="btn" id="stop-chat" type="button" disabled>Stop</button></div></form></section></div></section>`;
  $("#ai-settings").onclick = configure;
  // Beside a note, past conversations open on demand.
  const historyToggle = $("#ai-history-toggle");
  if (historyToggle)
    historyToggle.onclick = () => {
      const panel = $("#ai-history");
      panel.hidden = !panel.hidden;
      historyToggle.setAttribute("aria-expanded", String(!panel.hidden));
    };
  // The question box grows with its text; Enter sends, Shift+Enter breaks a line.
  const questionBox = $("#chat-question");
  const fit = () => {
    questionBox.style.height = "auto";
    questionBox.style.height = Math.min(questionBox.scrollHeight, 220) + "px";
  };
  questionBox.addEventListener("input", fit);
  questionBox.addEventListener("keydown", (e) => {
    if (e.key === "Enter" && !e.shiftKey && !e.isComposing) {
      e.preventDefault();
      $("#chat-form").requestSubmit();
    }
  });
  const scope = () => {
    const selected = scopeOptions[Number($("#chat-scope").value)];
    return selected && { kind: selected.kind, id: selected.id };
  };
  // What a conversation is about, by name, wherever it is.
  const scopeLabel = (scope) => {
    const find = (list, id) => (list || []).find((x) => x.id === id);
    if (scope.kind === "workspace")
      return `Workspace · ${find(data.workspaces, scope.id)?.name || "unavailable"}`;
    if (scope.kind === "folder")
      return `Folder · ${find(data.folders, scope.id)?.name || "unavailable"}`;
    const n = find(data.notes, scope.id) || find(data.shared_notes, scope.id);
    return `Note · ${n?.title || "unavailable"}`;
  };
  // The context select also offers a conversation's context from elsewhere.
  const scopeIndex = (scope) => {
    let i = scopeOptions.findIndex(
      (s) => s.kind === scope.kind && s.id === scope.id,
    );
    if (i < 0) {
      scopeOptions.push({ ...scope, label: scopeLabel(scope) });
      i = scopeOptions.length - 1;
      $("#chat-scope").insertAdjacentHTML(
        "beforeend",
        `<option value="${i}">${esc(scopeOptions[i].label)}</option>`,
      );
    }
    return i;
  };
  const ago = (seconds) => {
    const minutes = Math.round((Date.now() / 1000 - seconds) / 60);
    if (minutes < 1) return "just now";
    if (minutes < 60) return `${minutes} min ago`;
    if (minutes < 1440) return `${Math.round(minutes / 60)} h ago`;
    return new Date(seconds * 1000).toLocaleDateString();
  };
  const drawHistory = () => {
    // Every conversation, newest first; a chat beside a note shows that note's.
    const shown = docked
      ? history.filter(
          (c) =>
            c.scope.kind === initialScope.kind &&
            c.scope.id === initialScope.id,
        )
      : history;
    $("#chat-history").innerHTML =
      shown
        .map(
          (c) =>
            `<div class="chat-history-row ${chat?.id === c.id ? "current" : ""}"><button class="chat-history-item" data-chat="${c.id}" title="${esc(c.title)}"><span class="chat-history-title">${esc(c.title)}</span><small>${esc(scopeLabel(c.scope))} · ${ago(c.updated_at)}</small></button><button class="chat-delete" data-delete-chat="${c.id}" aria-label="Delete conversation “${esc(c.title)}”" title="Delete conversation">×</button></div>`,
        )
        .join("") || '<p class="chat-history-empty">No conversations yet.</p>';
    if (history.length)
      $("#chat-history").insertAdjacentHTML(
        "beforeend",
        `<button class="text-link chat-delete-all" id="delete-all-chats">Delete all conversations (${history.length})</button>`,
      );
    // Deleting asks once more on the same button, then cannot be undone.
    const confirmed = (button, label) => {
      if (button.dataset.armed) return true;
      button.dataset.armed = "1";
      const original = button.textContent;
      button.textContent = label;
      button.classList.add("armed");
      setTimeout(() => {
        if (!button.isConnected) return;
        delete button.dataset.armed;
        button.textContent = original;
        button.classList.remove("armed");
      }, 4000);
      return false;
    };
    $$("[data-delete-chat]").forEach(
      (button) =>
        (button.onclick = async () => {
          if (running || !confirmed(button, "Delete?")) return;
          const id = button.dataset.deleteChat;
          try {
            await api(`ai/chats/${id}`, "DELETE");
            history.splice(
              history.findIndex((c) => c.id === id),
              1,
            );
            if (chat?.id === id) reset();
            else drawHistory();
          } catch (err) {
            error(err);
          }
        }),
    );
    const all = $("#delete-all-chats");
    if (all)
      all.onclick = async () => {
        if (
          running ||
          !confirmed(all, "Delete every conversation? Click again")
        )
          return;
        try {
          await api("ai/chats", "DELETE");
          history.length = 0;
          reset();
        } catch (err) {
          error(err);
        }
      };
    $$("[data-chat]").forEach(
      (button) =>
        (button.onclick = async () => {
          if (running) return;
          try {
            const loaded = await api(`ai/chats/${button.dataset.chat}`);
            if (epoch !== generation) return;
            chat = loaded;
            $("#chat-scope").value = String(scopeIndex(chat.scope));
            drawTurns();
            drawHistory();
          } catch (err) {
            error(err);
          }
        }),
    );
  };
  function sourcesHTML(sources) {
    return sources
      .map(
        (s, i) =>
          `<button class="source-chip" data-source="${esc(s.id)}" data-kind="${esc(s.kind)}" title="Open ${esc(s.title)}"><span class="source-number">S${i + 1}</span>${esc(s.title)}</button>`,
      )
      .join("");
  }
  /** Who answers: the assistant, with the provider and model. */
  function replyMeta(provider, model) {
    return `<div class="chat-meta"><span class="chat-avatar" aria-hidden="true">✳</span><strong>${esc(provider || "Assistant")}</strong><span>${esc(model || "")}</span></div>`;
  }
  function bindSources(root) {
    $$("[data-source]", root).forEach(
      (button) =>
        (button.onclick = () => {
          (button.dataset.kind === "note"
            ? openNote(button.dataset.source)
            : openDatabase(button.dataset.source)
          ).catch(error);
        }),
    );
  }
  function drawTurns() {
    $("#chat-messages").innerHTML =
      chat?.turns
        .map(
          (t) =>
            `<article class="chat-turn"><p class="chat-question">${esc(t.question)}</p><div class="chat-reply">${replyMeta(t.provider, t.model)}<div class="chat-answer">${esc(t.answer)}</div><div class="chat-sources">${sourcesHTML(t.sources)}</div><small class="chat-usage">${usageLabel(t.usage)}</small></div></article>`,
        )
        .join("") ||
      '<div class="empty"><h3>A new conversation</h3><p>Ask a question to get started.</p></div>';
    bindSources($("#chat-messages"));
    const scope = chat?.scope;
    $$(".chat-turn", $("#chat-messages")).forEach((turn, i) =>
      reply(turn, chat.turns[i].answer, chat.turns[i].question, scope),
    );
  }
  // Rendered reply blocks by Markdown, shared by every redraw.
  const rendered = new Map();
  async function renderBlocks(blocks) {
    const missing = [...new Set(blocks.filter((b) => !rendered.has(b)))];
    if (missing.length) {
      try {
        const { html } = await api("render", "POST", { blocks: missing });
        missing.forEach((b, i) => rendered.set(b, html[i]));
      } catch {
        // A server without batch rendering: one block at a time.
        for (const b of missing)
          rendered.set(b, (await api("render", "POST", { markdown: b })).html);
      }
    }
    return blocks.map((b) => rendered.get(b));
  }
  /** Shows a finished reply as note blocks, with actions to keep it. */
  async function reply(turn, markdown, question, scope) {
    const answer = $(".chat-answer", turn);
    const blocks = splitMarkdown(markdown).filter((b) => b.trim());
    let html;
    try {
      html = await renderBlocks(blocks);
    } catch {
      return; // Keep the plain text.
    }
    if (!answer.isConnected) return;
    const target =
      scope?.kind === "note" && canEditNote(scope.id) ? scope.id : null;
    answer.classList.add("chat-blocks");
    answer.innerHTML = blocks
      .map(
        (_, i) =>
          `<div class="chat-block"><div class="chat-block-body">${html[i]}</div>${target ? `<button class="chat-block-add" data-add-block="${i}" title="Add this block to the end of the note" aria-label="Add this block to the note">＋ Note</button>` : ""}</div>`,
      )
      .join("");
    turn.querySelector(".chat-reply-actions")?.remove();
    const actions = document.createElement("div");
    actions.className = "chat-reply-actions";
    actions.innerHTML = `<button class="btn" data-reply="copy">Copy</button><button class="btn" data-reply="note">New note</button>${target ? '<button class="btn primary" data-reply="add">Add all to note</button>' : ""}`;
    answer.after(actions);
    // Button feedback: a short confirmation, or the error.
    const run = async (button, label, work) => {
      button.disabled = true;
      const original = button.textContent;
      try {
        await work();
        button.textContent = label;
        setTimeout(() => (button.textContent = original), 1800);
      } catch (err) {
        error(err);
      } finally {
        button.disabled = false;
      }
    };
    $$("[data-add-block]", answer).forEach(
      (button) =>
        (button.onclick = () =>
          run(button, "✓ Added", () =>
            sendToNote(target, [blocks[Number(button.dataset.addBlock)]]),
          )),
    );
    $$("[data-reply]", actions).forEach(
      (button) =>
        (button.onclick = () =>
          ({
            copy: () => run(button, "✓ Copied", () => copy(markdown)),
            note: () =>
              run(button, "✓ Saved", () =>
                noteFromReply(markdown, question, scope),
              ),
            add: () => run(button, "✓ Added", () => sendToNote(target, blocks)),
          })[button.dataset.reply]()),
    );
  }
  async function copy(text) {
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      // Clipboard API unavailable (for example without focus): select and copy.
      const area = document.createElement("textarea");
      area.value = text;
      area.style.position = "fixed";
      area.style.opacity = "0";
      document.body.append(area);
      area.select();
      const ok = document.execCommand("copy");
      area.remove();
      if (!ok) throw new Error("Copying is not allowed here.");
    }
  }
  function usageLabel(usage) {
    const input = usage?.input_tokens ?? usage?.prompt_tokens;
    const output = usage?.output_tokens ?? usage?.completion_tokens;
    return input != null || output != null
      ? `${input ?? "?"} input / ${output ?? "?"} output tokens`
      : "Usage not reported";
  }
  function reset() {
    if (running) return;
    chat = null;
    drawTurns();
    drawHistory();
    $("#chat-status").textContent = "New conversation · ready";
  }
  $("#new-chat").onclick = reset;
  $("#chat-scope").onchange = reset;
  $("#stop-chat").onclick = () => controller?.abort();
  $("#chat-form").onsubmit = async (e) => {
    e.preventDefault();
    if (running || !scope()) return;
    const question = $("#chat-question").value.trim();
    if (!question) return;
    running = true;
    controller = new AbortController();
    ["send-chat", "chat-scope", "chat-provider", "new-chat"].forEach(
      (id) => ($("#" + id).disabled = true),
    );
    $("#stop-chat").disabled = false;
    $("#chat-status").textContent = "Preparing context…";
    const messages = $("#chat-messages");
    if (!chat?.turns.length) messages.innerHTML = "";
    const turn = document.createElement("article");
    turn.className = "chat-turn";
    const chosen = providers.find((p) => p.id === $("#chat-provider").value);
    turn.innerHTML = `<p class="chat-question">${esc(question)}</p><div class="chat-reply">${replyMeta(chosen?.name, chosen?.model)}<div class="chat-answer chat-pending"><span class="chat-dots" aria-hidden="true"><i></i><i></i><i></i></span><span class="chat-pending-text">Reading your notes…</span></div><div class="chat-sources"></div><small class="chat-usage"></small></div>`;
    messages.append(turn);
    turn.scrollIntoView({ block: "start", behavior: "smooth" });
    // The question is on its way: the box is free for the next one.
    $("#chat-question").value = "";
    fit();
    let complete = false;
    // The reply is shown formatted while it streams, redrawn a few times a
    // second rather than on every token.
    let drawn = 0,
      drawing = null;
    const live = (text) => {
      const box = $(".chat-answer", turn);
      if (!drawn) box.classList.remove("chat-pending");
      if (drawing) return;
      drawing = setTimeout(
        async () => {
          drawing = null;
          const blocks = splitMarkdown(text).filter((b) => b.trim());
          try {
            const html = await renderBlocks(blocks);
            if (box.isConnected && !box.classList.contains("chat-blocks"))
              box.innerHTML = html
                .map((h) => `<div class="chat-block-body">${h}</div>`)
                .join("");
          } catch {
            box.textContent = text;
          }
        },
        drawn++ ? 350 : 0,
      );
    };
    try {
      if (!chat) {
        chat = await api("ai/chats", "POST", { scope: scope() });
        history.unshift(chat);
        drawHistory();
      }
      if (epoch !== generation || controller.signal.aborted)
        throw new DOMException("Stopped", "AbortError");
      const response = await fetch(`/api/v1/ai/chats/${chat.id}/messages`, {
        method: "POST",
        headers: {
          "Content-Type": "application/json",
          "X-Minerva-Client": "1",
        },
        body: JSON.stringify({
          message: question,
          provider_id: $("#chat-provider").value,
          revision: chat.turns.length,
        }),
        signal: controller.signal,
      });
      if (!response.ok) {
        const problem = await response.json();
        throw new Error(problem.error || "Unable to start response");
      }
      const reader = response.body.getReader();
      const decoder = new TextDecoder();
      let pending = "";
      let answer = "",
        sources = [],
        usage = {};
      while (true) {
        const { value, done } = await reader.read();
        if (done) break;
        pending += decoder.decode(value, { stream: true });
        let end;
        while ((end = pending.indexOf("\n")) >= 0) {
          const line = pending.slice(0, end);
          pending = pending.slice(end + 1);
          if (!line.trim()) continue;
          const event = JSON.parse(line);
          if (epoch !== generation) return;
          if (event.type === "context") {
            sources = event.sources;
            $(".chat-sources", turn).innerHTML = sourcesHTML(sources);
            bindSources(turn);
            $("#chat-status").textContent = event.truncated
              ? "Reading selected excerpts · context truncated"
              : "Reading " + sources.length + " sources";
            const pending = $(".chat-pending-text", turn);
            if (pending)
              pending.textContent = `Reading ${sources.length} source${sources.length === 1 ? "" : "s"}…`;
          }
          // Thinking models reason before they answer.
          if (event.type === "thinking" && !answer) {
            $("#chat-status").textContent = "Thinking…";
            const pending = $(".chat-pending-text", turn);
            if (pending) pending.textContent = "Thinking…";
          }
          if (event.type === "delta") {
            answer += event.text;
            live(answer);
            $("#chat-status").textContent = "Writing…";
          }
          if (event.type === "error") throw new Error(event.message);
          if (event.type === "done") {
            complete = true;
            usage = event.usage;
            $(".chat-usage", turn).textContent = usageLabel(usage);
            $("#chat-status").textContent = "Saved";
          }
        }
      }
      if (!complete)
        throw new Error("Response interrupted. Partial output was not saved.");
      chat = await api(`ai/chats/${chat.id}`);
      if (epoch !== generation) return;
      reply(turn, answer, question, chat.scope);
      Object.assign(
        history.find((c) => c.id === chat.id),
        chat,
      );
      drawHistory();
      $("#chat-question").value = "";
    } catch (err) {
      if (epoch === generation) {
        $("#chat-status").textContent =
          err.name === "AbortError"
            ? "Stopped · partial output was not saved"
            : err.message;
        turn.classList.add("chat-incomplete");
        // Not answered: the question goes back in the box to try again.
        if (!$("#chat-question").value) $("#chat-question").value = question;
      }
    } finally {
      if (epoch === generation) {
        running = false;
        controller = null;
        ["send-chat", "chat-scope", "chat-provider", "new-chat"].forEach(
          (id) => ($("#" + id).disabled = false),
        );
        $("#stop-chat").disabled = true;
      }
    }
  };
  drawHistory();
}
