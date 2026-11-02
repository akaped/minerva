# Changelog

All notable changes to Minerva. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/). Each release on GitHub uses its
section of this file as release notes.

Write new changes under **Unreleased** as you make them. `scripts/release.sh`
turns that section into the next version.

## [Unreleased]

### Added

- **Desktop:** when the server you had open last cannot be shown at startup (it does not answer, cannot be reached, signed you out, or another server answers at its address), the app still opens this computer, and now says why, with **Try again** and **Servers**.

- **MCP write access:** a credential can now be created with **Read and write** access (read only stays the default). Its AI tool can then create notes from Markdown, add blocks to a note, and add or update database records, always within your edit permissions. Nothing is deleted: notes keep their previous version in history, and open editors show the change at once.
- **Personal API tokens** (MCP connections → Personal API tokens) for scripts: named, expiring after up to a year, revocable, acting as you but unable to change your password, second factor or tokens. The API gains `markdown` when creating a note and `POST /notes/{id}/append`.
- **`minerva-cli`**, a small command line over the API: `search`, `folders`, `new` (Markdown from an argument or standard input), `append`, and `backup` (every note as Markdown or a zip with its files, every database as JSON).

- **Folding:** a block with blocks nested under it has an arrow beside it (or press Ctrl+↑ / Ctrl+↓ in it) that hides and shows them, with a count of what is hidden. What you fold is remembered on your device for each note and does not change the note for others; going to a hidden block unfolds it. **/ → Toggle** makes a line with a block nested under it, ready to fold.
- **Table of contents:** **/ → Table of contents** adds a block listing the note's headings; clicking one goes to it. Exported and published notes show it too, with links to the headings.
- **Link cards:** a block that is just a web address shows the page's title, description, site and picture. The server fetches these only for signed-in members and only from public addresses (never its own network), and keeps them for a day.
- **Comments** on notes: on a block (hover it and choose 💬), on selected text (💬 in the selection toolbar), or on the whole note. Threads have replies and can be resolved and reopened; the **Comments** panel lists open and resolved ones, and blocks with open threads show a count. Anyone who can read a note can comment; editors and a thread's author resolve it. `@person@example.com` in a comment, offered from the people who can open the note, emails them, and replies email the thread's earlier participants. Comments appear for others as soon as they are written.
- **Calendar view** for databases: records on a month calendar by any date property, or by when they were created or last edited. Open a record by clicking it, or add one on a day with **+**.
- **New database properties:** **Multi-select** (several options), **Person** (someone who can open the database), **Relation** (records of another database in the workspace, chosen from a searchable list and shown by name), and **Created time** and **Last edited time**, kept automatically. Filtering and sorting use what the cells show, such as people's and related records' names, and notes and exports show them by name too.

## [0.5.1] - 2026-11-02

### Changed

- A picture from a note's files shows a slim bar above it on hover, with its name, size, upload date and **Download**, instead of a download card below it. New pictures are added without the card; in existing notes the card under a picture is hidden.

### Fixed

- The cursor is visible again while writing in a block (it had become transparent with the live preview, in the desktop app).
- A task being edited shows its Markdown (`- [ ]`) without the checkbox beside it, and the text and the cursor line up again.
- Releases include the desktop app and server for Linux and Windows again: 0.5.0 was published without them because its macOS build could not run. Releases no longer include macOS packages; build them on a Mac with `scripts/package-desktop.sh macos`.

## [0.5.0] - 2026-11-01

### Added

