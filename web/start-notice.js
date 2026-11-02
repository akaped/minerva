/**
 * The desktop app opens this computer's page when the server open last cannot
 * be shown, and says why in the address (`?missed=…&server=…&label=…`, see
 * `crates/minerva-desktop/src/start.rs`). `startNotice` turns that into the
 * message the page shows, or `null` when there is nothing to say.
 */
const MESSAGES = {
  timeout: (label) =>
    `${label} did not answer, so Minerva opened this computer instead.`,
  offline: (label) =>
    `${label} could not be reached, so Minerva opened this computer instead.`,
  signed_out: (label) =>
    `You are signed out of ${label}. Sign in again from the server list.`,
  identity: (label) =>
    `A different server now answers at ${label}'s address. Check it in the server list.`,
};

export function startNotice(search) {
  const params = new URLSearchParams(search);
  const message = MESSAGES[params.get("missed")];
  const server = params.get("server") || "";
  if (!message || !/^[a-z0-9]+$/i.test(server)) return null;
  const label = (params.get("label") || "").trim().slice(0, 80) || "Your server";
  const missed = params.get("missed");
  return {
    text: message(label),
    // Trying again helps only when the server may answer now.
    retry: missed === "timeout" || missed === "offline",
    server,
  };
}

/** The address of a registered server's page, from this computer's page. */
export function serverPage({ protocol, host }, server) {
  return `${protocol}//${host.replace(/local\.localhost$/, `s${server}.localhost`)}/`;
}

/** The address without the notice's parameters, so a reload does not repeat it. */
export function withoutNotice(href) {
  const url = new URL(href);
  for (const key of ["missed", "server", "label"]) url.searchParams.delete(key);
  return url.pathname + url.search + url.hash;
}
