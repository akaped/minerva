import { test } from "node:test";
import assert from "node:assert/strict";
import {
  timestamp,
  actionTask,
  transcriptBlocks,
  recordingBlocks,
} from "../recording.js";

test("times and tasks", () => {
  assert.equal(timestamp(5.9), "0:05");
  assert.equal(timestamp(754), "12:34");
  assert.equal(timestamp(3725), "1:02:05");
  assert.equal(
    actionTask({ task: "Send  the notes", owner: "Ada", due: "2026-11-06" }),
    "- [ ] Send the notes — Ada ⏰2026-11-06 09:00",
  );
  assert.equal(
    actionTask({ task: "Call", owner: "", due: "2026-11-06 15:30" }),
    "- [ ] Call ⏰2026-11-06 15:30",
  );
  assert.equal(
    actionTask({ task: "Book", owner: "", due: "soon" }),
    "- [ ] Book",
  );
});

test("transcripts become nested paragraphs", () => {
  const timed = [
    { start: 0, text: "a".repeat(400) },
    { start: 65, text: "b".repeat(400) },
    { start: 130, text: "c" },
  ];
  assert.deepEqual(transcriptBlocks(timed), [
    `  \`0:00\` ${"a".repeat(400)}`,
    `  \`1:05\` ${"b".repeat(400)} c`,
  ]);
  assert.deepEqual(transcriptBlocks([{ text: "One." }, { text: "Two." }]), [
    "  One. Two.",
  ]);
});

test("only what was kept is inserted", () => {
  const job = {
    transcript: [{ text: "Hello." }],
    summary: {
      title: "Sync",
      summary: ["Point"],
      decisions: [],
      action_items: [
        { task: "A", owner: "", due: "" },
        { task: "B", owner: "", due: "" },
      ],
    },
  };
  assert.deepEqual(
    recordingBlocks(job, {
      summary: true,
      decisions: true,
      actions: [1],
      transcript: true,
    }),
    [
      "## Sync",
      "### Key points",
      "- Point",
      "### Action items",
      "- [ ] B",
      "### Transcript",
      "  Hello.",
    ],
  );
  assert.deepEqual(
    recordingBlocks({ transcript: job.transcript }, { transcript: true }),
    ["### Transcript", "  Hello."],
  );
});
