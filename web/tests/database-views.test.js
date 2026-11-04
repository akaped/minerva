import { test } from "node:test";
import assert from "node:assert/strict";
import {
  computedText,
  boardGroups,
  moveToGroup,
  calendarExtras,
  reminderText,
  formatNumber,
  boardKey,
  rankBetween,
} from "../database-views.js";

test("computed values as text", () => {
  assert.equal(computedText(null), "");
  assert.equal(computedText(3), "3");
  assert.equal(computedText(1 / 3), "0.3333");
  assert.equal(computedText(true), "☑");
  assert.equal(computedText(["a", 2.5]), "a, 2.5");
  assert.equal(computedText("2026-11-03"), "2026-11-03");
});

const rows = [
  { id: "1", cells: { s: "Done", m: ["a", "b"], p: "u1" } },
  { id: "2", cells: { s: "To do", m: ["b"], p: "gone" } },
  { id: "3", cells: {} },
];

test("board columns by select, multi-select and person", () => {
  const select = boardGroups(
    { id: "s", kind: "select", options: ["To do", "Done"] },
    rows,
  );
  assert.deepEqual(
    select.map((g) => [g.label, g.rows.map((r) => r.id)]),
    [
      ["To do", ["2"]],
      ["Done", ["1"]],
      ["", ["3"]],
    ],
  );
  const multi = boardGroups(
    { id: "m", kind: "multi_select", options: ["a", "b", "c"] },
    rows,
  );
  assert.deepEqual(
    multi.map((g) => g.rows.map((r) => r.id)),
    [["1"], ["1", "2"], [], ["3"]],
  );
  const people = boardGroups({ id: "p", kind: "person", options: [] }, rows, [
    { id: "u1", name: "Ada" },
    { id: "u2", name: "Bo" },
  ]);
  assert.deepEqual(
    people.map((g) => [g.label, g.rows.map((r) => r.id)]),
    [
      ["Ada", ["1"]],
      ["Someone", ["2"]],
      ["", ["3"]],
    ],
  );
});

test("moving a card between columns", () => {
  const multi = { id: "m", kind: "multi_select", options: ["a", "b", "c"] };
  assert.deepEqual(moveToGroup(multi, rows[0], "a", "c"), { m: ["b", "c"] });
  assert.deepEqual(moveToGroup(multi, rows[1], "b", ""), { m: null });
  assert.deepEqual(moveToGroup(multi, rows[2], "", "a"), { m: ["a"] });
  assert.deepEqual(
    moveToGroup({ id: "s", kind: "select" }, rows[0], "Done", "To do"),
    { s: "To do" },
  );
  assert.deepEqual(
    moveToGroup({ id: "p", kind: "person" }, rows[0], "u1", ""),
    {
      p: null,
    },
  );
});

test("reminders and journal entries on the calendar", () => {
  const extras = calendarExtras("2026-11", {
    reminders: [
      {
        stamp: "2026-11-03 09:30",
        text: "- [ ] Call **Ada** ⏰2026-11-03 09:30",
        note_id: "n1",
      },
      { stamp: "2026-12-01 10:00", text: "Later" },
    ],
    journal: [{ id: "j1", title: "Tuesday", journal: "2026-11-03" }],
  });
  assert.deepEqual([...extras.keys()], ["2026-11-03"]);
  assert.deepEqual(extras.get("2026-11-03"), [
    { kind: "reminder", label: "09:30 Call Ada", note_id: "n1" },
    { kind: "journal", label: "Tuesday", note_id: "j1" },
  ]);
});

test("reminder text without stamps and marks", () => {
  assert.equal(reminderText("Book club ⏰2026-11-12 18:30"), "Book club");
  assert.equal(reminderText("1. *Pay* `rent` ⏰2026-11-12"), "Pay rent");
});

test("numbers show in their property's format", () => {
  assert.equal(formatNumber(0.25, "percent", "en-US"), "25%");
  assert.equal(formatNumber(0.125, "percent", "en-US"), "12.5%");
  assert.equal(formatNumber(12.5, "currency:EUR", "en-US"), "€12.50");
  assert.equal(formatNumber(1234.5, "currency:USD", "en-US"), "$1,234.50");
  assert.match(
    formatNumber(1234.5, "currency:EUR", "it-IT"),
    /^1\.?234,50\s€$/,
  );
  assert.equal(formatNumber(3, "", "en-US"), "3");
  assert.equal(formatNumber("x", "percent"), "");
  // Computed values follow it too, lists included.
  assert.equal(computedText([0.5, 0.25], "percent"), "50%, 25%");
  assert.equal(computedText(2 / 3), "0.6667");
});

test("cards keep the place they were moved to in a column", () => {
  const col = { id: "s", kind: "select", options: ["A"] };
  const rows = [
    { id: "1", cells: { s: "A" } },
    { id: "2", cells: { s: "A" } },
    { id: "3", cells: { s: "A" }, rank: -0.5 },
  ];
  const [a] = boardGroups(col, rows);
  assert.deepEqual(
    a.rows.map((r) => r.id),
    ["3", "1", "2"],
  );
  const key = boardKey(rows);
  assert.equal(rankBetween(key(rows[0]), key(rows[1])), 0.5);
  assert.equal(rankBetween(null, key(rows[2])), -1.5);
  assert.equal(rankBetween(key(rows[1]), null), 2);
  assert.equal(rankBetween(null, null), 0);
});
