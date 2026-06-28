import assert from "node:assert/strict";
import test from "node:test";

import { safeNextPath } from "../../static/auth-page.js";

test("safeNextPath only accepts same-origin absolute paths", () => {
  assert.equal(safeNextPath("/"), "/");
  assert.equal(safeNextPath("/rooms/ABC123?tab=chat"), "/rooms/ABC123?tab=chat");
  assert.equal(safeNextPath(""), "/");
  assert.equal(safeNextPath("https://example.com/"), "/");
  assert.equal(safeNextPath("//example.com/"), "/");
  assert.equal(safeNextPath("javascript:alert(1)"), "/");
});
