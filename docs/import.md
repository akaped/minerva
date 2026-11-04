# Importing notes

Choose **Import** in a folder, or **Import notes** in the workspace menu, pick the files and the folder they go into. Each file is read on the server (`POST /api/v1/import`), up to 200 MB per file.

| From         | What to choose            | How to get it                                                                                                                                                           |
| ------------ | ------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| **Notion**   | The export zip            | Settings → _Export all workspace content_, or ··· → _Export_ on a page; format _Markdown & CSV_, with subpages. Large exports nest zips inside the zip; that works too. |
| **Obsidian** | Your vault folder, zipped | —                                                                                                                                                                       |
| **Logseq**   | Your graph folder, zipped | It holds `pages`, `journals` and `assets`.                                                                                                                              |
| **Evernote** | An `.enex` file           | Right-click a notebook → _Export notebook_, as ENEX.                                                                                                                    |
| **Markdown** | One or more `.md` files   | —                                                                                                                                                                       |

## What comes along

- **Pages** become notes. Folders in the export become subfolders of the chosen folder, if you can add folders in that workspace; otherwise every note goes into the chosen folder. A Notion page's subpages sit in a subfolder named after it. Notion's IDs are taken off titles and folder names; Logseq's namespaced pages (`books___dune`) are titled `books/dune`, and its journals by date (`2026-11-01`).
- **Text** is split into blocks as if you had typed it: a block per paragraph, per heading and per list item, nested by indentation (up to three levels); code, math, tables and quotes stay whole.
- **Links** between pages point at the imported notes: Notion's relative links, `[[wiki links]]` (with `|aliases`), and Logseq's `((block references))`, which link to the block itself (it keeps its Logseq ID). Logseq's `{{embed ((block))}}` and `{{embed [[page]]}}` become embeds of the block or note. Links to pages that were not imported stay as plain text.
- **Pictures and files** a page shows or links to (`![](…)`, `![[file.png]]`, Evernote's attachments) become that note's attachments. Obsidian's embeds are found by file name anywhere in the vault. Files no page uses are left out. Attachment limits apply: 60 MiB per file, 500 MiB per note.
- **Tags and details**: front matter (`tags`, other `key: value` fields), Logseq page properties (`tags:: …`) and Evernote tags become note tags (up to 20) and note details. Evernote also keeps the creation date and source address.
- **Notion tables** (CSV) become databases. The first column is the name; a column becomes checkboxes (Yes/No), numbers, dates, links or choices when all its values fit, and text otherwise. The complete table (`_all.csv`) is used when Notion exported both. Up to 32 columns and 10,000 rows.
- **Evernote** formatting comes over as Markdown: headings, bold, italics, strikethrough, links, lists, checkboxes, quotes, code, tables and rules. Encrypted text is left out.

Left out: the apps' own folders (`.obsidian`, `logseq`, `__MACOSX`), Logseq's folding state, and anything that is not Markdown, CSV, an Evernote note or a file a page uses. An import adds up to 5,000 notes, each with up to 2,000 blocks; the summary after an import lists anything cut or left out.
