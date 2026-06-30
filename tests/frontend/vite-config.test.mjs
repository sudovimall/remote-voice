import assert from "node:assert/strict";
import test from "node:test";

import viteConfig, {
  appBase,
  backendOriginFromEnv,
  backendProxy,
} from "../../frontend/vite.config.js";

test("vite base follows development and build entry paths", () => {
  assert.equal(appBase("serve"), "/");
  assert.equal(appBase("build"), "/ui/");
  assert.equal(viteConfig({ command: "serve" }).base, "/");
  assert.equal(viteConfig({ command: "build" }).base, "/ui/");
});

test("vite backend proxy defaults to the configured Rust port", () => {
  const proxy = backendProxy(backendOriginFromEnv({}));

  assert.equal(proxy["/api"].target, "http://127.0.0.1:18080");
  assert.equal(proxy["/ws"].target, "http://127.0.0.1:18080");
  assert.equal(proxy["/ws"].ws, true);
  assert.equal(proxy["/api"].changeOrigin, true);
});

test("vite backend proxy accepts an environment override", () => {
  const target = backendOriginFromEnv({
    REMOTE_VOICE_BACKEND_ORIGIN: " http://127.0.0.1:19090 ",
  });
  const proxy = backendProxy(target);

  assert.equal(target, "http://127.0.0.1:19090");
  assert.equal(proxy["/api"].target, "http://127.0.0.1:19090");
  assert.equal(proxy["/ws"].target, "http://127.0.0.1:19090");
});
