# Minerva API v1

Base URL: `/api/v1`. JSON request/response bodies. Errors are `{"error":"message"}` with an appropriate HTTP status. Request bodies are limited to 2 MiB. GET endpoints are read-only. All other methods require `X-Minerva-Client: 1`, including login. Unknown routes return 404.

## Authentication

`POST /login` accepts `{ "email": "...", "password": "..." }` and returns `{ "user": {...}, "token": "..." }`. Browser sessions use the HttpOnly, SameSite=Strict `minerva_session` cookie. Native clients can send `Authorization: Bearer <token>`. Tokens expire after seven days; only their SHA-256 hashes are persisted. `POST /logout` revokes the presented token and clears the cookie.

Use HTTPS for remote clients. `MINERVA_SECURE_COOKIE=1` enables the cookie's Secure flag. CORS is not enabled; browser clients are served from the same origin. The custom mutation header and JSON body prevent ordinary cross-site form submissions. Authentication tokens must never be placed in URLs.

| Method | Path                   | Body / result                                                                                                                                                                                                                                                                                                                      |
| ------ | ---------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| GET    | `/health`              | Public status, API version, persistent server ID                                                                                                                                                                                                                                                                                   |
| POST   | `/login`               | Email and password; session/token                                                                                                                                                                                                                                                                                                  |
| POST   | `/logout`              | Revoke current session                                                                                                                                                                                                                                                                                                             |
| GET    | `/bootstrap`           | Current user, accessible workspaces/folders, note metadata (each with a `sharing` summary), notes shared directly with the user (`shared_notes`), effective folder roles                                                                                                                                                           |
| POST   | `/search`              | `{query}`; up to 50 accessible non-trashed note matches by title/content. A query starting with `#` matches note tags and inline tags by prefix                                                                                                                                                                                    |
| POST   | `/workspaces`          | `{name}`; creates workspace owned by actor and General folder                                                                                                                                                                                                                                                                      |
| POST   | `/folders`             | `{workspace_id,name,parent_id?}`; administrator or workspace owner. `parent_id` nests the folder inside another folder of the same workspace, up to 8 levels. Folders carry `parent_id` when nested                                                                                                                                |
| PATCH  | `/folders/{id}`        | Any of `{name, parent_id}`; administrator or workspace owner. `parent_id: null` moves the folder to the top level; moving a folder into itself or a descendant is refused                                                                                                                                                          |
| DELETE | `/folders/{id}`        | Folder editor; deletes the folder and everything in it, including subfolders. Returns `{ok, deleted_notes}`                                                                                                                                                                                                                        |
| DELETE | `/workspaces/{id}`     | Administrator, or the owner while the workspace is personal (no other member has workspace or folder access); deletes everything in it. `bootstrap.workspaces[].can_delete` says whether the caller may                                                                                                                            |
| POST   | `/notes`               | `{folder_id,title}`; requires editor access                                                                                                                                                                                                                                                                                        |
| GET    | `/notes/{id}`          | Note with blocks, sanitized `rendered` HTML array, `cover`, `tags`, derived `inline_tags`, the caller's effective `role`, a `sharing` summary, and `backlinks` (notes the caller may open that link here)                                                                                                                          |
| PUT    | `/notes/{id}`          | `SaveNote` below; editor access and matching revision required                                                                                                                                                                                                                                                                     |
| PATCH  | `/notes/{id}`          | `{folder_id}`: moves the note to another folder of the same workspace. Folder editor on both folders; a share recipient cannot move a note                                                                                                                                                                                         |
| PUT    | `/notes/{id}/metadata` | `{metadata:{key:value}, print_template}`; note editor. Keys are normalized to lowercase `[a-z0-9_]` and must start with a letter; up to 50 fields of 4,000 characters; empty values are dropped. `print_template` is an installed PDF template or `""`. Saved separately from the note's text, so content saves never overwrite it |
| DELETE | `/notes/{id}`          | Folder editor; the note must already be in the trash. Also removes its history, files, shares, and published page                                                                                                                                                                                                                  |
| GET    | `/notes/{id}/history`  | Up to 50 previous complete note snapshots, oldest first                                                                                                                                                                                                                                                                            |
| POST   | `/render`              | `{markdown}` → `{html}`; authenticated, sanitized Markdown render with code highlighting (`hl-*` classes) and JSON diagnostics (`p.code-diagnostic`)                                                                                                                                                                               |
| GET    | `/admin`               | Users and grants; server administrator only                                                                                                                                                                                                                                                                                        |
| POST   | `/users`               | `{name,email,password,can_create_workspace}`; creates non-admin user                                                                                                                                                                                                                                                               |
| PATCH  | `/users/{id}`          | Optional `{disabled,can_create_workspace,password}`; admin cannot modify self here                                                                                                                                                                                                                                                 |
| PUT    | `/workspace-access`    | `{workspace_id,user_id,role}` where role is `viewer`, `editor`, or `none`; grants every folder of the workspace, including folders added later. `GET /admin` lists these as `workspace_grants`                                                                                                                                     |
| PUT    | `/permissions`         | `{folder_id,user_id,role}` where role is `viewer`, `editor`, or `none`; a single-folder grant. The effective role is the higher of workspace and folder access                                                                                                                                                                     |

