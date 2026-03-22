// Tests for the GitHub source view
const { test, expect } = require("@playwright/test");
const { loadApp } = require("./helpers");

test.describe("GitHub view", () => {
  test("renders stats cards and tables when connected", async ({ page }) => {
    await loadApp(page);
    await page.click('.nav-tab[data-view="github"]');

    // Wait for the view to render
    await expect(page.locator("#view-github")).toBeVisible();

    // Stats row should have stat cards
    const statsRow = page.locator("#github-stats");
    await expect(statsRow).not.toBeEmpty();

    // PR table should have rows from fixture data (2 PRs: 1 merged, 1 opened)
    const prRows = page.locator("#github-pr-tbody tr");
    const prCount = await prRows.count();
    expect(prCount).toBeGreaterThan(0);

    // Commit table should have rows
    const commitRows = page.locator("#github-commit-tbody tr");
    const commitCount = await commitRows.count();
    expect(commitCount).toBeGreaterThan(0);

    // Review table should have rows
    const reviewRows = page.locator("#github-review-tbody tr");
    const reviewCount = await reviewRows.count();
    expect(reviewCount).toBeGreaterThan(0);
  });

  test("shows 'not connected' placeholder when GitHub is disconnected", async ({ page }) => {
    await loadApp(page, {
      overrides: {
        get_auth_status: { github: false, linear: true, slack: true, notion: true, anthropic: false },
      },
    });

    await page.click('.nav-tab[data-view="github"]');
    await expect(page.locator("#view-github")).toBeVisible();

    // Should show the "Not Connected" overlay
    const notConnected = page.locator("#view-github .disconnected-overlay");
    await expect(notConnected).toBeVisible();
    await expect(notConnected).toContainText("Not Connected");
  });
});
