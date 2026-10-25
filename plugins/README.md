# Minerva plugins

This folder holds the themes that ship with Minerva. You can change them or add your own.

| Folder                 | What it styles                                        | Chosen by                                                                                       |
| ---------------------- | ----------------------------------------------------- | ----------------------------------------------------------------------------------------------- |
| `themes/`              | The Minerva app                                       | Each user, in the theme picker at the top of the app                                            |
| `public-notes-themes/` | Published pages under `/p/` and sites such as `/blog` | The workspace owner, per site (Sharing → Sites → Edit → Look); `MINERVA_PUBLIC_THEME` for `/p/` |
| `pdf-prints/`          | PDF (and standalone HTML) exports                     | Whoever exports, in the Export dialog; remembered per note                                      |

## How plugins load

- The themes in this folder are built into the server, so Minerva works even without the folder.
- The server also reads a plugin folder at run time: `MINERVA_PLUGINS`, default `plugins` in the working directory. In Docker it is `/plugins`, and `docker-compose.yml` mounts this folder there.
- A file in the plugin folder adds a theme or replaces the built-in theme with the same name. Changes apply on the next page load; no restart or rebuild is needed.
- If a theme is removed, users of an app theme fall back to the system theme, and sites fall back to the default look.

## File format

One CSS file per theme. The file name, without `.css`, is the theme's ID: lowercase letters, digits, and hyphens, up to 40 characters. Files over 512 KiB are ignored.

Each file starts with a comment header of `key: value` lines:

```css
/*
 * name: Ocean
 * base: dark
 * description: Deep blue with sea-green accents.
 */
```

- `name`: shown in pickers. Defaults to the ID.
- `description`: optional, shown next to public themes.
- `base` (app themes): `light` or `dark`. The theme is applied on top of Minerva's built-in light or dark theme, so it only needs to change what differs.
- `scheme` (public themes): `light` or `dark` pins the page to that color scheme. Leave it out to follow the reader's system setting.

The default **Light theme** uses cream paper, forest green, warm accents, and serif page titles, matching the project landing page. **Classic Light** (`themes/classic-light.css`) preserves the original white and indigo appearance. Select it from the app’s theme picker. Dark, Colorful, and Command line retain their existing appearance.

## Writing an app theme

Scope every rule to `[data-variant="<id>"]`. While your theme is active, the page also carries `data-theme="light"` or `data-theme="dark"` from your `base`, so all of Minerva's rules for that mode keep working.

```css
/*
 * name: Ocean
 * base: dark
 */
[data-variant="ocean"] {
  --surface: #0b1d2a;
  --surface-alt: #08151f;
  --ink: #d9eef7;
  --accent: #3dd6c6;
  --line: #17384a;
  background: #0b1d2a;
}
[data-variant="ocean"] .sidebar {
  background: #08151f;
}
[data-variant="ocean"] .btn.primary {
  background: #3dd6c6;
  border-color: #3dd6c6;
  color: #062126;
}
```

`themes/colorful.css` (based on light) and `themes/terminal.css` (based on dark) are complete examples. Most of `terminal.css` is a copy of the app's dark-mode rules with the colors changed, which is the quickest way to restyle a dark theme thoroughly.

## Writing a public theme

Published pages use `/public.css`, which is built on CSS custom properties. Override them in `:root`, and add any other rules you like:

```css
/*
 * name: Forest
 * scheme: light
 * description: Green and calm.
 */
:root {
  --page: #f6faf5;
  --ink: #1f3324;
  --muted: #5f7565;
  --accent: #2f7d4f;
  --line: #dbe8dd;
  --font-body: Georgia, serif;
}
```

| Token                                                                                                          | Purpose                                                    |
| -------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------- |
| `--page`, `--ink`, `--muted`, `--line`, `--accent`                                                             | Background, text, secondary text, rules and borders, links |
| `--code-bg`, `--mark`, `--tag-bg`, `--tag-ink`, `--danger`                                                     | Code blocks, highlights, tags, errors                      |
| `--font-body`, `--font-heading`, `--font-mono`                                                                 | Typefaces                                                  |
| `--measure`, `--radius`                                                                                        | Text column width, corner rounding                         |
| `--hl-comment`, `--hl-string`, `--hl-constant`, `--hl-keyword`, `--hl-function`, `--hl-type`, `--hl-attribute` | Syntax highlighting                                        |
| `--banner-bg`, `--banner-ink`                                                                                  | The preview banner                                         |

Useful selectors: `.site` (site name header), `.post` and `.post h1` (a page), `.index` and `.posts li` (the page list), `.post-date`, `.post-nav`, `.block` (one block of content).

