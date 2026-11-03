/**
 * The graph view: a workspace's notes as dots and their links as lines, laid
 * out by a small force simulation (linked notes pull together, all notes push
 * apart, the whole drifts to the middle) and drawn on a canvas that can be
 * panned, zoomed and dragged. The parts that do not touch the page (building
 * the graph, a note's neighbourhood, the simulation) are exported for tests.
 */

/**
 * Nodes and edges from `GET workspaces/{id}/graph`.
 * Options: `tags` adds a node per tag, linked to its notes; `orphans: false`
 * leaves out notes with no links; `journal: false` leaves out journal entries.
 */
export function buildGraph(
  raw,
  { tags = false, orphans = true, journal = true } = {},
) {
  const notes = raw.nodes.filter((n) => journal || !n.journal);
  const index = new Map(notes.map((n, i) => [n.id, i]));
  const nodes = notes.map((n) => ({
    id: n.id,
    label: n.title,
    kind: "note",
    folder: n.folder_id,
    degree: 0,
  }));
  const edges = [];
  for (const [from, to] of raw.links) {
    const a = index.get(from),
      b = index.get(to);
    if (a !== undefined && b !== undefined) edges.push([a, b]);
  }
  if (tags) {
    const tagIndex = new Map();
    notes.forEach((n, i) => {
      for (const t of n.tags || []) {
        if (!tagIndex.has(t)) {
          tagIndex.set(t, nodes.length);
          nodes.push({ id: "#" + t, label: "#" + t, kind: "tag", degree: 0 });
        }
        edges.push([i, tagIndex.get(t)]);
      }
    });
  }
  for (const [a, b] of edges) {
    nodes[a].degree++;
    nodes[b].degree++;
  }
  if (orphans) return { nodes, edges };
  // Keep connected nodes only, renumbering the edges.
  const keep = nodes.map((n) => n.degree > 0);
  const renumber = [];
  let next = 0;
  keep.forEach((k, i) => (renumber[i] = k ? next++ : -1));
  return {
    nodes: nodes.filter((_, i) => keep[i]),
    edges: edges.map(([a, b]) => [renumber[a], renumber[b]]),
  };
}

/** The part of `graph` within `depth` links of node `id` (in either direction). */
export function neighborhood(graph, id, depth = 1) {
  const start = graph.nodes.findIndex((n) => n.id === id);
  if (start < 0) return { nodes: [], edges: [] };
  const near = new Map([[start, 0]]);
  let frontier = [start];
  for (let d = 1; d <= depth; d++) {
    const nextFrontier = [];
    for (const [a, b] of graph.edges)
      for (const [from, to] of [
        [a, b],
        [b, a],
      ])
        if (frontier.includes(from) && !near.has(to)) {
          near.set(to, d);
          nextFrontier.push(to);
        }
    frontier = nextFrontier;
  }
  const kept = [...near.keys()].sort((x, y) => x - y);
  const renumber = new Map(kept.map((old, i) => [old, i]));
  return {
    nodes: kept.map((i) => ({ ...graph.nodes[i], distance: near.get(i) })),
    edges: graph.edges
      .filter(([a, b]) => renumber.has(a) && renumber.has(b))
      .map(([a, b]) => [renumber.get(a), renumber.get(b)]),
  };
}

// ---------------------------------------------------------------------------
// The simulation

const LINK = 60; // resting length of a link
const SPRING = 0.06;
const CHARGE = 420; // how strongly nodes push apart
const GRAVITY = 0.012;
const DECAY = 0.55; // velocity kept between steps
const THETA = 0.9; // Barnes-Hut: treat far groups as one body

/** A deterministic random generator, so layouts and tests repeat. */
function random(seed) {
  let s = seed >>> 0 || 1;
  return () => (s = (s * 1664525 + 1013904223) >>> 0) / 4294967296;
}

/** Positions for `graph`, started on a spiral so the first steps are calm. */
export function createSimulation(graph, { seed = 7 } = {}) {
  const r = random(seed);
  const points = graph.nodes.map((_, i) => {
    const angle = i * 2.399963; // golden angle
    const radius = 12 * Math.sqrt(i + 1);
    return {
      x: Math.cos(angle) * radius + (r() - 0.5),
      y: Math.sin(angle) * radius + (r() - 0.5),
      vx: 0,
      vy: 0,
      pinned: false,
    };
  });
  return { graph, points, alpha: 1 };
}

