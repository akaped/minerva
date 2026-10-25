import { test } from "node:test";
import assert from "node:assert/strict";
import { youtubeVideo } from "../youtube.js";
import { noteLocation, reorderTabs } from "../note-tabs.js";

test("only a standalone supported YouTube link becomes a player", () => {
  for (const link of [
    "https://www.youtube.com/watch?v=M7lc1UVf-VE",
    "https://youtu.be/M7lc1UVf-VE",
    "https://m.youtube.com/shorts/M7lc1UVf-VE",
    "https://youtube.com/live/M7lc1UVf-VE",
    "https://www.youtube-nocookie.com/embed/M7lc1UVf-VE",
  ])
    assert.equal(youtubeVideo(`  ${link}\n`).id, "M7lc1UVf-VE");
  for (const link of [
    "Watch https://youtu.be/M7lc1UVf-VE",
    "https://youtu.be/M7lc1UVf-VE\nOther text",
    "[Video](https://youtu.be/M7lc1UVf-VE)",
    "https://youtube.com.evil.test/watch?v=M7lc1UVf-VE",
    "https://youtube.com@evil.test/watch?v=M7lc1UVf-VE",
    "https://youtu.be/invalid",
    "javascript:alert(1)",
    "https://youtu.be/M7lc1UVf-VE/extra",
  ])
    assert.equal(youtubeVideo(link), null, link);
  assert.equal(
    youtubeVideo("https://youtu.be/M7lc1UVf-VE?t=1h2m3s").start,
    3723,
  );
  assert.equal(
    youtubeVideo("https://youtube.com/watch?v=M7lc1UVf-VE&start=42").start,
    42,
  );
});
test("tabs keep identity and isolate server-qualified note links", () => {
  const id = "12345678-1234-1234-1234-123456789012";
  assert.deepEqual(
    noteLocation(`https://one.example/?note=${id}`, "http://localhost"),
    { origin: "https://one.example", id },
  );
  assert.deepEqual(
    noteLocation(`https://two.example/#note/${id}`, "http://localhost"),
    { origin: "https://two.example", id },
  );
  assert.throws(() => noteLocation("javascript:alert(1)", "http://localhost"));
  assert.throws(() =>
    noteLocation("https://user:pass@server.example", "http://localhost"),
  );
  const a = { key: "a" },
    b = { key: "b" },
    c = { key: "c" };
  const original = [a, b, c];
  const result = reorderTabs(original, "a", "c");
  assert.deepEqual(result, [b, c, a]);
  assert.equal(result[2], a);
  assert.deepEqual(original, [a, b, c]);
  assert.deepEqual(reorderTabs(original, "missing", "a"), original);
});

test("playing video floats inside its editor pane, then returns without changing identity", async () => {
  const { videoPlacement } = await import("../youtube.js");
  const input = {
    rect: { left: 36, top: 220, bottom: 520, width: 460 },
    main: { right: 520, width: 520 },
    top: 88,
    viewportHeight: 800,
    playing: true,
    floating: false,
  };
  const inline = videoPlacement(input);
  assert.equal(inline.floating, false);
  assert.equal(inline.left, 36);
  assert.equal(inline.height, 300);
  assert.equal(inline.clip, 0);
  // Partly under the editor toolbar: that part is clipped, not floated.
  const under = videoPlacement({
    ...input,
    playing: false,
    rect: { ...input.rect, top: 40, bottom: 340 },
  });
  assert.equal(under.floating, false);
  assert.equal(under.clip, 48);
  const floating = videoPlacement({
    ...input,
    rect: { ...input.rect, top: -340, bottom: -40 },
  });
  assert.equal(floating.floating, true);
  assert.equal(floating.left + floating.width, 508);
  assert.equal(floating.top, 88);
  assert.equal(videoPlacement({ ...input, floating: true }).floating, false);
  assert.equal(
    videoPlacement({
      ...input,
      playing: false,
      rect: { ...input.rect, top: -340, bottom: -40 },
    }).floating,
    false,
  );
  assert.equal(
    videoPlacement({
      ...input,
      playing: false,
      floating: true,
      rect: { ...input.rect, top: -340, bottom: -40 },
    }).floating,
    true,
  );
});