## Save contract

```json
{
  "title": "A useful thought",
  "icon": "✳",
  "revision": 3,
  "favorite": false,
  "trashed": false,
  "blocks": [
    {
      "id": "stable-unique-block-id",
      "markdown": "## Hello",
      "x": 40.0,
      "y": 40.0
    }
  ]
}
```

`cover` (optional) is empty, `gradient:1`–`gradient:8`, `/api/v1/attachments/{id}`, or an HTTPS URL of at most 500 bytes. `tags` (optional) holds up to 20 tags, normalized to lowercase without `#`, each 1–40 letters, digits, `-`, `_`, or `/` with at least one letter. Inline `#tags` in block text are derived and returned as `inline_tags`. Links to other notes use the Markdown destination `#note/{id}`; guests and published pages receive their text without the link.

The note's folder is immutable through this endpoint. The server increments the revision after a successful save and returns the updated note. Maximum 2,000 blocks, 100,000 bytes per block, title 1–200 bytes, icon at most 24 bytes, unique nonempty block IDs at most 80 bytes, and finite coordinates within ±100,000. Overall request-size limit applies too. Names and titles should be kept short; server limits are UTF-8 byte lengths.

HTTP 409 means another client saved first. Preserve/export the local draft, fetch the latest note, and explicitly reconcile before retrying. Do not automatically overwrite the new revision with the old payload. To restore history, submit a previous snapshot's content with the **current** note revision. Trash/restore uses the same PUT with `trashed` toggled; no permanent deletion endpoint exists.

Authorization is evaluated against current state on every request. Treat 401 as requiring sign-in and 403 as loss of permission. The server uses a stable UUID per database; future clients should namespace cached entities by that UUID. API v1 supports independently authenticated servers and live CRDT note synchronization, but no federation, WebSocket event feed, attachment transfer, or durable offline synchronization.

## Live collaboration

`POST /notes/{id}/sync` accepts `{update?, client_id?, active_block?}`. `update` is a base64 Yjs update-v1 message; omission/empty string performs read synchronization and a presence heartbeat. Read synchronization requires folder read access. Nonempty updates require editor access. Body limits still apply, and malformed or oversized CRDT updates are rejected.

Response: `{update, note, peers, can_edit}`. `update` contains the merged encoded document state; `note` contains the validated canonical projection and sanitized rendered HTML. `peers` contains user ID, display name, per-tab client ID, active block, and heartbeat timestamp. Presence expires after 15 seconds and is permission-filtered. Duplicate CRDT updates are idempotent. The current browser polls every 650 ms and sends pending changes after a 180 ms debounce. Full CRDT states are returned; this is not a WebSocket or offline-sync API.

Yjs/Yrs roots: `title` text; `order` string-ID array; `block:<id>` text; `meta` map with `icon` string, `favorite` boolean, `trashed` boolean, and optional `cover` string and `tags` string array (validated as for saves). Folder IDs, user roles, and identity are not editable through CRDT payloads. Limits: 2,000 materialized blocks, 100,000 bytes per block, 200-byte title, 24-byte icon, encoded incoming update 1.5 MB, stored encoded state 2.5 MB. Exceeding a limit rejects the update and keeps the client's unsaved changes for export/recovery.

