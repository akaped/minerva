# Minerva product backlog

Checked items are implemented. Unchecked items remain planned work.

## Current delivery — finish and verify

- [x] CRDT-based live note collaboration and presence, with server permission enforcement.
- [x] Typed structured databases, table/board views, filtering, sorting, editable records, recoverable archive.
- [x] Light, dark, and system themes.
- [x] Workspace-scoped, read-only MCP with expiring/revocable credentials and note/database tools.
- [x] Complete browser regression checks, including rapid edits to adjacent database cells and two simultaneous note editors.
- [x] Update setup, API, architecture, and verification documentation for this release.

## Images and file attachments

- [x] Add images by upload and paste; display them inside notes.
- [x] Persist original image files and allow authorized users to download them.
- [x] Allow general file uploads and downloads from notes.
- [x] Generate opaque internal filenames/storage keys. Never use the uploaded filename as a filesystem path. Preserve a sanitized original filename only as download metadata.
- [x] Enforce folder/note permissions for upload, preview, and download; validate size/type, prevent path traversal, and serve untrusted files as attachments with safe headers.
- [x] Define initial retention and quotas: 10 MiB/file, 100 MiB/note, 1 GiB/server; retain uploads, including unreferenced files, for history and reinsertion. Files use UUID keys in a separate SQLite blob table.
- [x] Add administrator storage management and reference-aware orphan cleanup. Server administration shows usage by note and lists unreferenced files. A file is deletable only if no current note, retained history version, or published snapshot references it and it is older than 24 hours (so interrupted insertions stay recoverable). References are re-checked at deletion time.

## In-app AI providers and contextual chat

- [x] Configure common providers: DeepSeek, OpenAI/ChatGPT-compatible API access, and Anthropic Claude; include endpoint/model selection and server-side secret storage.
- [x] Distinguish external MCP connections from in-app LLM provider configuration in the interface.
- [x] Workspace-wide chat using only content the user may access.
- [x] Folder/project-wide chat restricted to that folder.
- [x] Note-only chat restricted to the current note.
- [x] Make the selected context scope visible; include source references, streaming responses, history, cancellation, and usage/error feedback.
- [x] Never send provider keys to browser storage or include documents outside the selected context or the user's permissions.
- Verified end to end against mock OpenAI Responses, Anthropic Messages, and OpenAI-compatible streams (`tests/ai_stream.rs`): permitted content only, streamed deltas, saved history and usage, stale-revision rejection, and cancellation without saving partial output.

## Editor improvements

- [x] Expand the `/` command menu, including searchable commands, keyboard navigation, and new block types.
- [x] Fix code text contrast in light/dark themes; add fenced and inline rendering regression tests and verify both themes in the browser.
- [x] Add language-aware syntax highlighting and evaluate lint/diagnostic support for code blocks. Highlighting is server-side (syntect, sanitized `hl-*` classes, light and dark palettes) for members, guests, and published pages. JSON blocks report parse errors. Other linting was evaluated and deferred; see `docs/architecture.md`. Code is never executed.
- [x] Preserve fenced code, tables, and lists during multiline Markdown import and paste.
- [x] Improve selection-aware block splitting/merging, undo, IME support, and accessibility. Enter splits at the cursor (but adds a newline inside code fences), and Backspace/Delete at block edges merge. Undo/redo (toolbar, Ctrl+Z / Ctrl+Shift+Z) reverts only your own changes via the CRDT, so collaborators' edits survive. IME composition is never interrupted by live updates and merges correctly with concurrent remote edits. Arrow keys move between blocks, blocks carry position labels, structural changes are announced, dialogs restore focus, and there is a skip link.

## Sharing, public pages, and blogging

- [x] Add a Share button with read-only access selected by default.
- [x] Support public links with no sign-in requirement, with explicit read-only or edit access and revocation.
- [x] Support sharing to a specific member on the same Minerva server.
- [x] Design identity verification and trust for sharing to a member of another Minerva server; do not assume matching email addresses establish cross-server identity. Design in `docs/sharing.md` (server keys, signed identity assertions, invitation flow); implementation is future work.
- [x] Enforce the selected mode on the server, including collaboration and attachment routes.
- [x] Render public read-only notes as clean HTML pages suitable for blog posts; omit editing controls and private workspace metadata.
- [x] Add publishing controls: preview, publish/unpublish, stable slugs, metadata, navigation/index, and a clear indicator of what is public. Includes an Atom feed at `/p/feed.xml`.
- [x] Define whether publishing pins a revision or follows ongoing edits; separate public edit links from published read-only pages. Pinned by default, live optional; edit links and published pages are separate mechanisms (see `docs/sharing.md`).

## Local-first desktop

- [x] Embedded local server in the desktop app for personal notes, with no setup and no network listener.
- [x] Server change log with tombstones and per-field hybrid logical clocks; pull/push sync API with idempotent operations and permission checks.
- [x] Offline copies of remote workspaces: durable outbox and cursors, background sync with backoff, CRDT text merging, per-field last-writer-wins, record conflict copies, edits winning over deletes, access revalidation, read-only copies where the server grants view access.
- [x] Attachment copying, launcher workspace selection, sync status badge and conflict review dialog.
- [ ] Relay presence through offline copies; replace polling with server push.
- [ ] Benchmark change tracking on large workspaces (normalized storage).

## Scope decisions

- Cross-server sharing is designed but not implemented; it needs server keys and signed assertions first (`docs/sharing.md`).
- Native Windows/Linux desktop client delivered as a local-first client (system webview, embedded server, offline copies of remote workspaces, multi-server, OS credential storage). Notifications, auto-update, and installers remain future work; see `docs/desktop.md` and `docs/sync.md`. The browser stays online-only.
- No further canvas features are requested. Keep compatibility with existing note data, but focus development on documents, databases, collaboration, and integrations.
