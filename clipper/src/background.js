/**
 * The background script: right-click menus that clip the page, a selection, a
 * link or a picture straight into the folder used last, showing the result on
 * the toolbar button.
 */
import { ext, t, connection, remembered, take, save } from "./minerva.js";
import { parseTags } from "./compose.js";

const MENUS = [
  ["page", ["page"], "menuPage"],
  ["selection", ["selection"], "menuSelection"],
  ["link", ["link"], "menuLink"],
  ["image", ["image"], "menuImage"],
];

ext.runtime.onInstalled.addListener(async ({ reason }) => {
  await ext.contextMenus.removeAll();
  for (const [id, contexts, title] of MENUS)
    ext.contextMenus.create({ id, contexts, title: t(title) });
  if (reason === "install") ext.runtime.openOptionsPage();
});

/** Shows how a clip went on the toolbar button for a few seconds. */
async function badge(tabId, ok, message) {
  await ext.action.setBadgeBackgroundColor({ color: ok ? "#2f7d4f" : "#b3261e", tabId });
  await ext.action.setBadgeText({ text: ok ? "✓" : "!", tabId });
  await ext.action.setTitle({ title: message, tabId });
  setTimeout(() => {
    ext.action.setBadgeText({ text: "", tabId }).catch(() => {});
    ext.action.setTitle({ title: t("actionTitle"), tabId }).catch(() => {});
  }, 6000);
}

ext.contextMenus.onClicked.addListener(async (info, tab) => {
  const kind = info.menuItemId;
  if (!(await connection())) return ext.runtime.openOptionsPage();
  const { folder } = await remembered();
  if (!folder) {
    // The first clip chooses its folder in the popup.
    await ext.action.openPopup?.().catch(() => {});
    return badge(tab.id, false, t("chooseFolderFirst"));
  }
  try {
    const taken = await take(tab.id, kind === "selection" ? "selection" : kind === "page" ? "page" : "details");
    // The menu knows the selection even where the page script cannot see it.
    if (kind === "selection" && !taken.markdown && info.selectionText)
      taken.markdown = info.selectionText;
    if (kind === "page" && taken.noArticle) taken.markdown = "";
    const extra = { linkUrl: info.linkUrl, srcUrl: info.srcUrl, text: info.linkText || "" };
    const { tags = "" } = await remembered();
    await save(kind, taken, extra, folder, { tags: parseTags(tags) });
    await badge(tab.id, true, t("savedShort"));
  } catch (e) {
    await badge(tab.id, false, e.message);
  }
});
