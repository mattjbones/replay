// Tests for tab/view navigation
const { test, expect } = require("@playwright/test");
const { loadApp } = require("./helpers");

test.describe("Tab navigation", () => {
  test.beforeEach(async ({ page }) => {
    await loadApp(page);
  });

  test("clicking GitHub tab shows the GitHub view", async ({ page }) => {
    await page.click('.nav-tab[data-view="github"]');
    await expect(page.locator("#view-github")).toBeVisible();
    await expect(page.locator("#view-overview")).toBeHidden();
  });

  test("clicking Linear tab shows the Linear view", async ({ page }) => {
    await page.click('.nav-tab[data-view="linear"]');
    await expect(page.locator("#view-linear")).toBeVisible();
    await expect(page.locator("#view-overview")).toBeHidden();
  });

  test("clicking Slack tab shows the coming soon view", async ({ page }) => {
    await page.click('.nav-tab[data-view="slack"]');
    await expect(page.locator("#view-slack")).toBeVisible();
    await expect(page.locator("#view-slack .coming-soon-badge")).toHaveText("Coming Soon");
  });

  test("clicking Notion tab shows the coming soon view", async ({ page }) => {
    await page.click('.nav-tab[data-view="notion"]');
    await expect(page.locator("#view-notion")).toBeVisible();
    await expect(page.locator("#view-notion .coming-soon-badge")).toHaveText("Coming Soon");
  });

  test("clicking Trends tab shows the trends view", async ({ page }) => {
    await page.click('.nav-tab[data-view="trends"]');
    await expect(page.locator("#view-trends")).toBeVisible();
    await expect(page.locator("#view-overview")).toBeHidden();
  });

  test("switching back to Overview restores the dashboard", async ({ page }) => {
    // Go to GitHub first
    await page.click('.nav-tab[data-view="github"]');
    await expect(page.locator("#view-github")).toBeVisible();

    // Switch back
    await page.click('.nav-tab[data-view="overview"]');
    await expect(page.locator("#view-overview")).toBeVisible();
    await expect(page.locator("#view-github")).toBeHidden();
  });

  test("active tab styling follows clicks", async ({ page }) => {
    const githubTab = page.locator('.nav-tab[data-view="github"]');
    const overviewTab = page.locator('.nav-tab[data-view="overview"]');

    await githubTab.click();
    await expect(githubTab).toHaveClass(/active/);
    await expect(overviewTab).not.toHaveClass(/active/);
  });
});
