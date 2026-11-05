/**
 * Notes kept on this device for editing offline (with "Keep notes for
 * offline" on): each note's collaborative document and the changes made to it
 * that the server has not received yet, in IndexedDB. A note opened without a
 * connection starts from them; its changes are sent and merged when the
 * server can be reached again. Everything is deleted on signing out (here and
 * by the service worker) and is kept per account.
 */
const DB = "minerva-offline";
const STORE = "docs";

function database() {
  return new Promise((resolve, reject) => {
    if (typeof indexedDB === "undefined")
      return reject(new Error("No IndexedDB"));
    const request = indexedDB.open(DB, 1);
    request.onupgradeneeded = () => request.result.createObjectStore(STORE);
    request.onsuccess = () => resolve(request.result);
    request.onerror = () => reject(request.error);
  });
}
async function run(mode, work) {
  const db = await database();
  try {
    return await new Promise((resolve, reject) => {
      const tx = db.transaction(STORE, mode);
      const result = work(tx.objectStore(STORE));
      tx.oncomplete = () => resolve(result?.result);
      tx.onerror = () => reject(tx.error);
      tx.onabort = () => reject(tx.error);
    });
  } finally {
    db.close();
  }
}

/** Keeps note `id`'s document (`state`) and unsent changes, for `user`. */
export async function keepDoc(user, id, state, pending) {
  await run("readwrite", (store) =>
    store.put({ user, state, pending, at: Date.now() }, id),
  );
}
/** What this device kept of note `id` for `user`, or null. */
export async function keptDoc(user, id) {
  const record = await run("readonly", (store) => store.get(id)).catch(
    () => null,
  );
  return record && record.user === user ? record : null;
}
/** The notes kept for `user`, by ID. */
export async function keptIds(user) {
  const all = await run("readonly", (store) => store.getAll()).catch(() => []);
  const keys = await run("readonly", (store) => store.getAllKeys()).catch(
    () => [],
  );
  return keys.filter((_, i) => all[i]?.user === user);
}
/** Deletes everything kept (signing out, another account). */
export async function forgetDocs() {
  await new Promise((resolve) => {
    if (typeof indexedDB === "undefined") return resolve();
    const request = indexedDB.deleteDatabase(DB);
    request.onsuccess = request.onerror = request.onblocked = () => resolve();
  });
}
