# Setup and operations

This guide covers installation, configuration, operations, and detailed behavior. For a product overview, start with the [README](../README.md). Minerva is an early-access project intended for evaluation and small workspaces.

## Start

Install Rust and a C toolchain (SQLite is compiled into the binary). This checkout was verified with Rust 1.96.0.

```sh
# Set your own initial administrator credentials; only needed on a new database.
export MINERVA_ADMIN_EMAIL='you@example.com'
read -rs MINERVA_ADMIN_PASSWORD
export MINERVA_ADMIN_PASSWORD
cargo run --locked -p minerva-server
```

Type a password of at least 12 characters at the hidden prompt and press Enter. Open **http://127.0.0.1:3000**. Sign in with those credentials. A private workspace, Getting started folder, and welcome note are created automatically. Subsequent launches use existing accounts; changing bootstrap environment variables does not reset them.

```sh
npm ci
npm run build
npm test
cargo test --locked --workspace
cargo fmt --all --check
node --check web/app.js
cargo build --locked -p minerva-server
node scripts/protocol-smoke.mjs  # starts an isolated local server and tests real MCP/Yjs clients
cargo build --locked --release -p minerva-server
./target/release/minerva-server
```

## Docker

The image contains the server with the web app embedded, built from source in three stages: the browser bundles (Node), the server (Rust), and a slim runtime. The runtime runs as a non-root user with a read-only root filesystem, no capabilities, and a data volume.

```sh
cp .env.example .env            # set administrator credentials and your URL
docker compose up -d --build    # http://127.0.0.1:3000 (bound to localhost only)
```

`.env.example` lists every setting with production values; edit the ones marked CHANGE. `COMPOSE_PROFILES` in `.env` chooses how Minerva is reached, so `docker compose up -d` alone starts the right services: `https` (Caddy) or empty (Minerva only, for your own reverse proxy).

For a local HTTP trial, set `COMPOSE_PROFILES=`, `MINERVA_PUBLIC_URL=http://localhost:3000`, and `MINERVA_SECURE_COOKIE=0` in `.env`.

