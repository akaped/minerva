/**
 * The quick-capture page (`/capture`): write or share something from a phone
 * and save it as a new note or into today's journal. Without a connection,
 * captures wait on the device and are sent when it is back.
 */
import { $t, $tn } from "./i18n.js";
import {
  sharedText,
  noteFrom,
  today,
  queued,
  setQueued,
} from "./capture-text.js";

const $ = (s) => document.querySelector(s);
const esc = (s) =>
  String(s ?? "").replace(
    /[&<>"']/g,
    (c) =>
      ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[
        c
      ],
  );
const LAST = "minerva-capture-target";

// The page's own text, in the reader's language.
for (const el of document.querySelectorAll("[data-t]"))
  el.textContent = $t(el.dataset.t);
for (const name of ["placeholder", "aria-label"])
  for (const el of document.querySelectorAll(`[data-t-${name}]`))
    el.setAttribute(name, $t(el.getAttribute(`data-t-${name}`)));

// The light or dark look chosen in the app (a plugin theme's base).
try {
  const chosen = localStorage.getItem("minerva.theme") || "system";
  const base =
    chosen === "system"
      ? matchMedia("(prefers-color-scheme: dark)").matches
        ? "dark"
        : "light"
      : ["light", "dark"].includes(chosen)
        ? chosen
        : localStorage.getItem("minerva.theme.base") || "light";
  document.documentElement.dataset.theme = base;
  document.documentElement.style.colorScheme = base;
} catch {}

async function api(path, method = "GET", body) {
  const r = await fetch("/api/v1/" + path, {
    method,
    headers: { "Content-Type": "application/json", "X-Minerva-Client": "1" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const v = await r.json().catch(() => ({}));
  if (!r.ok || r.headers.get("x-minerva-offline")) {
    const e = new Error(v.error ? $t(v.error) : $t("Request failed"));
    e.status = r.headers.get("x-minerva-offline") ? 0 : r.status;
    throw e;
  }
  return v;
}

/** Sends one capture; returns the note it went to. */
async function send({ target, text }) {
  const [kind, id] = target.split(":");
  if (kind === "journal") {
    const day = await api("journal", "POST", {
      workspace_id: id,
      date: today(),
    });
    await api(`notes/${day.id}/append`, "POST", { markdown: text });
    return day;
  }
  const { title, markdown } = noteFrom(text);
  return api("notes", "POST", { folder_id: id, title, markdown });
}

function status(text) {
  $("#capture-status").textContent = text;
}

/** Sends what waited on this device; keeps what still cannot be sent. */
async function flush() {
  const waiting = queued();
  if (!waiting.length || !navigator.onLine) return;
  const left = [];
  for (const item of waiting) {
    try {
      await send(item);
    } catch (e) {
      // Kept only while the server cannot be reached; refused ones are dropped
      // after saying so, since retrying will not help.
      if (!e.status) left.push(item);
      else
        status(
          $t("One waiting note could not be saved: {message}", {
            message: e.message,
          }),
        );
    }
  }
  setQueued(left);
  const sent = waiting.length - left.length;
  if (sent)
    status(
      $tn(sent, "{count} waiting note saved.", "{count} waiting notes saved."),
    );
}

function targets(data) {
  const out = [];
  for (const w of data.workspaces)
    if (w.can_edit)
      out.push([
        `journal:${w.id}`,
        $t("Today’s journal · {workspace}", { workspace: w.name }),
      ]);
  const special = new Set([
    ...Object.values(data.journal_folders || {}),
    ...Object.values(data.template_folders || {}),
  ]);
  for (const f of data.folders) {
    if (data.roles?.[f.id] !== "editor" || special.has(f.id)) continue;
    const w = data.workspaces.find((x) => x.id === f.workspace_id);
    out.push([
      `folder:${f.id}`,
      `${$t("New note in {name}", { name: f.name })}${w ? ` · ${w.name}` : ""}`,
    ]);
  }
  return out;
}

async function start() {
  let data;
  try {
    data = await api("bootstrap");
  } catch (e) {
    if (e.status === 401) {
      $("#capture-signin").hidden = false;
      return;
    }
    // Offline and nothing kept: the last choices still work, from the queue.
    data = null;
  }
  const select = $("#capture-target");
  const options = data ? targets(data) : [];
  let saved = "";
  try {
    saved = localStorage.getItem(LAST) || "";
    if (data)
      localStorage.setItem("minerva-capture-targets", JSON.stringify(options));
    else
      options.push(
        ...JSON.parse(localStorage.getItem("minerva-capture-targets") || "[]"),
      );
  } catch {}
  if (!options.length) {
    status(
      data ? $t("You cannot add notes anywhere yet.") : $t("You are offline."),
    );
    return;
  }
  select.innerHTML = options
    .map(
      ([value, label]) =>
        `<option value="${esc(value)}">${esc(label)}</option>`,
    )
    .join("");
  if (options.some(([v]) => v === saved)) select.value = saved;
  const params = new URLSearchParams(location.search);
  const shared = sharedText({
    title: params.get("title") || "",
    text: params.get("text") || "",
    url: params.get("url") || "",
  });
  const text = $("#capture-text");
  text.value = shared;
  $("#capture-form").hidden = false;
  text.focus();
  // A shared page should not be shared again on reload.
  if (location.search) history.replaceState(null, "", "/capture");
  $("#capture-form").onsubmit = async (e) => {
    e.preventDefault();
    const item = { target: select.value, text: text.value.trim() };
    if (!item.text) return;
    try {
      localStorage.setItem(LAST, item.target);
    } catch {}
    const button = $("#capture-form button");
    button.disabled = true;
    try {
      const saved = await send(item);
      const folder = data?.folders.find(
        (f) => `folder:${f.id}` === item.target,
      );
      done(
        item.target.startsWith("journal:")
          ? $t("Added to today’s journal.")
          : $t("Saved as a new note in {folder}.", {
              folder: folder?.name || "",
            }),
        saved.id,
      );
    } catch (err) {
      if (err.status) status(err.message);
      else if (setQueued([...queued(), { ...item, at: Date.now() }]))
        done(
          $t(
            "Saved on this device. It will be sent when you are online again.",
          ),
        );
      else
        status(
          $t(
            "Could not keep it on this device either. Copy your text before leaving.",
          ),
        );
    } finally {
      button.disabled = false;
    }
  };
  $("#capture-again").onclick = () => {
    text.value = "";
    $("#capture-done").hidden = true;
    $("#capture-form").hidden = false;
    text.focus();
  };
  flush();
}

function done(message, id) {
  $("#capture-done-text").textContent = message;
  $("#capture-open").hidden = !id;
  if (id) $("#capture-open").href = `/?note=${encodeURIComponent(id)}`;
  $("#capture-form").hidden = true;
  $("#capture-done").hidden = false;
  $("#capture-again").focus();
}

addEventListener("online", () => flush());
if ("serviceWorker" in navigator)
  navigator.serviceWorker.register("/sw.js").catch(() => {});
start();