## Structured databases

| Method | Path                            | Contract                                                                                                                                                                                          |
| ------ | ------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| POST   | `/databases`                    | `{folder_id,name,columns?}`; creates default Name/Status columns if omitted                                                                                                                       |
| GET    | `/databases/{id}`               | Complete schema and records; folder read permission required                                                                                                                                      |
| PATCH  | `/databases/{id}`               | `{revision,name?,columns?}`; matching schema revision and folder editor permission. Values that no longer fit the new schema (a removed property or select option, or a changed type) are cleared |
| DELETE | `/databases/{id}`               | Folder editor; deletes the database and its records permanently                                                                                                                                   |
| POST   | `/databases/{id}/rows`          | `{cells:{property_id:value}}`; returns record with revision 1                                                                                                                                     |
| PATCH  | `/databases/{id}/rows/{row_id}` | `{revision,cells?,archived?}`; merges only supplied cells, increments row revision                                                                                                                |
| DELETE | `/databases/{id}/rows/{row_id}` | Folder editor; deletes the record permanently (archiving is the recoverable option)                                                                                                               |

Column shape: `{id,name,kind,options?}` where kind is `text`, `number`, `checkbox`, `date`, `select`, or `url`. Null/missing cells are allowed. Dates must be real calendar dates in YYYY-MM-DD form. Select values must be in the property's declared options. URLs must start with HTTP(S); values are displayed as text inputs, not trusted HTML. Schemas support 1–32 columns and databases up to 10,000 records. Schema changes clear the values they make invalid instead of being rejected; each cleared record's revision increases. The UI can add, edit (name, type, options), and delete properties, rename and delete databases, and archive or delete records.

**Databases in notes.** A block containing `[[database:{id}]]` (or `[[database:{id}:board]]`) shows that database inside the note. For signed-in members the server renders `<span class="db-embed db-id-{id} db-view-{view}">`, which the app fills with the current records using the normal database endpoints, so folder permissions apply: readers without access see a notice, viewers see it read only, and editors can edit cells and add records in place. The view refreshes every few seconds. Share-link guests and published pages only see a short placeholder, never the records. Exports (Markdown, HTML, PDF, ZIP) include a snapshot table when the exporting member can read the database. In the editor, type `/` and choose **Database (live)**, or copy the code from the database page with **Use in a note**.

Records use optimistic concurrency. Local browser writes are serialized, but stale writes from other clients return 409 rather than overwriting their changes. Archiving is recoverable; no permanent deletion route is exposed. Database metadata is included in `/bootstrap`, filtered by current folder permissions.

## MCP credentials

| Method | Path                    | Contract                                                                          |
| ------ | ----------------------- | --------------------------------------------------------------------------------- |
| GET    | `/mcp-credentials`      | List current user's credential metadata; no raw tokens or hashes                  |
| POST   | `/mcp-credentials`      | `{workspace_id,name,expires_days}`; 1–90 days, default 30; returns raw token once |
| DELETE | `/mcp-credentials/{id}` | Revoke one of the current user's credentials immediately                          |

Browser mutation headers and session authentication apply to credential management. MCP calls use the separate `/api/workspaces/{workspace_id}/mcp` endpoint with the issued bearer token. Browser session tokens cannot authenticate MCP, and MCP tokens cannot authenticate the browser API. See `mcp.md`.

### Note attachments

- `POST /api/v1/notes/{id}/attachments?filename={URL-encoded-name}` accepts raw bytes with `X-Minerva-Client: 1`, a login session, and folder editor access. Returns 201 with `id`, sanitized `filename`, `media_type`, and `size`. Trashed notes reject new uploads.
- `GET /api/v1/notes/{id}/attachments` lists all retained uploads for an accessible note, including files no longer referenced by current blocks.
- `GET /api/v1/attachments/{id}` downloads the original bytes with attachment disposition and an encoded filename.
- `GET /api/v1/attachments/{id}/preview` displays signature-identified PNG/JPEG/GIF/WebP images only. Other formats, including SVG and HTML, are download-only. Signature detection is not full image validation or malware scanning.

