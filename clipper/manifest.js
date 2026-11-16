/**
 * The extension's manifest for each browser. Chrome runs the background script
 * as a service worker; Firefox runs it as an event page and needs an ID.
 */
export const FIREFOX_ID = "web-clipper@minerva-notes";

export function manifest(browser, version) {
  const common = {
    manifest_version: 3,
    name: "__MSG_name__",
    description: "__MSG_description__",
    default_locale: "en",
    version,
    icons: { 16: "icons/icon-16.png", 32: "icons/icon-32.png", 48: "icons/icon-48.png", 128: "icons/icon-128.png" },
    action: {
      default_popup: "popup.html",
      default_title: "__MSG_actionTitle__",
      default_icon: { 16: "icons/icon-16.png", 32: "icons/icon-32.png" },
    },
    options_ui: { page: "options.html", open_in_tab: true },
    // activeTab: the page is read only when the person clicks the clipper.
    permissions: ["activeTab", "scripting", "storage", "contextMenus", "identity"],
    // The person's own server, asked for when connecting.
    optional_host_permissions: ["https://*/*", "http://*/*"],
    commands: {
      _execute_action: { suggested_key: { default: "Alt+Shift+M" } },
    },
  };
  if (browser === "firefox")
    return {
      ...common,
      background: { scripts: ["background.js"] },
      browser_specific_settings: {
        gecko: {
          id: FIREFOX_ID,
          strict_min_version: "140.0",
          // What a clip sends goes to the person's own server only.
          data_collection_permissions: { required: ["websiteContent"] },
        },
        // Firefox for Android reads data_collection_permissions from 142.
        gecko_android: { strict_min_version: "142.0" },
      },
    };
  return {
    ...common,
    background: { service_worker: "background.js" },
    minimum_chrome_version: "116",
  };
}
