# TODO

What Minerva still misses compared with Notion, Logseq and AFFiNE (reviewed November 2026), and follow-ups found while building the last round. Ordered by what matters most for people switching to Minerva and for its learning focus. Move an item to `CHANGELOG.md` when it ships.

## Next




## Follow-ups from the last round

- [ ] **Comments**: in desktop offline copies (they are not replicated yet).
- [ ] **Live signals for the desktop app and share links**: `minerva://` pages cannot open the WebSocket, and guests are not signed in. Relay signals through the desktop gateway, and give guests a share-scoped socket; until then both poll.
- [ ] **Storage**: relational tables with foreign keys for queries that should not need everything in memory; evaluate PostgreSQL for larger installations. Each request still copies `Data` (cheap now, but linear in the number of entities).
- [ ] **Importers**: importing straight from Notion's API instead of an export.
- [ ] **MCP and API**: personal tokens limited to one workspace (read-only tokens exist); it touches every permission check, so it needs its own careful pass.

- [ ] **Recordings**: transcribing in the browser, so audio never leaves the device (it needs a speech model of 75 MB or more downloaded to the device).
- [ ] **Calendar**: CalDAV, so calendar apps can also add and change reminders. It is a protocol of its own (discovery, ETags, `REPORT` queries, each client's quirks) and needs its own project.
- [ ] **Installable app**: editing offline (now offline notes are read-only; it needs the CRDT updates queued and merged when back), keeping a whole workspace offline rather than the notes opened, files other than pictures offline, push notifications for reminders, and sharing files (not only text and links) to the app.

## MAYBE IN FUTURE. I will tell you when
- [ ] **Plugin API in the browser**: slash commands, block renderers and sidebar panels, sandboxed; a plugin list to browse.
- [ ] **Web clipper** browser extension, saving a page or selection to an inbox note.
- [ ] **AI**: semantic retrieval (embeddings stored next to the search index), per-person provider keys and usage limits, scheduled agents, AI actions on several selected blocks, streaming in the selection assistant.
- [ ] **Identity**: SSO (OIDC), passkeys, an audit log.
- [ ] **Markdown folder copy** in the desktop app: keep a plain Markdown copy of local workspaces in a folder, as Logseq's Markdown Mirror does (read-only first).
- [ ] **Translations, what is left**: a third language.
