# Changelog

All notable changes to Minerva. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow
[Semantic Versioning](https://semver.org/). Each release on GitHub uses its
section of this file as release notes.

Write new changes under **Unreleased** as you make them. `scripts/release.sh`
turns that section into the next version.

## [Unreleased]

### Changed

- New app icon: the blue Minerva asterisk.

### Fixed

- The sidebar updates as soon as notes are created, renamed, starred, moved, trashed or deleted, also from a note's tab.
- A note's tab closes when the note is moved to the trash or deleted.
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
