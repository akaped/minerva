/** The toolbar popup: choose what to clip, its title, folder and tags, and save. */
import {
  ext,
  t,
  translatePage,
  connection,
  remembered,
  remember,
  call,
  take,
  save,
  noteAddress,
} from "./minerva.js";
import { parseTags } from "./compose.js";

const $ = (s) => document.querySelector(s);
const show = (id) => {
  for (const s of ["disconnected", "blocked", "clip", "saved"]) $(`#${s}`).hidden = s !== id;
};
translatePage();
$("#settings").title = t("settings");
$("#settings").setAttribute("aria-label", t("settings"));
const settings = () => {
  ext.runtime.openOptionsPage();
  window.close();
};
$("#settings").onclick = settings;
$("#open-settings").onclick = settings;

/** The folder list: one group per workspace, personal first. */
function fillFolders(select, workspaces, chosen) {
  select.replaceChildren();
  const sorted = [...workspaces].sort((a, b) => b.personal - a.personal || a.name.localeCompare(b.name));
  for (const w of sorted) {
    const group = document.createElement("optgroup");
    group.label = w.name;
    for (const f of w.folders) group.append(new Option(f.path, f.id, false, f.id === chosen));
    select.append(group);
  }
}

/** What each kind would save, in a line. */
function describe(kind, taken) {
  if (kind === "bookmark") return t("previewBookmark");
  const words = (taken[kind]?.markdown || "").split(/\s+/).filter(Boolean).length;
  if (kind === "page" && taken.page?.noArticle) return t("noArticle");
  return t("previewWords", words.toLocaleString());
}

async function start() {
  const conn = await connection();
  if (!conn) return show("disconnected");
  $("#account").textContent = t("signedInAs", conn.user?.name || conn.user?.email || "", new URL(conn.server).host);
  const [tab] = await ext.tabs.query({ active: true, currentWindow: true });
  if (!tab || !/^https?:/.test(tab.url || "")) return show("blocked");

  const form = $("#clip");
  const last = await remembered();
  let page, chosenSelection;
  try {
    [page, chosenSelection] = await Promise.all([take(tab.id, "page"), take(tab.id, "selection")]);
  } catch {
    return show("blocked");
  }
  const taken = { page, selection: chosenSelection, bookmark: page };
  const selection = form.querySelector('input[value="selection"]');
  selection.disabled = !chosenSelection.markdown;
  selection.closest("label").classList.toggle("off", selection.disabled);
  // A selection is what someone means to clip, when there is one.
  const initial = chosenSelection.markdown ? "selection" : page.noArticle ? "bookmark" : "page";
  form.kind.value = initial;
  form.title.value = page.title || tab.title || "";
  form.tags.value = last.tags || "";
  const update = () => ($("#preview").textContent = describe(form.kind.value, taken));
  form.addEventListener("change", update);
  update();
  show("clip");

  try {
    const { workspaces } = await call("GET", "clipper/folders", null, conn);
    if (!workspaces.length) {
      $("#error").textContent = t("noFolders");
      $("#save").disabled = true;
    }
    fillFolders(form.folder, workspaces, last.folder);
  } catch (e) {
    $("#error").textContent = e.message;
    if (e.status === 401) return show("disconnected");
    $("#save").disabled = true;
  }

  form.onsubmit = async (e) => {
    e.preventDefault();
    $("#error").textContent = "";
    $("#save").disabled = true;
    $("#save").textContent = t("saving");
    const kind = form.kind.value;
    try {
      const saved = await save(kind, taken[kind], {}, form.folder.value, {
        title: form.title.value,
        tags: parseTags(form.tags.value),
      });
      await remember({ folder: form.folder.value, tags: form.tags.value });
      const folder = form.folder.selectedOptions[0]?.textContent || "";
      $("#saved-text").textContent = t("savedTo", folder);
      $("#open-note").onclick = () => {
        ext.tabs.create({ url: noteAddress(conn, saved) });
        window.close();
      };
      show("saved");
    } catch (err) {
      $("#error").textContent = err.message;
      if (err.status === 401) show("disconnected");
    } finally {
      $("#save").disabled = false;
      $("#save").textContent = t("save");
    }
  };
}
start();
