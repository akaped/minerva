import test from "node:test";
import assert from "node:assert/strict";
import {
  repeatLabel,
  stampTime,
  relative,
  groupReminders,
  blockText,
} from "../reminders.js";

test("repeat labels read naturally", () => {
  assert.equal(repeatLabel(null), "Once");
  assert.equal(repeatLabel({ every: 1, unit: "year" }), "Every year");
  assert.equal(repeatLabel({ every: 2, unit: "year" }), "Every 2 years");
});

test("times in notes are local times", () => {
  assert.equal(
    stampTime("2026-11-02 09:30"),
    new Date(2026, 10, 2, 9, 30).getTime(),
  );
});

test("relative days", () => {
  const now = new Date(2026, 9, 30, 12).getTime();
  assert.equal(relative(new Date(2026, 9, 30, 18).getTime(), now), "Today");
  assert.equal(relative(new Date(2026, 9, 31, 8).getTime(), now), "Tomorrow");
  assert.equal(relative(new Date(2026, 10, 3).getTime(), now), "In 4 days");
  assert.equal(relative(new Date(2026, 9, 27).getTime(), now), "3 days ago");
  assert.equal(relative(new Date(2028, 9, 30).getTime(), now), "In 2 years");
});

test("own reminders stay due; past note times are history", () => {
  const now = Date.UTC(2026, 9, 30);
  const day = 86400e3;
  const g = groupReminders(
    [
      { kind: "own", at: now - day, title: "late" },
      { kind: "note", at: now - day, title: "passed" },
      { kind: "own", at: now + 40 * day, title: "later" },
      { kind: "own", at: now + 2 * day, title: "soon" },
      { kind: "note", at: now + 10 * day, title: "month" },
    ],
    now,
  );
  assert.deepEqual(
    Object.fromEntries(
      Object.entries(g).map(([k, v]) => [k, v.map((r) => r.title)]),
    ),
    {
      due: ["late"],
      week: ["soon"],
      month: ["month"],
      later: ["later"],
      past: ["passed"],
    },
  );
});

test("block text without the time and Markdown marks", () => {
  assert.equal(
    blockText("- [ ] Renew the **passport** ⏰2026-11-02 09:00"),
    "Renew the passport",
  );
  assert.equal(blockText("⏰2026-11-02 09:00"), "Reminder");
});
