import { isPane } from "./note-tabs.js";
// Cross-server panes sign in on their own origin. Keep their session in memory,
// never in the host page, localStorage, a URL, or a host-page postMessage payload.
const host =
  typeof location === "undefined"
    ? null
    : new URLSearchParams(location.search).get("host");
if (isPane && host && host !== location.origin) {
  const originalFetch = window.fetch.bind(window);
  let session = null;
  const group = new URLSearchParams(location.search).get("group");
  const channel =
    group && typeof BroadcastChannel !== "undefined"
      ? new BroadcastChannel(`minerva-pane-session:${host}:${group}`)
      : null;
  const requester = crypto.randomUUID();
  let finishHandshake;
  const handshake = new Promise((resolve) => {
    finishHandshake = resolve;
    setTimeout(resolve, 250);
  });
  if (channel) {
    channel.onmessage = ({ data }) => {
      if (data?.type === "request" && session)
        channel.postMessage({
          type: "session",
          to: data.requester,
          token: session,
        });
      if (
        data?.type === "session" &&
        data.to === requester &&
        typeof data.token === "string"
      ) {
        session = data.token;
        finishHandshake();
      }
      if (data?.type === "logout") {
        session = null;
        clearImages();
      }
    };
    channel.postMessage({ type: "request", requester });
  } else finishHandshake();
  const local = (value) => {
    try {
      const url = new URL(value, location.href);
      return (
        url.host === location.host &&
        url.protocol === location.protocol &&
        url.pathname.startsWith("/api/v1/")
      );
    } catch {
      return false;
    }
  };
  window.fetch = async (input, init = {}) => {
    const address =
      typeof input === "string"
        ? input
        : input instanceof URL
          ? input.href
          : input.url;
    if (!local(address)) return originalFetch(input, init);
    if (!session) await handshake;
    const headers = new Headers(
      init.headers || (input instanceof Request ? input.headers : undefined),
    );
    if (session) headers.set("Authorization", `Bearer ${session}`);
    const response = await originalFetch(input, {
      ...init,
      headers,
      credentials: "omit",
    });
    const path = new URL(address, location.href).pathname;
    if (
      response.ok &&
      (path.endsWith("/login") ||
        path.endsWith("/login/verify") ||
        path.includes("/login/"))
    ) {
      const result = await response
        .clone()
        .json()
        .catch(() => null);
      if (result?.token) {
        session = result.token;
        hydrate();
      }
    }
    if (path.endsWith("/logout") && response.ok) {
      session = null;
      channel?.postMessage({ type: "logout" });
      clearImages();
    }
    return response;
  };
  const images = new Map();
  function clearImages() {
    for (const blob of images.values()) URL.revokeObjectURL(blob);
    images.clear();
  }
  function hydrate() {
    for (const [img, blob] of images)
      if (!img.isConnected) {
        URL.revokeObjectURL(blob);
        images.delete(img);
      }
    if (!session) return;
    document.querySelectorAll("img").forEach(async (img) => {
      const source = img.getAttribute("src");
      if (!source || !local(source) || images.has(img)) return;
      images.set(img, "");
      try {
        const response = await fetch(source);
        if (!response.ok) throw new Error("Image unavailable");
        const blob = URL.createObjectURL(await response.blob());
        if (!img.isConnected || !session) {
          URL.revokeObjectURL(blob);
          images.delete(img);
          return;
        }
        images.set(img, blob);
        img.src = blob;
      } catch {
        images.delete(img);
        img.alt = img.alt || "Image unavailable";
      }
    });
  }
  let timer;
  new MutationObserver(() => {
    clearTimeout(timer);
    timer = setTimeout(hydrate, 40);
  }).observe(document.documentElement, { childList: true, subtree: true });
  document.addEventListener(
    "click",
    async (e) => {
      const a = e.target.closest?.("a[href]");
      if (!a || !session || !local(a.href)) return;
      const url = new URL(a.href);
      if (!/^\/api\/v1\/attachments\/[a-f\d-]+$/.test(url.pathname)) return;
      e.preventDefault();
      e.stopPropagation();
      try {
        const response = await fetch(url.href);
        if (!response.ok) throw new Error("Download denied");
        const blob = URL.createObjectURL(await response.blob()),
          download = document.createElement("a");
        download.href = blob;
        const name = response.headers
          .get("content-disposition")
          ?.match(/filename\*=UTF-8''([^;]+)/)?.[1];
        download.download = name ? decodeURIComponent(name) : "attachment";
        download.click();
        setTimeout(() => URL.revokeObjectURL(blob), 1000);
      } catch (err) {
        const toast = document.querySelector("#toast");
        if (toast) {
          toast.textContent = err.message;
          toast.classList.add("visible");
        }
      }
    },
    true,
  );
  addEventListener("pagehide", () => {
    clearImages();
    channel?.close();
  });
}
