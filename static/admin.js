const createInvite = document.querySelector("#create-invite");
const output = document.querySelector("#admin-output");

createInvite?.addEventListener("click", async () => {
  const response = await fetch("/api/admin/invites", {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify({ ttl_hours: 168 }),
  });
  const body = await response.json().catch(() => ({}));
  output.textContent = response.ok
    ? `邀请码：${body.code}`
    : body.message || "生成失败。";
});
