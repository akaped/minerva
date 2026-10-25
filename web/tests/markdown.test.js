import { test } from "node:test";
import assert from "node:assert/strict";
import { splitMarkdown, blockCommands } from "../markdown.js";

test("fenced code stays one block through paste/import, including blank lines", () => {
  assert.deepEqual(
    splitMarkdown("Title\r\n```rust\r\nlet x = 1;\r\n\r\n```\r\nAfter"),
    ["Title", "```rust\nlet x = 1;\n\n```", "After"],
  );
  assert.deepEqual(splitMarkdown("~~~~js\n```\n~~~~"), ["~~~~js\n```\n~~~~"]);
  assert.deepEqual(splitMarkdown("```\nunfinished"), ["```\nunfinished"]);
});
test("table and list rows preserve multiline Markdown semantics", () => {
  assert.deepEqual(splitMarkdown("| A |\n| --- |\n| B |\n\n- one\n- two"), [
    "| A |\n| --- |\n| B |",
    "",
    "- one\n- two",
  ]);
  assert.equal(
    blockCommands.find((c) => c[0] === "Code")[1].split("\n").length,
    3,
  );
});
