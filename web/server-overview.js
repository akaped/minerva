export async function renderServerOverview({
  api,
  esc,
  formDialog,
  refresh,
  error,
}) {
  const root = document.querySelector("#content");
  const [stats, accounts] = await Promise.all([
    api("admin/overview"),
    api("admin"),
  ]);
  if (!root.isConnected) return;
  document.querySelector("#breadcrumb").textContent = "Server overview";
  const bytes = (n) =>
    n < 1024
      ? `${n} B`
      : n < 1048576
        ? `${(n / 1024).toFixed(1)} KB`
        : `${(n / 1048576).toFixed(1)} MB`;
  const colors = [
    "#2a493b",
    "#9bab70",
    "#c4623e",
    "#d2b173",
    "#668e9b",
    "#89779e",
  ];
  const total = stats.users.reduce(
    (n, u) => n + u.file_bytes + u.text_bytes,
    0,
  );
  let at = 0;
  const segments = stats.users
    .filter((u) => u.file_bytes + u.text_bytes > 0)
    .map((u, i) => {
      const start = at;
      at += ((u.file_bytes + u.text_bytes) / total) * 100;
      return `${colors[i % colors.length]} ${start}% ${at}%`;
    });
  const members = accounts.users.filter((u) => !u.admin && !u.disabled);
  root.innerHTML = `<section class="page server-overview"><div class="eyebrow">SERVER HEALTH, AT A GLANCE</div><div class="page-heading"><div><h1>Server overview</h1><p class="subtitle">Manage people and capacity. Member content stays in their workspaces.</p></div></div><div class="server-stat-grid">${[
    ["Accounts", stats.total_users],
    ["Workspaces", stats.total_workspaces],
    ["Notes", stats.total_notes],
    ["Uploaded files", bytes(stats.file_bytes)],
  ]
    .map(
      ([label, value]) =>
        `<article class="panel"><span>${label}</span><strong>${value}</strong></article>`,
    )
    .join(
      "",
    )}</div><section class="panel"><h2>Storage by workspace owner</h2><p>Current note text and uploaded files, counted once under each workspace owner. History, database overhead, and backups are excluded.</p><div class="server-storage"><div class="storage-pie" role="img" aria-label="Storage distribution; exact values are listed in the table" style="background:${total ? "conic-gradient(" + segments.join(",") + ")" : "var(--line)"}"></div><div class="table-wrap"><table class="admin-table"><thead><tr><th>Account</th><th>Workspaces</th><th>Notes</th><th>Note text</th><th>Files</th></tr></thead><tbody>${stats.users.map((u) => `<tr><td>${esc(u.name)}${u.admin ? " (administrator)" : ""}</td><td>${u.workspaces}</td><td>${u.notes}</td><td>${bytes(u.text_bytes)}</td><td>${bytes(u.file_bytes)}</td></tr>`).join("")}</tbody></table></div></div></section>${stats.legacy_workspaces.length ? `<section class="panel"><h2>Preserved administrator workspaces</h2><p>These pre-existing workspaces are retained, but administrators cannot open their content. Transfer them to a member to restore content management.</p>${stats.legacy_workspaces.map((w) => `<div class="legacy-workspace"><strong>${esc(w.name)}</strong><button class="btn" data-transfer="${esc(w.id)}" ${members.length ? "" : "disabled"}>Transfer to member</button></div>`).join("")}${!members.length ? "<p>Add a regular user first.</p>" : ""}</section>` : ""}</section>`;
  root.querySelectorAll("[data-transfer]").forEach(
    (b) =>
      (b.onclick = () =>
        formDialog(
          "Transfer preserved workspace",
          "The selected member becomes its owner. Existing member access and content are preserved.",
          `<label>New owner<select name="user_id">${members.map((u) => `<option value="${esc(u.id)}">${esc(u.name)} · ${esc(u.email)}</option>`).join("")}</select></label>`,
          async (v) => {
            await api(
              `admin/workspaces/${b.dataset.transfer}/transfer`,
              "POST",
              v,
            );
            await refresh();
            await renderServerOverview({
              api,
              esc,
              formDialog,
              refresh,
              error,
            });
          },
          "Transfer workspace",
        )),
  );
}