/** A quadtree of points, with each cell's centre of mass and count. */
function quadtree(points) {
  let minX = Infinity,
    minY = Infinity,
    maxX = -Infinity,
    maxY = -Infinity;
  for (const p of points) {
    minX = Math.min(minX, p.x);
    minY = Math.min(minY, p.y);
    maxX = Math.max(maxX, p.x);
    maxY = Math.max(maxY, p.y);
  }
  const size = Math.max(maxX - minX, maxY - minY, 1) + 1;
  const root = {
    x: minX,
    y: minY,
    size,
    count: 0,
    cx: 0,
    cy: 0,
    point: null,
    kids: null,
  };
  const insert = (cell, p, depth) => {
    cell.cx = (cell.cx * cell.count + p.x) / (cell.count + 1);
    cell.cy = (cell.cy * cell.count + p.y) / (cell.count + 1);
    cell.count++;
    if (cell.count === 1) {
      cell.point = p;
      return;
    }
    // Points on the same spot stop splitting after a while.
    if (depth > 24) return;
    if (!cell.kids) {
      const half = cell.size / 2;
      cell.kids = [0, 1, 2, 3].map((k) => ({
        x: cell.x + (k & 1 ? half : 0),
        y: cell.y + (k & 2 ? half : 0),
        size: half,
        count: 0,
        cx: 0,
        cy: 0,
        point: null,
        kids: null,
      }));
      const old = cell.point;
      cell.point = null;
      if (old) place(cell, old, depth);
    }
    place(cell, p, depth);
  };
  const place = (cell, p, depth) => {
    const half = cell.size / 2;
    const k = (p.x >= cell.x + half ? 1 : 0) + (p.y >= cell.y + half ? 2 : 0);
    insert(cell.kids[k], p, depth + 1);
  };
  for (const p of points) insert(root, p, 0);
  return root;
}

function repel(cell, p, strength) {
  if (!cell.count || cell.point === p) return;
  let dx = cell.cx - p.x,
    dy = cell.cy - p.y;
  let d2 = dx * dx + dy * dy;
  if (cell.kids && (cell.size * cell.size) / (d2 || 1) > THETA * THETA) {
    for (const kid of cell.kids) repel(kid, p, strength);
    return;
  }
  if (d2 < 1) {
    // Two nodes on the same spot: nudge them apart in some direction.
    dx = Math.random() - 0.5 || 0.1;
    dy = Math.random() - 0.5 || 0.1;
    d2 = 1;
  }
  const f = (strength * cell.count) / d2;
  p.vx -= dx * f;
  p.vy -= dy * f;
}

/** One step of the simulation; returns whether it still moves. */
export function tick(sim) {
  const { points, graph } = sim;
  if (!points.length) return false;
  const tree = quadtree(points);
  const strength = CHARGE * sim.alpha;
  for (const p of points) repel(tree, p, strength / points.length ** 0.15);
  for (const [a, b] of graph.edges) {
    const p = points[a],
      q = points[b];
    const dx = q.x - p.x,
      dy = q.y - p.y;
    const d = Math.sqrt(dx * dx + dy * dy) || 1;
    const f = ((d - LINK) / d) * SPRING * sim.alpha;
    p.vx += dx * f;
    p.vy += dy * f;
    q.vx -= dx * f;
    q.vy -= dy * f;
  }
  for (const p of points) {
    p.vx -= p.x * GRAVITY * sim.alpha;
    p.vy -= p.y * GRAVITY * sim.alpha;
    if (p.pinned) {
      p.vx = p.vy = 0;
      continue;
    }
    p.vx *= DECAY;
    p.vy *= DECAY;
    p.x += p.vx;
    p.y += p.vy;
  }
  sim.alpha *= 0.985;
  return sim.alpha > 0.01;
}

/** Runs the simulation until it settles (or `steps` steps). */
export function settle(sim, steps = 400) {
  for (let i = 0; i < steps && tick(sim); i++);
  return sim;
}

// ---------------------------------------------------------------------------
// The view

const PALETTE = [
  "#5b6ee1",
  "#2f9e74",
  "#d9822b",
  "#c2417a",
  "#8a63d2",
  "#2b9bb3",
  "#b8a028",
  "#d4543f",
];
/** A colour per folder, the same each time. */
export function folderColor(folders, id) {
  const i = folders.indexOf(id);
  return i < 0 ? null : PALETTE[i % PALETTE.length];
}
const radius = (n) =>
  n.kind === "tag" ? 3.5 : 3.5 + Math.sqrt(n.degree) * 1.6;

/**
 * Shows `graph` in `canvas`. Options: `color(node)`, `onOpen(node)` (a click on
 * a node), `onSelect(node | null)` (hover or keyboard selection), `focus` (a
 * node ID to centre on). Returns `{ select(id), center(id), destroy() }`.
 */
