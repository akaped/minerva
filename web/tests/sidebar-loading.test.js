import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";

// Exercise the app's actual loader without the editor's DOM and media setup.
const source = fs.readFileSync(new URL("../app.js", import.meta.url), "utf8");
const loader = source.slice(source.indexOf("let refreshGeneration = 0;"), source.indexOf("/** The workspace this person last had open"));

test("sidebar loads only the chosen workspace, follows cursors and preserves other tabs", async () => {
  const requests = [];
  const pruned = [];
  const metadata = {
    server_id: "server", user: { id: "member" }, language: "en", notes_paged: true,
    workspaces: [{ id: "a", name: "A" }, { id: "b", name: "B" }],
    folders: [{ id: "fa", workspace_id: "a" }, { id: "fb", workspace_id: "b" }],
    notes: [], shared_notes: [], default_workspace: "a",
  };
  const context = vm.createContext({
    URLSearchParams, location: { search: "", origin: "https://minerva.test" },
    data: null, workspaceId: null, shareMode: false,
    $t: (s) => s, language: () => "en", redrawSidebar() {},
    lastWorkspace: (state) => { assert.equal(state.user.id, "member"); return "b"; },
    noteTabs: { prune: (kept, origin, considered) => pruned.push({ kept, considered }) },
    api: async (path) => {
      requests.push(path);
      if (path === "bootstrap?notes=0") return structuredClone(metadata);
      const q = new URL(`https://minerva.test/${path}`).searchParams;
      const ws = q.get("workspace");
      if (ws === "a") return { notes: [{ id: "a-note", folder_id: "fa" }], next: null };
      assert.equal(ws, "b");
      return q.get("after")
        ? { notes: [{ id: "record", folder_id: "fb", record: {} }, { id: "archived", folder_id: "fb", archived: true }], next: null }
        : { notes: [{ id: "b-note", folder_id: "fb" }], next: "cursor" };
    },
  });
  vm.runInContext(loader, context);
  await context.refresh();
  assert.equal(context.workspaceId, "b");
  assert.equal(context.data.notes.length, 1);
  assert.equal(context.data.record_notes[0].id, "record");
  assert.equal(context.data.archived_notes[0].id, "archived");
  assert.equal(requests.filter((p) => p.includes("workspace=b")).length, 2);
  assert.equal(requests.filter((p) => p.includes("workspace=a")).length, 0);
  context.workspaceId = "a";
  await context.refresh();
  assert.equal(context.data.notes[0].id, "a-note");
  assert.equal(pruned.at(-1).considered.has("b-note"), false);
  // A later refresh can remove a deleted note from this workspace's tabs.
  await context.refresh();
  assert.equal(pruned.at(-1).considered.has("a-note"), true);
});
