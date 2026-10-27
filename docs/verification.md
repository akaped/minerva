# Verification

## Automated checks

Run:

```sh
npm ci
npm run build
npm test
cargo test --locked --workspace
cargo fmt --all --check
cargo build --locked -p minerva-server
node scripts/protocol-smoke.mjs
```

The protocol smoke test creates its own temporary database and localhost server, then removes them. It does not use the user's accounts or preview database.

Verified scenarios:

- **Access and persistence:** unauthenticated bootstrap rejection; inaccessible note/workspace filtering; denied admin/workspace creation; viewer/editor boundaries; note revision conflicts; history; grant revocation; disabled sessions; SQLite close/reopen persistence; session logout; Markdown script/event/unsafe-link sanitization; missing mutation header rejection.
- **Collaboration:** two independent Rust CRDT replicas edit the same block concurrently; server merges retain both changes, replicas converge, duplicate updates are idempotent, canonical note content is updated, and CRDT state survives a server restart. Presence is ephemeral. Viewers may synchronize reads but cannot submit updates.
- **Structured databases:** schema validation, number/checkbox/select/date type checks (including leap dates and malformed date format), unknown-property rejection, row revisions, stale-write rejection, recoverable archive, and folder-based read/write authorization.
- **MCP:** initialize, tool enumeration, read-only annotations, search, raw-token/hash non-disclosure, separate browser/MCP credentials, workspace-scoped endpoint checks, explicit cross-workspace document ID rejection, Origin validation, permission revocation, credential revocation, and expired credential rejection even for administrators.
- **Browser CRDT library:** minimal text replacements and Unicode/surrogate handling, concurrent edits in the same block, convergence, and base64 round-trip/idempotence.
- **Real protocol interoperability:** the official MCP TypeScript SDK initializes the Rust endpoint, lists tools, searches/reads notes, queries/filters records, and fails after token revocation. Real Yjs clients merge Unicode text through Rust/Yrs, proving the encoding and root model work across languages.

## Browser checks

Performed against `/tmp/minerva-preview.db`, separate from the normal `data/minerva.db` path:

- Sign-in and existing data survive restart/upgrade.
- Dark theme applies to navigation, dialogs, database tables/boards, and document editor; selected theme persists in a fresh tab.
- Create a structured database and record through the UI.
- Rapid edits to adjacent Name and Status cells save in sequence; switching to Board waits for writes and shows the record in the correct status column.
- Two browser tabs edit the same note text while both textareas remain active. Both converge to the combined content, with collaborator presence shown.
- Enter closes/renders the collaborative block and opens a new block; the new block appears in the second client without reopening the note.
- Existing baseline note creation/search/Markdown editing/admin controls were checked in the preceding release.

A browser reload of an older tab was blocked by automatic review because that tab held an unsaved test value. It was preserved; verification continued in a new tab. No draft was discarded to complete testing.

## Remaining validation

The desktop client has been exercised on Linux only; the Windows build is cross-compiled and statically checked (see below). Current collaboration uses full-state HTTP polling and in-memory retry queues, not durable offline sync. The store remains a single-process JSON snapshot in SQLite and needs scale/load testing and relational/indexed persistence before large deployments. Larger concurrent sessions, repeated structural block moves, server failures during sustained editing, browser/IME/editor selection edge cases, arbitrary multiline Markdown import fidelity, complete keyboard/screen-reader coverage, and production security review remain follow-up work.

These tests are not a comprehensive penetration test or a claim of full AFFiNE feature parity.

## Attachments and editor increment

`cargo test --locked --workspace`: attachment tests cover byte-exact original downloads, safe filename metadata, UUID keys, authenticated access, viewer upload denial, immediate permission revocation, CSRF headers, oversize rejection, fake-image preview rejection, and persistence after reopening SQLite. Rendering regression covers escaped fenced code and inline code.

