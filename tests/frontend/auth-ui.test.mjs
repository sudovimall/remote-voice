import assert from "node:assert/strict";
import test from "node:test";
import {
  authDisplayName,
  normalizeAuthState,
  shouldShowAdminLink,
} from "../../static/auth-ui.mjs";

test("authDisplayName prefers display_name and falls back to username", () => {
  assert.equal(authDisplayName({ display_name: "管理员", username: "admin" }), "管理员");
  assert.equal(authDisplayName({ username: "admin" }), "admin");
  assert.equal(authDisplayName(null), "");
});

test("normalizeAuthState hides controls when auth is disabled", () => {
  assert.deepEqual(normalizeAuthState({ auth_enabled: false, user: { username: "admin" } }), {
    enabled: false,
    user: null,
  });
});

test("shouldShowAdminLink only returns true for admin users", () => {
  assert.equal(shouldShowAdminLink({ role: "admin" }), true);
  assert.equal(shouldShowAdminLink({ role: "user" }), false);
  assert.equal(shouldShowAdminLink(null), false);
});