- A **Reminders** page in the sidebar collects everything coming up in a workspace: every `⏰` time written in its notes, and the workspace's reminders, shared with everyone in it. These can happen once or repeat every so many days, weeks, months or years (“Car insurance, every year”, “Car revision, every 2 years”), and have a category (_car_, _health_, …) to filter by. Due reminders stay at the top until marked **Done**, which moves a repeating one to its next time. With email set up, everyone in the workspace gets an email when one is due. Members who can only view the workspace see its reminders but cannot change them.
- **AI chat** answers plain questions: **No context** is the default, and sends none of your notes. Attach images (PNG, JPEG, GIF, WebP), PDFs and text documents to a question, with the paper clip, by dropping them on the question box, or by pasting an image. They go to the model in the form each provider reads (OpenAI, Anthropic, OpenAI-compatible); DeepSeek takes text documents only. Files are sent with that question only and are not stored.
- Web addresses written in a block (`https://…`) become links. Pasting an address over selected text links that text.
- A note tab shows a sketch of a page while the note loads, instead of a blank white frame.
- The workspace you had open last opens again on your next visit, in the browser and in the desktop app. The desktop app starts on the server you had open last (falling back to this computer when that server is signed out or does not answer), and that server's page reopens its workspace when the server runs this version.
- Files in notes show as cards: an icon for the type of file, its name, size and upload date, with Download. PDFs also have **Open**, which shows the PDF in a tab beside the note.
- **/emoji** in the editor opens an emoji picker: every emoji, searchable by name or keyword (“heart”, “pizza”, “rocket”), grouped like a phone keyboard, with recently used ones first.
- When several people are in a note, a small name tag beside each block shows who is on it: solid while they edit the block, outlined while they read it. The tag moves with them and goes away when they leave the note.
- Only one person can edit a block at a time. Clicking a block someone else is editing tells you who has it, and if two people open the same block at once, the first one keeps it. Deleting that block, or merging into it with Backspace or Delete, waits until they move on.
- **Search** (Ctrl+K) finds notes by their title, text and tags, best matches first: a title match ranks above a match in the text. Each result shows the folder it is in and a line of its text with the matching words highlighted. Search ignores accents and capitals (“perche” finds “perché”), finds words while you are still typing them, and needs every word you type. Before you type, it lists the notes changed most recently. ↑/↓ choose a result, Enter opens it, and Shift+Enter opens it side by side with the note you have open. Linking with `[[` and AI tools connected over MCP get the same ranked results.
- **Live preview while you write:** in the block being edited, Markdown marks are dimmed and what they mark shows styled (bold, links, code, highlights, strikethrough), and a heading is edited at its size. The text stays plain Markdown, and the cursor, selection and undo work as before.
- **Select whole blocks:** Shift+click, Shift+↑/↓ at the edge of a block, Ctrl+A twice, or a text selection dragged across blocks selects them. A bar then turns them into a heading, list, to-do or quote, indents or moves them out, moves them up or down, copies, cuts or deletes them, each as one undo step; the same works with Tab, Shift+Tab, Alt+Shift+↑/↓, Backspace, Ctrl+C and Ctrl+X.
- **AI on selected text:** select text and press Ctrl+J, or **✦ AI** in the selection toolbar, to improve it, fix spelling and grammar, make it shorter or longer, translate it, or continue it. The suggestion is shown first; replace the selection with it, add it after, insert it below the block, copy it, or discard it.
- **AI chat about a workspace or folder** sends the notes related to your question first (using the search index), then the most recent ones, instead of only the most recent.
- **Flashcards in your notes:** a deck can follow a workspace or folder. A question tagged `#card` (answered by the blocks indented under it, or `Question :: Answer #card`) and cloze deletions such as `{{c1::Rome}} is the capital of Italy` become cards, kept in step with the notes along with their review history. Cloze deletions show highlighted in notes.
- **Import from Notion, Obsidian, Logseq and Evernote**: choose a Notion export zip, a zipped Obsidian vault or Logseq graph, an Evernote `.enex`, or Markdown files. Pages become notes in folders like the export's, with their links to each other, tags, pictures and files; Notion tables become databases; lists become nested blocks. See `docs/import.md`.

### Changed