Current folder permissions are checked on every operation. Login credentials work; MCP tokens do not authorize these endpoints. Files use UUID primary keys in a separate SQLite blob table, never uploaded filesystem names. Limits: 10 MiB per upload, 100 MiB per note, 1 GiB per server. Uploads and note edits are separate operations; interrupted insertions can be recovered from the note's Files dialog. No automatic deletion: retained blobs protect history and can be reinserted. Future administrator cleanup must account for revision and share references.

Markdown images support HTTP/HTTPS image URLs and uploaded attachment URLs, for example `![Photo](/api/v1/attachments/{id})`; image destinations are automatically rendered through `/preview`. Regular Markdown links still download originals. External images are fetched directly by the browser, not proxied by the Rust server. HTTPS deployments remain subject to browser mixed-content restrictions for HTTP images. Server filesystem paths are not public URLs; upload the file and use its attachment URL.

## Sharing and publishing

Effective note access is the caller's folder role, raised by a direct member share. Note reads, saves, history, sync, attachments, search, and note-scoped AI chat all use it. Only folder-level editors (folder editors, workspace owners, administrators) manage sharing. See `sharing.md` for the model.

| Method | Path                            | Contract                                                                                                                                                                                                                                                                                                                                                                                                                 |
| ------ | ------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| GET    | `/sharing`                      | `{managed, shared_with_me}`: notes your folders share or publish, and notes shared directly with you                                                                                                                                                                                                                                                                                                                     |
| GET    | `/notes/{id}/shares`            | `{shares, publication}`; member shares include the recipient's name and email; raw link tokens are never returned                                                                                                                                                                                                                                                                                                        |
| POST   | `/notes/{id}/shares`            | `{kind:"member", email, role?}` or `{kind:"link", role?}`; `role` is `viewer` (default) or `editor`. Links return `{share, token}` once. Re-sharing to a member replaces their role. Up to 50 shares per note; trashed notes are refused                                                                                                                                                                                 |
| DELETE | `/notes/{id}/shares/{share_id}` | Revoke immediately                                                                                                                                                                                                                                                                                                                                                                                                       |
| PUT    | `/notes/{id}/publication`       | `{slug, site_id?, mode?, description?, refresh?}`; `site_id` is a site of the note's workspace, `""` for `/shared/`, and omitted keeps the current site; `mode` is `pinned` (default) or `live`; `refresh` takes a new pinned snapshot. Slugs: 1–80 of `[a-z0-9-]`, no leading/trailing hyphen, unique within the site (409 otherwise). Description ≤ 300 characters. The response includes `path`, such as `blog/hello` |
| POST   | `/sites`                        | Workspace owner or admin: `{workspace_id, path, title?, layout?, description?, theme?}`; `theme` is an installed public theme ID or `""` for the default look; `layout` is `blog` (default) or `wiki`; paths are unique, up to 40 of `[a-z0-9-]`, and not reserved. Sites are listed in `bootstrap.sites` with a `pages` count                                                                                           |
| PATCH  | `/sites/{id}`                   | Workspace owner or admin: any of `path`, `title`, `layout`, `description`, `theme`                                                                                                                                                                                                                                                                                                                                       |
| DELETE | `/sites/{id}`                   | Workspace owner or admin; 409 while the site still has pages                                                                                                                                                                                                                                                                                                                                                             |
| DELETE | `/notes/{id}/publication`       | Unpublish                                                                                                                                                                                                                                                                                                                                                                                                                |

`sharing` summary: `{links, members, published}`, where `published` is the slug or null.

### Share-link guests (`/api/v1/shared/*`)

