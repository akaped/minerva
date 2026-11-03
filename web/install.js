/**
 * The installable app on this device: the service worker (`/sw.js`), the
 * choice to keep notes for reading offline, and the browser's install prompt.
 */

const KEY = "minerva-offline-reading";
let installEvent = null;
const listeners = new Set();

/** Whether the app runs installed (its own window, not a browser tab). */
export function installed() {
  return (
    globalThis.matchMedia?.("(display-mode: standalone)").matches ||
    globalThis.navigator?.standalone === true
  );
}

/**
 * Whether notes opened on this device are kept for reading offline: what the
 * person chose, or, before any choice, yes when the app is installed.
 */
export function offlineReading(storage = globalThis.localStorage) {
  let chosen = null;
  try {
    chosen = storage.getItem(KEY);
  } catch {}
  return chosen === null ? installed() : chosen === "1";
}

function tellWorker(message) {
  navigator.serviceWorker?.ready
    .then((r) => r.active?.postMessage(message))
    .catch(() => {});
}

export function setOfflineReading(on) {
  try {
    localStorage.setItem(KEY, on ? "1" : "0");
  } catch {}
  tellWorker({ type: "offline-reading", on });
}

/** Registers the worker and tells it this device's choice. */
export function startWorker() {
  if (!("serviceWorker" in navigator) || !isSecureContext) return;
  navigator.serviceWorker.register("/sw.js").catch(() => {});
  tellWorker({ type: "offline-reading", on: offlineReading() });
}

// The browser offers installing the app; kept until the person asks.
globalThis.addEventListener?.("beforeinstallprompt", (e) => {
  e.preventDefault();
  installEvent = e;
  listeners.forEach((f) => f());
});
globalThis.addEventListener?.("appinstalled", () => {
  installEvent = null;
  listeners.forEach((f) => f());
});

/** Whether the browser can install the app now (Chrome, Edge, Android). */
export function canInstall() {
  return !!installEvent;
}
export function onInstallChange(f) {
  listeners.add(f);
  return () => listeners.delete(f);
}
/** Shows the browser's install dialog; resolves to whether it was installed. */
export async function install() {
  if (!installEvent) return false;
  const e = installEvent;
  installEvent = null;
  await e.prompt();
  const choice = await e.userChoice.catch(() => null);
  listeners.forEach((f) => f());
  return choice?.outcome === "accepted";
}
