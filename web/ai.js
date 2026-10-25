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
    <div class="ai-provider-grid">${providers.map((p) => `<section class="panel ai-provider"><span class="pill">${esc(presets[p.kind]?.label || p.kind)}</span><h2>${esc(p.name)}</h2><p>${esc(p.model)}</p><small>${esc(p.endpoint)}</small><p>Available to server members · key stored</p>${data.user.admin ? `<button class="btn danger" data-remove-provider="${p.id}">Remove provider</button>` : ""}</section>`).join("") || '<div class="empty"><h3>No providers configured</h3><p>A server administrator can add a model and API key to enable chat.</p></div>'}</div></section>`;
  $("#add-provider")?.addEventListener("click", () => {
    const modal = $("#modal");
    modal.innerHTML = `<form id="provider-form"><h2>Add an AI provider</h2><p>This profile is available to server members. Requests use this API key and its billing account.</p><label>Provider<select name="kind">${Object.entries(
      presets,
    )
      .map(([k, p]) => `<option value="${k}">${esc(p.label)}</option>`)
      .join(
        "",
      )}</select></label><label>Profile name<input name="name" maxlength="200" placeholder="e.g. Team assistant" required></label><label>API base URL<input name="endpoint" type="url" value="https://api.openai.com/v1" required></label><label>Model ID<input name="model" placeholder="Exact model ID from your provider" maxlength="200" required></label><label>API key<input name="api_key" type="password" autocomplete="new-password" required maxlength="4096"></label><p>Use the provider’s API base URL, or the address of a self-hosted or compatible server. Questions and permitted note content are sent there, so use HTTPS unless the server is on a trusted network. Keep a backup of the server’s .ai-key file alongside the database.</p><div class="form-error" role="alert"></div><footer><button type="button" class="btn" id="cancel-provider">Cancel</button><button class="btn primary" type="submit">Save provider</button></footer></form>`;
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
    $("select[name=kind]", modal).onchange = (e) => {
      $("input[name=endpoint]", modal).value = presets[e.target.value].endpoint;
    };
    $("#provider-form").onsubmit = async (e) => {
      e.preventDefault();
      const button = $("button[type=submit]", modal);
      button.disabled = true;
      try {
        const values = Object.fromEntries(new FormData(e.target));
        await api("ai/providers", "POST", values);
        clear();
        await renderAISettings({ api, esc, data, error });
      } catch (err) {
        $(".form-error", modal).textContent = err.message;
        button.disabled = false;
      }
    };
  });
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
  openNote,
  openDatabase,
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
  const scopeOptions = [
    {
      kind: "workspace",
      id: workspaceId,
      label:
        "Workspace · " +
        (data.workspaces.find((w) => w.id === workspaceId)?.name || ""),
    },
    ...data.folders
      .filter((f) => f.workspace_id === workspaceId)
      .map((f) => ({ kind: "folder", id: f.id, label: "Folder · " + f.name })),
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
    `<section class="page ai-page"><div class="eyebrow">ANSWERS FROM YOUR KNOWLEDGE</div><div class="page-heading"><div><h1>Ask your workspace</h1><p class="subtitle">A focused conversation, with sources you can open.</p></div><button class="btn" id="ai-settings">Provider settings</button></div><div class="ai-layout"><aside class="panel ai-history"><button class="btn" id="new-chat">+ New conversation</button><h3>Your conversations</h3><div id="chat-history"></div></aside><section class="panel ai-conversation"><div class="ai-controls"><label>Context<select id="chat-scope">${scopeOptions.map((s, i) => `<option value="${i}" ${i === Math.max(0, chosen) ? "selected" : ""}>${esc(s.label)}</option>`).join("")}</select></label><label>Model<select id="chat-provider">${providers.map((p) => `<option value="${p.id}">${esc(p.name)} · ${esc(p.model)}</option>`).join("") || '<option value="">No provider configured</option>'}</select></label></div><p class="ai-disclosure">Only permitted text from this context is sent to the selected provider, together with recent conversation. Context is capped; source coverage appears with each answer. Replies may be inaccurate.</p><div id="chat-messages" class="chat-messages" aria-label="Conversation"><div class="empty"><h3>What would you like to understand?</h3><p>Summarize a project, compare ideas, or ask about a note.</p></div></div><form id="chat-form"><label for="chat-question">Your question</label><textarea id="chat-question" rows="3" maxlength="8000" placeholder="Ask a question about this context…" required ${providers.length ? "" : "disabled"}></textarea><div class="chat-compose-actions"><span id="chat-status" role="status">${providers.length ? "Ready" : "Configure a provider to begin."}</span><button class="btn" id="stop-chat" type="button" disabled>Stop</button><button class="btn primary" id="send-chat" ${providers.length ? "" : "disabled"}>Send</button></div></form></section></div></section>`;
  $("#ai-settings").onclick = configure;
  const scope = () => {
    const selected = scopeOptions[Number($("#chat-scope").value)];
    return selected && { kind: selected.kind, id: selected.id };
  };
  const drawHistory = () => {
    $("#chat-history").innerHTML =
      history
        .filter((c) =>
          scopeOptions.some(
            (s) => s.kind === c.scope.kind && s.id === c.scope.id,
          ),
        )
        .map(
          (c) =>
            `<button class="search-result ${chat?.id === c.id ? "command-selected" : ""}" data-chat="${c.id}">${esc(c.title)}</button>`,
        )
        .join("") || "<p>No conversations yet.</p>";
    $$("[data-chat]").forEach(
      (button) =>
        (button.onclick = async () => {
          if (running) return;
          try {
            const loaded = await api(`ai/chats/${button.dataset.chat}`);
            if (epoch !== generation) return;
            chat = loaded;
            $("#chat-scope").value = String(
              scopeOptions.findIndex(
                (s) => s.kind === chat.scope.kind && s.id === chat.scope.id,
              ),
            );
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
          `<button class="btn source-chip" data-source="${esc(s.id)}" data-kind="${esc(s.kind)}">[S${i + 1}] ${esc(s.title)}</button>`,
      )
      .join("");
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
            `<article class="chat-turn"><p class="chat-question">${esc(t.question)}</p><div class="chat-answer">${esc(t.answer)}</div><div class="chat-sources">${sourcesHTML(t.sources)}</div><small>${esc(t.provider)} · ${esc(t.model)} · ${usageLabel(t.usage)}</small></article>`,
        )
        .join("") ||
      '<div class="empty"><h3>A new conversation</h3><p>Ask a question to get started.</p></div>';
    bindSources($("#chat-messages"));
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
    turn.innerHTML = `<p class="chat-question">${esc(question)}</p><div class="chat-answer"></div><div class="chat-sources"></div><small class="chat-usage"></small>`;
    messages.append(turn);
    let complete = false;
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
          }
          if (event.type === "delta") {
            answer += event.text;
            $(".chat-answer", turn).textContent = answer;
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
