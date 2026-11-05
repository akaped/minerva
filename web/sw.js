/*
 * Minerva's service worker: the installable app and reading offline.
 *
 * - The app's pages and code are fetched from the network first, as before,
 *   and kept, so the app opens without a connection.
 * - With offline reading on for this device (Account → This device), the
 *   workspace list, the notes opened and their pictures are kept too, and
 *   shown read-only when the server cannot be reached.
 * - Kept notes are deleted on signing out, when the server says the session
 *   ended (a 401 for the workspace list or a note), when another account
 *   signs in, and when offline reading is turned off.
 *
 * The server fills in VERSION, so a new release replaces the kept app.
 */
const VERSION = "__VERSION__";
const SHELL = `minerva-shell-${VERSION}`;
const DATA = "minerva-data";
const SETTINGS = "minerva-settings";
// Pages and files the app needs to open.
const PAGES = ["/", "/pane", "/capture"];
const FILES = [
  "/app.js",
  "/style.css",
  "/capture.js",
  "/manifest.webmanifest",
  "/favicon.ico",
  "/favicon.png",
  "/icons/icon-192.png",
];
// The most notes (and files) kept for offline: a whole workspace can be kept.
const MAX_KEPT = 3000;
// Files larger than this are not kept.
const MAX_FILE = 25 * 1024 * 1024;

self.addEventListener("install", (event) => {
  event.waitUntil(
    caches
      .open(SHELL)
      .then((cache) => cache.addAll([...PAGES, ...FILES]))
      .then(() => self.skipWaiting()),
  );
});

self.addEventListener("activate", (event) => {
  event.waitUntil(
    caches
      .keys()
      .then((keys) =>
        Promise.all(
          keys
            .filter((k) => k.startsWith("minerva-shell-") && k !== SHELL)
            .map((k) => caches.delete(k)),
        ),
      )
      .then(() => self.clients.claim()),
  );
});

// Settings kept as small responses, since a worker has no localStorage.
async function setting(name) {
  const r = await caches.match(`/__minerva/${name}`, { cacheName: SETTINGS });
  return r ? r.text() : "";
}
async function setSetting(name, value) {
  const cache = await caches.open(SETTINGS);
  await cache.put(`/__minerva/${name}`, new Response(value));
}
async function forget() {
  await caches.delete(DATA);
  await setSetting("user", "");
  // Notes kept for editing offline (see offline-docs.js).
  await new Promise((resolve) => {
    const request = indexedDB.deleteDatabase("minerva-offline");
    request.onsuccess = request.onerror = request.onblocked = () => resolve();
  });
}

self.addEventListener("message", (event) => {
  const { type, on } = event.data || {};
  if (type === "offline-reading")
    event.waitUntil(
      setSetting("offline", on ? "1" : "").then(() => (on ? null : forget())),
    );
  if (type === "forget") event.waitUntil(forget());
});

/** What the worker keeps of a request, or null for what it leaves alone. */
function kind(url, request) {
  if (url.origin !== self.location.origin || request.method !== "GET")
    return null;
  if (request.mode === "navigate")
    return PAGES.includes(url.pathname) ? "page" : null;
  if (
    FILES.includes(url.pathname) ||
    url.pathname === "/mermaid.js" ||
    url.pathname === "/emoji.json"
  )
    return "file";
  if (url.pathname === "/api/v1/bootstrap") return "data";
  if (/^\/api\/v1\/notes\/[0-9a-f-]{36}$/.test(url.pathname)) return "data";
  // Files: pictures and previews, and the files themselves (PDFs, sound…).
  if (/^\/api\/v1\/attachments\/[0-9a-f-]{36}(\/preview)?$/.test(url.pathname))
    return "data";
  return null;
}

// Things shared to the app from other apps: the text goes on in the address,
// the files wait here until the quick-note page takes them.
const SHARED = "minerva-shared";
async function received(request) {
  const form = await request.formData();
  const cache = await caches.open(SHARED);
  for (const key of await cache.keys()) await cache.delete(key);
  const files = form.getAll("files").filter((f) => f && f.size);
  let i = 0;
  for (const file of files.slice(0, 10)) {
    await cache.put(
      `/__minerva/shared/${i++}`,
      new Response(file, {
        headers: {
          "content-type": file.type || "application/octet-stream",
          "x-file-name": encodeURIComponent(file.name || "file"),
        },
      }),
    );
  }
  const query = new URLSearchParams();
  for (const name of ["title", "text", "url"])
    if (form.get(name)) query.set(name, form.get(name));
  if (i) query.set("files", String(i));
  return Response.redirect(`/capture?${query}`, 303);
}

self.addEventListener("fetch", (event) => {
  const request = event.request;
  const url = new URL(request.url);
  if (url.origin !== self.location.origin) return;
  if (url.pathname === "/capture/share" && request.method === "POST") {
    event.respondWith(received(request));
    return;
  }
  // Signing out deletes what was kept, whatever the answer.
  if (url.pathname === "/api/v1/logout" && request.method === "POST") {
    event.waitUntil(forget());
    return;
  }
  const what = kind(url, request);
  if (what === "page" || what === "file")
    event.respondWith(networkFirst(request, SHELL, { ignoreSearch: true }));
  else if (what === "data") event.respondWith(data(request));
});

async function networkFirst(request, cacheName, options = {}) {
  try {
    const response = await fetch(request);
    if (response.ok) {
      const cache = await caches.open(cacheName);
      await cache.put(new URL(request.url).pathname, response.clone());
    }
    return response;
  } catch (error) {
    const kept = await caches.match(new URL(request.url).pathname, {
      cacheName,
      ...options,
    });
    if (kept) return kept;
    throw error;
  }
}

// The workspace list, notes and pictures: from the server when it answers,
// otherwise the kept copy, marked as one.
async function data(request) {
  const url = new URL(request.url);
  const keep = (await setting("offline")) === "1";
  const fallback = async (response) => {
    const kept =
      keep && (await caches.match(url.pathname, { cacheName: DATA }));
    if (!kept) return response || offline();
    const headers = new Headers(kept.headers);
    headers.set("x-minerva-offline", "1");
    return new Response(kept.body, { status: kept.status, headers });
  };
  let response;
  try {
    response = await fetch(request);
  } catch {
    return fallback(null);
  }
  // A proxy answering for a server that is down.
  if ([502, 503, 504].includes(response.status)) return fallback(response);
  if (response.status === 401) await forget();
  if (!keep || !response.ok) return response;
  if (Number(response.headers.get("content-length") || 0) > MAX_FILE)
    return response;
  if (url.pathname === "/api/v1/bootstrap") {
    // Another account's notes are never shown to this one.
    const user = (await response.clone().json())?.user?.id || "";
    if (user !== (await setting("user"))) {
      const before = await setting("user");
      await caches.delete(DATA);
      await setSetting("user", user);
      if (before) await forget().then(() => setSetting("user", user));
    }
  }
  const cache = await caches.open(DATA);
  await cache.put(url.pathname, response.clone());
  const keys = await cache.keys();
  for (const old of keys.slice(0, Math.max(0, keys.length - MAX_KEPT)))
    if (!old.url.endsWith("/api/v1/bootstrap")) await cache.delete(old);
  return response;
}

function offline() {
  return new Response(
    JSON.stringify({
      error:
        "You are offline, and this was not kept on this device. It opens again when you are back online.",
    }),
    {
      status: 503,
      headers: { "content-type": "application/json", "x-minerva-offline": "1" },
    },
  );
}
