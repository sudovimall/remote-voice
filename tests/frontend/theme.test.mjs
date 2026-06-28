import assert from "node:assert/strict";
import test from "node:test";

import {
  applyTheme,
  nextTheme,
  preferredTheme,
  storedTheme,
  THEME_STORAGE_KEY,
} from "../../frontend/src/lib/theme.js";

function storage(initial = {}) {
  const values = new Map(Object.entries(initial));
  return {
    getItem(key) {
      return values.has(key) ? values.get(key) : null;
    },
    setItem(key, value) {
      values.set(key, value);
    },
  };
}

test("theme defaults to dark when storage has no saved choice", () => {
  assert.equal(storedTheme(storage()), "dark");
});

test("theme ignores unsupported stored values", () => {
  assert.equal(storedTheme(storage({ [THEME_STORAGE_KEY]: "blue" })), "dark");
});

test("theme toggles between signal deck dark and light modes", () => {
  assert.equal(nextTheme("dark"), "light");
  assert.equal(nextTheme("light"), "dark");
});

test("theme can use browser preference before explicit storage", () => {
  assert.equal(preferredTheme(false), "dark");
  assert.equal(preferredTheme(true), "light");
});

test("applyTheme writes the root data attribute and persists the choice", () => {
  const root = {
    dataset: {},
  };
  const store = storage();

  applyTheme(root, store, "light");

  assert.equal(root.dataset.theme, "light");
  assert.equal(store.getItem(THEME_STORAGE_KEY), "light");
});
