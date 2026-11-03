import { $t } from "./i18n.js";
/**
 * From a recording to notes: what is inserted into the note from a finished
 * transcription job (`POST /api/v1/ai/recordings`), as block Markdown.
 */

/** Seconds as `m:ss` or `h:mm:ss`. */
export function timestamp(seconds) {
  const s = Math.max(0, Math.floor(seconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}

const clean = (text) => String(text).replace(/\s+/g, " ").trim();

/** An action item as a task, with its owner and a `⏰` time when one was said. */
export function actionTask({ task, owner, due }) {
  const when = /^\d{4}-\d{2}-\d{2}$/.test(due || "")
    ? `${due} 09:00`
    : /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}$/.test(due || "")
      ? due
      : "";
  return `- [ ] ${clean(task)}${owner ? ` — ${clean(owner)}` : ""}${when ? ` ⏰${when}` : ""}`;
}

/**
 * The transcript in paragraphs of about `size` characters, each starting with
 * its time when the provider gave times; nested under a heading, so it folds.
 */
export function transcriptBlocks(segments, size = 700) {
  const out = [];
  let current = "";
  let start = null;
  const push = () => {
    if (!current) return;
    const time = start == null ? "" : `\`${timestamp(start)}\` `;
    out.push(`  ${time}${current}`);
    current = "";
    start = null;
  };
  for (const s of segments) {
    if (current && current.length + s.text.length > size) push();
    if (!current) start = s.start ?? null;
    current = current ? `${current} ${clean(s.text)}` : clean(s.text);
  }
  push();
  return out;
}

/**
 * The blocks to insert. `keep` says what to include: `summary`, `decisions`,
 * `transcript` (booleans) and `actions` (the indices of the action items).
 */
export function recordingBlocks(job, keep) {
  const notes = job.summary || {};
  const blocks = [];
  const section = (title, items) => {
    if (!items.length) return;
    blocks.push(`### ${title}`, ...items);
  };
  if (notes.title && (keep.summary || keep.decisions || keep.actions?.length))
    blocks.push(`## ${clean(notes.title)}`);
  if (keep.summary)
    section(
      $t("Key points"),
      (notes.summary || []).map((p) => `- ${clean(p)}`),
    );
  if (keep.decisions)
    section(
      $t("Decisions"),
      (notes.decisions || []).map((p) => `- ${clean(p)}`),
    );
  section(
    $t("Action items"),
    (notes.action_items || [])
      .filter((_, i) => keep.actions?.includes(i))
      .map(actionTask),
  );
  if (keep.transcript && job.transcript?.length)
    section($t("Transcript"), transcriptBlocks(job.transcript));
  return blocks;
}
