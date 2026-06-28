export function authDisplayName(user) {
  return user?.display_name || user?.username || "";
}

export function normalizeAuthState(payload) {
  if (!payload?.auth_enabled) {
    return {
      enabled: false,
      user: null,
    };
  }

  return {
    enabled: true,
    user: payload.user || null,
  };
}

export function shouldShowAdminLink(user) {
  return user?.role === "admin";
}

export async function fetchAuthState(fetchImpl) {
  const response = await fetchImpl("/api/auth/me", {
    headers: { accept: "application/json" },
  });
  if (!response.ok) {
    return {
      enabled: true,
      user: null,
    };
  }
  return normalizeAuthState(await response.json());
}

export function renderAuthControls(container, authState, onLogout) {
  if (!container) return;
  container.replaceChildren();
  if (!authState.enabled || !authState.user) {
    container.hidden = true;
    return;
  }

  container.hidden = false;
  const name = document.createElement("span");
  name.textContent = authDisplayName(authState.user);

  const actions = document.createElement("div");
  actions.className = "auth-actions";

  if (shouldShowAdminLink(authState.user)) {
    const admin = document.createElement("a");
    admin.href = "/admin";
    admin.textContent = "管理";
    actions.append(admin);
  }

  const logout = document.createElement("button");
  logout.type = "button";
  logout.textContent = "退出";
  logout.addEventListener("click", onLogout);
  actions.append(logout);

  container.append(name, actions);
}
