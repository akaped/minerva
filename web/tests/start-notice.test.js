import { test } from "node:test";
import assert from "node:assert/strict";
import { startNotice, serverPage, withoutNotice } from "../start-notice.js";

test("says why the last server is not shown", () => {
  const silent = startNotice("?missed=timeout&server=0123abcd&label=Work+%26+home");
  assert.equal(silent.text, "Work & home did not answer, so Minerva opened this computer instead.");
  assert.equal(silent.retry, true);
  assert.equal(silent.server, "0123abcd");
  const out = startNotice("?missed=signed_out&server=ab12");
  assert.match(out.text, /^You are signed out of Your server\./);
  assert.equal(out.retry, false);
  assert.equal(startNotice("?missed=identity&server=ab").retry, false);
});

test("ignores missing or odd parameters", () => {
  assert.equal(startNotice(""), null);
  assert.equal(startNotice("?missed=nope&server=ab"), null);
  assert.equal(startNotice("?missed=timeout"), null);
  assert.equal(startNotice("?missed=timeout&server=../x"), null);
});

test("server pages and clean addresses", () => {
  assert.equal(
    serverPage({ protocol: "minerva:", host: "local.localhost" }, "ab12"),
    "minerva://sab12.localhost/",
  );
  assert.equal(
    serverPage({ protocol: "https:", host: "minerva.local.localhost" }, "ab12"),
    "https://minerva.sab12.localhost/",
  );
  assert.equal(
    withoutNotice("minerva://local.localhost/?missed=timeout&server=a&label=b&workspace=w#note/1"),
    "/?workspace=w#note/1",
  );
  assert.equal(withoutNotice("minerva://local.localhost/?missed=x&server=a"), "/");
});
