import { test } from "node:test";
import assert from "node:assert/strict";
import fs from "node:fs";
import vm from "node:vm";

// Exercise the actual polling/approval UI without executing any OS command.
const source = fs.readFileSync(new URL("../ai.js", import.meta.url), "utf8");
function setup() {
  const calls = [],
    timers = [],
    dialogs = [];
  const api = async (path, method, body) => {
    calls.push({ path, method, body });
    return path.includes("pending")
      ? [{ id: "one", command: "echo hello", directory: "/tmp" }]
      : {};
  };
  const document = {
    body: { append: (d) => dialogs.push(d) },
    createElement: () => ({
      buttons: {},
      showModal() {},
      remove() {
        this.removed = true;
      },
      close() {
        this.onclose();
      },
    }),
  };
  const context = vm.createContext({
    document,
    $t: (text) => text,
    $: (selector, root) => (root.buttons[selector] ||= {}),
    setTimeout: (fn) => {
      timers.push(fn);
      return fn;
    },
    clearTimeout: (fn) => {
      const i = timers.indexOf(fn);
      if (i >= 0) timers.splice(i, 1);
    },
  });
  vm.runInContext(source.slice(source.indexOf("function pollShell(")), context);
  const controller = new AbortController();
  const stop = context.pollShell(api, (x) => x, "chat", controller.signal);
  return { calls, timers, dialogs, controller, stop };
}
const tick = () => new Promise((resolve) => setImmediate(resolve));

test("a pending desktop command waits for an explicit approval", async () => {
  const s = setup();
  s.timers.shift()();
  await tick();
  assert.equal(s.dialogs.length, 1);
  assert.equal(s.calls.filter((c) => c.path.includes("approval")).length, 0);
  s.dialogs[0].buttons["[data-allow]"].onclick();
  await tick();
  const approved = s.calls.find((c) => c.path.includes("approval"));
  assert.equal(approved.body.approved, true);
  s.stop();
});
test("closing the approval dialog denies; stopping never approves", async () => {
  const s = setup();
  s.timers.shift()();
  await tick();
  s.dialogs[0].close();
  await tick();
  assert.equal(
    s.calls.find((c) => c.path.includes("approval")).body.approved,
    false,
  );
  s.stop();
  const aborted = setup();
  aborted.timers.shift()();
  await tick();
  aborted.controller.abort();
  await tick();
  assert.equal(
    aborted.calls.filter((c) => c.path.includes("approval")).length,
    0,
  );
  assert(aborted.calls.some((c) => c.path.includes("cancel")));
  assert.equal(aborted.timers.length, 0);
});