`npm test`: Markdown grouping tests cover CRLF, blank lines in fences, backtick/tilde fences, unfinished fences, tables, and lists. Existing collaboration tests remain passing.

Browser checks on the separate port-3001 preview confirmed searchable commands, Enter-to-select Code, readable fenced/inline code in light and dark themes, the Files dialog, and a real file-chooser upload of a generated 160×80 PNG. The browser reported the uploaded image fully decoded at its original dimensions and the note fully saved. Image clipboard paste is implemented but not yet exercised end-to-end through the browser clipboard.

Markdown image regression: HTTP and HTTPS image sources survive sanitization and are allowed by the image-specific Content Security Policy. Uploaded attachment URLs in image syntax are mapped to their authenticated preview endpoints (relative and absolute URLs); download links and code literals are unchanged. Script URLs remain sanitized and script policy remains restricted. All Rust tests pass.

## Offline sync increment

- `crates/minerva-server/tests/replication.rs` covers:
  - the first full pull, including note documents;
  - offline note creation and a concurrent online and offline text merge;
  - idempotent retries;
  - re-stamping of normalized values;
  - stale field losses;
  - a folder move that became a cycle;
  - record conflict copies, with no copy for unchanged fields;
  - delete versus later edit, and tombstones;
  - revoked and view-only members, access digests, UUID-only IDs for new entities, and journal pointers kept within their workspace;
  - read-only local replicas for view access.
- `crates/minerva-client/tests/sync.rs` runs a real server over TCP with the embedded local server and the sync worker. It covers personal notes with no server, subscribing, offline edits while the server is down (status `offline`, pending count, refusal to drop unsent changes), a concurrent online edit, an app restart before syncing, convergence with a conflict copy on both sides, an idle second pass, and unsubscribing.
- Not yet exercised: the desktop shell on Windows, large workspaces, and long offline periods with many attachments.

## Desktop client

