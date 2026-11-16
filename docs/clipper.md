# Web clipper

The Minerva Web Clipper is a browser extension for Firefox and for Chrome and the browsers built on it (Edge, Brave, Vivaldi, Opera). It saves what you are reading as a new note:

- **Article:** the page's main text, found the way Firefox's Reader View finds it, without menus, ads and footers. Headings, lists, quotes, tables, code, links and pictures are kept. Pictures stay linked to the site; they are not copied into Minerva.
- **Selection:** only the part of the page you selected, with its formatting. When something is selected, the clipper picks this for you.
- **Link only:** the page's address and description, as a bookmark.

Each clip becomes a note in the folder you choose, titled after the page (you can change the title), with optional tags. It starts with a "Clipped from" line linking to the page, and **Document details** keeps the page's address as `source`. The clipper remembers the last folder and tags.

Right-click a page, a selection, a link or a picture and choose **Clip … to Minerva** to save it straight into the folder you used last, without opening the clipper. The toolbar button shows ✓ when it is saved. The keyboard shortcut Alt+Shift+M opens the clipper; change it in the browser's extension shortcut settings.

## Connecting

1. Install the extension. Its settings page opens.
2. Enter your server's address, such as `notes.example.com`, and choose **Connect**. The browser asks once whether the clipper may reach that server.
3. A Minerva window opens. Sign in if you are not signed in already (with your password, two-factor code or single sign-on), then choose **Connect**.

The server then gives the clipper its own token. That token can only list the folders you can add notes to and add new notes. It cannot open, search, change or delete notes, or anything else, and it works for a year. It shows as **Web clipper (Firefox)** or **(Chrome)** under Account → Personal API tokens. Revoke it there to disconnect a browser, or choose **Disconnect** in the clipper's settings. Changing your password also disconnects it. Connecting the same browser again replaces its token.

Server administrator accounts cannot connect a clipper; they hold no notes.

## Privacy

The clipper reads a page only when you click it, its menu or its shortcut. It sends what you clip only to your own server; nothing goes to anyone else. It reaches only the server you connected to, and asks for that permission when connecting. Disconnecting removes the permission. The token is kept in the browser's extension storage.

## Building and installing

```sh
npm install
npm run build:clipper
```

This writes `dist/clipper/chrome/` and `dist/clipper/firefox/` and a zip of each, versioned like the server (`Cargo.toml`).

- **Chrome, Edge, Brave:** open `chrome://extensions`, turn on **Developer mode**, choose **Load unpacked** and pick `dist/clipper/chrome`. To publish, upload `minerva-clipper-chrome-<version>.zip` to the Chrome Web Store (or Edge Add-ons).
- **Firefox (140 or later):** for a trial, open `about:debugging#/runtime/this-firefox`, choose **Load Temporary Add-on** and pick `dist/clipper/firefox/manifest.json`; it stays until Firefox restarts. To install it for good, Firefox needs it signed: upload `minerva-clipper-firefox-<version>.zip` to addons.mozilla.org, listed or unlisted (self-distribution). Check the package first with `npx web-ext lint --source-dir dist/clipper/firefox`: it should show no errors and two `UNSAFE_VAR_ASSIGNMENT` warnings, both inside Mozilla's Readability library, which works on an inert copy of the page. The add-on's ID is `web-clipper@minerva-notes`.

The code is bundled but not minified, so store reviewers can read it. The source is in `clipper/`:

- `src/extract.js` is injected into the page on a click. It reads the article with Readability, or the selection, and converts it to Markdown with the same converter the web app uses for pasted HTML (`web/editor-tools.js`). It parses in an inert document, keeps only http(s) links and pictures, and drops pictures embedded in the page as data.
- `src/minerva.js` handles connecting, the token and calls to the server. `src/compose.js` builds the note's Markdown, cut at 190,000 bytes with a notice.
- `src/popup.*`, `src/options.*` and `src/background.js` are the toolbar popup, the settings page and the right-click menus.
- `manifest.js` writes each browser's manifest. Chrome runs the background script as a service worker, and Firefox as an event page with an ID.
- `_locales/` holds the English and Italian text.
- `tests/` holds the Node tests, run by `npm test`.

## How connecting works

Connecting uses the browser's extension sign-in window (`identity.launchWebAuthFlow`), with PKCE:

1. The extension opens `/clipper/connect?redirect_uri=…&state=…&challenge=…&browser=…` on the server.
   - `redirect_uri` is the browser's own address for handing sign-ins back to the extension: `https://<id>.chromiumapp.org/clipper` in Chrome, or `https://<id>.extensions.allizom.org/clipper` in Firefox. The server accepts only those.
2. The web app keeps the request for the tab while you sign in, and asks you to approve. It then calls `POST /api/v1/clipper/approve` with your session.
3. The server keeps a one-time code for two minutes. It answers with the redirect address carrying `code` and `state`. The browser hands that address to the extension without loading it.
4. The extension checks `state`. It then exchanges the code and its PKCE verifier at `POST /api/v1/clipper/token` for the clipper token.

See [the API](api.md#web-clipper) for the endpoints.