For a public domain with automatic HTTPS (Caddy and Let's Encrypt):

1. Point DNS at the host and open ports 80 and 443.
2. In `.env`, set `MINERVA_DOMAIN`, `MINERVA_PUBLIC_URL=https://…`, and `MINERVA_SECURE_COOKIE=1`.
3. Start with `docker compose --profile https up -d --build`.

To publish through a [Cloudflare Tunnel](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/) instead, with no open ports and no public IP address, use `deploy/compose.yaml`, which runs the published image:

1. In the Cloudflare Zero Trust dashboard, create a tunnel (Networks → Tunnels) and copy its token.
2. Add a public hostname to the tunnel, for example `notes.example.com`, with service `http://minerva:3000`.
3. Copy `deploy/compose.yaml` and `deploy/.env.server.example` (as `.env`) to the server. In `.env`, set `CLOUDFLARE_TUNNEL_TOKEN`, `MINERVA_PUBLIC_URL=https://notes.example.com`, and `MINERVA_SECURE_COOKIE=1`.
4. Uncomment the `cloudflared` service in `compose.yaml`, then run `docker compose up -d`.

Cloudflare terminates HTTPS and can read the traffic it forwards.

To use your own reverse proxy instead, forward to `127.0.0.1:3000` and keep `MINERVA_SECURE_COOKIE=1` when serving over HTTPS.

**Data.** The database and its `.ai-key` file live in the `minerva_minerva-data` volume. To back up, stop the container and copy both, for example `docker compose stop minerva && docker run --rm -v minerva_minerva-data:/data -v "$PWD":/backup debian:bookworm-slim tar czf /backup/minerva-backup.tgz -C /data . && docker compose start minerva`.

**Prebuilt image.** Every version tag and every push to `main` publishes the server image to `ghcr.io/akaped/minerva` (`:latest` is the newest release, `:main` the newest commit). Set `MINERVA_IMAGE=ghcr.io/akaped/minerva:latest` in `.env` and run `docker compose pull && docker compose up -d` to skip building.

**Upgrades.** Run `git pull && docker compose up -d --build`, or `docker compose pull && docker compose up -d` with the prebuilt image. The administrator variables only matter on the first start; you can remove the password from `.env` afterwards.

## Desktop client

```sh
cargo run -p minerva-desktop        # Linux; needs WebKitGTK 4.1 and GTK 3 development packages
scripts/package-desktop.sh          # dist/ packages for Linux (tar.gz) and Windows (zip, cross-compiled)
```

The desktop client works on its own: it runs a Minerva server in-process for personal notes, with no setup and no network needed. It can also keep **offline copies** of workspaces from your servers. Edits made offline are stored on the computer, sent when the server is reachable again, and merged with other people's changes; conflicts are listed for review. It also opens each server's own interface online, and stores session tokens in Windows Credential Manager or the Linux Secret Service. See [docs/desktop.md](desktop.md) and [docs/sync.md](sync.md).

Web assets are embedded at compile time. Run `npm run build` after changing browser source, then restart through `cargo run`. The generated `web/dist/app.js` is included, so running an unchanged checkout only requires Rust. Rebuild it when changing source.

## Optional learning workflows

The [Superhuman Learning plugin](../plugins/server/superhuman-learning/README.md) adds `/summary`, `/wisdom`, `/youtube`, and `/transcribe`. Python, yt-dlp, and a transcription profile are optional capabilities, not core requirements. Use its installation guide for the optional Docker image and provider settings.

## Detailed features

- Server → workspaces → folders → notes, with stable UUID identities.
- Workspace and folder creation, folder renaming, note creation and title editing.
- Administrator-created accounts that administrators can edit (name, email, password, administrator role), disable, or delete (owned workspaces pass to the administrator), self-service password changes (people an administrator adds choose their own password at their first sign-in; their My Workspace opens by default), workspace-creation privileges, and workspace access: members added to a workspace can edit (or view) every folder and note in it.
- Deletion: notes (from the trash) and folders by their editors; workspaces by their owners only. Every account has a private My Workspace that cannot be shared, renamed, or deleted. Administrators cannot open other people's workspaces; they add, rename, and share their own in Server administration.
- Plugins: app color themes, public website themes, and 20 PDF templates (company report, meeting minutes, proposal, academic paper, security assessment, and more) in [plugins/](../plugins/README.md), loaded from a folder at run time.
- Note metadata (author, company, version, ...) that PDF templates place on covers, headers, footers, and tables, and that `{{key}}` placeholders insert into exports.
- Inline Markdown blocks:
  - Click to edit; blur saves.
  - Enter splits at the cursor (inside code fences it adds a newline), and Backspace/Delete at the edges merge blocks.
  - Shift+Enter keeps multiple lines in one block, and arrow keys move between blocks.
  - Tab nests a block under the one above (up to six levels) and Shift+Tab moves it out. Lists continue on Enter.
  - Undo/redo reverts only your own changes, IME composition is safe during live collaboration, and the editor works with screen readers.
- Headings, emphasis, lists, links, quotes, fenced code with server-side syntax highlighting (and JSON error reporting), Markdown tables, task toggles, dividers, block ordering and removal.
- Writing tools:
  - Formatting shortcuts (Ctrl+B/I/E/K, Ctrl+Shift+H to highlight, Ctrl+Shift+X to strike through) and a floating toolbar for selected text.
  - Pasting from web pages, Google Docs, or Word converts formatting to Markdown.
  - Find and replace in a note (Ctrl+F), and focus mode (Ctrl+Shift+F).
  - Word count and reading time.
- Rich blocks:
  - Callouts (`> [!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]`, `[!CAUTION]`), `==highlights==`, and math (`$…$`, `$$…$$`) rendered to MathML on the server.
  - Mermaid diagrams, rendered in the app and shown as source on published pages.
- Note covers (gradients, uploaded images, or HTTPS images) and an icon picker.
- Journals and templates:
  - One journal note per date, with a calendar, Today, previous/next day, and an optional template for new days.
  - Templates with `{{date}}`, `{{weekday}}`, `{{long_date}}`, and `{{title}}` placeholders, offered as "Fill from template" on empty notes.
  - `[[uncompleted_journal_tasks]]` shows open tasks from earlier entries without duplicates; ticking one completes it in its entries.
- Tags and links between notes:
  - Note tags plus inline `#tags`, a tag list in the sidebar, tag pages, and `#tag` search.
  - `[[` links to other notes that survive renames, with a "Linked from" backlinks list. Searchable `/` commands with keyboard navigation; multiline import/paste preserves fenced code and tables.
- Image upload/paste and general file attachments with authenticated previews/downloads, opaque UUID storage keys, size quotas, and originals retained for note history. The Files dialog lists retained uploads for download or reinsertion.
- Live collaborative editing using Yjs in the browser and Yrs in Rust: concurrent text merging, collaborator presence, stable block IDs, and reconnect retries.
- Structured databases inside folders: typed text, number, checkbox, date, select, and URL properties; inline cell editing; filtering and sorting; table and select-grouped board views; recoverable record archive; editable properties and permanent deletion of records, properties, and databases; live databases inside notes (`/` → Database).
- Light, Dark, and System themes, remembered in the browser.
- AI chat with OpenAI, Anthropic, DeepSeek, or any OpenAI-compatible API, limited to the workspace, folder, or note you choose and to what you may read:
  - Ask AI in a note opens the chat in a tab beside it, about that note.
  - Replies appear as note blocks. Add one block or the whole reply to the note, save the reply as a new note, or copy its Markdown.
  - Providers can be edited by administrators, who can also allow web search for OpenAI and Anthropic providers: the model then researches the web, and the pages it cites are listed under its answer.
  - Conversations can be deleted one by one or all at once.
- Built-in read-only MCP over stateless Streamable HTTP, with workspace-scoped credentials, expiry, revocation, note search/read, and typed database queries. See [MCP setup](mcp.md).
- Sharing: share a note with a member (view or edit), or create view/edit links that work without signing in, with revocation. Publish notes as clean public pages at `/shared/{slug}`, pinned or live, with an index and an Atom feed, or group them into sites with their own paths, such as a `/blog` or a `/wiki`. See [Sharing](sharing.md).
- Attachment storage administration with reference-aware cleanup of unreferenced files.
- Search across accessible note titles and text, grid/list views, favorites, recoverable trash.
- Export a note as PDF (print-designed, A4 or Letter, with cover, highlighted code, math and diagrams, via the system's "Save as PDF"), Markdown with optional front matter, Markdown + attachments (.zip, ready for Obsidian or Joplin), or a self-contained HTML file. Markdown import.
- The last 50 saved note revisions, with restore.
- Two-factor authentication with authenticator apps (TOTP), with recovery codes and administrator reset.
- SQLite persistence, Argon2id password hashes, seven-day revocable sessions, server-enforced authorization, sanitized Markdown HTML, request-size limits, CSRF header checks, and optimistic concurrency.

Favorites belong to the note, so they are shared among its readers. Open notes synchronize automatically: edits are sent after a short debounce and active clients exchange CRDT updates every 650 ms. Presence expires after 15 seconds without a heartbeat. This is online collaboration using HTTP polling, not WebSocket transport. Pending changes retry after temporary connection loss while the page remains open; there is no durable offline queue across page reloads. Export an unsaved draft before closing the browser. History keeps up to 50 checkpoints, grouping frequent live edits into roughly 30-second intervals.

Structured records use optimistic revision checks rather than text CRDTs; concurrent edits to the same record produce a visible conflict. Local cell writes are serialized. Database views refresh every 2.5 seconds when there are no active/pending cell edits. Invalid or conflicting values remain visible until corrected or explicitly discarded with Reload records.

Import preserves each Markdown source line as a block. Multiline structures such as fenced code and tables need an explicit multiline block for correct rendering. Existing canvas coordinates remain compatible in stored notes, but the canvas UI is no longer offered and further canvas work is out of scope.

## Configuration

| Variable                 | Default               | Purpose                                                                                                       |
| ------------------------ | --------------------- | ------------------------------------------------------------------------------------------------------------- |
| `MINERVA_BIND`           | `127.0.0.1:3000`      | Listen address                                                                                                |
| `MINERVA_DATABASE`       | `data/minerva.db`     | SQLite file                                                                                                   |
| `MINERVA_ADMIN_EMAIL`    | `admin@minerva.local` | Initial admin only                                                                                            |
| `MINERVA_ADMIN_PASSWORD` | none                  | Required on initial setup, minimum 12 characters                                                              |
| `MINERVA_SECURE_COOKIE`  | unset                 | Set to `1` behind HTTPS                                                                                       |
| `MINERVA_PUBLIC_URL`     | HTTP listen address   | Exact allowed MCP browser origin and base for published-page feed links, e.g. `https://notes.example.com`     |
| `MINERVA_SITE_NAME`      | `Published notes`     | Title of the published pages index and feed                                                                   |
| `MINERVA_TOTP_ISSUER`    | `Minerva`             | Name shown in authenticator apps                                                                              |
| `MINERVA_SMTP_HOST`      | none                  | SMTP server; with `MINERVA_MAIL_FROM`, turns email on (see [Email](#email))                                   |
| `MINERVA_SMTP_PORT`      | by security           | 587 for `starttls`, 465 for `tls`, 25 for `none`                                                              |
| `MINERVA_SMTP_SECURITY`  | `starttls`            | `starttls`, `tls`, or `none` (only for a relay on a private network)                                          |
| `MINERVA_SMTP_USERNAME`  | none                  | SMTP sign-in; empty sends without authentication                                                              |
| `MINERVA_SMTP_PASSWORD`  | none                  | SMTP password or app password                                                                                 |
| `MINERVA_MAIL_FROM`      | none                  | Sender, e.g. `Minerva <notes@example.com>`                                                                    |
| `MINERVA_PLUGINS`        | `plugins`             | Folder with extra or replacement themes; read on each request (see [plugins/README.md](../plugins/README.md)) |
| `MINERVA_PUBLIC_THEME`   | none                  | Public theme for pages under `/shared/`, e.g. `paper`                                                         |

Run **one process per database**. The current store is a transactionally persisted JSON snapshot inside SQLite, loaded in memory; it is suitable for evaluation and small workspaces. It needs normalized tables, migrations, pagination, and indexed search before large deployments. Argon2 and database work run on Tokio's blocking pool; the store serializes operations with a mutex.

For network access, terminate TLS at a reverse proxy and set `MINERVA_SECURE_COOKIE=1`. Do not expose the development listener directly to the internet. No CORS origins are allowed by default. Published pages under `/shared/` and site paths, and share links, are intentionally reachable without signing in. Two-factor authentication with authenticator apps is available from Account security. Audit logs, SSO, and end-to-end encryption are not implemented. Sign-in attempts are limited to 10 per account and 300 for the whole server per minute, and unknown accounts take as long to reject as wrong passwords. Add a reverse-proxy rate limit for public deployments.

### Email

With `MINERVA_SMTP_HOST` and `MINERVA_MAIL_FROM` set, the server sends email. Links in emails use `MINERVA_PUBLIC_URL`, so set it too. The server sends:

- **Invitations.** A new account gets a link to choose its password, valid for 7 days. With email on, the administrator may leave the temporary password empty.
- **Password resets.** "Forgot your password?" on the sign-in page sends a link valid for one hour. The answer is the same whether or not the address has an account. Using the link signs the account out everywhere.
- **Access.** When someone is given access to a workspace or a folder, or their role changes.
- **Assignments.** `/assigntouser` writes `@person@example.com` in a block. When a block gains an assignment, that person gets the block and a link to the note, if they can open it. Assigning yourself sends nothing.
- **Reminders.** `/remindme` picks a date and time, writes `⏰2026-10-27 09:00` in the block, and emails you the block at that time. Remove the `⏰` from the block to cancel it.

Messages wait in an outbox stored with the data, so a restart does not lose them. Failed sends are retried with backoff for about a day. Errors are written to the server log. Without email, `@` assignments still show in notes, and `/remindme` is not offered.

To back up: stop the server and copy the database, or use SQLite's online backup API while it runs. Do not copy only the main file from a running WAL-mode database. Restore with the server stopped. Database file access exposes private notes and password hashes, so protect the file and its backups.

## Project

- `crates/minerva-core`: serializable Rust wire models shared by server and clients.
- `crates/minerva-client`: native client transport, server connection registry, OS credential storage, and the authenticating webview gateway.
- `crates/minerva-desktop`: Windows/Linux desktop application (system webview, launcher, menus, downloads).
- `crates/minerva-server`: Axum API, authorization, persistence, rendering, embedded UI.
- `web`: browser interface and Yjs collaboration provider; esbuild emits `web/dist/app.js`.
- `docs/architecture.md`: AFFiNE study, architectural decisions, client design, and feature roadmap.
- `docs/api.md`: versioned API and permission contract.
- `docs/verification.md`: checks performed and remaining validation.
- `docs/mcp.md`: client configuration, tools, credential scope, and security model.
- `docs/desktop.md`: desktop client build, packaging, data locations, and security model.
- `docs/sync.md`: offline copies, the sync protocol, merge rules, and limits.
- `docs/sharing.md`: member shares, share links, published pages, and the cross-server sharing design.
- `TODO.md`: requested follow-up features, including images/files, contextual LLM chat, editor improvements, and public/member sharing.
- `THIRD_PARTY_NOTICES.md`: licenses for bundled browser dependencies.
- `CHANGELOG.md`: what changed in each version. Add your changes under **Unreleased**.

### Releasing

1. Commit your work, with its changes listed under **Unreleased** in `CHANGELOG.md`.
2. Run `scripts/release.sh 0.3.0`. It sets the version in `Cargo.toml` and `Cargo.lock`, turns **Unreleased** into `## [0.3.0] - <date>`, commits, and tags `v0.3.0`.
3. Push both with `git push origin main v0.3.0`.

The tag starts two workflows. **Release** builds the desktop app and the server for Linux (`.tar.gz`), Windows (`.zip`) and macOS (universal: a `.zip` with `Minerva.app`, and a `.tar.gz` server). It publishes them on GitHub Releases with a `SHA256SUMS.txt` file, using the version's changelog section as the release notes. **Docker image** publishes `ghcr.io/<owner>/minerva:0.3.0`. Run `scripts/package-server.sh` or `scripts/package-desktop.sh` to build the same packages locally in `dist/`.

This is independently authored code. No AFFiNE implementation code or artwork was copied.