`cargo test --locked --workspace` runs the `minerva-client` gateway tests against a real server on a random local port. They cover: sign-in through the gateway with the token kept out of the page and no `Set-Cookie`; token storage and bearer forwarding; cross-origin rejection (`Origin`/`Referer` from another connection or the launcher); a byte-exact 1.5 MB binary upload and download; typed note read/save with a 409 conflict; logout revocation on the server; refusing tokens after a server UUID change until it is trusted; and a registry that persists without secrets. Desktop unit tests cover origin mapping and download filename safety (traversal, reserved characters, no overwrite). `cargo test -p minerva-client -- --ignored` performs a real credential-store round trip (passed against KWallet's Secret Service).

Linux end-to-end, on Fedora with WebKitGTK 2.52 in an isolated headless KWin session, driven through AT-SPI and the WebKit inspector, against a disposable server and database:

- Added a server from the launcher, opened it, and signed in through the server's own interface. The account appeared in the registry; the token went to the credential store.
- Live collaboration in both directions: a REST edit from another client appeared in the desktop editor, and a desktop block edit reached the server.
- 2 MB and 9.5 MB attachment uploads from the webview arrived byte-exact (SHA-256 compared). Downloads were saved to Downloads, with a numbered name on repeat.
- Returning to the launcher (menu and shortcut command), signing out (session revoked, sign-in required again), and the server identity-change warning followed by trust.
- A server page posting launcher commands (`remove`) was ignored.
- With the real KWallet Secret Service: still signed in after restarting the application; signing out deleted the stored item.
- The release binary starts and renders the launcher.

Windows: `cargo build`/`clippy --target x86_64-pc-windows-gnu` pass. The release executable uses the GUI subsystem, embeds its icon and version resources, and imports only system DLLs plus the bundled `WebView2Loader.dll`. It has not yet been run on Windows; validate WebView2 rendering, Credential Manager storage, downloads, and the `https://minerva.<host>` origin mapping on Windows 10/11 before release.

## Sharing, publishing, editor, AI, and storage increment

`cargo test --locked --workspace` adds:

- **`tests/sharing.rs`, member shares:**
  - The share grants one note only, with read-only as the default and no implied folder access.
  - Shared notes appear in bootstrap and search.
  - Viewer writes are refused on save and sync, and recipients cannot re-share.
  - An upgrade replaces the existing share, and revocation takes effect at once.
- **`tests/sharing.rs`, share links:**
  - Tokens are returned once and never listed.
  - The cookie is `HttpOnly`, `SameSite=Strict`, and scoped to `/api/v1/shared`, and the share cookie cannot reach member APIs.
  - Guest views omit folder identity.
  - View links read but cannot sync updates or upload, while edit links merge real Yrs CRDT updates and upload.
  - Attachments are limited to the shared note.
  - Peers are anonymized in both directions, and revocation and trash end access immediately.
- **`tests/sharing.rs`, published pages:**
  - Slugs are validated, managers only can publish, and slugs are unique (409 on conflict).
  - Pinned is the default: edits stay hidden until refresh, while live mode follows edits.
  - The rendered page contains no workspace, folder, author, script, or editor content.
  - Only referenced files are served, with an index and an Atom feed.
  - Preview requires a manager, and unpublishing or trashing returns 404.
- **`tests/sharing.rs`, storage cleanup:**
  - Only a file unreferenced by current notes, retained history, and pinned publications, and past the grace period, is listed and deleted.
  - Referenced and recent files are kept even when requested, and the endpoints are administrator-only.
- **`tests/ai_stream.rs`:** mock OpenAI Responses, Anthropic Messages, and OpenAI-compatible streaming providers.
  - Restricted notes never reach the provider, deltas stream, and history and usage are saved.
  - Previous turns are sent as history, and stale revisions are rejected.
  - Dropping a response cancels without saving and frees the user's slot.
- **Render unit tests:** language highlighting with escaping, rejection of injected classes and hostile info strings, JSON diagnostics, attachment URL mapping for members, guests, and public pages, and public-only task markers.

`npm test` adds collaboration tests showing that undo reverts only local changes while keeping a concurrent remote edit (including structural steps), and that IME composition merges correctly with remote edits that arrive during composition.

Browser checks used WebKitGTK MiniBrowser in an isolated headless session against a disposable server, driven through the WebKit inspector:

- Split at the cursor, merge with Backspace and Delete, note-level undo and redo, and Enter inside a code fence inserting a newline.
- Arrow navigation between blocks with position labels, and live-region announcements.
- Rust highlighting and the JSON error message rendered correctly.
- The share dialog shared with a member, created a link shown once, and published (pinned).
- As a view-only guest, the token was stripped from the URL, with read-only content, highlighted code, and live sync.
- An edit-link guest's edit reached the server.
- The published page was static, pinned, and carried metadata.
- The Sharing overview and the storage panel rendered.
- As the member recipient, the note appeared under "Shared with you", read-only, with sharing disabled and the public indicator shown.

The browser run found and fixed two bugs: the app's `history()` function shadowed `window.history` on the guest page, and opening a second share link on `/share` changed only the fragment, so the page now reloads.

Remaining: real screen-reader and IME testing (only synthetic events were used), cross-browser checks beyond WebKit, and load testing of guest polling. Share links do not work inside the desktop client, which keeps no cookies; they open in the system browser.

## Editor writing tools increment

`cargo test --locked --workspace` adds `tests/notes.rs`:

- Covers and tags are validated and normalized on save and in CRDT updates.
- Inline tags are derived, and `#tag` search matches note tags and inline tags.
- Backlinks only include notes the reader may open.
- An uploaded cover appears on the published page, and storage cleanup keeps it.

Render unit tests cover:

- callouts and math, including `\le`, display math, and hostile TeX/MathML (no `href`, `onclick`, or scripts);
- highlight edge cases;
- inline tag rules (not `C#`, `#123`, code, or headings);
- note links dropped for guests and the public.

`npm test` adds `editor-tools.test.js`: formatting toggles, link placeholders, `[[` queries, find/replace, word counts, tag normalization, and HTML-to-Markdown conversion (headings, emphasis, highlights, lists, tables, code, Google Docs styling, script removal).

Browser checks used WebKitGTK MiniBrowser in an isolated session:

- Ctrl+B and the selection toolbar, highlight, and Ctrl+K link creation, with native undo intact.
- A Google Docs paste producing heading, paragraph, and list blocks.
- Callout, inline tag, inline and display math, and a rendered Mermaid diagram.
- Find (4 matches with highlights), replace all with an announcement, and undo restoring the text.
- A tag added from the tag bar and listed in the sidebar, and the tag page.
- The icon picker and a gradient cover.
- A `[[` link picked from the suggestions, navigated, and shown under "Linked from".
- Focus mode on and off with Escape.
- A published page with callout, tag, and math, and no scripts.

The browser run found and fixed three bugs:

- Ctrl+K inside a block also opened global search.
- The tag bar did not refresh after inline tags changed.
- Consecutive task blocks stair-stepped because floats were not contained. This bug predated this change.

Not yet verified: real clipboard pastes from Word, and math rendering in browsers without MathML Core support. Mermaid's ELK layout is intentionally not bundled.

## Export increment

- **Export tests:** `tests/export.rs` checks that:
  - Markdown has front matter, a tight task list, `[[Title]]` note links, absolute attachment URLs, and an optional no-front-matter variant;
  - the zip contains the note and byte-exact files with rewritten links, and excludes files from notes the reader cannot open;
  - standalone HTML has inline styles, embedded images, math, highlights, and tags, and no scripts;
  - the print page carries the `frame-ancestors 'self'` policy and the selected page size, while the app itself still refuses framing;
  - export requires sign-in and read access, and rejects unknown formats.
- **Unit test:** portable link rewriting and file names.
- **Browser check:** the dialog shows the right options per format. PDF export rendered the Mermaid diagram in the hidden frame, reported ready, and called `print()` with the note title as the document title. The HTML export downloaded with the note's file name.

The browser run also fixed a dialog bug where an option label overrode the `hidden` attribute. It also caught a spacing bug: the Markdown title was followed by an extra blank line.

PDF output itself depends on the browser's print engine and was not inspected as a file.

## Journals and templates increment

- **`tests/journal.rs`:**
  - **Setup:** only owners can set up the folders, and invalid dates are rejected.
  - **Entries:** one entry per date, reopened rather than duplicated, and members need folder access.
  - **Templates:** placeholders are filled with new block IDs, templates apply to empty notes only and must come from the Templates folder, and readers need access to the template.
  - **Journal settings:** owners only, and the template is used for new days.
  - **Unfinished tasks:** tasks completed later, repeated, already present in the note, or from later entries are excluded. Repeats keep their earliest date, and task Markdown is rendered.
  - **Completing:** marks every open occurrence, and readers only see tasks from entries they may open.
- **Unit tests:** weekday and long-date computation, task parsing and matching, and the placeholder rendering (kept literal in code, omitted publicly).
- **Browser check:**
  - Created a template from the Templates page, with a badge on the template note.
  - Set it as the journal template, saw days with entries marked and today ringed on the calendar, and opened a day that used the template.
  - The live list showed exactly the expected tasks, and ticking one updated the list and its source entries.
  - "Fill from template" appeared on an empty note and filled it with the local date, and typing hid the button.

The browser run fixed one bug: the calendar's blank cells reused the app's empty-state style and broke the grid. It also replaced the 📅 icon, which shows a misleading date in some emoji fonts.

## PDF styling and empty blocks increment

Real PDFs were generated with headless Chromium in forced dark mode and rendered to images for inspection. Exports now embed a self-contained stylesheet that is always light: typography, title block, code with highlighting, tables, callouts, highlights, tags, ☐/☑ task markers, consecutive list blocks joined into one list, and print rules for margins, page breaks, preserved backgrounds, and external link URLs. Note links become plain text, and the live task list is omitted.

Defects found by inspecting the PDFs and fixed:

- Dark-mode colors leaked into printed documents.
- `math[display="block"] { display: block }` broke Chromium's MathML layout (also in the app and on published pages).
- Without a math font, display math was cramped. A math font stack fixes this, with STIX Two Math as the first choice.
- Stubbing ELK broke every Mermaid diagram, because Mermaid 12 defaults to the ELK layout. Diagrams now use `layout: "dagre"`, and Mermaid's error graphic is removed when a diagram fails.

Empty blocks render as one line of blank space. Only a new note's single empty block shows "Write something…".

## Two-factor authentication increment

- **Unit tests:** RFC 6238 SHA-1 test vectors, Base32 round trip, replay rejection, the one-step drift window, and recovery-code formatting.
- **`tests/twofactor.rs`:**
  - **Setup:** a valid code is required, the URI and QR code are produced, and other sessions are signed out on enable.
  - **Sign-in:** the password alone gives no session, the enabling code cannot be replayed, challenges are single use, and forged challenges fail.
  - **Limits:** five wrong codes end a challenge, which exposed and fixed a bug where failed attempts were discarded with the request.
  - **Recovery codes:** each works once (in any case or spacing), and new codes require the app.
  - **Disabling and reset:** disabling needs password and code, and the admin listing and reset work.
- **`minerva-client` gateway test:** the first step stores no credential, the second step's token is captured and hidden from the page, and `login_with_code` works with a recovery code.
- **Browser check:**
  - Setup from Account security. The QR code was decoded with jsQR to the expected `otpauth://` URI and secret.
  - Enabled with a computed code, and ten recovery codes were shown.
  - Signed out and signed back in through the code step: a wrong code was rejected and the right code signed in.
  - The admin 2FA column shows the status.

## Block nesting increment

- **Render unit test:** indentation levels, dedenting (four spaces stay a paragraph), indented fenced code, the six-level cap, and blank blocks.
- **Browser check:**
  - A list continued on Enter, Tab nested a block under its parent, and a second Tab was refused beyond one level deeper than the block above.
  - Enter kept the level, and Enter on an empty item stepped out.
  - Backspace at the start of the text stepped out level by level, then removed the bullet.
  - Tab and Shift+Tab work on paragraphs, and bullet styles alternate by level.
- **Export check:** Markdown export produced a nested list, and the HTML export carried the indent classes.

## Security review increment

A manual review covered authentication and sessions, CSRF, authorization on every route, rendering and sanitization, share links, published pages, uploads, exports, the desktop gateway, and two-factor authentication. Fixes, with `tests/security.rs`:

- **Render cache memory exhaustion:** any member, or any holder of a share link, could fill the render cache with up to 4,096 entries of up to 2 MB each. The cache now has a 32 MiB byte budget and caches only blocks up to 16 KiB, and `/render` refuses input over 100,000 bytes, matching blocks.
- **Lockout by anyone:** the global limit of 20 sign-ins per minute let any anonymous client lock every user out. It is now 10 attempts per account and 300 per server per minute, and attacking one account leaves others able to sign in.
- **Account discovery:** unknown or disabled accounts returned before password hashing, revealing valid emails through timing. They now verify against a dummy Argon2 hash.
- **Stale 2FA sign-ins:** pending second-factor challenges survived administrator account changes, such as a password reset. They are now cancelled.

Reviewed and unchanged (by design, or documented):

- AI provider endpoints are unrestricted for administrators, as requested.
- TOTP secrets are stored in the database like password hashes.
- MCP credentials remain valid after enabling 2FA; they are read-only and individually revocable.
- Member-share recipients can read the note's saved history.
- Without `MINERVA_PUBLIC_URL`, feed links use the request's Host header.
