import { Page } from "@playwright/test";

/**
 * Helper functions for CleanScope E2E tests.
 */

/**
 * Wait for the app to fully load and be ready for interaction.
 */
export async function waitForAppReady(page: Page, timeout = 5000): Promise<void> {
  // Wait for the canvas to be visible
  await page.locator("#camera-canvas").waitFor({ state: "visible", timeout });

  // Wait for status bar to be visible
  await page.locator(".status-bar").waitFor({ state: "visible", timeout });
}

/**
 * Get the current connection status from the status indicator.
 */
export async function getConnectionStatus(page: Page): Promise<string | null> {
  const statusText = page.locator(".status-text");
  return await statusText.textContent();
}

/**
 * Check if the device is currently connected.
 */
export async function isConnected(page: Page): Promise<boolean> {
  const statusText = await getConnectionStatus(page);
  return statusText ? !statusText.toLowerCase().includes("waiting") : false;
}

/**
 * Get the current resolution display text.
 */
export async function getResolution(page: Page): Promise<string | null> {
  const resolutionBtn = page.locator(".debug-btn.detected");
  const isVisible = await resolutionBtn.isVisible().catch(() => false);
  return isVisible ? await resolutionBtn.textContent() : null;
}

/**
 * Get the current frame count.
 */
export async function getFrameCount(page: Page): Promise<number> {
  const frameCountEl = page.locator(".frame-count");
  const isVisible = await frameCountEl.isVisible().catch(() => false);

  if (!isVisible) {
    return 0;
  }

  const text = await frameCountEl.textContent();
  const match = text?.match(/(\d+)\s+frames/);
  return match ? parseInt(match[1], 10) : 0;
}

/**
 * Get the build info text.
 */
export async function getBuildInfo(page: Page): Promise<string | null> {
  const buildInfoEl = page.locator(".build-info");
  const isVisible = await buildInfoEl.isVisible().catch(() => false);
  return isVisible ? await buildInfoEl.textContent() : null;
}

/**
 * Click a debug button by its text content.
 */
export async function clickDebugButton(page: Page, text: string): Promise<void> {
  const button = page.locator(`button:has-text('${text}')`);
  await button.click();
}

/**
 * Get canvas element and verify it has rendered content.
 */
export async function getCanvasInfo(page: Page): Promise<{ width: number; height: number }> {
  const canvas = page.locator("#camera-canvas");

  // Get canvas dimensions via JavaScript
  const dimensions = await canvas.evaluate((el: HTMLCanvasElement) => ({
    width: el.width,
    height: el.height,
  }));

  return dimensions as { width: number; height: number };
}

/**
 * Wait for frame count to increase (indicating frames are streaming).
 */
export async function waitForFrames(
  page: Page,
  expectedFrames = 1,
  timeout = 5000,
): Promise<number> {
  const frameCountEl = page.locator(".frame-count");

  const startTime = Date.now();
  while (Date.now() - startTime < timeout) {
    const count = await getFrameCount(page);
    if (count >= expectedFrames) {
      return count;
    }
    await page.waitForTimeout(100);
  }

  throw new Error(
    `Timeout waiting for ${expectedFrames} frames. Current count: ${await getFrameCount(page)}`,
  );
}

/**
 * Dismiss error banner if visible.
 */
export async function dismissError(page: Page): Promise<void> {
  const dismissBtn = page.locator(".error-banner button");
  const errorBanner = page.locator(".error-banner");

  const isVisible = await errorBanner.isVisible().catch(() => false);
  if (isVisible) {
    await dismissBtn.click();
  }
}

/**
 * Get error message text if banner is visible.
 */
export async function getErrorMessage(page: Page): Promise<string | null> {
  const errorBanner = page.locator(".error-banner");
  const isVisible = await errorBanner.isVisible().catch(() => false);

  if (!isVisible) {
    return null;
  }

  const text = await errorBanner.textContent();
  // Remove the dismiss button text
  return text?.replace("Dismiss", "").trim() || null;
}

/**
 * Check if waiting message is displayed (no device connected).
 */
export async function isWaitingForDevice(page: Page): Promise<boolean> {
  const waitingMsg = page.locator(".waiting-message");
  return await waitingMsg.isVisible().catch(() => false);
}
