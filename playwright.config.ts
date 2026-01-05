import { defineConfig, devices } from "@playwright/test";
import * as path from "path";

/**
 * Playwright configuration for CleanScope Tauri app E2E testing.
 *
 * For Tauri v2 apps, we test the web content inside the webview by
 * starting the Tauri dev server and connecting via Chromium.
 *
 * Replay mode is enabled via CLEANSCOPE_REPLAY_PATH environment variable,
 * allowing tests to run without physical USB hardware.
 */

export default defineConfig({
  testDir: "./e2e",
  testMatch: "**/*.spec.ts",

  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 2 : 0,
  workers: process.env.CI ? 1 : undefined,

  reporter: [
    ["html"],
    ["list"],
    process.env.CI ? ["github"] : ["list"],
  ],

  use: {
    // Base URL pointing to local Tauri dev server
    baseURL: "http://localhost:1420",
    // Screenshot on failure for debugging
    screenshot: "only-on-failure",
    trace: "on-first-retry",
  },

  projects: [
    {
      name: "chromium",
      use: { ...devices["Desktop Chrome"] },
    },
  ],

  // Start Tauri dev server before tests
  webServer: {
    command: "npm run tauri:dev",
    port: 1420,
    timeout: 120 * 1000,
    reuseExistingServer: !process.env.CI,
    env: {
      // Enable replay mode for testing (path set per test suite or globally)
      CLEANSCOPE_REPLAY_PATH: process.env.CLEANSCOPE_REPLAY_PATH || "",
    },
  },
});
