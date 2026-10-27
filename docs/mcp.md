# Minerva MCP server

Minerva exposes a read-only knowledge interface directly from the Rust server, following the workspace-scoped pattern described by [AFFiNE MCP](https://affine.pro/mcp). No external bridge service or AI provider is required by the server.

## Connect

1. Open **AI connections** in Minerva.
2. Choose **Create credential**, a client label, one accessible workspace, and a 7/30/90-day expiry.
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

`query_database` filter example:

```json
{
  "database_id": "DATABASE_ID",
  "filter": { "property_id": "STATUS_PROPERTY_ID", "value": "In progress" },
  "offset": 0,
  "limit": 25
}
```

All tools are read-only. This release does not implement MCP create/update tools, semantic/vector search, attachments, provider-backed chat, or public shares. Returned note/record content is user-authored data and should not be treated as instructions by consuming AI tools. Tools include descriptions and read-only annotations to make that boundary explicit.

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

**AI connections** currently lets an external assistant read Minerva through MCP. It does not configure an LLM or send documents to a provider by itself. DeepSeek/OpenAI/Claude configuration and workspace/folder/note-scoped chat are separately tracked in `TODO.md`.