| Method | Path                                 | Contract                                                                                                                                                |
| ------ | ------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| POST   | `/shared/session`                    | `{token}` → `{role}`; sets the `minerva_share` cookie (`HttpOnly; SameSite=Strict; Path=/api/v1/shared`, one day). 404 for unknown, revoked, or trashed |
| GET    | `/shared/note`                       | Note content, `role`, and guest-rendered HTML; `folder_id` is always `"shared"`                                                                         |
| POST   | `/shared/sync`                       | Same contract as note sync; updates require an edit link. Peers are anonymized                                                                          |
| POST   | `/shared/render`                     | Markdown render with guest attachment URLs                                                                                                              |
| GET    | `/shared/attachments/{id}[/preview]` | Files belonging to the shared note only                                                                                                                 |
| POST   | `/shared/attachments?filename=`      | Edit links only; normal limits and quotas                                                                                                               |

Mutations require `X-Minerva-Client: 1`. Revocation and trashing return 401 on the next request.

### Public pages (outside `/api`)

| Path                                                                | Content                                                                                                                                                                               |
| ------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `/share`                                                            | Guest editor page (token in the URL fragment)                                                                                                                                         |
| `/shared/`                                                          | Index of published pages, newest first                                                                                                                                                |
| `/p/…`                                                              | Former address: permanent redirect (308) to the same path under `/shared/`                                                                                                            |
| `/shared/{slug}`                                                    | Published page: static HTML, no scripts, no workspace metadata                                                                                                                        |
| `/shared/{slug}/files/{id}[/preview]`                               | Attachments of the published note that its published content references                                                                                                               |
| `/shared/feed.xml`                                                  | Atom feed (absolute links use `MINERVA_PUBLIC_URL` when set)                                                                                                                          |
| `/{site}/`, `/{site}/{slug}`, `/{site}/{slug}/files/{id}[/preview]` | The same for a site's pages; wiki indexes are alphabetical                                                                                                                            |
| `/{site}/feed.xml`                                                  | Atom feed of a blog site                                                                                                                                                              |
| `/plugins/index.json`                                               | Installed themes: `{themes: [{id, name, description, base}], public_themes: [{id, name, description, scheme}], pdf_prints: [{id, name, description, users, variables, orientation}]}` |
| `/export/{id}?template={pdf template}`                              | Applies a PDF template to `print` and `html` exports; `{{key}}` placeholders are filled from the note's metadata in every format                                                      |
| `/plugins/themes/{id}.css`, `/plugins/public-notes-themes/{id}.css` | Theme stylesheets (see `plugins/README.md`)                                                                                                                                           |
| `/preview/{note_id}`                                                | Authenticated preview with the public template, for members who manage the note's sharing                                                                                             |

## Attachment storage administration

| Method | Path                     | Contract                                                                                                                               |
| ------ | ------------------------ | -------------------------------------------------------------------------------------------------------------------------------------- |
| GET    | `/admin/storage`         | Administrator only. `{used, files, server_quota, note_quota, file_limit, grace_seconds, notes, orphans, recent_unreferenced}`          |
| POST   | `/admin/storage/cleanup` | Administrator only. `{ids}` → `{deleted, bytes, kept}`; deletes only files that are still unreferenced and older than the grace period |

A file is referenced if its ID appears in any current note, any retained history version, or a pinned published snapshot. The grace period (24 hours) keeps uploads whose insertion was interrupted recoverable from the note's Files dialog.

## Export

`GET /export/{note_id}?format=…` requires read access to the note (session cookie or bearer token).

| `format`          | Result                                                                                                                                                                                                                                         |
| ----------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `md`              | Markdown download. Front matter (`title`, `tags`, `icon`, `cover`, `updated`) unless `front_matter=0`. Note links become `[[Title]]`; attachment links become absolute server URLs. Consecutive list blocks stay one list.                     |
| `zip`             | The Markdown plus `files/` with every referenced attachment the user may read, links rewritten to `files/…`.                                                                                                                                   |
| `html`            | A self-contained page: styles inline, readable images embedded as data URIs (up to 50 MiB), no scripts.                                                                                                                                        |
| `print` (default) | A print-designed page for "Save as PDF". It loads `/export.js` to render diagrams, then posts `minerva-export-ready` to the parent frame, or prints when opened directly. It may only be framed by the same origin (`frame-ancestors 'self'`). |

Options: `cover=0|1` and `meta=0|1` (title block with date and tags) for `print`/`html`; `size=a4|letter` for `print`.

## Journals and templates

