# TODO

What Minerva still misses compared with Notion, Logseq and AFFiNE (reviewed November 2026), and follow-ups found while building the last round. Ordered by what matters most for people switching to Minerva and for its learning focus. Move an item to `CHANGELOG.md` when it ships.

## Next

- [ ] **Graph view** of note links, per workspace and per note (backlinks are already computed).

## Soon

- [ ] **Block references and embeds**: link to a block (`#note/<id>#<block>`), show a block or note inside another, kept live.
- [ ] **Installable web app (PWA)**: manifest, service worker for offline reading, and a quick-capture page for phones.
- [ ] **Calendar feed**: reminders and journal dates as an iCal URL; CalDAV later.
- [ ] **Recording to notes**: recording → transcript → summary with action items, turned into tasks and reminders.
- [ ] **Translations** of the interface, Italian first: move strings out of `web/*.js`.
- [ ] **Flashcards**: study statistics, a desired-retention setting, per-person FSRS parameters fitted from review history, and media cards.

## Later

- [ ] **Plugin API in the browser**: slash commands, block renderers and sidebar panels, sandboxed; a plugin list to browse.
- [ ] **Web clipper** browser extension, saving a page or selection to an inbox note.
- [ ] **Archive** for notes: out of search and the sidebar without being in the trash.
- [ ] **Presentation mode**: a note as slides, split at dividers or headings.


## Follow-ups from the last round

- [ ] **Comments**: in desktop offline copies (they are not replicated yet) and for share-link guests with edit access; editing a comment; comments in the Mentions dashboard widget; highlighting the quoted text in the block.
- [ ] **Folding and cards**: fold state shared with the note (as Logseq's `collapsed::`) as an option; link cards on published pages and for share-link guests; choosing between a card and a plain link.
- [ ] **Live signals for the desktop app and share links**: `minerva://` pages cannot open the WebSocket, and guests are not signed in. Relay signals through the desktop gateway, and give guests a share-scoped socket; until then both poll.
- [ ] **Delta sync**: `POST /notes/{id}/sync` returns the full CRDT state each time; send only what the client lacks (Yjs state vectors).
- [ ] **Storage**: relational tables with foreign keys for queries that should not need everything in memory; evaluate PostgreSQL for larger installations. Each request still copies `Data` (cheap now, but linear in the number of entities).
- [ ] **Importers**: OneNote and Bear; Notion's multi-select and relation columns (now text); Logseq's page embeds `{{embed …}}`; importing straight from Notion's API instead of an export.
- [ ] **Search**: index database records and attachment text (PDFs); search filters (folder, tag, date) in the Ctrl+K dialog.
- [ ] **Databases**: saving the board grouping and calendar property chosen for an embedded view (embeds use the first that fits); formulas over lists of related records, and rollups of rollups; number formats (currency, percent); records' pages in the desktop app's offline copies; reordering cards within a board column.
- [ ] **MCP and API**: tools to change or move a block and to attach files; OAuth for clients that only support it; a token's last use shown in the list; scopes narrower than "everything I can do" for personal tokens.
- [ ] **`minerva-cli` in releases**: build it for Linux and Windows with the desktop apps; `backup` that only fetches what changed since the last run.

## MAYBE IN FUTURE. I will tell you when
- [ ] **AI**: semantic retrieval (embeddings stored next to the search index), per-person provider keys and usage limits, scheduled agents, AI actions on several selected blocks, streaming in the selection assistant.
- [ ] **Identity**: SSO (OIDC), passkeys, an audit log.
- [ ] **Markdown folder copy** in the desktop app: keep a plain Markdown copy of local workspaces in a folder, as Logseq's Markdown Mirror does (read-only first).