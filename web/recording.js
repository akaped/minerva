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

/**
 * The member an owner said aloud is: their full name, or a first name only one
 * member has. `people` are `{name, email}`; returns an email or null.
 */
export function ownerEmail(owner, people = []) {
  const said = clean(owner || "").toLowerCase();
  if (!said) return null;
  const full = people.filter((p) => p.name?.toLowerCase() === said);
  if (full.length === 1) return full[0].email;
  const first = people.filter(
    (p) => p.name?.toLowerCase().split(/\s+/)[0] === said,
  );
  return first.length === 1 ? first[0].email : null;
}

/**
 * An action item as a task, with its owner (as an `@mention` when they are a
 * member) and a `⏰` time when one was said.
 */
export function actionTask({ task, owner, due }, people = []) {
  const when = /^\d{4}-\d{2}-\d{2}$/.test(due || "")
    ? `${due} 09:00`
    : /^\d{4}-\d{2}-\d{2} \d{2}:\d{2}$/.test(due || "")
      ? due
      : "";
  const email = ownerEmail(owner, people);
  const who = email ? `@${email}` : owner ? clean(owner) : "";
  return `- [ ] ${clean(task)}${who ? ` — ${who}` : ""}${when ? ` ⏰${when}` : ""}`;
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
  // A new speaker starts a new paragraph, named.
  let speaker = null;
  for (const s of segments) {
    const changed = s.speaker && s.speaker !== speaker;
    if (current && (changed || current.length + s.text.length > size)) push();
    if (!current) start = s.start ?? null;
    const text =
      changed || (!current && s.speaker)
        ? `**${clean(s.speaker)}:** ${clean(s.text)}`
        : clean(s.text);
    if (s.speaker) speaker = s.speaker;
    current = current ? `${current} ${text}` : text;
  }
  push();
  return out;
}

/**
 * The blocks to insert. `keep` says what to include: `summary`, `decisions`,
 * `transcript` (booleans) and `actions` (the indices of the action items).
 */
export function recordingBlocks(job, keep, people = []) {
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
      .map((item) => actionTask(item, people)),
  );
  if (keep.transcript && job.transcript?.length)
    section($t("Transcript"), transcriptBlocks(job.transcript));
  return blocks;
}
