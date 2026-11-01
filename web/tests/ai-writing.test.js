import { test } from "node:test";
import assert from "node:assert/strict";
import {
  locateSelection,
  continuation,
  WRITING_ACTIONS,
} from "../ai-writing.js";

test("a selection is found where it was, or again after edits around it", () => {
  const text = "One two three";
  assert.deepEqual(locateSelection(text, 4, 7, "two"), { from: 4, to: 7 });
  assert.deepEqual(locateSelection("Zero. " + text, 4, 7, "two"), {
    from: 10,
    to: 13,
  });
  assert.equal(locateSelection("One 2 three", 4, 7, "two"), null);
});

test("a continuation reads on after the selection", () => {
  assert.equal(continuation("It was late", " and dark. "), " and dark.");
  assert.equal(continuation("It was late. ", "Then rain."), "Then rain.");
  assert.equal(continuation("", "Start."), "Start.");
});

test("every action is one the server knows", () => {
  assert.deepEqual(
    WRITING_ACTIONS.map((a) => a[0]),
    ["improve", "fix", "shorter", "longer", "translate", "continue"],
  );
});
