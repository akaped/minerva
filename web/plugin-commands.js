import { $t } from "./i18n.js";
/** Generic worker UI. Commands and capability availability come from the server. */
export function timestamp(seconds) {
  const total = Math.max(0, Math.floor(Number(seconds) || 0));
  return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, "0")}`;
}
export function sourceReference(segment, source, noteId) {
  if (source?.type === "youtube" && /^[\w-]{11}$/.test(source.id)) {
    return `[${timestamp(segment?.start)}](https://www.youtube.com/watch?v=${source.id}&t=${Math.floor(segment?.start || 0)})`;
  }
  if (source?.type === "audio")
    return `[${timestamp(segment?.start)} · audio](#note/${noteId})`;
  return `[Source block ${segment?.id || ""}](#note/${noteId})`;
}
export function knowledgeMarkdown(item, noteId, segments = [], source = null) {
  const heading = item.kind.replaceAll("_", " ");
  const references = item.source_ids
    .map((id) =>
      sourceReference(
        segments.find((s) => s.id === id) || { id },
        source,
        noteId,
      ),
    )
    .join(" · ");
  return `### ${heading}${item.grounding === "analysis" ? " (AI analysis)" : ""}\n\n${item.text}\n\n${references}`.trim();
}
export function transcriptMarkdown(job, noteId) {
  const parts = [];
  let current = `## Transcript\n\n${(job.source?.title || "Source").replace(/[\[\]<>]/g, "")}\n\n`;
  for (const segment of job.sources) {
    const line = `${sourceReference(segment, job.source, noteId)} ${segment.text}\n\n`;
    if (current.length + line.length > 8000 && current.trim()) {
      parts.push(current.trim());
      current = "";
    }
    current += line;
  }
  if (current.trim()) parts.push(current.trim());
  return parts;
}
export async function pluginDialog({
  command,
  context,
  api,
  esc,
  modal,
  insert,
  upload,
}) {
  const providers = await api("ai/providers");
  const kind = command.input || "text";
  const media = kind !== "text";
  const files =
    kind === "audio"
      ? (await api(`notes/${context.noteId}/attachments`)).attachments
      : [];
  const q = (s) => modal.querySelector(s);
  const input =
    kind === "youtube"
      ? `<label>${$t("YouTube video URL")}<input id="plugin-url" type="url" placeholder="https://www.youtube.com/watch?v=…" required></label><label>${$t("Transcript language")}<input id="plugin-language" value="${esc(command.language || "en")}" maxlength="20"></label>${command.audio_fallback ? `<label class="checkbox-label"><input id="plugin-audio" type="checkbox">${$t("If captions are missing, transcribe audio I have permission to process. This may incur provider charges.")}</label>` : `<p>${$t("Uses available captions. Audio fallback is not configured.")}</p>`}`
      : kind === "audio"
        ? `<label>${$t("Audio attachment")}<select id="plugin-attachment"><option value="">${$t("Choose an existing recording")}</option>${files.map((f) => `<option value="${esc(f.id)}">${esc(f.filename)}</option>`).join("")}</select></label><label>${$t("Or upload a recording (WAV, MP3, FLAC, M4A; up to 60 MiB)")}<input id="plugin-file" type="file" accept=".wav,.mp3,.flac,.m4a,audio/*"></label><p>${$t("Audio is sent to the server’s configured transcription provider. Uploaded originals remain in this note’s Files.")}</p>`
        : `<label>${$t("Source")}<select id="plugin-source"><option value="note">${$t("Current note")}</option>${context.blockId ? `<option value="block">${$t("Current block")}</option>` : ""}${context.selection ? `<option value="selection">${$t("Selected text")}</option>` : ""}</select></label>`;
  modal.innerHTML = `<h2>${esc(command.name)}</h2><p>${$t("Review generated content before inserting it. References show where an idea came from; they do not independently verify it.")}</p>${input}<label>${media ? $t("Knowledge extraction") : $t("AI provider")}<select id="plugin-provider">${media ? `<option value="">${$t("Transcript only · no knowledge extraction")}</option>` : ""}${providers.map((p) => `<option value="${esc(p.id)}">${esc(p.name)} · ${esc(p.model)}</option>`).join("")}</select></label><p>${$t("AI processing sends the chosen source to your provider and may incur charges. Longer sources are processed in sections, with up to 24 model calls.")}</p><p id="plugin-status" role="status"></p><div id="plugin-preview"></div><footer><button class="btn" id="plugin-close">${$t("Close")}</button><button class="btn" id="plugin-cancel" hidden>${$t("Cancel processing")}</button><button class="btn" id="plugin-retry" hidden>${$t("Retry unfinished stages")}</button><button class="btn primary" id="plugin-start">${media ? $t("Import") : $t("Generate")}</button><button class="btn primary" id="plugin-insert" hidden>${$t("Insert selected sections")}</button></footer>`;
  modal.showModal();
  if (!media)
    q("#plugin-source").value = context.selection
      ? "selection"
      : context.blockId
        ? "block"
        : "note";
  const storageKey = `minerva-plugin-job:${context.noteId}:${command.plugin_id}:${command.id}`;
  let closed = false,
    timer,
    jobId,
    running = false,
    lastJob;
  const status = (message) => {
    if (!closed) q("#plugin-status").textContent = message;
  };
  modal.addEventListener(
    "close",
    () => {
      closed = true;
      clearTimeout(timer);
    },
    { once: true },
  );
  q("#plugin-close").onclick = () => modal.close();
  function controls(active) {
    running = active;
    for (const selector of [
      "#plugin-start",
      "#plugin-source",
      "#plugin-provider",
      "#plugin-url",
      "#plugin-language",
      "#plugin-attachment",
      "#plugin-file",
      "#plugin-audio",
      "#plugin-retry",
    ]) {
      if (q(selector)) q(selector).disabled = active;
    }
    q("#plugin-cancel").hidden = !active;
  }
  function preview(job, completed) {
    const items = job.result?.items || [];
    const segments = job.sources || [];
    q("#plugin-preview").innerHTML =
      `<details><summary>${$t("Source{value}", { value: job.source?.title ? ` · ${esc(job.source.title)}` : $t(" · note revision {revision}", { revision: esc(job.note_revision) }) })}</summary><pre style="white-space:pre-wrap">${esc(segments.map((s) => `${s.start != null ? timestamp(s.start) : s.id} ${s.text}`).join("\n\n"))}</pre></details>${media && segments.length ? `<label class="checkbox-label"><input id="plugin-include-transcript" type="checkbox" checked>${$t("Include timestamped transcript")}</label>` : ""}` +
      items
        .map(
          (item, i) =>
            `<section><label class="checkbox-label"><input type="checkbox" data-include="${i}" checked>${$t("Include {value}{value2}", { value: esc(item.kind.replaceAll("_", " ")), value2: item.grounding === "analysis" ? $t(" (AI analysis)") : "" })}</label><label>${$t("Editable Markdown")}<textarea data-section="${i}" rows="6">${esc(knowledgeMarkdown(item, context.noteId, segments, job.source))}</textarea></label></section>`,
        )
        .join("");
    q("#plugin-insert").hidden = !items.length && !(media && segments.length);
    q("#plugin-insert").disabled = false;
    q("#plugin-insert").onclick = async () => {
      const button = q("#plugin-insert");
      button.disabled = true;
      try {
        await api(`server-plugins/jobs/${jobId}`);
        const text = items
          .map((_, i) =>
            q(`[data-include="${i}"]`).checked
              ? q(`[data-section="${i}"]`).value.trim()
              : "",
          )
          .filter(Boolean);
        if (q("#plugin-include-transcript")?.checked)
          text.unshift(...transcriptMarkdown(job, context.noteId));
        if (!text.length)
          throw new Error($t("Select at least one nonempty section."));
        await insert(text, context.noteId, context.blockId);
        sessionStorage.removeItem(storageKey);
        modal.close();
      } catch (err) {
        status(err.message);
        button.disabled = false;
      }
    };
    if (completed)
      status(
        items.length
          ? $t(
              "Review, edit and choose what to keep. Long sources produce section-by-section insights.",
            )
          : $t(
              "Transcript ready. You can insert it without sending it to a language model.",
            ),
      );
  }
  async function poll() {
    try {
      const job = await api(`server-plugins/jobs/${jobId}`);
      if (closed) return;
      lastJob = job;
      const labels = {
        preparing: "Preparing source…",
        retrieving_captions: "Retrieving YouTube captions…",
        retrieving_audio: "Retrieving permitted audio…",
        transcribing: "Transcribing audio…",
        transcript_ready: "Transcript ready…",
        extracting_knowledge: $t(
          "Extracting knowledge · {done}/{total} sections complete…",
          { done: job.completed_chunks || 0, total: job.total_chunks || 1 },
        ),
      };
      status(labels[job.stage] || job.status);
      if (["queued", "running"].includes(job.status)) {
        controls(true);
        timer = setTimeout(poll, 1000);
        return;
      }
      controls(false);
      q("#plugin-retry").hidden = ![
        "failed",
        "cancelled",
        "interrupted",
      ].includes(job.status);
      if (job.status !== "completed") {
        if (job.sources?.length) preview(job, false);
        status(
          (job.error && $t(job.error)) ||
            $t("Processing {status}. Retry to reuse completed stages.", {
              status: job.status,
            }),
        );
        return;
      }
      preview(job, true);
    } catch (err) {
      if (!closed) {
        controls(false);
        status(err.message);
      }
    }
  }
  q("#plugin-cancel").onclick = async () => {
    try {
      await api(`server-plugins/jobs/${jobId}`, "POST", { action: "cancel" });
      clearTimeout(timer);
      await poll();
    } catch (err) {
      status(err.message);
    }
  };
  async function start(retry = false) {
    if (running) return;
    controls(true);
    q("#plugin-preview").replaceChildren();
    q("#plugin-insert").hidden = true;
    q("#plugin-retry").hidden = true;
    status($t("Starting…"));
    try {
      let attachment = q("#plugin-attachment")?.value;
      if (!retry && kind === "audio" && q("#plugin-file").files[0])
        attachment = (await upload(q("#plugin-file").files[0], context.noteId))
          .id;
      const scope = q("#plugin-source")?.value;
      const request = retry
        ? { ...lastJob, retry_of: jobId }
        : {
            plugin_id: command.plugin_id,
            command: command.id,
            note_id: context.noteId,
            provider_id: q("#plugin-provider").value,
            block_id: !media && scope !== "note" ? context.blockId : null,
            selection: scope === "selection" ? context.selection : "",
            url: q("#plugin-url")?.value,
            language: q("#plugin-language")?.value,
            attachment_id: attachment,
            allow_audio: !!q("#plugin-audio")?.checked,
          };
      // Never resend large snapshots: retry references the authorized persisted job.
      delete request.sources;
      delete request.result;
      delete request.chunk_results;
      const job = await api("server-plugins/jobs", "POST", request);
      jobId = job.id;
      sessionStorage.setItem(storageKey, jobId);
      if (!closed) await poll();
    } catch (err) {
      if (!closed) {
        controls(false);
        status(err.message);
      }
    }
  }
  q("#plugin-start").onclick = () => start();
  q("#plugin-retry").onclick = () => start(true);
  jobId = sessionStorage.getItem(storageKey);
  if (jobId) await poll();
}
