import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";

test("offline sidebar pages keep each workspace and cursor separate", async () => {
  const stores = new Map();
  const store = (name) => {
    if (!stores.has(name)) stores.set(name, new Map());
    return stores.get(name);
  };
  const caches = {
    match: async (key, { cacheName }) => store(cacheName).get(key)?.clone(),
    open: async (name) => ({
      put: async (key, response) => store(name).set(key, response.clone()),
      keys: async () => [...store(name).keys()].map((key) => ({ url: `https://minerva.test${key}` })),
      delete: async (key) => store(name).delete(key),
    }),
  };
  let online = true;
  const context = vm.createContext({
    self: { addEventListener() {}, location: { origin: "https://minerva.test" } },
    URL, Response, Headers, caches,
    fetch: async (request) => {
      if (!online) throw new Error("Offline");
      const q = new URL(request.url).searchParams;
      return Response.json({ notes: [{ id: `${q.get("workspace")}:${q.get("after")}` }], next: null });
    },
  });
  vm.runInContext(fs.readFileSync(new URL("../sw.js", import.meta.url), "utf8"), context);
  await context.setSetting("offline", "1");
  const urls = ["workspace=a&after=", "workspace=a&after=second", "workspace=b&after="];
  for (const query of urls) {
    await context.data({ url: `https://minerva.test/api/v1/bootstrap/notes?${query}` });
  }
  online = false;
  for (const query of urls) {
    const response = await context.data({ url: `https://minerva.test/api/v1/bootstrap/notes?${query}` });
    const q = new URLSearchParams(query);
    assert.equal(response.headers.get("x-minerva-offline"), "1");
    assert.equal((await response.json()).notes[0].id, `${q.get("workspace")}:${q.get("after")}`);
  }
  const missing = await context.data({ url: "https://minerva.test/api/v1/bootstrap/notes?workspace=c&after=" });
  assert.equal(missing.status, 503);
});