Published pages contain no scripts, and plugins cannot add any: themes are stylesheets only.

## Writing a PDF template

PDF templates live in `pdf-prints/`. Twenty ship with Minerva, from **Standard** to **Personal Journal**. The Export dialog lists them in file-name order, without the number prefix, and Standard (`01-standard`) is the default.

A template is a stylesheet with a header:

```css
/*
 * name: Company Report
 * description: Cover with logo, author and date; header, footer, page numbers.
 * users: Businesses, consultants, professionals
 * variables: title, subtitle, company, logo, author, department, date, version, classification
 * toc: yes
 * orientation: portrait
 */
```

- `variables`: the metadata fields the template uses, in order. The note's **Metadata** dialog suggests them (the chosen template's first) for the user to add, and covers list them. A field without a value prints its variable name in a dashed box, such as `{{company}}`, so readers of the PDF see what to set.
- `toc: yes` adds a table of contents built from the note's headings.
- `orientation: landscape` prints landscape pages.

### Metadata and placeholders

Notes have metadata: the **Metadata** button in the note toolbar. Notes start with none; users add their own variables, with any name, and the dialog lists the names PDF templates use and those Minerva fills in automatically. Names are normalized to lowercase with underscores, so "Client Name" becomes `client_name`. Fields the template does not declare still appear in its document information table.

- In the note's text, `{{client_name}}` is replaced by the value when exporting (PDF, HTML, and Markdown, where metadata is also added to the front matter). Unknown names stay as written; code is never changed.
- Always available, unless the note sets them: `title` (the note title), `date` (today, or the journal entry's date), `generated` (export date and time), `updated`, and `tags`.
- `logo` and `photo` take an HTTPS image address or a file link from the note.
- Templates (Templates page) can carry metadata and a PDF template; notes filled from them inherit both, with `{{date}}` and similar placeholders filled in.

### Layout

Each template is applied to a shared layout (`web/pdf-layout.html`) with these regions, all hidden by default; a template shows the ones it uses, for example `.pdf-cover { display: flex; }`:

| Region                       | Contents                                                                                 |
| ---------------------------- | ---------------------------------------------------------------------------------------- |
| `.pdf-cover`                 | Logo, classification, title, subtitle, and the template's fields; ends with a page break |
| `.pdf-header`, `.pdf-footer` | Repeated at the top and bottom of every page                                             |
| `.pdf-titleblock`            | The note icon, title, date, and tags (shown unless hidden)                               |
| `.pdf-docinfo`               | A table of all fields, template fields first                                             |
| `.pdf-abstract`              | The `abstract` and `keywords` fields                                                     |
| `.pdf-toc`                   | The table of contents (with `toc: yes`)                                                  |
| `.pdf-content`               | The note                                                                                 |
| `.pdf-approval`              | Prepared by / Approved by, with signature and date columns                               |

A template can replace the layout with its own `{id}.html` (see `16-resume.html`). Placeholders:

- `{{key}}`: the field's value, HTML-escaped.
- `{{#key}}...{{/key}}`: shown only when the field has a value; `{{^key}}...{{/key}}`: only when it does not.
- `{{{content}}}`, `{{{toc}}}`, `{{{titleblock}}}`, `{{{cover_facts}}}`, `{{{metadata_table}}}`, `{{{cover_image}}}`: HTML regions built by Minerva. Field values can never be inserted unescaped.

### Printing notes

- Page margins come from `@page { margin: … }`. WebKit ignores them, so the Linux desktop app gives its print jobs 18/17/20/17 mm paper margins instead.
- Headers and footers repeat on every page in Chromium and Firefox; WebKit prints them once. Page numbers use CSS page margin boxes (`@page { @bottom-right { content: counter(page) } }`), which Chromium-based browsers print; elsewhere they are left out.
- Headings are numbered with `counter-set`, so numbering continues across blocks.
- Inline tags carry their name, so templates can style particular tags: the Security Assessment turns `#critical`, `#high`, `#medium`, `#low`, and `#info` into severity labels, and the Project Status Report styles `#on-track`, `#at-risk`, `#off-track`, `#blocked`, and `#done`.

## Optional server workers

`server/` contains independently discovered executable plugins, separate from the
built-in themes above. Unlike themes, these are never embedded or restored from a
compiled fallback. Missing, disabled, or invalid worker plugins contribute no
commands. Only administrators should be able to write this directory.

[Superhuman Learning](server/superhuman-learning/README.md) adds `/summary`, `/wisdom`, `/youtube`, and `/transcribe` using Python 3, optional media dependencies, and existing AI provider profiles. Its documentation covers
installation, diagnostics, the versioned worker protocol and current limits.
