"use strict";
const pending = new Map();
let sequence = 0;
// Called by the native shell with the outcome of an IPC request.
window.__minervaReply = (id, reply) => {
  const request = pending.get(id);
  if (!request) return;
  pending.delete(id);
  if (reply.ok) request.resolve(reply.value);
  else request.reject(Object.assign(new Error(reply.error), reply));
};
function call(cmd, args = {}) {
  return new Promise((resolve, reject) => {
    const id = ++sequence;
    pending.set(id, { resolve, reject });
    window.ipc.postMessage(JSON.stringify({ id, cmd, args }));
  });
}
const $ = (selector) => document.querySelector(selector);
function element(tag, props = {}, children = []) {
  const node = Object.assign(document.createElement(tag), props);
  node.append(...children);
  return node;
}
function status(message = "", isError = false) {
  $("#status").textContent = message;
  $("#status").classList.toggle("error", isError);
}
async function busy(button, fn) {
  button.disabled = true;
  try {
    await fn();
  } catch (e) {
    status(e.message, true);
  } finally {
    button.disabled = false;
  }
}
function button(label, onClick, className = "btn") {
  const node = element("button", {
    type: "button",
    className,
    textContent: label,
  });
  node.addEventListener("click", () => busy(node, onClick));
  return node;
}

async function open(connection, card) {
  status(`Connecting to ${connection.label}…`);
  try {
    await call("open", { id: connection.id });
  } catch (e) {
    if (!e.identity_changed) throw e;
    status();
    card.querySelector(".warning")?.remove();
    card.querySelector(".info").append(
      element("div", { className: "warning" }, [
        element("p", {
          textContent:
            "This address now answers as a different Minerva server than the one you added. " +
            "This happens after a server is reinstalled, but can also mean the address points somewhere unexpected. " +
            "Trusting it discards the saved sign-in.",
        }),
        element("div", { className: "buttons" }, [
          button(
            "Trust this server",
            () => call("trust", { id: connection.id }),
            "btn danger",
          ),
          button("Cancel", async () => card.querySelector(".warning").remove()),
        ]),
      ]),
    );
  }
}

function rename(connection, card) {
  const name = card.querySelector(".name");
  const input = element("input", {
    value: connection.label,
    maxLength: 80,
    ariaLabel: "Server name",
  });
  const save = async () => {
    await call("rename", { id: connection.id, label: input.value });
    await refresh();
  };
  input.addEventListener("keydown", (e) => {
    if (e.key === "Enter") busy(input, save);
    if (e.key === "Escape") refresh();
  });
  name.replaceChildren(input, " ", button("Save", save, "btn primary"));
  input.focus();
  input.select();
}

function confirmRemove(connection, node) {
  if (node.dataset.armed) {
    return call("remove", { id: connection.id }).then(() => {
      status(`Removed ${connection.label}.`);
      return refresh();
    });
  }
  node.dataset.armed = "1";
  node.textContent = "Confirm remove";
  setTimeout(() => {
    delete node.dataset.armed;
    node.textContent = "Remove";
  }, 4000);
}

// Workspaces of a server, each with an "Available offline" switch.
async function offline(connection, card) {
  const existing = card.querySelector(".offline");
  if (existing) return existing.remove();
  const panel = element("div", { className: "offline" });
  card.querySelector(".info").append(panel);
  let workspaces;
  try {
    workspaces = await call("workspaces", { id: connection.id });
  } catch (e) {
    panel.append(
      element("p", {
        textContent: e.signed_out
          ? "Open this server and sign in first; then choose workspaces to keep offline."
          : e.message,
      }),
    );
    return;
  }
  if (!workspaces.length)
    panel.append(
      element("p", { textContent: "This server has no workspaces for you." }),
    );
  for (const workspace of workspaces) {
    const box = element("input", {
      type: "checkbox",
      checked: workspace.offline,
    });
    box.addEventListener("change", () =>
      busy(box, async () => {
        const args = {
          id: connection.id,
          workspace_id: workspace.id,
          enabled: box.checked,
        };
        try {
          await call("offline", args);
        } catch (e) {
          if (!e.unsent) {
            box.checked = !box.checked;
            throw e;
          }
          // Removing a copy with unsent changes loses them; ask once more.
          box.checked = true;
          panel.querySelector(".warning")?.remove();
          const warning = element("div", { className: "warning" }, [
            element("p", {
              textContent: `${workspace.name}: ${e.message}. Removing the offline copy loses them.`,
            }),
            element("div", { className: "buttons" }, [
              button(
                "Remove anyway",
                async () => {
                  await call("offline", { ...args, discard: true });
                  box.checked = false;
                  warning.remove();
                  status(`Removed the offline copy of ${workspace.name}.`);
                },
                "btn danger",
              ),
              button("Keep it", async () => warning.remove()),
            ]),
          ]);
          panel.append(warning);
          return;
        }
        status(
          box.checked
            ? `${workspace.name} will be available offline. Open This computer to use it.`
            : `Removed the offline copy of ${workspace.name}.`,
        );
      }),
    );
    panel.append(
      element("label", { className: "check" }, [
        box,
        element("span", { textContent: workspace.name }),
      ]),
    );
  }
}

function card(connection) {
  const remove = element("button", {
    type: "button",
    className: "btn danger",
    textContent: "Remove",
  });
  remove.addEventListener("click", () =>
    busy(remove, () => confirmRemove(connection, remove)),
  );
  const meta = connection.account
    ? `${connection.account} · ${connection.url}`
    : connection.url;
  const node = element("li", { className: "server" }, [
    element("div", { className: "info" }, [
      element("div", { className: "name" }, [
        connection.label,
        ...(connection.signed_in
          ? [element("span", { className: "badge", textContent: "Signed in" })]
          : []),
      ]),
      element("div", { className: "meta", textContent: meta, title: meta }),
    ]),
  ]);
  node.append(
    element("div", { className: "buttons" }, [
      button("Open", () => open(connection, node), "btn primary"),
      ...(connection.signed_in
        ? [
            button("Sign out", async () => {
              await call("sign_out", { id: connection.id });
              status(`Signed out of ${connection.label}.`);
              await refresh();
            }),
          ]
        : []),
      button("Offline…", () => offline(connection, node)),
      button("Rename", async () => rename(connection, node)),
      remove,
    ]),
  );
  return node;
}

async function refresh() {
  const connections = await call("list");
  $("#servers").replaceChildren(...connections.map(card));
  $("#empty").hidden = connections.length > 0;
}

$("#add").addEventListener("submit", (e) => {
  e.preventDefault();
  const form = e.target;
  const submit = form.querySelector("button[type=submit]");
  busy(submit, async () => {
    status("Checking server…");
    const connection = await call("add", {
      url: form.url.value,
      label: form.label.value,
      allow_insecure_http: form.allow_insecure_http.checked,
    });
    form.reset();
    status(`Added ${connection.label}.`);
    await refresh();
  });
});

$("#open-local").addEventListener("click", (e) =>
  busy(e.target, () => call("open_local")),
);
refresh().catch((e) => status(e.message, true));