Each workspace can have one Journal folder and one Templates folder. A workspace owner creates them the first time journals or templates are used. After that, access follows normal folder permissions. `bootstrap` returns `journal_folders`, `template_folders`, and `journal_templates` (workspace ID → folder or note ID). Journal entries carry `journal: "YYYY-MM-DD"` on the note.

| Method | Path                      | Contract                                                                                                                                                                                                                                                                                                                  |
| ------ | ------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| POST   | `/journal`                | `{workspace_id, date}`. Returns the entry for that date, creating it (with the journal template, if set) when missing. `created` tells which. Creating requires editor access to the Journal folder                                                                                                                       |
| PUT    | `/journal/settings`       | `{workspace_id, template_id \| null}`; owners only; the template must be in the workspace's Templates folder                                                                                                                                                                                                              |
| POST   | `/templates`              | `{workspace_id, title}`; creates a template note                                                                                                                                                                                                                                                                          |
| POST   | `/notes/{id}/template`    | `{template_id, date}`. Fills an empty note (409 otherwise) with a copy of the template: new block IDs, `{{date}}`, `{{weekday}}`, `{{long_date}}`, and `{{title}}` replaced; icon, cover, and tags added when the note has none. `date` is the caller's local date; journal entries use their own date                    |
| POST   | `/journal/tasks`          | `{note_id, today}`. Open tasks from journal entries before the note's date (or up to `today` for other notes) that the caller may read, oldest first. A task later marked done disappears, repeats collapse to their earliest occurrence (same text ignoring case and spacing), and tasks already in the note are omitted |
| POST   | `/journal/tasks/complete` | `{note_id, today, text}`; marks every open occurrence in that scope that the caller may edit as done; returns `{completed}`                                                                                                                                                                                               |

`[[uncompleted_journal_tasks]]` in Markdown renders as `<span class="journal-tasks">` for members and guests (the app fills it in; guests see a notice) and is omitted on published pages.

## Offline sync

`GET sync/changes` and `POST sync/push` let desktop replicas pull and push changes. A desktop app's local server also has `local/*` endpoints and reports `mode: "local"` in `/health`. `PUT notes/{id}/attachments/{attachment_id}` stores a replicated attachment under its existing ID, and does nothing if it is already stored. See [sync.md](sync.md) for entry formats, merge rules, and permissions.

## Two-factor authentication

Accounts can require a code from an authenticator app (TOTP, RFC 6238: SHA-1, 6 digits, 30 seconds).

**Sign-in.** When 2FA is on, `POST /login` with a correct password returns `{two_factor_required: true, challenge}` instead of a session. `POST /login/2fa` with `{challenge, code}` returns the usual `{user, token}` and session cookie. `code` is an authenticator code or a recovery code (case and spacing ignored).

- A challenge lasts 5 minutes, is single use, and ends after 5 wrong codes. Wrong codes count even though the request fails.
- Codes are accepted one step either side of the current time, and each code works only once per account.
- Both steps count toward the sign-in rate limit.

| Method | Path                    | Contract                                                                                                                   |
| ------ | ----------------------- | -------------------------------------------------------------------------------------------------------------------------- |
| GET    | `/account/2fa`          | `{enabled, enabled_at, recovery_remaining}`                                                                                |
| POST   | `/account/2fa/setup`    | Starts setup: `{secret, uri, qr_svg}` (`otpauth://` URI and an SVG QR code). Valid for 15 minutes                          |
| POST   | `/account/2fa/enable`   | `{code}` from the app → `{recovery_codes}` (10 single-use codes, shown once). Other sessions of the account are signed out |
| POST   | `/account/2fa/recovery` | `{code}` from the app (not a recovery code) → new `{recovery_codes}`, replacing the old ones                               |
| POST   | `/account/2fa/disable`  | `{password, code}` (authenticator or recovery code)                                                                        |

Administrators see `two_factor` in `GET /admin` users and can reset it with `PATCH /users/{id}` `{two_factor: false}`. Recovery codes are stored as SHA-256 hashes. The TOTP secret is stored in the database, so protect the database file as you would password hashes. The desktop gateway captures the session from `/login/2fa`, and `minerva-client` provides `Transport::login_with_code`.
