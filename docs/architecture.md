# Architecture and product direction

## Study of AFFiNE

Reviewed on 2026-10-24 against the repository's moving `canary` branch; these links are reference sources, not pinned implementation dependencies.

- [Repository and product overview](https://github.com/toeverything/AFFiNE): documents, edgeless canvas, databases, self-hosting, local-first collaboration, and AI are distinct substantial product areas.
- [Backend package manifest](https://github.com/toeverything/AFFiNE/blob/canary/packages/backend/server/package.json): the server uses Node.js, NestJS, GraphQL, Prisma, Redis, and native Rust modules. Rewriting it is a new system design, not simply replacing its HTTP layer.
- [Rust workspace](https://github.com/toeverything/AFFiNE/blob/canary/Cargo.toml): Rust already supplies native storage and common functionality. The transferable lesson is to share protocol/domain logic between platforms.
- [Permission service](https://github.com/toeverything/AFFiNE/blob/canary/packages/backend/server/src/core/permission/service.ts): authorization is evaluated for workspace/document actions at the backend boundary. Minerva follows that principle with folder-level access.
- [License boundary](https://github.com/toeverything/AFFiNE/blob/canary/LICENSE): different parts of AFFiNE carry different license terms. Minerva uses original implementation and visual assets; it does not vendor AFFiNE code.

The key product insight is that a block should retain its identity and content when switching between a document and a spatial view. Minerva retains stable block identity and now supports live CRDT editing and typed databases. Further canvas work is out of scope. Offline native-client synchronization and in-app AI chat remain separate milestones.

## Current design

Browser → same-origin `/api/v1` → Axum → authenticated operation → folder authorization → in-memory domain snapshot → SQLite transaction.

`minerva-core` contains `User`, `Workspace`, `Folder`, `Grant`, `Note`, `Block`, `SaveNote`, `Database`, `Column`, and `Record`. The server owns a persistent server UUID. Each note has a folder, an ordered block vector, a revision counter, and timestamps. Blocks carry stable UUIDs and Markdown text. Legacy canvas coordinates remain serialized for compatibility, but the canvas UI is no longer offered.

Enter splits a block at the cursor (or removes the selection and splits there), renders the first part in Rust, and continues in a new block with the remainder. Inside an unclosed code fence, Enter inserts a newline instead. Backspace at the start of a block merges it into the previous one, and Delete at the end merges the next one in. Shift+Enter keeps multiple lines inside one block. Arrow keys at a block's first or last line move between blocks.

**Nesting.** Tab nests a block under the block above, at most one level deeper than that block, and Shift+Tab moves it out. The level is stored as leading spaces (two per level, at most six), so it syncs through the CRDT, works with undo, and exports as genuinely nested Markdown lists.

- The server strips the indentation before rendering, so four spaces never turn a paragraph into a code block. It then wraps the HTML in an `md-indent-N` class, so published pages, guests, and exports show the nesting.
- Enter keeps the level and continues lists (`- `, `- [ ] `, or the next number). Enter on an empty item moves it out a level or ends the list.
- Backspace at the start of an item's text first moves it out, then turns a bullet into text.
- Inside code fences, Tab inserts two spaces.
- Escape leaves the field, so keyboard users are never trapped by Tab. This is still a Markdown textarea per block, not a rich-text selection engine: selections cannot span blocks.

Undo and redo use a Yjs `UndoManager` that tracks only the local client's transactions, so Ctrl+Z never reverts a collaborator's concurrent edit. Typing within 600 ms groups into one step; splits, merges, moves, deletions, and pastes start new steps. Inside a text field, the browser's native undo applies first. A field with no native history (for example, right after a split) falls back to note-level undo. Persisted revision restore remains the document-level recovery path.

IME composition is protected in both directions. Live sync never rewrites a field while it is composing, and the app does not commit intermediate composition text. When composition ends, the composed text is applied at CRDT relative positions captured at composition start, so it lands correctly even if collaborators edited the same block meanwhile.

Accessibility: blocks form a labelled list with position-aware labels ("Edit block 3 of 12"), and the textarea has a matching label and keyboard help. Structural changes (split, merge, move, delete, undo) are announced through a polite live region. Dialogs take their heading as label and return focus to the control that opened them, and a skip link leads to the content. Screen-reader and IME coverage on real assistive technology remains part of production validation.

Code blocks are highlighted on the server with syntect, as class-based spans (`hl-*`) that the sanitizer allow-lists by prefix; arbitrary classes are stripped. The same output is used for members, share-link guests, and published pages, with light and dark palettes. Rendered HTML is cached by audience and Markdown, which also reduces the cost of sync polling.

**Writing tools.**

- Formatting shortcuts and the selection toolbar insert Markdown markers through `insertText`, so the browser's native undo still works. Toggling a format a second time removes its markers.
- Pasted HTML is converted to Markdown in the browser with Turndown and its GFM plugin. Rules handle Google Docs' bold wrapper and its inline-style formatting, and `<mark>` becomes `==…==`.
- Find and replace works on block Markdown. Matches in the rendered text are highlighted with the CSS Custom Highlight API where supported, and replacements are a single undo step.
- Callouts (GFM alerts), highlights, inline tags, and math are rendered on the server, so share-link guests and published pages get them without scripts. Math is converted to MathML Core by math-core, and the sanitizer allow-lists MathML presentation elements (no links or events).
- Mermaid diagrams render in the browser from a separately loaded bundle (`/mermaid.js`, with `securityLevel: strict`). Published pages show the diagram source.
- Note covers and tags are CRDT metadata (`meta.cover`, `meta.tags`), validated on the server like other fields. Note links point at IDs (`#note/{id}`), so they survive renames, and backlinks are computed per request from notes the reader may open.

**Code diagnostics evaluation.** Minerva treats code blocks as text and never executes them. JSON blocks are parsed server-side, and syntax errors are shown with line and column. General linting was evaluated and deferred for these reasons:

- Linters are language-specific tools that need per-language configuration and versions.
- Running them server-side means executing third-party binaries on user input. Each would need sandboxing and resource limits, and that is execution in all but name.
- Notes rarely contain complete programs, so most diagnostics would be noise.
- Client-side parsers for many languages would inflate the browser bundle.

Revisit linting only as an opt-in, client-side feature for a small set of data formats (YAML, TOML) or with a sandboxed language-server design.

Writes are serialized by the store. Live note editing uses the Yjs/Yrs update-v1 protocol: the browser emits CRDT changes and Rust applies and validates them before persisting a canonical note projection plus the CRDT state. Root types are `title` (Y.Text), `order` (Y.Array of stable block IDs), `block:<id>` (Y.Text per block), and `meta` (Y.Map containing icon/favorite/trashed). Character edits use minimal replacements so concurrent unaffected text retains its identity. Duplicate IDs caused by concurrent reorder are materialized once. Every update is authorized against the note's current folder; CRDT data cannot change folder membership.

The transport is authenticated JSON HTTP polling, every 650 ms, with a 180 ms write debounce. Responses contain merged state, sanitized rendered content, and permission-filtered presence. Presence is ephemeral, has a 15-second TTL, and is not persisted. The client uses relative positions to retain cursor selections while applying remote text updates. In-memory pending updates are retried idempotently after connection loss; there is no persisted browser offline queue. A future delta/event transport can replace polling without changing the CRDT data model. Current sync returns full state and is intended for modest documents and teams; benchmark and replace full-state polling before scaling.

The legacy note PUT endpoint retains revision checking, making explicit snapshot replacement safe against already-committed changes. It also updates the CRDT representation so connected editors receive REST edits and history restores. Live history keeps up to 50 checkpoints, grouping successive edits over 30-second windows. Schema and structured-record edits use revisions and return HTTP 409 for stale versions. Database cell writes from the same browser are serialized; other clients refresh automatically when they are not editing. Tables have validated property IDs/types and records, with recoverable archived records. These are structured product databases, not arbitrary SQL exposed to clients.

Sharing adds per-note access on top of folder roles. Member shares raise a member's effective role on one note. Link shares give guests a separate, cookie-authenticated API with anonymized presence and note-scoped attachments. Publications render pinned snapshots or live content as static pages under `/shared/` or a workspace's site paths (`/blog/`, `/wiki/`). See `sharing.md`, including the cross-server identity design.

The built-in MCP endpoint is stateless Streamable HTTP, separate from the browser API. Tokens are read-only, scoped to one workspace and issuing user, stored as hashes, and expire in 1–90 days. Folder permissions and account status are re-evaluated on every call. The credential-management UI never stores raw tokens in browser localStorage. See `docs/mcp.md` for the wire contract and tested SDK compatibility.

The SQLite database currently stores one JSON state row, committed atomically per successful mutation. This is deliberately a small initial persistence implementation, not the target scalable schema. Before production, migrate to relational tables (`users`, `sessions`, `workspaces`, `folders`, `folder_grants`, `notes`, `blocks`, `note_revisions`, `audit_events`) with foreign keys and transactional migrations. Add FTS5 for local deployments and evaluate PostgreSQL for larger installations. Retain API shapes through the migration.

## Permission contract

| Actor                        | Scope                                                        | Read                 | Edit and delete notes, delete folders | Create folders      | Create workspaces             | Delete workspaces | Manage users/grants                   |
| ---------------------------- | ------------------------------------------------------------ | -------------------- | ------------------------------------- | ------------------- | ----------------------------- | ----------------- | ------------------------------------- |
| Server administrator         | Their own workspaces, like any owner                         | Only owned or shared | Only owned or shared                  | In owned workspaces | Yes                           | Only their own    | Users; grants on their own workspaces |
| Workspace owner              | Owned workspace                                              | Yes                  | Yes                                   | Yes                 | Only with creation capability | Their own         | Grants on their own workspace         |
| Workspace editor             | Every folder of the assigned workspace, including later ones | Yes                  | Yes                                   | No                  | Only with creation capability | No                | No                                    |
| Workspace viewer             | Every folder of the assigned workspace                       | Yes                  | No                                    | No                  | Only with creation capability | No                | No                                    |
| Folder editor (older grants) | Granted folder                                               | Yes                  | Yes                                   | No                  | Only with creation capability | No                | No                                    |
| Folder viewer (older grants) | Granted folder                                               | Yes                  | No                                    | No                  | Only with creation capability | No                | No                                    |
| Ungranted member             | None                                                         | No                   | No                                    | No                  | Only with creation capability | No                | No                                    |

Every account has one personal workspace, My Workspace, created with the account: it cannot be shared, renamed, or deleted, and access to it cannot be granted. A new server also starts with a Team workspace for sharing. Administrators have no access to other people's workspaces, personal or shared: they see what they own and what is shared with them. Owners share their workspaces (administrators do so in Server administration → Workspace access, with edit access by default). The storage overview shows sizes for every note but titles only for notes the administrator may open. Deleting an account deletes its unshared workspaces and passes its shared ones to the administrator, so their members keep them. A member's role on a folder is the higher of their workspace access and any older folder grant; a folder grant also covers that folder's subfolders. Removing either does not override workspace ownership. There is no implicit access just because a user can create workspaces. Disabling a user invalidates their sessions and rejects future sign-in. User-management changes revoke that user's existing sessions. Revoking workspace or folder access takes effect at the next API request.

**Deletion.** Notes are moved to the trash first; folder editors can then delete them forever. Folder editors can delete a folder with everything in it. Only a workspace's owner can delete it, shared or not; nobody else can, administrators included. Deleting removes everything that belongs to the item (notes, version history, live editing state, shares, published pages, databases, sites, grants, MCP credentials, and attachment files) and cannot be undone. A share recipient with edit access cannot delete the note. Previously downloaded information cannot be remotely erased.

## Native clients: Windows and Linux

Implemented as a local-first desktop client; see `desktop.md` and `sync.md`. `minerva-client` holds the connection registry (`server_id`, URL, account, TLS policy), stores tokens in Windows Credential Manager / Linux Secret Service, and provides typed API access plus an authenticating gateway. It also embeds `minerva-server` as the app's local server and runs the sync worker that replicates subscribed workspaces. `minerva-desktop` (tao/wry/muda) hosts the system webview. It serves the local server and each remote server's own interface from separate local origins. The webview supplies editor input, IME, and accessibility behavior identical to the browser. That resolves the shell choice below in favor of a Rust-hosted webview.

Offline sync uses a per-entity change log with tombstones, per-field hybrid logical clocks, idempotent operation IDs, permission revalidation through an access digest, durable cursors, and attachment copying. Note text reuses the Yjs/Yrs CRDT. The original design notes follow.

A client should maintain a registry of independently authenticated server connections, each with `server_id`, URL, account, TLS policy, and sync cursor. All cached identifiers must be qualified by `(server_id, entity_id)`; never assume account IDs or permissions are shared across servers. Store credentials in Windows Credential Manager or Linux Secret Service, not application configuration files. A multi-server client is not server federation: there is no cross-server trust or replication in this MVP.

Reuse `minerva-core` and add a `minerva-client` transport crate around the versioned JSON API. UI framework selection (for example native Rust UI versus a Rust-hosted webview) should follow an editor/IME/accessibility prototype. Do not commit to the desktop shell before validating editor performance and platform input behavior.

Before offline editing, introduce an authenticated operation/change-feed protocol, idempotency keys, tombstones, schema versions, attachment checksums, permission revalidation, and bounded retry queues. Reuse the Yjs-compatible CRDT representation already implemented in Rust, but validate large documents, offline deletes, permission revocation, durable queues, and reconnect behavior before native-client offline releases. Revision checks remain appropriate for structured records and administrative operations.

## Delivery roadmap and parity map

| Capability                              | Current status                                                                                                                                                  | Next work                                                                                            |
| --------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| Hierarchy, accounts, folder permissions | Implemented                                                                                                                                                     | Workspace rename/archive, nested folders if required, ownership transfer, audit log                  |
| Markdown block notes                    | Cursor-aware split/merge, local CRDT undo/redo, IME-safe collaboration, keyboard navigation, server-side highlighting, attachments with reference-aware cleanup | Cross-block selection, backlinks, rich-text editing evaluation                                       |
| Sharing and publishing                  | Member shares, view/edit links, pinned or live public pages with index and feed                                                                                 | Cross-server sharing (designed in `sharing.md`), custom domains, page themes                         |
| Canvas                                  | Legacy data retained; UI removed                                                                                                                                | No further canvas development requested                                                              |
| Tables                                  | Typed database records, properties, table/board views, filter/sort, record archive                                                                              | Relations, formulas, schema migration UI, richer database views                                      |
| Collaboration                           | Yjs/Yrs CRDT merging, presence, HTTP polling, convergence tests                                                                                                 | Delta/event transport, scale tests, richer presence                                                  |
| Local-first and multiple servers        | Desktop connection registry, OS credential storage, embedded local server, offline copies with change-log sync and conflict review (`sync.md`)                  | Presence relay for offline copies, push notifications instead of polling, large-workspace benchmarks |
| Windows/Linux native clients            | Local-first desktop client (`minerva-client`, `minerva-desktop`)                                                                                                | Windows hardware validation, streaming transport, installers, auto-update                            |
| AI                                      | Read-only MCP; scoped in-app chat (workspace/folder/note) with OpenAI, Anthropic, DeepSeek, and compatible providers                                            | Per-user provider keys, usage limits, retrieval for large workspaces                                 |
| Operations                              | Local binary, SQLite, tests                                                                                                                                     | Relational storage, migrations, audit, metrics, quotas, backup/restore automation, load tests        |
| Identity                                | Password sessions, admin-created accounts, TOTP two-factor authentication with recovery codes                                                                   | Password changes/recovery, invitations, SSO, WebAuthn/passkeys, finer session controls               |

The `TODO.md` backlog is complete. Cross-server sharing is designed but not implemented. Continue hardening persistence (normalized tables would replace snapshot diffing for change tracking), editor correctness/accessibility, and identity. No further canvas features are planned.
