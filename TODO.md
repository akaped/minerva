# TODO

What Minerva still misses compared with Notion, Logseq and AFFiNE (reviewed November 2026), and follow-ups found while building the last round. Ordered by what matters most for people switching to Minerva and for its learning focus. Move an item to `CHANGELOG.md` when it ships.

## Next

- [ ] **Databases**: rollups (counts, sums and lists through a relation) and formulas; records that open as pages; the calendar view in notes' embedded databases; reminders and journal dates on the same calendar; boards grouped by person or multi-select.

## Soon

- [ ] **Graph view** of note links, per workspace and per note (backlinks are already computed).
- [ ] **Block references and embeds**: link to a block (`#note/<id>#<block>`), show a block or note inside another, kept live.
- [ ] **Installable web app (PWA)**: manifest, service worker for offline reading, and a quick-capture page for phones.
- [ ] **Markdown folder copy** in the desktop app: keep a plain Markdown copy of local workspaces in a folder, as Logseq's Markdown Mirror does (read-only first).
- [ ] **Calendar feed**: reminders and journal dates as an iCal URL; CalDAV later.
- [ ] **Recording to notes**: recording → transcript → summary with action items, turned into tasks and reminders.
- [ ] **Translations** of the interface, Italian first: move strings out of `web/*.js`.
- [ ] **Flashcards**: study statistics, a desired-retention setting, per-person FSRS parameters fitted from review history, and media cards.

## Later

- [ ] **Plugin API in the browser**: slash commands, block renderers and sidebar panels, sandboxed; a plugin list to browse.
- [ ] **Web clipper** browser extension, saving a page or selection to an inbox note.
- [ ] **Archive** for notes: out of search and the sidebar without being in the trash.
- [ ] **Presentation mode**: a note as slides, split at dividers or headings.
- [ ] **AI**: semantic retrieval (embeddings stored next to the search index), per-person provider keys and usage limits, scheduled agents, AI actions on several selected blocks, streaming in the selection assistant.
- [ ] **Identity**: SSO (OIDC), passkeys, an audit log.

## Follow-ups from the last round

- [ ] **Editor**: evaluate a rich-text block editor (ProseMirror/Tiptap bound to the existing Yjs documents, still storing Markdown) against the textarea + live preview approach, for selection inside a block that spans formatting and for IME and accessibility.
- [ ] **Editor**: a click on a block right after a heading block closes can land elsewhere, because the closing heading changes height between mouse down and up. Keep the block's height until the click completes.
- [ ] **Comments**: in desktop offline copies (they are not replicated yet) and for share-link guests with edit access; editing a comment; comments in the Mentions dashboard widget; highlighting the quoted text in the block.
- [ ] **Folding and cards**: fold state shared with the note (as Logseq's `collapsed::`) as an option; link cards on published pages and for share-link guests; choosing between a card and a plain link.
- [ ] **Live signals for the desktop app and share links**: `minerva://` pages cannot open the WebSocket, and guests are not signed in. Relay signals through the desktop gateway, and give guests a share-scoped socket; until then both poll.
- [ ] **Delta sync**: `POST /notes/{id}/sync` returns the full CRDT state each time; send only what the client lacks (Yjs state vectors).
- [ ] **Storage**: relational tables with foreign keys for queries that should not need everything in memory; evaluate PostgreSQL for larger installations. Each request still copies `Data` (cheap now, but linear in the number of entities).
- [ ] **Importers**: OneNote and Bear; Notion's multi-select and relation columns (now text); Logseq's page embeds `{{embed …}}`; importing straight from Notion's API instead of an export.
- [ ] **Search**: index database records and attachment text (PDFs); search filters (folder, tag, date) in the Ctrl+K dialog.
- [ ] **MCP and API**: tools to change or move a block and to attach files; OAuth for clients that only support it; a token's last use shown in the list; scopes narrower than "everything I can do" for personal tokens.
- [ ] **`minerva-cli` in releases**: build it for Linux and Windows with the desktop apps; `backup` that only fetches what changed since the last run.
