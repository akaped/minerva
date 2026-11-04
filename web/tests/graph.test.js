import { test } from "node:test";
import assert from "node:assert/strict";
import {
  buildGraph,
  neighborhood,
  createSimulation,
  settle,
  folderColor,
  tagColors,
} from "../graph.js";

const raw = {
  nodes: [
    { id: "a", title: "A", folder_id: "f", tags: ["x"] },
    { id: "b", title: "B", folder_id: "f", tags: ["x", "y"] },
    { id: "c", title: "C", folder_id: "g", tags: [] },
    { id: "d", title: "Day", folder_id: "j", tags: [], journal: "2026-11-03" },
    { id: "e", title: "Alone", folder_id: "g", tags: [] },
  ],
  links: [
    ["a", "b"],
    ["b", "c"],
    ["d", "a"],
    ["a", "gone"],
  ],
};

test("notes, links, tags and filters", () => {
  const g = buildGraph(raw);
  assert.equal(g.nodes.length, 5);
  assert.deepEqual(g.edges, [
    [0, 1],
    [1, 2],
    [3, 0],
  ]);
  assert.deepEqual(
    g.nodes.map((n) => n.degree),
    [2, 2, 1, 1, 0],
  );
  const tagged = buildGraph(raw, { tags: true });
  assert.deepEqual(
    tagged.nodes.filter((n) => n.kind === "tag").map((n) => n.label),
    ["#x", "#y"],
  );
  assert.equal(tagged.edges.length, 3 + 3);
  const tidy = buildGraph(raw, { orphans: false, journal: false });
  assert.deepEqual(
    tidy.nodes.map((n) => n.id),
    ["a", "b", "c"],
  );
  assert.deepEqual(tidy.edges, [
    [0, 1],
    [1, 2],
  ]);
});

test("a note's neighbourhood", () => {
  const g = buildGraph(raw);
  const one = neighborhood(g, "a", 1);
  assert.deepEqual(
    one.nodes.map((n) => [n.id, n.distance]),
    [
      ["a", 0],
      ["b", 1],
      ["d", 1],
    ],
  );
  assert.equal(one.edges.length, 2);
  const two = neighborhood(g, "a", 2);
  assert.ok(two.nodes.some((n) => n.id === "c" && n.distance === 2));
  assert.deepEqual(neighborhood(g, "nope"), { nodes: [], edges: [] });
});

test("the layout pulls linked notes together and settles", () => {
  // Two groups of five, each a ring, with no link between the groups.
  const nodes = Array.from({ length: 10 }, (_, i) => ({
    id: String(i),
    title: String(i),
    tags: [],
  }));
  const links = [];
  for (const base of [0, 5])
    for (let i = 0; i < 5; i++)
      links.push([String(base + i), String(base + ((i + 1) % 5))]);
  const sim = settle(createSimulation(buildGraph({ nodes, links })));
  assert.ok(sim.alpha < 0.05, "it settles");
  const p = sim.points;
  const dist = (a, b) => Math.hypot(p[a].x - p[b].x, p[a].y - p[b].y);
  const within = [dist(0, 1), dist(5, 6)];
  const across = dist(0, 5);
  for (const d of within) assert.ok(d < across, `${d} < ${across}`);
  for (const q of p) assert.ok(Number.isFinite(q.x) && Number.isFinite(q.y));
  // The same graph lays out the same way.
  const again = settle(createSimulation(buildGraph({ nodes, links })));
  assert.ok(Math.abs(again.points[3].x - p[3].x) < 1e-6);
});

test("folder colours repeat", () => {
  assert.equal(folderColor(["f", "g"], "g"), folderColor(["f", "g"], "g"));
  assert.notEqual(folderColor(["f", "g"], "f"), folderColor(["f", "g"], "g"));
  assert.equal(folderColor(["f"], "x"), null);
});

test("filters by folder and by the date of the last change", () => {
  const dated = {
    nodes: raw.nodes.map((n, i) => ({ ...n, updated_at: 1000 * (i + 1) })),
    links: raw.links,
  };
  const inF = buildGraph(dated, { folders: new Set(["f"]) });
  assert.deepEqual(
    inF.nodes.map((n) => n.id),
    ["a", "b"],
  );
  assert.deepEqual(inF.edges, [[0, 1]]);
  const recent = buildGraph(dated, { since: 3500 });
  assert.deepEqual(
    recent.nodes.map((n) => n.id),
    ["d", "e"],
  );
});

test("colours by the most used tag", () => {
  const g = buildGraph(raw);
  const t = tagColors(g.nodes);
  assert.deepEqual(t.tags, ["x", "y"]);
  const [a, b, c] = g.nodes;
  assert.equal(t.color(a), t.colorOf("x"));
  assert.equal(t.color(b), t.colorOf("x"), "x is used more than y");
  assert.equal(t.color(c), null);
});

test("arranged nodes stay where they were put", () => {
  const g = buildGraph(raw);
  const sim = settle(createSimulation(g, { positions: { b: [500, -40] } }));
  const b = sim.points[1];
  assert.deepEqual([b.x, b.y, b.fixed], [500, -40, true]);
  assert.equal(sim.points[0].fixed, false);
});
