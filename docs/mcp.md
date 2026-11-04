# Minerva MCP server

Minerva exposes a workspace knowledge interface directly from the Rust server: read only by default, and read and write for credentials you create with write access. It follows the workspace-scoped pattern described by [AFFiNE MCP](https://affine.pro/mcp). No external bridge service or AI provider is required by the server.

## Connect

1. Open **MCP connections** in Minerva.
2. Choose **Create credential**, a client label, one accessible workspace, a 7/30/90-day expiry, and the access: **Read only** (the default) or **Read and write**.
3. Copy the generated JSON into your MCP client's configuration. The secret is displayed once.
4. Ask the connected tool to search notes, read a document, or query structured records.

Example (replace placeholders; never commit a real token):

```json
{
  "mcpServers": {
    "minerva": {
      "url": "https://notes.example.com/api/workspaces/WORKSPACE_ID/mcp",
      "headers": {
        "Authorization": "Bearer mnv_mcp_v1_REPLACE_ME"
      }
    }
  }
}
```

Configuration wrappers and transport-name fields differ by MCP client. The endpoint implements stateless Streamable HTTP with JSON responses. Use clients that support custom bearer headers. OAuth-only clients need separate compatibility work; this release does not implement OAuth discovery or authorization flows. The official MCP TypeScript SDK is exercised by `scripts/protocol-smoke.mjs`.

## Tools

| Tool             | Purpose                                                                                                                            |
| ---------------- | ---------------------------------------------------------------------------------------------------------------------------------- |
| `doc_search`     | Case-insensitive keyword search of permitted note titles and Markdown; up to 50 results with bounded passages and stable block IDs |
| `read_document`  | Read a permitted document with up to 100 blocks per page; follow `next_offset`                                                     |
| `list_folders`   | List accessible folders within the credential's workspace                                                                          |
| `list_databases` | List accessible structured databases and their property schemas                                                                    |
| `query_database` | Read up to 100 records per page, with optional text query and exact typed property filter                                          |

Credentials with write access also list these tools; read-only credentials neither list nor may call them:

| Tool              | Purpose                                                                                                                                                         |
| ----------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `create_document` | `{folder_id, title, markdown?}`: a note in a folder of the workspace. Markdown becomes blocks (a paragraph or list item each)                                   |
| `append_blocks`   | `{document_id, markdown, after_block_id?}`: adds blocks at the end of a note or after a block; returns the new block IDs                                        |
| `update_block`    | `{document_id, block_id, markdown}`: replaces one block's Markdown                                                                                              |
| `move_block`      | `{document_id, block_id, after_block_id?}`: moves a block after another, or to the top without `after_block_id`; returns the new order                          |
| `attach_file`     | `{document_id, filename, content_base64, after_block_id?}`: stores a file (up to 10 MB) on the note and adds a block showing a picture or linking anything else |
| `add_record`      | `{database_id, cells}`: a database record                                                                                                                       |
| `update_record`   | `{database_id, record_id, cells}`: changes some values of a record (`null` clears one); the others stay                                                         |

Changes to notes keep the previous version in the note's history and reach open editors, as edits in the app do.

The credentials list (`GET /api/v1/mcp-credentials`) includes `last_used_at`, recorded at most once a minute.

`cells` are keyed by property ID or name and checked like the app's: text, URL, select and dates (`YYYY-MM-DD`) are strings, numbers numbers, checkboxes booleans, multi-select and relation lists of options or record IDs, person a user ID. Created and last-edited times are kept by the server.

`query_database` filter example:

```json
{
  "database_id": "DATABASE_ID",
  "filter": { "property_id": "STATUS_PROPERTY_ID", "value": "In progress" },
  "offset": 0,
  "limit": 25
}
```

Write tools need edit access to the folder, as in the app. They never delete or archive anything: an appended note keeps its previous version in its history, and changes reach open editors at once. Read tools are annotated `readOnlyHint: true`, write tools `readOnlyHint: false, destructiveHint: false`. There is no semantic/vector search, attachments, or sharing through MCP. Returned note/record content is user-authored data and should not be treated as instructions by consuming AI tools.

## Permissions and credentials

A credential is scoped to its issuing user and exactly one workspace. It never exceeds that user's current folder permissions. A user can create credentials only for an accessible workspace. Administrators can read their workspace through the same tools, but credentials do not gain cross-workspace access. Both explicit IDs and search results are permission-filtered.

Tokens are generated from random UUIDs, stored only as SHA-256 hashes, and expire after 1–90 days. Create one per client. Revoking a token takes effect on the next call. Disabling an account or removing folder access also stops relevant access. Raw tokens are never returned by the listing API and are not saved in localStorage. Revoke and create a replacement to rotate a credential; overlapping/grace-period rotation is not implemented.

Use HTTPS when connecting remotely. Set `MINERVA_PUBLIC_URL` to the exact public origin and `MINERVA_SECURE_COOKIE=1` behind your TLS reverse proxy. If an Origin header is supplied, it must match the configured public origin (or, by default, the local listen origin). Requests with no Origin—typical native MCP clients—still require a valid bearer credential. CORS is not enabled.

## Protocol

- Endpoint: `POST /api/workspaces/{workspace_id}/mcp`.
- `Content-Type: application/json`.
- `Accept: application/json, text/event-stream`.
- Negotiated version: `2025-06-18`; subsequent supported version headers are `2025-06-18` and `2025-03-26`.
- Methods: `initialize`, `notifications/initialized`, `ping`, `tools/list`, `tools/call`.
- Initialization advertises only tools. No session ID is issued.
- Notifications receive 202 with an empty body.
- GET/DELETE return 405: no standalone SSE stream or transport sessions are provided.
- Authentication, Origin, and protocol headers are checked independently of tool execution.
- Tool execution failures are returned as MCP tool error content; malformed JSON and unknown RPC methods use JSON-RPC errors.

## Distinction from in-app chat

**MCP connections** lets an external assistant read Minerva through MCP. It does not configure an LLM or send documents to a provider by itself. In-app chat is configured under **AI providers**.

For scripts that are not AI tools, use a personal API token and the REST API, or the `minerva-cli` command (see `api.md`).

## OAuth for clients that need it

Some MCP clients cannot take a pasted credential and sign in with OAuth instead. Set `MINERVA_MCP_OAUTH=1` (and `MINERVA_PUBLIC_URL` to the address clients use) to let them, as the MCP authorization specification describes:

- An MCP request without a valid credential gets `401` with `WWW-Authenticate: Bearer resource_metadata="…/.well-known/oauth-protected-resource/api/workspaces/{id}/mcp"`; that document names this server, whose `/.well-known/oauth-authorization-server` lists the endpoints.
- Clients register at `POST /oauth/register` (public clients only, `token_endpoint_auth_method` `none`). Redirect addresses must be HTTPS, or HTTP to `localhost`, `127.0.0.1` or `[::1]`; up to five each.
- `GET /oauth/authorize` needs PKCE (`S256`) and shows a consent page to the person signed in to Minerva in that browser: the client's name and return host, the workspace (from the `resource` parameter, or chosen there), and read-only or read-write access (preselected from `scope` `mcp:read` / `mcp:write`). The page cannot be framed, and only its own one-time key can approve.
- `POST /oauth/token` exchanges the code (once, within 10 minutes, with the matching `code_verifier`) for an access token and a refresh token. The access token is an MCP credential named “{client} (OAuth)” that lasts an hour; refresh tokens last 30 days and change at each use. Revoking the connection in **MCP connections** ends both.
