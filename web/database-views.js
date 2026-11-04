/**
 * Pieces of the database views that do not touch the page: how computed values
 * read, which records go in which board column, and what a calendar day holds.
 */

/** How a rollup can combine the related records' values, for the property dialog. */
export const ROLLUP_FUNCTIONS = [
  ["count", "Count records"],
  ["count_values", "Count values"],
  ["sum", "Sum"],
  ["average", "Average"],
  ["min", "Smallest"],
  ["max", "Largest"],
  ["earliest", "Earliest date"],
  ["latest", "Latest date"],
  ["list", "List the values"],
];

/**
 * A rollup or formula value as text: numbers to at most 4 decimals, or as the
 * property's `format` shows them (see `formatNumber`).
 */
export function computedText(value, format = "") {
  if (value == null) return "";
  if (typeof value === "boolean") return value ? "☑" : "☐";
  if (typeof value === "number")
    return format
      ? formatNumber(value, format)
      : Number.isInteger(value)
        ? String(value)
        : String(Math.round(value * 10000) / 10000);
  if (Array.isArray(value))
    return value.map((v) => computedText(v, format)).join(", ");
  return String(value);
}

/** Number formats a property can have: `percent`, or `currency:{code}`. */
export const CURRENCIES = ["EUR", "USD", "GBP", "CHF", "JPY", "CAD", "AUD"];

/**
 * A number as its property's `format` shows it, in the reader's language:
 * `percent` shows 0.25 as 25%, `currency:EUR` as €12.50.
 */
export function formatNumber(value, format = "", locale = undefined) {
  if (typeof value !== "number" || !Number.isFinite(value)) return "";
  try {
    if (format === "percent")
      return new Intl.NumberFormat(locale, {
        style: "percent",
        maximumFractionDigits: 2,
      }).format(value);
    const code = format.startsWith("currency:") ? format.slice(9) : "";
    if (code)
      return new Intl.NumberFormat(locale, {
        style: "currency",
        currency: code,
      }).format(value);
  } catch {
    // An unknown currency code: shown plain.
  }
  return new Intl.NumberFormat(locale, { maximumFractionDigits: 10 }).format(
    value,
  );
}

/** Properties a board can be grouped by. */
export const GROUPABLE = ["select", "multi_select", "person"];

/**
 * Board columns: `[{key, label, rows}]`, one per option (or person), then the
 * records without a value. A multi-select record shows in each of its options.
 * `people` is `[{id, name}]` for a person property.
 */
export function boardGroups(col, rows, people = [], all = rows) {
  const value = (r) => r.cells[col.id];
  // Within a column: by the place each card was moved to (see `boardKey`).
  const key = boardKey(all);
  const sorted = (list) => [...list].sort((a, b) => key(a) - key(b));
  let keys;
  if (col.kind === "person") {
    const used = new Set(rows.map(value).filter(Boolean));
    keys = [
      ...people.filter((p) => used.has(p.id)).map((p) => [p.id, p.name]),
      ...[...used]
        .filter((id) => !people.some((p) => p.id === id))
        .map((id) => [id, "Someone"]),
    ];
  } else keys = col.options.map((o) => [o, o]);
  const has = (r, key) =>
    col.kind === "multi_select"
      ? (value(r) || []).includes(key)
      : value(r) === key;
  const empty = (r) =>
    col.kind === "multi_select" ? !(value(r) || []).length : !value(r);
  return [
    ...keys.map(([key, label]) => ({
      key,
      label,
      rows: sorted(rows.filter((r) => has(r, key))),
    })),
    { key: "", label: "", rows: sorted(rows.filter(empty)) },
  ];
}

/**
 * Where a record sits in a board column: the `rank` it was moved to, else its
 * place among `all` the database's records (so unmoved cards keep the order
 * they were added in).
 */
export function boardKey(all) {
  const index = new Map(all.map((r, i) => [r.id, i]));
  return (r) => r.rank ?? index.get(r.id) ?? all.length;
}

/** A rank between two neighbours' keys (either may be missing, at an end). */
export function rankBetween(before, after) {
  if (before == null && after == null) return 0;
  if (before == null) return after - 1;
  if (after == null) return before + 1;
  return (before + after) / 2;
}

/** The cells to send when a card moves from one board column to another. */
export function moveToGroup(col, row, from, to) {
  if (col.kind !== "multi_select") return { [col.id]: to || null };
  const now = (row.cells[col.id] || []).filter((o) => o !== from);
  if (to && !now.includes(to)) now.push(to);
  const ordered = col.options.filter((o) => now.includes(o));
  return { [col.id]: ordered.length ? ordered : null };
}

/** A reminder's text without its time stamp, list marks or Markdown emphasis. */
export function reminderText(text = "") {
  return String(text)
    .replace(/⏰\s*\d{4}-\d{2}-\d{2}(\s+\d{2}:\d{2})?/g, "")
    .replace(/^\s*(?:[-*+]|\d+\.)\s+(?:\[[ xX]\]\s+)?/, "")
    .replace(/[*_`~]+/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

/**
 * Reminders and journal entries for a calendar month, by day:
 * `Map(day → [{kind, label, note_id?}])`. `month` is `YYYY-MM`; reminder
 * stamps are `YYYY-MM-DD HH:MM` (local time).
 */
export function calendarExtras(month, { reminders = [], journal = [] }) {
  const out = new Map();
  const add = (day, item) => {
    if (!day?.startsWith(month + "-")) return;
    out.set(day, [...(out.get(day) || []), item]);
  };
  for (const r of reminders)
    add(String(r.stamp || "").slice(0, 10), {
      kind: "reminder",
      label: `${String(r.stamp).slice(11, 16)} ${reminderText(r.text)}`.trim(),
      note_id: r.note_id,
    });
  for (const n of journal)
    add(n.journal, { kind: "journal", label: n.title, note_id: n.id });
  return out;
}
