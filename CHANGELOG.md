# Changelog

All notable changes to Minerva. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/). Each release on GitHub uses its
section of this file as release notes.

Write new changes under **Unreleased** as you make them. `scripts/release.sh`
turns that section into the next version.

## [Unreleased]

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
