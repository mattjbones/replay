// Tests for the default Overview view
const { test, expect } = require("@playwright/test");
const { loadApp } = require("./helpers");

test.describe("Overview (default view)", () => {
  test.beforeEach(async ({ page }) => {
    await loadApp(page);
  });

  test("renders the header with app name and nav tabs", async ({ page }) => {
    await expect(page.locator(".logo")).toHaveText("Recap");

    const navTabs = page.locator("#nav-tabs .nav-tab");
    await expect(navTabs).toHaveCount(6); // Overview, GitHub, Linear, Slack, Notion, Trends

    // Overview tab is active by default
    const overviewTab = navTabs.first();
    await expect(overviewTab).toHaveClass(/active/);
    await expect(overviewTab).toHaveText("Overview");
  });

  test("shows headline metrics with fixture data", async ({ page }) => {
    const metrics = page.locator("#headline-metrics");
    await expect(metrics).toBeVisible();

    // Total activities = 10 from fixtures
    const totalValue = page.locator("#hl-total .headline-value");
    await expect(totalValue).toHaveText("10");

    // PRs merged = 2
    const prsValue = page.locator("#hl-prs .headline-value");
    await expect(prsValue).toHaveText("2");

    // Reviews = 2
    const reviewsValue = page.locator("#hl-reviews .headline-value");
    await expect(reviewsValue).toHaveText("2");

    // Issues completed = 1
    const issuesValue = page.locator("#hl-issues .headline-value");
    await expect(issuesValue).toHaveText("1");
  });

  test("renders the dashboard grid with cards", async ({ page }) => {
    const dashboard = page.locator("#dashboard");
    await expect(dashboard).toBeVisible();

    // Verify key cards are present
    await expect(page.locator("#card-briefing")).toBeVisible();
    await expect(page.locator("#card-activity-chart")).toBeVisible();
    await expect(page.locator("#card-pr-stats")).toBeVisible();
    await expect(page.locator("#card-features")).toBeVisible();
    await expect(page.locator("#card-activity-table")).toBeVisible();
  });

  test("renders the activity table with rows", async ({ page }) => {
    const rows = page.locator("#activity-tbody tr");
    // Should have rows from the fixture activities
    const count = await rows.count();
    expect(count).toBeGreaterThan(0);
  });

  test("displays the date in the header", async ({ page }) => {
    const dateEl = page.locator("#header-date");
    await expect(dateEl).not.toBeEmpty();
    // Should contain a readable date string
    const text = await dateEl.textContent();
    expect(text).toMatch(/\w+,\s+\w+\s+\d+,\s+\d{4}/); // e.g. "Sunday, March 22, 2026"
  });

  test("period tabs are present and day is active by default", async ({ page }) => {
    const dayTab = page.locator('.tab[data-period="day"]');
    await expect(dayTab).toHaveClass(/active/);

    const weekTab = page.locator('.tab[data-period="week"]');
    await expect(weekTab).not.toHaveClass(/active/);
  });
});
