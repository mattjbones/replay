// Tests for GitHub view card ordering (PR #22, issue #12)
// Verifies that the PRs and Commits cards use CSS `order` to control
// visual position based on the workflow mode (pr vs trunk).
const { test, expect } = require("@playwright/test");
const { loadApp } = require("./helpers");

test.describe("GitHub card order", () => {
  test("both cards have the correct id attributes", async ({ page }) => {
    await loadApp(page);
    await page.click('.nav-tab[data-view="github"]');
    await expect(page.locator("#view-github")).toBeVisible();

    const prsCard = page.locator("#github-card-prs");
    const commitsCard = page.locator("#github-card-commits");
    await expect(prsCard).toBeAttached();
    await expect(commitsCard).toBeAttached();

    // Verify they are .card elements within the source-view
    await expect(prsCard).toHaveClass(/card/);
    await expect(commitsCard).toHaveClass(/card/);
  });

  test("in PR workflow (default), PRs card has order 1 and Commits card has order 2", async ({ page }) => {
    // Default fixture config has github.workflow = "pr"
    await loadApp(page);
    await page.click('.nav-tab[data-view="github"]');
    await expect(page.locator("#view-github")).toBeVisible();

    const prsOrder = await page.locator("#github-card-prs").evaluate(
      (el) => getComputedStyle(el).order
    );
    const commitsOrder = await page.locator("#github-card-commits").evaluate(
      (el) => getComputedStyle(el).order
    );

    expect(prsOrder).toBe("1");
    expect(commitsOrder).toBe("2");
  });

  test("in trunk workflow, Commits card has order 1 and PRs card has order 2", async ({ page }) => {
    // Override config to set trunk workflow
    const trunkConfig = {
      schedule: { sync_interval_minutes: 5, daily_reminder_time: "17:00", weekly_reminder_day: "Friday" },
      ttl: { hot_minutes: 5, warm_minutes: 60, cold_minutes: 1440 },
      github: { username: "testuser", workflow: "trunk" },
      linear: {},
      slack: { user_id: null, ignored_channels: [], client_id: null, client_secret: null },
      notion: {},
      llm: { enabled: false, model: "claude-haiku-4-5-20251001" },
      working_hours: { work_start: "09:00", work_end: "17:00", working_days: ["Mon", "Tue", "Wed", "Thu", "Fri"], timezone: "UTC" },
      dashboard_layout: {},
    };

    await loadApp(page, {
      overrides: { get_config: trunkConfig },
    });
    await page.click('.nav-tab[data-view="github"]');
    await expect(page.locator("#view-github")).toBeVisible();

    const prsOrder = await page.locator("#github-card-prs").evaluate(
      (el) => getComputedStyle(el).order
    );
    const commitsOrder = await page.locator("#github-card-commits").evaluate(
      (el) => getComputedStyle(el).order
    );

    expect(prsOrder).toBe("2");
    expect(commitsOrder).toBe("1");
  });

  test("ordering uses CSS order property, not DOM position", async ({ page }) => {
    // In PR workflow (default), PRs card comes first visually via CSS order,
    // but in the DOM the PRs card also comes first (it's defined first in HTML).
    // In trunk workflow, the DOM order stays the same but CSS order flips.
    // We verify this by checking that DOM order is always PRs-before-Commits
    // regardless of workflow, while CSS order changes.

    // First: trunk workflow
    const trunkConfig = {
      schedule: { sync_interval_minutes: 5, daily_reminder_time: "17:00", weekly_reminder_day: "Friday" },
      ttl: { hot_minutes: 5, warm_minutes: 60, cold_minutes: 1440 },
      github: { username: "testuser", workflow: "trunk" },
      linear: {},
      slack: { user_id: null, ignored_channels: [], client_id: null, client_secret: null },
      notion: {},
      llm: { enabled: false, model: "claude-haiku-4-5-20251001" },
      working_hours: { work_start: "09:00", work_end: "17:00", working_days: ["Mon", "Tue", "Wed", "Thu", "Fri"], timezone: "UTC" },
      dashboard_layout: {},
    };

    await loadApp(page, {
      overrides: { get_config: trunkConfig },
    });
    await page.click('.nav-tab[data-view="github"]');
    await expect(page.locator("#view-github")).toBeVisible();

    // DOM order: PRs card always comes before Commits card in HTML
    const domOrder = await page.evaluate(() => {
      const sourceView = document.querySelector("#view-github .source-view");
      const cards = [...sourceView.querySelectorAll(":scope > .card")];
      const prsIndex = cards.findIndex((c) => c.id === "github-card-prs");
      const commitsIndex = cards.findIndex((c) => c.id === "github-card-commits");
      return { prsIndex, commitsIndex };
    });
    expect(domOrder.prsIndex).toBeLessThan(domOrder.commitsIndex);

    // But CSS order is flipped for trunk: commits=1, prs=2
    const prsOrder = await page.locator("#github-card-prs").evaluate(
      (el) => getComputedStyle(el).order
    );
    const commitsOrder = await page.locator("#github-card-commits").evaluate(
      (el) => getComputedStyle(el).order
    );
    expect(Number(commitsOrder)).toBeLessThan(Number(prsOrder));

    // Verify the source-view is a flex container (required for order to work)
    const display = await page.locator("#view-github .source-view").evaluate(
      (el) => getComputedStyle(el).display
    );
    expect(display).toBe("flex");
  });
});
