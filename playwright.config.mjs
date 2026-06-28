import { existsSync } from "node:fs";
import { defineConfig, devices } from "@playwright/test";

const baseURL = process.env.PLAYWRIGHT_BASE_URL ?? "http://127.0.0.1:18080";
const systemChrome =
  process.env.PLAYWRIGHT_CHROMIUM_EXECUTABLE_PATH ||
  (existsSync("/usr/bin/google-chrome") ? "/usr/bin/google-chrome" : undefined);

const launchOptions = {
  args: [
    "--autoplay-policy=no-user-gesture-required",
    "--no-sandbox",
    "--use-fake-device-for-media-stream",
    "--use-fake-ui-for-media-stream",
  ],
};

if (systemChrome) {
  launchOptions.executablePath = systemChrome;
}

export default defineConfig({
  testDir: "./tests/browser",
  fullyParallel: false,
  reporter: [["list"]],
  timeout: 30_000,
  expect: {
    timeout: 10_000,
  },
  use: {
    ...devices["Desktop Chrome"],
    baseURL,
    launchOptions,
    screenshot: "only-on-failure",
    trace: "retain-on-failure",
  },
  webServer: {
    command: "node tests/browser/auth-server.mjs",
    url: `${baseURL}/health`,
    timeout: 120_000,
    reuseExistingServer: false,
  },
});
