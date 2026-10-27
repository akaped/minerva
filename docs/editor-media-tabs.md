# Video blocks and note tabs

## YouTube

Paste a bare YouTube link into an otherwise empty block, then finish the block (Enter or click elsewhere). Supported URLs include `youtube.com/watch?v=…`, `youtu.be/…`, `/shorts/…`, `/live/…`, and `/embed/…`. Surrounding whitespace is fine; prose, Markdown links, and code remain ordinary Markdown. Start times (`t=1m30s` or `start=90`) are preserved.

The block shows the video player directly, using YouTube's privacy-enhanced iframe (`youtube-nocookie.com`); opening a note with videos therefore contacts YouTube. Each player lives outside the note's markup, positioned over its block, so typing elsewhere in the note (which redraws blocks) never reloads or interrupts it. Editing the block's link, deleting the block, or leaving the note removes the player.

While a playing video scrolls out of view, its player floats at the upper-right of that editor pane. Back to video scrolls to the original block; × pauses it and returns it to its block. Hidden tabs pause their videos. Below each player, Open on YouTube is the fallback for videos disabled for embedding, and Edit link changes the address. Network restrictions, consent requirements, and video-owner restrictions can prevent playback.

Published pages (`/shared/…` and sites such as `/blog/…`) show the same video as an embedded player, rendered by the server since published pages run no scripts, with a Watch on YouTube link below it. The player iframe sends the page's origin as referrer, which YouTube requires; the rest of the page still sends none to other sites. Atom feeds carry the same player; readers that drop iframes still show the Watch on YouTube link. Exports keep the plain link.

Implementation reference: https://developers.google.com/youtube/iframe_api_reference

## Tabs and split editing

Opening a note creates an application tab, identified by server origin plus note ID. Opening that note again activates its existing tab. Each editor has its own live collaboration, undo history, cursor, scroll position, and modal dialogs; tab switching does not recreate the editor.

- Drag tabs to reorder; Alt+Left/Right on a focused tab is the keyboard alternative.
- The side-by-side button on a tab places it beside the active note. Single pane collapses the split.
- Browse notes returns to navigation without closing editors. Workspaces can be switched while notes remain open.
- Closing a connected tab first asks its editor to flush pending edits. If saving fails or times out, the tab stays open. Reloading the app ends these in-memory tab sessions; saved notes remain on their servers.
- Copy link in a note copies a server-qualified link. The tab bar's + button accepts a note link or another server's address.
- Up to twelve tabs may be open at once. Two panes are displayed at a time.

## Another server in a browser tab

The remote server must run this version and explicitly trust the host application's origin. On the **remote** server, set an exact comma-separated allow-list, for example:

```
MINERVA_EMBED_ORIGINS=https://notes.example.com
```

Only `/pane` permits embedding, by its own origin and this list. The full application and administration entry page continue to reject framing. Do not trust origins you do not control. HTTPS host apps require HTTPS remote servers because browsers block mixed-content frames.

Remote panes sign in independently and keep their bearer session only in the remote frame's memory; the host never receives a token. This avoids relying on third-party cookies. Attachment images and downloads use authenticated fetches. Reloading the app requires signing in again to remote panes. Server permissions still apply to every API request. The message bridge verifies both the sender window and exact origin, and carries only tab labels, save status, visibility, and navigation/save requests.

The native desktop client's existing server gateway has separate origin/credential rules. Cross-server embedded tabs in that client are not yet verified; the browser workflow above is the supported cross-server path for this increment.
