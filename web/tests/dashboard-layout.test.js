import test from "node:test";
import assert from "node:assert/strict";
import { moveWidget, packed, rows } from "../dashboard.js";

const groups = (widgets) => rows(widgets).map((row) => row.map(({ id }) => id));
const widget = (id, newRow = false) => ({ id, newRow, options: { title: id } });

test("legacy sizes recover the original rows while explicit breaks take precedence", () => {
  assert.deepEqual(groups(packed([
    { id: "welcome", size: "full" },
    { id: "recent", size: "l" },
    { id: "favorites", size: "s" },
    { id: "journal", size: "s" },
  ])), [["welcome"], ["recent", "favorites"], ["journal"]]);
  assert.deepEqual(groups(packed([
    { ...widget("a", true), size: "full" },
    { ...widget("b"), size: "full" },
  ])), [["a", "b"]]);
});

test("a beside drop preserves order and lets layout wrap more than four widgets", () => {
  const original = [widget("a", true), widget("b"), widget("c"), widget("d"), widget("e", true)];
  const moved = moveWidget(original, "e", "b", "before");
  assert.deepEqual(groups(moved), [["a", "e", "b", "c", "d"]]);
  assert.deepEqual(moved[1].options, { title: "e" });
  assert.deepEqual(groups(original), [["a", "b", "c", "d"], ["e"]]);
});

test("a drop onto a wrapped line inserts a row at the visible location", () => {
  const original = [widget("a", true), widget("b"), widget("c"), widget("d"), widget("e", true), widget("f")];
  assert.deepEqual(
    groups(moveWidget(original, "e", "c", "above", ["c", "d"])),
    [["a", "b"], ["e"], ["c", "d"], ["f"]],
  );
  assert.deepEqual(
    groups(moveWidget(original, "e", "b", "below", ["a", "b"])),
    [["a", "b"], ["e"], ["c", "d"], ["f"]],
  );
});
