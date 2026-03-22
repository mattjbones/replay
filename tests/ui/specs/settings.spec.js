// Tests for the Settings modal
const { test, expect } = require("@playwright/test");
const { loadApp } = require("./helpers");

test.describe("Settings modal", () => {
  test.beforeEach(async ({ page }) => {
    await loadApp(page);
  });

  test("opens when clicking the Settings button", async ({ page }) => {
    await expect(page.locator("#settings-overlay")).toBeHidden();
    await page.click("#action-settings");
    await expect(page.locator("#settings-overlay")).toBeVisible();
    await expect(page.locator(".settings-modal h2")).toHaveText("Settings");
  });

  test("closes when clicking the X button", async ({ page }) => {
    await page.click("#action-settings");
    await expect(page.locator("#settings-overlay")).toBeVisible();

    await page.click("#settings-close");
    await expect(page.locator("#settings-overlay")).toBeHidden();
  });

  test("closes when clicking the overlay backdrop", async ({ page }) => {
    await page.click("#action-settings");
    await expect(page.locator("#settings-overlay")).toBeVisible();

    // Click on the overlay (outside the modal)
    await page.locator("#settings-overlay").click({ position: { x: 10, y: 10 } });
    await expect(page.locator("#settings-overlay")).toBeHidden();
  });

  test("closes when pressing Escape", async ({ page }) => {
    await page.click("#action-settings");
    await expect(page.locator("#settings-overlay")).toBeVisible();

    await page.keyboard.press("Escape");
    await expect(page.locator("#settings-overlay")).toBeHidden();
  });

  test("shows Connections section", async ({ page }) => {
    await page.click("#action-settings");
    const connectionsTitle = page.locator(".settings-section-title", { hasText: "Connections" });
    await expect(connectionsTitle).toBeVisible();
  });
});
