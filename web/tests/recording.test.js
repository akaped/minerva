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

test("owners who are members become mentions; speakers start paragraphs", async () => {
  const { actionTask, ownerEmail, transcriptBlocks } =
    await import("../recording.js");
  const people = [
    { name: "Ada Lovelace", email: "ada@test.local" },
    { name: "Bo Chen", email: "bo@test.local" },
    { name: "Bo Smith", email: "bosmith@test.local" },
  ];
  assert.equal(ownerEmail("Ada", people), "ada@test.local");
  assert.equal(ownerEmail("bo chen", people), "bo@test.local");
  assert.equal(ownerEmail("Bo", people), null, "two Bos: as said");
  assert.equal(
    actionTask(
      { task: "Send the deck", owner: "Ada", due: "2026-11-09" },
      people,
    ),
    "- [ ] Send the deck — @ada@test.local ⏰2026-11-09 09:00",
  );
  assert.equal(
    actionTask({ task: "Call", owner: "Bo" }, people),
    "- [ ] Call — Bo",
  );
  const blocks = transcriptBlocks([
    { start: 0, text: "Hello.", speaker: "A" },
    { start: 2, text: "How are you?", speaker: "A" },
    { start: 4, text: "Fine.", speaker: "B" },
  ]);
  assert.deepEqual(blocks, [
    "  `0:00` **A:** Hello. How are you?",
    "  `0:04` **B:** Fine.",
  ]);
});
