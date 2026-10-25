# Contributing to Minerva

Thanks for helping build a better home for people’s knowledge. Useful feedback, clear documentation, thoughtful design, and careful testing matter just as much as code.

## Start with something real

Try Minerva for a task you actually care about. Tell us what you were trying to do, what got in your way, and what a better experience would look like. Use the [issue tracker](https://github.com/akaped/minerva/issues) for bugs and suggestions. Search existing issues first.

For a bug, include steps to reproduce, expected and actual behavior, your Minerva revision, and your browser or operating system. Screenshots help; remove private notes, credentials, and personal information first. Never post API keys or database files.

For security concerns, avoid public exploit details or private data. Use GitHub’s private vulnerability reporting if it is enabled for this repository; otherwise ask the maintainer for a private reporting channel without disclosing the vulnerability publicly.

## Ways to help without code

- Improve onboarding, examples, and documentation.
- Create original note templates or themes and explain who they help.
- Test keyboard navigation, assistive technology, desktop sync, or export layouts.
- Suggest clearer wording or help plan translations. A full translation system is not yet available.
- Share a small, reproducible example of a workflow that feels unnecessarily difficult.

## Before a larger change

Open an issue describing the user problem and your proposed scope. Check existing work so we do not duplicate effort. Keep each contribution focused. Avoid introducing a mandatory dependency for an optional feature, and do not bundle unrelated cleanup into a fix.

Treat people with patience and respect. Critique ideas and behavior, not people. Do not harass others or publish private information.

## Development setup

Install a current Rust toolchain and C compiler, plus Node.js/npm for browser changes. The server was previously verified with Rust 1.96.0. Desktop builds have additional [platform requirements](docs/desktop.md).

```sh
git clone https://github.com/akaped/minerva.git
cd minerva
npm ci
npm run build
export MINERVA_ADMIN_EMAIL='you@example.com'
read -rs MINERVA_ADMIN_PASSWORD
export MINERVA_ADMIN_PASSWORD
cargo run --locked -p minerva-server
```

At the hidden prompt, enter a new local development password of at least 12 characters and press Enter. Open http://127.0.0.1:3000. Use a separate disposable database (`MINERVA_DATABASE=/tmp/minerva-development.db`) for tests involving data removal.

Browser assets are embedded in the server. Rebuild with `npm run build` and restart after browser changes.

## Check your work

Run checks relevant to your changes:

```sh
npm test
npm run build
cargo fmt --all --check
cargo test --locked -p minerva-server
cargo test --locked -p minerva-core -p minerva-client
```

For the optional learning plugin:

```sh
python3 -m unittest discover -s plugins/server/superhuman-learning -p 'test_*.py'
cargo test --locked -p minerva-server --test learning
```

Learning tests use local mock providers and do not require real media downloads or paid requests. Some server tests bind loopback ports. Python 3 is required for learning-plugin tests; media unit tests mock yt-dlp. Test the browser workflow when changing the editor, and check small screens when changing the public landing page.

The repository-root `index.html` is the public landing page. `web/index.html` is the actual Minerva app. Keep these separate. See [GitHub Pages](docs/github-pages.md).

## Send a pull request

1. Fork the repository and create a focused branch.
2. Make the change, update relevant documentation, and test it.
3. Open a pull request explaining the user problem, the behavior after the change, and what you verified.
4. Include screenshots for visible interface changes and note any limitations.

Do not include local databases, secrets, generated desktop packages, or unrelated files. `web/dist/app.js` is intentionally tracked and must match browser source changes.

Contributions are made under this project’s MIT License. Only submit code, text, or assets you have the right to contribute, and retain required third-party attribution.
