# Sharing and publishing

Minerva offers three ways to give someone access to a single note. All three are separate from folder permissions and can be revoked at any time.

| Mechanism      | Who                                      | Access                  | Sign-in  | Revocation                 |
| -------------- | ---------------------------------------- | ----------------------- | -------- | -------------------------- |
| Member share   | A member of this server, chosen by email | View (default) or edit  | Required | Remove the member in Share |
| Share link     | Anyone holding the link                  | View (default) or edit  | None     | Revoke the link in Share   |
| Published page | The public web                           | Read-only rendered page | None     | Unpublish                  |

Only **folder-level editors** of the note's folder manage sharing: folder editors, workspace owners, and administrators. People who receive a note through a share cannot re-share it. Sharing a trashed note is refused, and trashing a note immediately ends link and member-share access and takes its published page down. Restoring the note brings existing shares and the publication back.

The Share button in the note toolbar opens all three controls. Notes with active sharing show indicators: a **Public** badge on the note card and toolbar when a link or published page exists, and a status line in the note header such as "Published at /shared/roadmap" or "Shared with 2 members". The **Sharing** page lists every note your folders share or publish, plus notes shared with you.

## Member shares

A member share grants one note to one active member, located by exact email address. The member's effective role on the note is the higher of their folder role and the share role, so a folder viewer can be given edit access to one note. Shared notes appear under **Sharing → Shared with you**, in search, and in note-scoped AI chat. They do not expose the rest of the folder, its other notes, its databases, or the note's version history, which can hold text removed before the note was shared. Granting a member a share again replaces their previous role.

## Share links

Creating a link returns a random 256-bit token **once**. The server stores only its SHA-256 hash, so a lost link cannot be shown again; create a new one and revoke the old one. Link URLs place the token in the fragment (`https://notes.example.com/share#<token>`). Fragments are not sent to the server, recorded in access logs, or included in `Referer` headers. The guest page exchanges the token for an `HttpOnly`, `SameSite=Strict` cookie scoped to `/api/v1/shared` and valid for one day, then removes the token from the address bar.

Guests use a separate API (`/api/v1/shared/*`) whose every request is checked against the current share:

- `GET note` returns the note content without folder, workspace, favorite, or member information.
- `POST sync` performs live collaboration. View links may only read; edit links may submit CRDT updates. Guests appear to members as "Guest", and members appear to guests as "Member", so names and IDs are not disclosed.
- `GET attachments/{id}` and `/preview` serve only files that belong to the shared note. Rendered Markdown for guests points attachment links at these routes.
- `POST attachments` (edit links only) uploads to the shared note, with the usual size limits and quotas.
- `POST render` renders Markdown for the guest editor.

The member session cookie and the share cookie are distinct: a share cookie never authenticates the member API.

## Published pages

