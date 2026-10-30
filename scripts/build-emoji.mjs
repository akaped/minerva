// Builds web/dist/emoji.json, the data behind the emoji pickers, from
// emojibase-data (MIT): every emoji with its English name and search tags, in
// Unicode order and grouped. Skin-tone components and lone regional
// indicator letters are left out.
import fs from "node:fs";
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
const emoji = require("emojibase-data/en/compact.json");
const { groups } = require("emojibase-data/meta/groups.json");

const names = {
  "smileys-emotion": "Smileys",
  "people-body": "People",
  "animals-nature": "Nature",
  "food-drink": "Food",
  "travel-places": "Travel",
  activities: "Activities",
  objects: "Objects",
  symbols: "Symbols",
  flags: "Flags",
};
const kept = Object.entries(groups).filter(([, key]) => names[key]);
const index = new Map(kept.map(([id], i) => [Number(id), i]));

const list = emoji
  .filter((e) => index.has(e.group))
  .sort((a, b) => a.order - b.order)
  .map((e) => [
    e.unicode,
    e.label,
    [e.label, ...(e.tags || [])].join(" ").toLowerCase(),
    index.get(e.group),
  ]);

fs.mkdirSync("web/dist", { recursive: true });
fs.writeFileSync(
  "web/dist/emoji.json",
  JSON.stringify({
    groups: kept.map(([, key]) => names[key]),
    emoji: list,
  }),
);
console.log(`web/dist/emoji.json: ${list.length} emoji`);
