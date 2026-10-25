# Project landing page

The repository-root `index.html` explains Minerva to visitors and invites contributions.
It is a self-contained, responsive page with no build step, analytics, remote fonts,
or third-party scripts. Its example workspace is an explicitly labelled illustration,
not a live Minerva instance. `web/index.html` remains the actual application.

## Publish on GitHub

1. Push the repository to `git@github.com:akaped/minerva.git`.
2. In **Settings → Pages**, set the source to **GitHub Actions**.
3. Run **Publish project page** from Actions, or push a change to `index.html` on
   `main` or `master`.
4. After the deployment succeeds, the default project URL is
   `https://akaped.github.io/minerva/`.

The workflow publishes only `index.html` and `.nojekyll`, never the whole repository,
application data, attachments, or configuration. A custom domain is not configured.
The URL is the intended deployment address; adding these files does not itself
publish the page or enable Pages on GitHub.

## Preview locally

From the repository root, run `python3 -m http.server 8080 --bind 127.0.0.1` and open
http://127.0.0.1:8080/. Stop the local server when finished. Do not expose this preview
server publicly: it serves the checkout, not just the landing page.

Keep claims aligned with shipped features. When updating media or AI capabilities,
update both this page and the README. Avoid fabricated adoption numbers, testimonials,
or untested claims of superiority over other products.
