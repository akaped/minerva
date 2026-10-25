/** Accept only a single, bare YouTube URL. Never interpret arbitrary HTML. */
export function youtubeVideo(text) {
  const raw = text.trim();
  if (/\s/.test(raw)) return null;
  let url;
  try {
    url = new URL(raw);
  } catch {
    return null;
  }
  if (
    !["https:", "http:"].includes(url.protocol) ||
    url.username ||
    url.password ||
    url.port
  )
    return null;
  const host = url.hostname.toLowerCase();
  const parts = url.pathname.split("/").filter(Boolean);
  let id;
  if (host === "youtu.be" && parts.length === 1) id = parts[0];
  else if (
    [
      "youtube.com",
      "www.youtube.com",
      "m.youtube.com",
      "www.youtube-nocookie.com",
      "youtube-nocookie.com",
    ].includes(host)
  ) {
    if (url.pathname === "/watch") id = url.searchParams.get("v");
    else if (
      parts.length === 2 &&
      ["embed", "shorts", "live"].includes(parts[0])
    )
      id = parts[1];
  }
  if (!/^[A-Za-z0-9_-]{11}$/.test(id || "")) return null;
  const time = url.searchParams.get("t") || url.searchParams.get("start") || "";
  const match = /^(?:(\d+)h)?(?:(\d+)m)?(?:(\d+)s)?$/.exec(time);
  const start = /^\d+$/.test(time)
    ? Number(time)
    : match
      ? Number(match[1] || 0) * 3600 +
        Number(match[2] || 0) * 60 +
        Number(match[3] || 0)
      : 0;
  return {
    id,
    start: Math.min(86400, start),
    url: `https://www.youtube.com/watch?v=${id}`,
  };
}

let loading;
function loadAPI() {
  if (window.YT?.Player) return Promise.resolve(window.YT);
  return (loading ||= new Promise((resolve, reject) => {
    const timeout = setTimeout(() => {
      loading = null;
      reject(
        new Error(
          "YouTube could not load. Check your connection or open the video on YouTube.",
        ),
      );
    }, 15000);
    window.onYouTubeIframeAPIReady = () => {
      clearTimeout(timeout);
      resolve(window.YT);
    };
    const script = document.createElement("script");
    script.src = "https://www.youtube.com/iframe_api";
    script.onerror = () => {
      clearTimeout(timeout);
      loading = null;
      reject(new Error("YouTube could not load."));
    };
    document.head.append(script);
  }));
}

/** Pure positioning policy: an inline player covers its block; a playing video
 * that scrolls out of view floats, and paused or unstarted ones never start to. */
export function videoPlacement({
  rect,
  main,
  top,
  viewportHeight,
  playing,
  floating,
}) {
  const out = rect.bottom < top + 40 || rect.top > viewportHeight - 40;
  floating = out && (playing || floating);
  if (!floating)
    return {
      floating,
      left: rect.left,
      top: rect.top,
      width: rect.width,
      height: rect.bottom - rect.top,
      // The part hidden under the editor toolbar.
      clip: Math.max(0, top - rect.top),
    };
  const width = Math.max(200, Math.min(380, main.width - 24));
  return {
    floating,
    left: main.right - width - 12,
    top,
    width,
    height: Math.max(200, (width * 9) / 16) + 34,
    clip: 0,
  };
}

/** Every video block of the open note plays inline. Players live in portals
 * outside the note's markup, positioned over their blocks, so redrawing blocks
 * while typing never reloads or interrupts them. */
