# The note editor

How Minerva's note editor works, why it edits Markdown in a plain text field rather than being a rich-text editor, and what was measured to decide that (November 2026).

## How it works

A note is a list of **blocks**. Each block's content is a piece of Markdown: a paragraph, a heading, a list item, a table, a code block. That Markdown is the single source of truth everywhere:

- **Storage and sync:** the server stores it (`Note::blocks`), and live collaboration holds each block as a Yjs text (`block:<id>`) next to the block order (`order`), the title and a `meta` map (`web/collaboration.js`, `crates/minerva-server/src/collaboration.rs`).
- **Everything else that reads notes:** search indexes it, history keeps it, and exports, published pages, MCP, `minerva-cli`, the desktop replicas and the importers all read and write it.

A block is shown in one of two ways:

1. **Rendered.** The server turns the block's Markdown into sanitized HTML (`POST /render`, pulldown-cmark plus Minerva's own syntax: highlights, cloze, note links, tags, mentions, ⏰ times, database embeds, `[[toc]]`, math, callouts). The app caches the HTML by Markdown, so unchanged blocks never go back to the server.
2. **Edited.** Clicking a block (or pressing Enter on it) replaces its HTML with a `<textarea>` holding its Markdown. A layer under the field (`web/live-preview.js`) shows the same text with markers dimmed and marked text styled: bold, italics, links, code, highlights, and headings at their size. The field's own text is transparent, so the cursor, selection, typing, input methods and undo are all the browser's own.

Leaving a block renders it again. Keys in the field split blocks (Enter), merge them (Backspace at the start, Delete at the end), move between them (↑/↓ at the edges), indent them (Tab), and fold them (Ctrl+↑/↓). Whole blocks are selected with Shift+click or Shift+↑/↓ past the edge.

## The evaluation: a rich-text editor or Markdown in a field?

The question (from `TODO.md`) was whether a rich-text block editor would serve better: ProseMirror or Tiptap, bound to the existing Yjs documents and still storing Markdown. Three concerns were raised: selecting text that spans formatting, input methods (IME), and accessibility.

### What a rich-text editor would cost

A prototype bundled ProseMirror (model, state, view, commands, history, input rules), `prosemirror-markdown` and `y-prosemirror`, and passed Minerva's Markdown through it.

- **Size:** 489 KB minified (161 KB gzipped). Yjs, already in the app, accounts for 96 KB of it, so the editor would add about 390 KB, roughly doubling the app's 453 KB bundle.
- **Fidelity:** of the 50 blocks of the built-in guide and 20 samples of Minerva's own syntax, **24 of 70 changed** after one parse and serialize, many destructively:
  - tables collapsed into one line;
  - display math lost its line breaks and its backslashes were doubled;
  - task boxes became the escaped text `\[ \]`;
  - database embeds, `[[toc]]` and callouts were escaped, so they stop working;
  - nested list items lost their nesting;
  - `~~strike~~` was escaped;
  - every `-` list marker was rewritten as `*`.

  Each of these can be fixed with a custom schema, but that means a second implementation of Minerva's Markdown dialect, in JavaScript, which must round-trip *exactly* what the server's Rust renderer reads. Any difference silently rewrites notes as people open them. Notes would then change in history, collaboration and replication without anyone editing them.
- **Collaboration:** `y-prosemirror` binds to a `Y.XmlFragment`, a tree, not to the Markdown texts the server, replication and the desktop app read. Either the server learns to convert that tree to and from Markdown (a third implementation of the dialect, in Rust), or the editor keeps the per-block Yjs texts and re-parses a block on every remote change. The second loses ProseMirror's cursor mapping, which is most of what the binding is for.
- **Input methods:** contenteditable editors (ProseMirror included) handle composition themselves, and IME and Android keyboards remain their most reported class of bugs. A `<textarea>` composes natively, and Minerva already merges compositions with collaborators' edits (`beginComposition`/`endComposition`).

### What the current editor actually got wrong

Driving the editor in Chromium (and WebKit's API where it differs) found that the reported problems were not in the textarea approach itself. They were in how a block moved between its rendered and edited forms:

| Problem | Cause | Now |
| --- | --- | --- |
| A click on the block right after a heading being edited landed nowhere | Two mismatches: the heading's field was measured before its heading size applied, so its text spilled over the next block; and the rendered heading was 46 px taller than the field (85 against 39 px), so the next block jumped down while the button was still pressed | The field re-measures when its size changes; headings are edited with their rendered line height and margins (85 = 85 px); and a block that closes while a pointer is pressed keeps its height until the click completes |
| Clicking into a paragraph put the cursor at the end of the block | The click's place in the rendered text was never mapped to the Markdown | `web/caret-map.js` matches the rendered text before the click against the Markdown, skipping what Markdown writes but does not show (markers, link addresses, cloze braces); the cursor lands where the reader clicked |
| Text selected in a rendered block could not be formatted | Formatting only worked in the field | A selection in a rendered block shows the format bar, and the shortcuts (Ctrl+B/I/E/K, Ctrl+Shift+H/X, Ctrl+J for AI) and typing over it work as in the field: the block opens with the same text selected, without its markers |
| Screen readers heard "Edit block 3 of 8" instead of the text | Rendered blocks were `role="button"` with a label, which hides their content | Blocks are list items with their own content (headings, paragraphs, links) and a "Press Enter to edit" description; the list gives their position |
| Typing in long notes lagged | Splitting or merging redrew every block; finding each block's element for folding searched the page once per block; naming mentions ran once per added block; and the collaboration commit read the Yjs block order item by item, all quadratic | Splits, merges and deletions change only the blocks concerned; those three loops are now linear |

Times in a 2,000-block note, before and after (Chromium, debug server):

| Action | Before | After |
| --- | --- | --- |
| Enter (split a block) | 866 ms | 82 ms |
| Backspace (merge with the block above) | 631 ms | 53 ms |
| ↓ to the next block | 152 ms | 48 ms |

In a 500-block note, Enter went from 177 to 29 ms. The heading and paragraph sizes were checked by measuring rendered and edited blocks in the browser.

### Decision

**Keep Markdown in a text field, with the live preview.** A rich-text editor would double the bundle, need a second (and for collaboration a third) exact implementation of Minerva's Markdown, risk silently rewriting notes, and trade a native input path for a reimplemented one. What made the editor feel unfinished (the cursor, selections, the heading click, screen readers, long notes) was in the hand-over between rendered and edited blocks, and is fixed there.

Revisit this if the editor needs things a text field cannot do: editing inside a table's cells, or an image or embed edited in place. Even then, a structured editor for those block kinds (the textarea stays for text) is likely cheaper than replacing the editor.

## Still worth improving

- **Block kinds whose rendered form differs a lot from their Markdown** still change height when they open and close: quotes and callouts (a quote grows by 45 px when rendered), tables, code blocks, images, math. The height lock keeps clicks reliable, but the preview layer could draw quotes and callouts with their border and padding, and tables in columns.
- **Rendering goes to the server** each time a changed block closes. That is instant on a local network or the desktop app, but on a slow connection the Markdown stays visible until the answer arrives. A client-side renderer would be a second implementation again; rendering the preview layer's styling as a stopgap may be enough.
- **Mapping clicks** is an approximation. Text the Markdown does not contain (a person's name shown for an email address, a task box) is skipped, and a click on an image, embed or formula opens the block with the cursor at the end.
