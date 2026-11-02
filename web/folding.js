/**
 * Folding: a block with blocks nested under it (indented further, right after
 * it) can be folded to hide them. Which blocks are folded is kept on this device,
 * per note, so folding a long outline never changes the note for anyone else.
 * `level(markdown)` gives a block's nesting level.
 */

/** The index just after the blocks nested under block `i`. */
export function subtreeEnd(blocks, i, level) {
  const own = level(blocks[i].markdown);
  let j = i + 1;
  while (j < blocks.length && level(blocks[j].markdown) > own) j++;
  return j;
}

export function hasChildren(blocks, i, level) {
  return subtreeEnd(blocks, i, level) > i + 1;
}

/** IDs of the blocks hidden inside folded blocks. */
export function hiddenBlocks(blocks, folded, level) {
  const hidden = new Set();
  for (let i = 0; i < blocks.length; i++) {
    if (hidden.has(blocks[i].id) || !folded.has(blocks[i].id)) continue;
    const end = subtreeEnd(blocks, i, level);
    for (let j = i + 1; j < end; j++) hidden.add(blocks[j].id);
  }
  return hidden;
}

/** The folded blocks that hide block `id` (its folded ancestors). */
export function foldedAbove(blocks, id, folded, level) {
  const at = blocks.findIndex((b) => b.id === id);
  if (at < 0) return [];
  const out = [];
  let need = level(blocks[at].markdown);
  for (let i = at - 1; i >= 0 && need > 0; i--) {
    const l = level(blocks[i].markdown);
    if (l < need) {
      if (folded.has(blocks[i].id)) out.push(blocks[i].id);
      need = l;
    }
  }
  return out;
}

const key = (noteId) => `minerva.folded.${noteId}`;
export function loadFolded(noteId) {
  try {
    return new Set(JSON.parse(localStorage.getItem(key(noteId)) || "[]"));
  } catch {
    return new Set();
  }
}
/** Keeps only blocks that still exist in the note. */
export function saveFolded(noteId, folded, blocks) {
  const ids = new Set(blocks.map((b) => b.id));
  try {
    const kept = [...folded].filter((id) => ids.has(id));
    if (kept.length) localStorage.setItem(key(noteId), JSON.stringify(kept));
    else localStorage.removeItem(key(noteId));
  } catch {
    // Folding still works for this visit.
  }
}