export function mountGraph(canvas, graph, options = {}) {
  const sim = createSimulation(graph);
  const ctx = canvas.getContext("2d");
  const view = { x: 0, y: 0, k: 1 };
  let hover = -1,
    selected = -1,
    drag = null,
    frame = 0,
    destroyed = false,
    fitted = false,
    // Set once the reader pans, zooms or picks a note: no more fitting then.
    moved = false;
  const neighbors = graph.nodes.map(() => new Set());
  for (const [a, b] of graph.edges) {
    neighbors[a].add(b);
    neighbors[b].add(a);
  }
  const styles = getComputedStyle(canvas);
  const ink = styles.getPropertyValue("--ink").trim() || "#263249";
  const muted = styles.getPropertyValue("--muted").trim() || "#8590a2";
  const line = styles.getPropertyValue("--line").trim() || "#e9edf2";
  const accent = styles.getPropertyValue("--accent").trim() || "#5865d8";
  const font = styles.fontFamily || "sans-serif";

  function size() {
    const ratio = devicePixelRatio || 1;
    const { width, height } = canvas.getBoundingClientRect();
    if (canvas.width !== Math.round(width * ratio))
      canvas.width = Math.round(width * ratio);
    if (canvas.height !== Math.round(height * ratio))
      canvas.height = Math.round(height * ratio);
    return { width, height, ratio };
  }
  /** Zooms and moves so every node is in sight. */
  function fit() {
    const { width, height } = size();
    if (!sim.points.length || !width) return;
    const xs = sim.points.map((p) => p.x),
      ys = sim.points.map((p) => p.y);
    const w = Math.max(...xs) - Math.min(...xs) + 80,
      h = Math.max(...ys) - Math.min(...ys) + 80;
    view.k = Math.min(2, Math.max(0.15, Math.min(width / w, height / h)));
    view.x = width / 2 - ((Math.max(...xs) + Math.min(...xs)) / 2) * view.k;
    view.y = height / 2 - ((Math.max(...ys) + Math.min(...ys)) / 2) * view.k;
  }
  const toScreen = (p) => [p.x * view.k + view.x, p.y * view.k + view.y];
  const toWorld = (sx, sy) => [(sx - view.x) / view.k, (sy - view.y) / view.k];
  function nodeAt(sx, sy) {
    const [wx, wy] = toWorld(sx, sy);
    let best = -1,
      bestD = Infinity;
    sim.points.forEach((p, i) => {
      const d = Math.hypot(p.x - wx, p.y - wy);
      const r = radius(graph.nodes[i]) + 4 / view.k;
      if (d < r && d < bestD) {
        best = i;
        bestD = d;
      }
    });
    return best;
  }

  function draw() {
    const { width, height, ratio } = size();
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    ctx.clearRect(0, 0, width, height);
    const focus = hover >= 0 ? hover : selected;
    const near = focus >= 0 ? neighbors[focus] : null;
    ctx.lineWidth = 1;
    for (const [a, b] of graph.edges) {
      const lit = focus >= 0 && (a === focus || b === focus);
      ctx.strokeStyle = lit ? accent : line;
      ctx.globalAlpha = focus >= 0 && !lit ? 0.35 : 1;
      const [x1, y1] = toScreen(sim.points[a]);
      const [x2, y2] = toScreen(sim.points[b]);
      ctx.beginPath();
      ctx.moveTo(x1, y1);
      ctx.lineTo(x2, y2);
      ctx.stroke();
    }
    graph.nodes.forEach((n, i) => {
      const [x, y] = toScreen(sim.points[i]);
      if (x < -20 || y < -20 || x > width + 20 || y > height + 20) return;
      const dim = focus >= 0 && i !== focus && !near.has(i);
      ctx.globalAlpha = dim ? 0.25 : 1;
      const r = radius(n) * Math.max(0.6, Math.min(view.k, 1.6));
      ctx.fillStyle = n.kind === "tag" ? muted : options.color?.(n) || accent;
      ctx.beginPath();
      if (n.kind === "tag") ctx.rect(x - r, y - r, r * 2, r * 2);
      else ctx.arc(x, y, r, 0, Math.PI * 2);
      ctx.fill();
      if (i === focus) {
        ctx.strokeStyle = ink;
        ctx.lineWidth = 2;
        ctx.stroke();
        ctx.lineWidth = 1;
      }
    });
    // Labels: when zoomed in, for the focused note and its neighbours, and for
    // the most linked notes.
    ctx.font = `12px ${font}`;
    ctx.textAlign = "center";
    ctx.textBaseline = "top";
    // Hubs only: the few most linked notes, when clearly more linked than most.
    const busiest = [...graph.nodes.keys()]
      .filter((i) => graph.nodes[i].degree >= 4)
      .sort((a, b) => graph.nodes[b].degree - graph.nodes[a].degree)
      .slice(0, 8);
    graph.nodes.forEach((n, i) => {
      const show =
        view.k > 1.3 ||
        i === focus ||
        near?.has(i) ||
        (focus < 0 && busiest.includes(i) && n.degree > 1);
      if (!show) return;
      const [x, y] = toScreen(sim.points[i]);
      if (x < -100 || y < -20 || x > width + 100 || y > height + 20) return;
      ctx.globalAlpha = focus >= 0 && i !== focus && !near.has(i) ? 0.3 : 1;
      ctx.fillStyle = i === focus ? ink : muted;
      const text = n.label.length > 40 ? n.label.slice(0, 39) + "…" : n.label;
      ctx.fillText(text, x, y + radius(n) * Math.min(view.k, 1.6) + 3);
    });
    ctx.globalAlpha = 1;
  }

  function loop() {
    if (destroyed) return;
    const moving = tick(sim);
    // Fit while the layout spreads out, and once more when it settles.
    if (!moved && !fitted) {
      fit();
      if (!moving) fitted = true;
      if (options.focus) center(options.focus, false);
    }
    draw();
    frame = moving || drag ? requestAnimationFrame(loop) : 0;
  }
  const wake = (alpha = 0.3) => {
    sim.alpha = Math.max(sim.alpha, alpha);
    if (!frame) frame = requestAnimationFrame(loop);
  };
  // Lay out most of the way before the first frame, so it opens calm.
  for (let i = 0; i < 120 && tick(sim); i++);
  fit();
  frame = requestAnimationFrame(loop);

  const local = (e) => {
    const r = canvas.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top];
  };
  canvas.addEventListener("pointerdown", (e) => {
    const [sx, sy] = local(e);
    const i = nodeAt(sx, sy);
    canvas.setPointerCapture(e.pointerId);
    drag = { i, sx, sy, vx: view.x, vy: view.y, moved: false };
    if (i >= 0) sim.points[i].pinned = true;
  });
  canvas.addEventListener("pointermove", (e) => {
    const [sx, sy] = local(e);
    if (!drag) {
      const i = nodeAt(sx, sy);
      if (i !== hover) {
        hover = i;
        canvas.style.cursor = i >= 0 ? "pointer" : "grab";
        options.onSelect?.(
          i >= 0
            ? graph.nodes[i]
            : selected >= 0
              ? graph.nodes[selected]
              : null,
        );
        if (!frame) draw();
      }
      return;
    }
    if (Math.hypot(sx - drag.sx, sy - drag.sy) > 3) drag.moved = true;
    if (drag.i >= 0) {
      const [wx, wy] = toWorld(sx, sy);
      Object.assign(sim.points[drag.i], { x: wx, y: wy });
      wake(0.2);
    } else {
      view.x = drag.vx + sx - drag.sx;
      view.y = drag.vy + sy - drag.sy;
      moved = true;
      if (!frame) draw();
    }
  });
  const release = () => {
    if (!drag) return;
    const { i, moved } = drag;
    if (i >= 0) sim.points[i].pinned = false;
    drag = null;
    if (i >= 0 && !moved) options.onOpen?.(graph.nodes[i]);
  };
  canvas.addEventListener("pointerup", release);
  canvas.addEventListener("pointercancel", release);
  canvas.addEventListener("pointerleave", () => {
    if (hover >= 0 && !drag) {
      hover = -1;
      options.onSelect?.(selected >= 0 ? graph.nodes[selected] : null);
      if (!frame) draw();
    }
  });
  canvas.addEventListener(
    "wheel",
    (e) => {
      e.preventDefault();
      const [sx, sy] = local(e);
      const [wx, wy] = toWorld(sx, sy);
      view.k = Math.min(
        6,
        Math.max(0.1, view.k * Math.exp(-e.deltaY * 0.0015)),
      );
      view.x = sx - wx * view.k;
      view.y = sy - wy * view.k;
      moved = true;
      if (!frame) draw();
    },
    { passive: false },
  );
  const resize = new ResizeObserver(() => !frame && draw());
  resize.observe(canvas);

  function select(id) {
    selected = graph.nodes.findIndex((n) => n.id === id);
    options.onSelect?.(selected >= 0 ? graph.nodes[selected] : null);
    if (!frame) draw();
  }
  function center(id, chosen = true) {
    const i = graph.nodes.findIndex((n) => n.id === id);
    if (i < 0) return;
    if (chosen) moved = true;
    const { width, height } = size();
    view.k = Math.max(view.k, 1.2);
    view.x = width / 2 - sim.points[i].x * view.k;
    view.y = height / 2 - sim.points[i].y * view.k;
    select(id);
  }
  return {
    select,
    center,
    fit: () => {
      moved = false;
      fit();
      if (!frame) draw();
    },
    neighbors: (id) => {
      const i = graph.nodes.findIndex((n) => n.id === id);
      return i < 0 ? [] : [...neighbors[i]].map((j) => graph.nodes[j]);
    },
    destroy() {
      destroyed = true;
      cancelAnimationFrame(frame);
      resize.disconnect();
    },
  };
}
