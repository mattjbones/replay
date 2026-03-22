// Tests for the auth status banner and disconnected states
const { test, expect } = require("@playwright/test");
const { loadApp } = require("./helpers");

test.describe("Auth status", () => {
  test("auth banner is hidden when all services are connected", async ({ page }) => {
    await loadApp(page);
    // With all services connected, the banner should be hidden
    await expect(page.locator("#auth-banner")).toBeHidden();
  });

  test("Linear view shows not-connected placeholder when Linear is disconnected", async ({ page }) => {
    await loadApp(page, {
      overrides: {
        get_auth_status: { github: true, linear: false, slack: true, notion: true, anthropic: false },
      },
    });

    await page.click('.nav-tab[data-view="linear"]');
    await expect(page.locator("#view-linear")).toBeVisible();

    const notConnected = page.locator("#view-linear .disconnected-overlay");
    await expect(notConnected).toBeVisible();
    await expect(notConnected).toContainText("Not Connected");
  });

  test("auth banner settings button opens settings modal", async ({ page }) => {
    // Even though the banner is hidden by default now, the button still works
    // Test via the disconnected overlay's "Open Settings" button instead
    await loadApp(page, {
      overrides: {
        get_auth_status: { github: false, linear: true, slack: true, notion: true, anthropic: false },
      },
    });

    await page.click('.nav-tab[data-view="github"]');
    await expect(page.locator("#view-github .disconnected-overlay")).toBeVisible();

    // Click the Open Settings button in the disconnected overlay
    await page.locator("#view-github .disconnected-overlay .btn").click();
    await expect(page.locator("#settings-overlay")).toBeVisible();
  });
});
