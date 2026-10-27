# Offline copies and sync

The desktop app runs its own Minerva server in-process. It holds personal workspaces and **offline copies** of workspaces on other servers. The app always works with this local server, and a background worker keeps each offline copy in sync with its server. Edits made without a network are stored durably on this computer and sent when the server is reachable again. Concurrent edits are merged.

The browser interface remains online-only.

## How it fits together

```
 Desktop app
 ┌────────────────────────────────────────────────────────┐
 │ webview ──► local.localhost ──► embedded minerva-server │  local.db
 │                                   ▲  change log, cursors │
 │                    sync worker ───┘                       │
 └──────────────────────│─────────────────────────────────┘
                        ▼  HTTPS, token from the OS credential store
                remote server: /api/v1/sync/changes, /api/v1/sync/push
```

- `minerva-server` keeps a **change log** (table `changes`). Every committed change replaces the row of each entity it touched, and a deleted entity leaves a tombstone. The log is written in the same transaction as the data, by comparing the old and new snapshots in `persist()`, so every handler is covered.
- Each synced field carries a **hybrid logical clock**, `{wall_ms:013}.{counter:06}.{node}`. Clocks compare as text, and the node is the server's UUID. A field changed by an ordinary request gets a fresh clock; a field applied from another replica keeps that replica's clock.
- `minerva-client::LocalServer` opens `local.db` with `Store::open_local`. It creates one owner account that cannot sign in with a password, plus a fresh session for the app process each time. Nothing listens on the network.
- `minerva-client::SyncWorker` runs a pass every 2 seconds while servers answer, backing off to 5 minutes while they do not. A pass also runs when the window regains focus. Each pass first **pushes** local changes, then **pulls** the server's changes.

## What merges how

| Data                                                                                                                                                                                | Rule                                                                                                                                                                                                                           |
| ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Note title, text, block order, icon, favorite, trash, cover, tags                                                                                                                   | Yjs/Yrs CRDT (the note's existing collaboration document). Concurrent edits merge character by character.                                                                                                                      |
| Note folder, metadata, PDF template; folder name and parent; workspace name, Journal/Templates folders, journal template; database name and properties; record cells, archived flag | Last writer wins **per field**, by clock.                                                                                                                                                                                      |
| A record cell edited offline that loses to a different value                                                                                                                        | The server keeps the newer value and saves the offline values as an **archived conflict copy** (`conflict_of` names the record). It is listed in the app's _Offline copies_ dialog: _Use mine_, _Keep theirs_, or _Keep both_. |
| Delete versus a later edit                                                                                                                                                          | The edit wins. The entity is kept (a deleted note comes back in the trash) and restored on the replica that deleted it.                                                                                                        |
| Folder move that became a cycle or exceeded eight levels                                                                                                                            | The folder goes to the workspace's top level and gets a new clock, so every replica agrees.                                                                                                                                    |
| Values the receiving server normalizes (metadata keys, names)                                                                                                                       | The stored value gets a new clock, so every replica converges on it.                                                                                                                                                           |

Other field losses (a field changed offline but overwritten by a newer change) and refused operations are also listed in the dialog.

## Permissions

The server checks every pushed operation with the same rules as the regular API: folder editors for notes, databases, and records, and workspace owners for folders and workspace settings. Pulls return only what the user may read. An entity the user can no longer read arrives as a tombstone. Each pull reports a digest of the user's access, and when it changes the replica reads the whole workspace again.

On the replica, the local server enforces the roles the remote server reported, so a workspace or folder the user may only view is read-only offline too. When access to the workspace is removed, the copy becomes read-only and its status says so.

Replicas choose the IDs of entities they create. These must be canonical UUIDs, because IDs appear in pages and URLs.

## Durability and retries

- The **pull cursor** commits in the same transaction as the pulled changes. The **push cursor** and the **watermark** (the local clock up to which fields were pushed) are stored after the server acknowledges a batch.
- Operation IDs derive from change-log positions (`{server_id}:{seq}`), and the server remembers applied IDs for seven days. A batch retried after a lost response is therefore applied only once.
- Unsent changes survive restarts, because they are the local change log itself. An offline copy with unsent changes cannot be removed without explicitly discarding them.
- Attachments are copied for each changed note in whichever direction is missing them. They keep their IDs (`PUT /api/v1/notes/{id}/attachments/{attachment_id}`). Size limits and quotas apply as for uploads.

## API

All endpoints are under `/api/v1`, need a session, and need `X-Minerva-Client: 1` for writes.

- `GET sync/changes?workspace=&since=&limit=` returns `{revoked, server_id, hlc, access, cursor, more, changes: [entry]}`. An entry is `{seq, kind, id, fields, clocks, role, crdt?}`, or a tombstone `{seq, kind, id, deleted: true, clocks: {deleted}}`. `kind` is `workspace`, `folder`, `note`, `database`, or `record`. Record cells are fields named `cells/{property_id}`.
- `POST sync/push {workspace_id, ops: [entry + {op_id, changed}]}` returns one result per operation: `applied`, `merged` (with `lost` fields), `conflict` (with `copy_id`, or `kept: true` and the current `entity`), `forbidden`, `invalid`, or `missing`. `changed` lists the fields this replica edited; only those can produce conflict copies.
- `GET /health` reports `sync_protocol: 1` and `mode: "server" | "local"`.

These endpoints exist only on a desktop app's local server:

- `local/replicas` (`GET`, `POST {workspace_id, connection_id, server_id, name}`), `local/replicas/{workspace_id}` (`PUT` worker state, `DELETE {discard?}`)
- `local/conflicts` (`GET`, `POST {items}`), `local/conflicts/{id}` (`DELETE`)
- `sync/push` with `upstream: true`, which applies a server's changes without sending them back

## Limits

- Presence (who else is editing) is not relayed through offline copies, and remote edits appear after the next pass (about two seconds) rather than live. Use **Open** on a server in the launcher for live co-editing with presence.
- Sharing, publishing, sites, MCP credentials, and account administration are online features. Use them on the server itself.
- Two replicas that create a journal entry for the same date while offline end up with two entries.
- The store is a JSON snapshot loaded in memory, and change tracking compares snapshots on every write. This suits personal and small-team sizes; normalized tables remain the long-term fix.
- `local.db` is not encrypted by Minerva. Use the operating system's disk encryption.
- An attachment that the receiving server refuses (for example over quota) is retried only when its note changes again.
