/** The settings page: connect to a Minerva server, or disconnect. */
import { ext, t, translatePage, connection, connect, disconnect } from "./minerva.js";
import { serverOrigin } from "./compose.js";

const $ = (s) => document.querySelector(s);
translatePage();

async function draw() {
  const conn = await connection();
  $("#connect").hidden = !!conn;
  $("#connected").hidden = !conn;
  if (conn) {
    $("#connected-text").textContent = t(
      "connectedTo",
      new URL(conn.server).host,
      conn.user?.name || "",
      conn.user?.email || "",
    );
  } else {
    const { lastServer = "" } = await ext.storage.local.get("lastServer");
    if (!$("#connect").server.value) $("#connect").server.value = lastServer;
  }
}

$("#connect").onsubmit = async (e) => {
  e.preventDefault();
  const error = $("#error");
  error.textContent = "";
  const origin = serverOrigin(e.target.server.value);
  if (!origin) {
    error.textContent = t("badServer");
    return;
  }
  // The browser asks once whether the extension may reach this server. It
  // must be asked straight from the click.
  const granted = await ext.permissions
    .request({ origins: [`${origin}/*`] })
    .catch(() => false);
  if (!granted) {
    error.textContent = t("permissionRefused");
    return;
  }
  const button = $("#connect-button");
  button.disabled = true;
  button.textContent = t("connecting");
  try {
    await ext.storage.local.set({ lastServer: origin });
    await connect(origin);
    await draw();
  } catch (err) {
    error.textContent = err.message;
  } finally {
    button.disabled = false;
    button.textContent = t("connectButton");
  }
};

$("#disconnect").onclick = async () => {
  const conn = await connection();
  await disconnect();
  // The permission for the server goes too; connecting asks again.
  if (conn) await ext.permissions.remove({ origins: [`${conn.server}/*`] }).catch(() => {});
  await draw();
};

draw();