export function createYouTubeEmbeds({ getNote, edit, error }) {
  const players = new Map();
  let visible = true,
    frame = 0;
  function remove(block) {
    const entry = players.get(block);
    if (!entry) return;
    clearTimeout(entry.timeout);
    entry.player?.destroy?.();
    entry.portal.remove();
    players.delete(block);
  }
  function stop() {
    for (const block of [...players.keys()]) remove(block);
  }
  function layout() {
    frame = 0;
    const main = document.querySelector(".main")?.getBoundingClientRect();
    const top = Math.max(
      8,
      (document.querySelector(".editor-toolbar")?.getBoundingClientRect()
        .bottom ||
        document.querySelector(".topbar")?.getBoundingClientRect().bottom ||
        0) + 8,
    );
    for (const entry of players.values()) {
      const anchor = document.querySelector(
        `[data-video-block="${CSS.escape(entry.block)}"] .youtube-frame`,
      );
      if (!visible) {
        entry.portal.hidden = true;
        entry.player?.pauseVideo?.();
        continue;
      }
      // While blocks are redrawn the anchor is briefly missing: keep the player
      // where it is. `reconcile` removes players whose block really went away.
      if (!anchor) continue;
      const position = videoPlacement({
        rect: anchor.getBoundingClientRect(),
        main: main || { width: innerWidth, right: innerWidth },
        top,
        viewportHeight: innerHeight,
        playing: entry.playing,
        floating: entry.floating,
      });
      entry.floating = position.floating;
      Object.assign(entry.portal.style, {
        left: `${position.left}px`,
        top: `${position.top}px`,
        width: `${position.width}px`,
        height: `${position.height}px`,
        clipPath: position.clip ? `inset(${position.clip}px 0 0 0)` : "",
      });
      entry.portal.classList.toggle("floating", entry.floating);
      entry.portal.hidden = position.clip >= position.height;
    }
  }
  const schedule = () => {
    if (!frame) frame = requestAnimationFrame(layout);
  };
  addEventListener("scroll", schedule, true);
  addEventListener("resize", schedule);
  new ResizeObserver(schedule).observe(document.body);
  function create(block, video) {
    const portal = document.createElement("section");
    portal.className = "youtube-portal";
    portal.setAttribute("aria-label", "YouTube player");
    portal.hidden = true;
    const controls = document.createElement("div");
    controls.className = "youtube-controls";
    const back = document.createElement("button");
    back.textContent = "↙ Back to video";
    back.onclick = () =>
      document
        .querySelector(`[data-video-block="${CSS.escape(block)}"]`)
        ?.scrollIntoView({ block: "center", behavior: "smooth" });
    const close = document.createElement("button");
    close.textContent = "×";
    close.setAttribute("aria-label", "Pause and return the video to its block");
    close.onclick = () => {
      entry.player?.pauseVideo?.();
      entry.playing = entry.floating = false;
      layout();
    };
    controls.append(back, close);
    const iframe = document.createElement("iframe");
    iframe.title = "YouTube video";
    iframe.referrerPolicy = "strict-origin-when-cross-origin";
    iframe.allow = "autoplay; encrypted-media; fullscreen; picture-in-picture";
    iframe.allowFullscreen = true;
    const embed = new URL(`https://www.youtube-nocookie.com/embed/${video.id}`);
    embed.search = new URLSearchParams({
      enablejsapi: "1",
      origin: location.origin,
      playsinline: "1",
      start: String(video.start),
    });
    iframe.src = embed.href;
    portal.append(controls, iframe);
    document.body.append(portal);
    const entry = {
      block,
      video,
      portal,
      playing: false,
      floating: false,
      player: null,
    };
    players.set(block, entry);
    // The IFrame API reports playback (to float while playing) and pauses
    // hidden tabs; the video itself plays without it.
    loadAPI()
      .then((YT) => {
        if (players.get(block) !== entry) return;
        entry.player = new YT.Player(iframe, {
          events: {
            onStateChange: (e) => {
              entry.playing = e.data === 1 || e.data === 3;
              if (e.data === 0) entry.floating = false;
              layout();
            },
            onError: () =>
              error(
                new Error(
                  "This video cannot be embedded. Use “Open on YouTube” instead.",
                ),
              ),
          },
        });
      })
      .catch(() => {});
    return entry;
  }
  function reconcile() {
    const note = getNote();
    const wanted = new Map();
    for (const block of note?.blocks || []) {
      const video = youtubeVideo(block.markdown);
      const body = document.querySelector(
        `[data-block="${CSS.escape(block.id)}"] .block-body`,
      );
      // A block being edited shows its link instead of the player.
      if (!video || body?.querySelector("textarea")) continue;
      wanted.set(block.id, video);
      if (
        !body ||
        body.querySelector(`[data-video-block="${CSS.escape(block.id)}"]`)
      )
        continue;
      const card = document.createElement("div");
      card.className = "youtube-card";
      card.dataset.videoBlock = block.id;
      const box = document.createElement("div");
      box.className = "youtube-frame";
      const caption = document.createElement("div");
      caption.className = "youtube-caption";
      const link = document.createElement("a");
      link.href = video.url;
      link.target = "_blank";
      link.rel = "noopener noreferrer";
      link.textContent = "Open on YouTube";
      const editButton = document.createElement("button");
      editButton.className = "text-link";
      editButton.textContent = "Edit link";
      editButton.onclick = () => {
        remove(block.id);
        edit(block.id);
      };
      caption.append(link, editButton);
      card.append(box, caption);
      body.replaceChildren(card);
    }
    for (const [block, entry] of players)
      if (wanted.get(block)?.id !== entry.video.id) remove(block);
    for (const [block, video] of wanted)
      if (!players.has(block)) create(block, video);
    schedule();
  }
  return {
    reconcile,
    stop,
    setVisible(value) {
      visible = value;
      layout();
    },
  };
}
