export const THEME_STORAGE_KEY = "remote-voice.theme";
const THEMES = new Set(["dark", "light"]);

export function preferredTheme(prefersLight) {
  return prefersLight ? "light" : "dark";
}

export function storedTheme(storage, fallback = "dark") {
  try {
    const value = storage.getItem(THEME_STORAGE_KEY);
    return THEMES.has(value) ? value : fallback;
  } catch (_error) {
    return fallback;
  }
}

export function nextTheme(theme) {
  return theme === "dark" ? "light" : "dark";
}

export function applyTheme(root, storage, theme) {
  const value = THEMES.has(theme) ? theme : "dark";
  root.dataset.theme = value;
  try {
    storage.setItem(THEME_STORAGE_KEY, value);
  } catch (_error) {
    // Theme persistence should never block the UI.
  }
  return value;
}
