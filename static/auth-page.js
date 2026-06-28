const pageDocument = typeof document === "undefined" ? null : document;
const error = pageDocument?.querySelector("#auth-error");
const loginForm = pageDocument?.querySelector("#login-form");
const registerForm = pageDocument?.querySelector("#register-form");

export function safeNextPath(value) {
  const next = typeof value === "string" ? value.trim() : "";
  if (!next || !next.startsWith("/") || next.startsWith("//")) {
    return "/";
  }
  return next;
}

function nextPath() {
  const next = new URLSearchParams(window.location.search).get("next");
  return safeNextPath(next);
}

function showError(message) {
  if (!error) return;
  error.textContent = message;
  error.hidden = false;
}

async function submitJson(path, payload) {
  const response = await fetch(path, {
    method: "POST",
    headers: { "content-type": "application/json" },
    body: JSON.stringify(payload),
  });
  if (!response.ok) {
    const body = await response.json().catch(() => ({}));
    throw new Error(body.message || "请求失败。");
  }
  return response.json();
}

loginForm?.addEventListener("submit", async (event) => {
  event.preventDefault();
  const data = new FormData(loginForm);
  try {
    await submitJson("/api/auth/login", {
      username: data.get("username"),
      password: data.get("password"),
    });
    window.location.assign(nextPath());
  } catch (submitError) {
    showError(submitError.message);
  }
});

registerForm?.addEventListener("submit", async (event) => {
  event.preventDefault();
  const data = new FormData(registerForm);
  try {
    await submitJson("/api/auth/register", {
      code: data.get("code"),
      username: data.get("username"),
      password: data.get("password"),
      display_name: data.get("display_name"),
    });
    window.location.assign("/");
  } catch (submitError) {
    showError(submitError.message);
  }
});

if (registerForm) {
  const code = new URLSearchParams(window.location.search).get("code");
  if (code) registerForm.elements.code.value = code;
}
