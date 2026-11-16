/**
 * The extension's connection to a Minerva server: connecting through the
 * browser's extension sign-in window (PKCE), the token it gets, and the calls
 * it makes. Shared by the popup, the settings page and the background script.
 */
import { compose } from "./compose.js";

/** Firefox's promise-based `browser`, or Chrome's `chrome`. */
export const ext = globalThis.browser ?? globalThis.chrome;

/** The words, in the browser's language (see `_locales`). */
export function t(key, ...values) {
  return ext.i18n.getMessage(key, values.map(String)) || key;
}
/** Fills `data-i18n` elements (and `data-i18n-placeholder` attributes). */
export function translatePage() {
  for (const el of document.querySelectorAll("[data-i18n]"))
    el.textContent = t(el.dataset.i18n);
  for (const el of document.querySelectorAll("[data-i18n-placeholder]"))
    el.placeholder = t(el.dataset.i18nPlaceholder);
  document.documentElement.lang = ext.i18n.getUILanguage();
}

/** `{server, token, user, expires_at}`, or null when not connected. */
export async function connection() {
  const { connection } = await ext.storage.local.get("connection");
  return connection?.token ? connection : null;
}
export async function remembered() {
  const { last = {} } = await ext.storage.local.get("last");
  return last;
}
export async function remember(values) {
  await ext.storage.local.set({ last: { ...(await remembered()), ...values } });
}

/** An error from the server, with its status and message. */
export class ServerError extends Error {
  constructor(status, message) {
    super(message);
    this.status = status;
  }
}
/** A call to the server's API, signed in with the clipper's token. */
export async function call(method, path, body, conn) {
  conn ??= await connection();
  if (!conn) throw new ServerError(401, t("notConnected"));
  let response;
  try {
    response = await fetch(`${conn.server}/api/v1/${path}`, {
      method,
      credentials: "omit",
      headers: {
        authorization: `Bearer ${conn.token}`,
        "x-minerva-client": "1",
        ...(body ? { "content-type": "application/json" } : {}),
      },
      body: body ? JSON.stringify(body) : undefined,
    });
  } catch {
    throw new ServerError(0, t("unreachable", conn.server));
  }
  const value = await response.json().catch(() => ({}));
  if (!response.ok) {
    if (response.status === 401) await ext.storage.local.remove("connection");
    throw new ServerError(
      response.status,
      response.status === 401 ? t("expired") : value.error || response.statusText,
    );
  }
  return value;
}

function base64url(bytes) {
  return btoa(String.fromCharCode(...new Uint8Array(bytes)))
    .replace(/\+/g, "-")
    .replace(/\//g, "_")
    .replace(/=+$/, "");
}
/** A PKCE verifier and its S256 challenge. */
export async function pkce() {
  const verifier = base64url(crypto.getRandomValues(new Uint8Array(32)));
  const digest = await crypto.subtle.digest("SHA-256", new TextEncoder().encode(verifier));
  return { verifier, challenge: base64url(digest) };
}
/** Which browser this is, for the token's name. */
export function browserName() {
  if (typeof globalThis.browser !== "undefined" && navigator.userAgent.includes("Firefox"))
    return "firefox";
  if (navigator.userAgent.includes("Edg/")) return "edge";
  return "chrome";
}

/**
 * Connects to the server at `origin`: the person approves in Minerva, in the
 * browser's sign-in window, and the extension collects its token. The
 * permission to reach the server must already be granted (it needs a click).
 */
export async function connect(origin) {
  const health = await fetch(`${origin}/api/v1/health`, { credentials: "omit" })
    .then((r) => r.json())
    .catch(() => null);
  if (health?.status !== "ok") throw new ServerError(0, t("notMinerva", origin));
  const { verifier, challenge } = await pkce();
  const state = base64url(crypto.getRandomValues(new Uint8Array(16)));
  const redirect = ext.identity.getRedirectURL("clipper");
  const start = new URL(`${origin}/clipper/connect`);
  start.search = new URLSearchParams({
    redirect_uri: redirect,
    state,
    challenge,
    browser: browserName(),
  });
  let back;
  try {
    back = new URL(await ext.identity.launchWebAuthFlow({ url: start.href, interactive: true }));
  } catch {
    throw new ServerError(0, t("connectCancelled"));
  }
  if (back.searchParams.get("state") !== state) throw new ServerError(0, t("connectCancelled"));
  if (back.searchParams.get("error")) throw new ServerError(0, t("connectCancelled"));
  const response = await fetch(`${origin}/api/v1/clipper/token`, {
    method: "POST",
    credentials: "omit",
    headers: { "content-type": "application/json", "x-minerva-client": "1" },
    body: JSON.stringify({ code: back.searchParams.get("code"), verifier, redirect_uri: redirect }),
  });
  const value = await response.json().catch(() => ({}));
  if (!response.ok) throw new ServerError(response.status, value.error || response.statusText);
  const conn = { server: origin, token: value.token, user: value.user, expires_at: value.expires_at };
  await ext.storage.local.set({ connection: conn });
  return conn;
}
/** Disconnects: revokes the token on the server, if it can, and forgets it. */
export async function disconnect() {
  const conn = await connection();
  if (conn) await call("DELETE", "clipper/token", null, conn).catch(() => {});
  await ext.storage.local.remove(["connection", "last"]);
}

/** Reads the page in `tabId`: its article, its selection, or just its details. */
export async function take(tabId, kind) {
  await ext.scripting.executeScript({ target: { tabId }, files: ["extract.js"] });
  const [result] = await ext.scripting.executeScript({
    target: { tabId },
    func: (kind) => globalThis.__minervaTake(kind),
    args: [kind],
  });
  if (!result?.result) throw new ServerError(0, t("cannotRead"));
  return result.result;
}

/** Saves a clip into `folderId`; returns the server's answer. */
export async function save(kind, taken, extra, folderId, options = {}) {
  const note = compose(kind, taken, extra, t);
  return call("POST", "clipper/clip", {
    folder_id: folderId,
    title: options.title?.trim() || note.title,
    url: note.url,
    markdown: note.markdown,
    tags: options.tags || [],
  });
}
/** The address that opens a saved note in Minerva. */
export function noteAddress(conn, saved) {
  return saved.address || `${conn.server}/#note/${saved.id}`;
}