- Files attached to notes can be up to **60 MiB** (was 10 MiB), and up to 500 MiB per note (was 100 MiB). The total for the server is 10 GiB by default (was 1 GiB) and can be set with `MINERVA_ATTACHMENT_STORAGE_GB`.
- **Flashcards are scheduled with FSRS**, the algorithm Anki uses, instead of fixed multipliers: each card's stability and difficulty follow your answers, and it returns when you are likely to still remember it 90% of the time. A new card answered Good returns after 4 days, Easy after 14. Cards keep their schedule and switch over at their next review. Each rating button shows when the card would return.
- **Open notes update at once** when someone else edits them or moves around in them: the server signals changes over a WebSocket (`/api/v1/live`), and pages poll every 5 seconds only as a fallback, instead of every 650 ms. The desktop app's own pages and share links still poll. A reverse proxy must pass WebSocket upgrades; Caddy and Cloudflare Tunnel do.
- Earlier versions of notes stay on disk and are read when you open a note's history, instead of all being kept in memory: a large workspace opens in under a second instead of many, with a fraction of the memory. The overview loads faster too.
- Minerva saves each note, its history and its live editing state separately, instead of rewriting all data on every change, and no longer copies every note and every saved version for each request. In a workspace of 2,000 notes, live editing updates take about 13 ms instead of about 1 s, opening a note about 35 ms instead of 630 ms, and search about 80 ms instead of 1.3 s. The data file is upgraded the first time the new version opens it (the old copy is kept inside it). **Back up the data file before updating: earlier versions of Minerva cannot open an upgraded file.**
- The note icon is chosen with the same searchable emoji picker, instead of a short list and a box to type an emoji into.
- The **/** menu opens by the block as a compact popover, grouped (Basic, Lists, Media & links, Advanced, People & time, Plugins), with an icon, a description and the Markdown shortcut for each block type. **Image or file** opens a drop area: drop files or click to choose them.

### Fixed

- The emoji picker lists only emoji this computer can draw: emoji newer than the system's emoji font no longer show as empty boxes or broken pictures. It also fits its panel, with every group's tab visible.
- YouTube videos play in the desktop app. YouTube refused to play in `minerva://` pages (error 153), and the app blocked the player's frame; the app now serves the player from a page of its own on `127.0.0.1`.
- New folders could seem not to be created: the sidebar jumped back to the top after each one, so folders past the bottom edge were out of sight. The sidebar keeps its scroll position, scrolls to a new folder and highlights it, and fades at the bottom when there is more below.

## [0.4.0] - 2026-10-29

### Added

- AI providers have a **Max answer length** setting (default 4096 tokens). Raise it for thinking models such as Qwen or DeepSeek R1, which spend part of it reasoning before they answer.
- Import several Markdown files at once: each becomes a note in the chosen folder, with progress and a list of any files that could not be imported.
- Mentions: type `@` in a note to pick someone who can open it; the mention shows their name, emails them the block, and appears in their new **Mentions** dashboard widget. `/assigntouser` opens the same picker. Published pages and exports show only the name part of a mentioned address.

### Changed

- Ask AI beside a note: a compact header with History and Providers, the conversation fills the panel with the question box pinned at the bottom (Enter sends, Shift+Enter for a new line), each reply shows who answers and an animated “Reading… / Thinking…” indicator, sources are compact chips, and tables in replies have borders.
- Published pages without a site now live under `/shared/` instead of `/p/`. Old `/p/…` links redirect permanently to the same page under `/shared/`.
- The workspace share dialog lists the people with access, each with their own Edit or Read only menu and a button to remove them, and invites new people at the top; changes apply at once.
- The Sharing page covers all your workspaces: shared workspaces (yours, with who has access, and those shared with you), notes you share grouped by workspace with names, links and public addresses and a filter, notes shared with you, and the public sites of every workspace. It moved to **Others** in the sidebar.

### Fixed

- Callouts, quotes and multi-line `$$` formulas stay one block when Markdown is imported, pasted or shown in an AI reply.
- AI chat: an answer cut off at the model's length limit is kept and saved with a warning, instead of being discarded; long answers from slow local models are no longer stopped after two minutes while they are still streaming; thinking models (Qwen, DeepSeek R1) show “Thinking…” while they reason.
- A note's Share dialog offers sites created after the note was opened, for example on the Sharing page.
- Importing Markdown no longer turns the blank lines between paragraphs into empty blocks.
- Sidebar items under **Others** (Sharing, Trash, AI providers…) respond to clicks again after you work in a note tab, and the sidebar no longer highlights a page while a note tab is in front.

## [0.3.0] - 2026-10-26

### Added

- Images: pasting a link to an image shows it, and images or files dropped onto a note are added to it (#2).
- Audio: links to audio files (MP3, WAV, OGG, FLAC, M4A, playlists) and uploaded audio files play in the page, and **/ → Record audio** records from the microphone into the note (#1).

### Changed

- The server administrator only runs the server: accounts, AI providers, account security, and a server overview with accounts, workspaces, notes and storage per member. It has no workspaces, notes or folders, and cannot open members' content. Workspaces the administrator owned before are kept and can be transferred to a member from the overview.
- Every member creates workspaces and shares them with other members, as Read only or Edit, from the workspace's badge in the top bar. Members who can edit a workspace manage it like its owner: folders, notes, journal, sharing and public sites. My Workspace stays private.
- Deleting an account that owns shared workspaces asks which member takes them over.
- New app icon: the blue Minerva asterisk.

### Fixed

- Members who can edit a shared workspace add folders and subfolders and start its journal and templates, not only its owner (#3).
- The sidebar updates as soon as notes are created, renamed, starred, moved, trashed or deleted, also from a note's tab.
- A note's tab closes when the note is moved to the trash or deleted.
- Desktop app: a note's Share dialog shows your server's public address for published pages and links, instead of the app's internal `minerva://` address.
- Linux: the window moves by dragging its title bar, and a double-click maximizes it.
- Linux: the application launcher starts Minerva even when `~/.local/bin` is not in the desktop session's path, and Minerva appears once in the applications menu.

## [0.2.0] - 2026-10-26

### Added

- Email, configured in `.env`: invitations for new accounts, “Forgot your password?” links, notices of new access to workspaces and folders, `/assigntouser` assignments, and `/remindme` reminders that send the block at the chosen time.
- Dashboard widgets are dragged into rows that always fill the page width, beside another widget or in a row of their own.
- Blocks are moved by dragging a handle that appears on hover. Copying several blocks puts their Markdown on the clipboard as plain text and the formatted text for rich editors.
- macOS desktop app and server (universal, for Apple Silicon and Intel), with a native menu bar and sign-ins kept in the Keychain.
- Server packages for Linux, Windows and macOS in each GitHub release, next to the desktop app.
- A “Getting Started” guide in every new My Workspace, with examples of everything Minerva does.
- Customizable dashboard: favorites, recently updated, journal calendar, flashcards to review, and database charts; widgets can be added, removed, resized and reordered.
- Workspace menu with this computer, each server and its workspaces, adding servers, and creating or importing workspaces.
- Personal “My Workspace” for each account, which cannot be shared; administrators cannot open other people's personal workspaces.
- Account management: administrators edit and delete accounts; everyone changes their own password, and new members choose theirs at first sign-in.
- Anki deck import (`.apkg`, `.colpkg`) with pictures and links, and a link to AnkiWeb's shared decks.
- AI chat beside the note, with replies as blocks to add to the note, conversation history and deletion, and web search per provider.
- Note metadata with your own variables, used by PDF templates and `{{variable}}` placeholders.
- Local-first desktop app: an embedded server for notes on this computer, and offline copies of server workspaces that sync when back online.
- Release, Docker image and documentation workflows; server `compose.yaml` with optional Cloudflare Tunnel.

### Changed

- The block being edited looks like the text around it, with only a faint line on its left.
- Blocks are no longer highlighted on hover.
- PDF templates have proper margins and no number prefixes; the “Minerva” template was removed.
- YouTube videos play inline, keep playing while you type, and are embedded on published pages.
