import { test } from "node:test";
import assert from "node:assert/strict";
import { knowledgeMarkdown } from "../plugin-commands.js";
test("knowledge retains source identity and labels generated analysis", () => {
  const result = knowledgeMarkdown(
    {
      kind: "mental_model",
      text: "Feedback loops",
      source_ids: ["12345678-abcd"],
      grounding: "analysis",
    },
    "note-id",
  );
  assert.match(result, /AI analysis/);
  assert.match(result, /12345678/);
  assert.match(result, /#note\/note-id/);
  assert.match(result, /Feedback loops/);
});
import {
  sourceReference,
  transcriptMarkdown,
  timestamp,
} from "../plugin-commands.js";
test("video references retain original seconds and a canonical source", () => {
  assert.equal(timestamp(125.9), "2:05");
  assert.match(
    sourceReference(
      { start: 125.9 },
      { type: "youtube", id: "abcdefghijk" },
      "note",
    ),
    /watch\?v=abcdefghijk&t=125/,
  );
  const text = transcriptMarkdown(
    {
      source: { type: "youtube", id: "abcdefghijk", title: "Lecture" },
      sources: [{ id: "s1", text: "Keep the evidence.", start: 12, end: 15 }],
    },
    "note",
  );
  assert.match(text.join("\n"), /&t=12/);
  assert.match(text.join("\n"), /Keep the evidence/);
});
