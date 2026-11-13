import { expect, type Page, test } from "@playwright/test";
import { type MockBuildInfo, mockTauriBackend } from "./helpers";

/**
 * E2E tests for the visibility of the debug controls and the build label.
 *
 * Release builds hide the camera-diagnosis buttons and show only the version.
 * Tapping the version label seven times reveals the buttons for field diagnosis.
 * The Tauri backend is stubbed, so these run against the vite dev server alone.
 */

const RELEASE_BUILD: MockBuildInfo = {
  version: "0.5.0",
  git_hash: "07097ed+",
  build_time: "2026-01-01 00:00 UTC",
  debug: false,
};
const DEBUG_BUILD: MockBuildInfo = { ...RELEASE_BUILD, debug: true };

const DEBUG_BUTTON_LABELS = [
  "W:Auto",
  "H:Auto",
  "S:Auto",
  "FMT:Auto",
  "MJPEG:Try",
  "FMT:YUYV",
  "Capture",
];
const UNLOCK_TAPS = 7;
const TAP_WINDOW_MS = 2000;

async function tapVersion(page: Page, times: number): Promise<void> {
  const label = page.locator(".build-info");
  for (let i = 0; i < times; i++) {
    await label.click();
  }
}

async function expectDebugButtons(page: Page, visible: boolean): Promise<void> {
  for (const name of DEBUG_BUTTON_LABELS) {
    const button = page.getByRole("button", { name, exact: true });
    if (visible) {
      await expect(button).toBeVisible();
    } else {
      await expect(button).toHaveCount(0);
    }
  }
}

test.describe("release build", () => {
  test.beforeEach(async ({ page }) => {
    await mockTauriBackend(page, RELEASE_BUILD);
    await page.goto("/");
    await expect(page.locator(".build-info")).toBeVisible();
  });

  test("shows only the version, without commit hash or dirty marker", async ({ page }) => {
    await expect(page.locator(".build-info")).toHaveText("v0.5.0");
  });

  test("hides the debug buttons", async ({ page }) => {
    await expectDebugButtons(page, false);
  });

  test("keeps the resolution button", async ({ page }) => {
    await expect(page.getByRole("button", { name: "Resolution", exact: true })).toBeVisible();
  });

  test("seven taps on the version reveal the debug buttons, seven more hide them", async ({
    page,
  }) => {
    await tapVersion(page, UNLOCK_TAPS - 1);
    await expectDebugButtons(page, false);

    await tapVersion(page, 1);
    await expectDebugButtons(page, true);
    await expect(page.locator(".build-info")).toHaveText("v0.5.0");

    await tapVersion(page, UNLOCK_TAPS);
    await expectDebugButtons(page, false);
  });

  test("taps separated by a pause do not add up", async ({ page }) => {
    await tapVersion(page, UNLOCK_TAPS - 1);
    await page.waitForTimeout(TAP_WINDOW_MS + 100);
    await tapVersion(page, 1);
    await expectDebugButtons(page, false);
  });
});

test.describe("debug build", () => {
  test.beforeEach(async ({ page }) => {
    await mockTauriBackend(page, DEBUG_BUILD);
    await page.goto("/");
    await expect(page.locator(".build-info")).toBeVisible();
  });

  test("shows the version with the commit hash", async ({ page }) => {
    await expect(page.locator(".build-info")).toHaveText("v0.5.0 (07097ed+)");
  });

  test("shows the debug buttons and the resolution button", async ({ page }) => {
    await expectDebugButtons(page, true);
    await expect(page.getByRole("button", { name: "Resolution", exact: true })).toBeVisible();
  });
});

test("hides the debug buttons when build info is unavailable", async ({ page }) => {
  await page.goto("/");
  await expect(page.getByRole("button", { name: "Resolution", exact: true })).toBeVisible();
  await expectDebugButtons(page, false);
});