Publishing renders a note as a static HTML page at `/shared/{slug}`. There is no JavaScript, editing control, folder or workspace name, author identity, or revision history. Pages include an index at `/shared/` and an Atom feed at `/shared/feed.xml`; pages can also be grouped into [sites](#sites) with their own paths. Addresses use 1–80 lowercase letters, digits, and inner hyphens, and stay stable until you change them. The page title and the optional description become the page `<title>`, description, and Open Graph metadata. Set `MINERVA_SITE_NAME` to name the site and `MINERVA_PUBLIC_URL` for absolute feed links.

## Sites

A **site** gives published pages their own address, such as `/blog/hello-world` or `/wiki/how-to-train-your-dragon`. Pages without a site stay under `/shared/`.

- **Ownership.** A site belongs to one workspace. The workspace owner or an administrator creates, edits, and deletes it from **Sharing → Sites**. Any folder editor in that workspace can publish into it by choosing the site in the publish dialog. Only notes from the site's workspace can be published there, so one workspace cannot add pages under another's path.
- **Paths.** Site paths use up to 40 lowercase letters, digits, and inner hyphens, are unique on the server (first come, first served), and cannot take names the server uses (`p`, `api`, `export`, `share`, `preview`, and a few reserved for later). The same page address can exist in different sites. Renaming a site moves all its pages to the new path, and a site that still has pages cannot be deleted.
- **Layouts.** A **blog** lists pages newest first with publication dates and has an Atom feed at `/{site}/feed.xml`. A **wiki** lists pages alphabetically, shows when each page was last updated, and has no feed.
- **Look.** Each site can use a public theme (Paper, Midnight, Sunrise, Terminal, or one you add to `plugins/public-notes-themes`). Pages under `/shared/` use `MINERVA_PUBLIC_THEME`. See [plugins/README.md](../plugins/README.md) to write your own.
- **Links between pages.** A link to another note (`[[` in the editor) points to that note's public address when it is published, in any site. Links to unpublished notes are removed, so private note titles and IDs never appear in public HTML.

**Pinned or live.** Publishing defaults to a **pinned** snapshot taken when you publish. Later edits stay private until you choose **Update published page**, which takes a new snapshot. **Live** mode follows the note's current content instead. Pinned is the default because a public page should change only when someone deliberately publishes. Snapshots are retained only for the current publication.

**Separate from edit links.** Publishing never grants edit access. Public edit links and published pages are different mechanisms with different URLs, and either can exist without the other.

**Preview** (`/preview/{note_id}`) renders the current content with the public template for members who manage the note's sharing. It is clearly labelled as not public.

**Files on published pages.** Only attachments that belong to the published note _and_ are referenced by the published content are served, at `/shared/{slug}/files/{id}` (or `/{site}/{slug}/files/{id}`). Other files of the same note remain private.

## Sharing with members of another Minerva server (design)

Cross-server sharing is not implemented. This section records the design constraints for when it is.

**Do not trust email addresses.** `alice@example.com` on server A and an account with the same email on server B are unrelated. Each server's administrator controls its own accounts, and nothing proves the email was verified or belongs to the same person. A share must therefore name a **(server identity, account identity)** pair, never an email address alone.

**Server identity.** Each server already has a persistent UUID. Federation would add a long-lived Ed25519 key pair per server, published at `/.well-known/minerva-server` together with the server UUID, its public origin, and the key. The well-known document is fetched over HTTPS, so it is bound to the origin through TLS. Clients and servers pin the key on first use, the same trust-on-first-use model the desktop client uses for server UUIDs, and treat a key change as a security event that requires an administrator to confirm.

**Account identity.** The recipient's server issues a signed **identity assertion**: `{issuer_server_id, issuer_origin, account_id, display_name, audience_server_id, nonce, expires_at}`, signed with the issuer's server key. The recipient obtains it by signing in to their own server. The sharing server verifies the signature against the pinned key, checks the audience and expiry, and rejects reused nonces.

**Invitation flow.**

1. The sharer creates a pending invitation on server A for a note and role. The result is a one-time invitation code, delivered out of band (it is a secret, like a share link).
2. The recipient opens the invitation and chooses their home server B. Server A redirects to B, where the recipient signs in and approves the request.
3. B returns a signed identity assertion for A. A verifies it, binds the invitation to `(B's server id, account id)`, and marks the invitation consumed.
4. Later access uses short-lived assertions from B, which A re-verifies on every session. Disabling the account on B, revoking B's trust on A, or revoking the share on A each end access.

**Authorization stays local.** Server A enforces note roles exactly as for member shares, and remote users never receive folder access. Presence and author names for remote users are labelled with their server (for example, "Alice · notes.example.org").

**Administrator control.** Federation is off by default. Administrators may restrict trusted peer servers to an allow-list, and can see and revoke every remote share. Audit events record invitation, acceptance, access, and revocation.

**Out of scope.** Replicating notes between servers, cross-server search, and cross-server AI context. Content stays on the sharing server, and remote recipients collaborate there.
