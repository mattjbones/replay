// Tests for the Debug drawer (slide-out panel with all activities)
const { test, expect } = require("@playwright/test");
const { loadApp } = require("./helpers");

/**
 * Helper: open the settings modal, then click "Show All Activities" to open the debug drawer.
 */
async function openDebugDrawer(page) {
  await page.click("#action-settings");
  await expect(page.locator("#settings-overlay")).toBeVisible();

  const debugBtn = page.locator("#debug-show-activities-btn");
  await expect(debugBtn).toBeVisible();
  await debugBtn.click();

  // Wait for the drawer overlay to appear
  await expect(page.locator("#debug-drawer-overlay")).toBeVisible();
}

test.describe("Debug drawer", () => {
  test.beforeEach(async ({ page }) => {
    await loadApp(page);
  });

  test('"Show All Activities" button exists in settings', async ({ page }) => {
    await page.click("#action-settings");
    await expect(page.locator("#settings-overlay")).toBeVisible();

    const debugBtn = page.locator("#debug-show-activities-btn");
    await expect(debugBtn).toBeVisible();
    await expect(debugBtn).toHaveText("Show All Activities");
  });

  test("clicking the button opens the debug drawer", async ({ page }) => {
    await openDebugDrawer(page);

    // Drawer should be visible with the correct heading
    const drawer = page.locator(".debug-drawer");
    await expect(drawer).toBeVisible();
    await expect(drawer.locator("h2")).toHaveText("All Activities");
  });

  test("drawer shows a table with activity rows", async ({ page }) => {
    await openDebugDrawer(page);

    // The table should be rendered with rows from fixture data (10 activities)
    const table = page.locator("#debug-activities-table table.activity-table");
    await expect(table).toBeVisible();

    const rows = table.locator("tbody tr");
    const count = await rows.count();
    expect(count).toBeGreaterThan(0);
    // Fixture data has 10 activities
    expect(count).toBe(10);
  });

  test("drawer has filter controls", async ({ page }) => {
    await openDebugDrawer(page);

    // Text search input
    const textFilter = page.locator("#debug-filter");
    await expect(textFilter).toBeVisible();
    await expect(textFilter).toHaveAttribute("placeholder", /filter/i);

    // Source dropdown
    const sourceFilter = page.locator("#debug-source-filter");
    await expect(sourceFilter).toBeVisible();

    // Kind dropdown
    const kindFilter = page.locator("#debug-kind-filter");
    await expect(kindFilter).toBeVisible();

    // Activity count display
    const count = page.locator("#debug-count");
    await expect(count).toBeVisible();
    await expect(count).toContainText("10 / 10 activities");
  });

  test("text filter narrows the displayed rows", async ({ page }) => {
    await openDebugDrawer(page);

    const textFilter = page.locator("#debug-filter");
    await textFilter.fill("auth");

    // Wait for re-render
    await page.waitForTimeout(100);

    const rows = page.locator("#debug-activities-table tbody tr");
    const count = await rows.count();
    // "refactor auth module" should match
    expect(count).toBeGreaterThan(0);
    expect(count).toBeLessThan(10);
  });

  test("source filter narrows the displayed rows", async ({ page }) => {
    await openDebugDrawer(page);

    const sourceFilter = page.locator("#debug-source-filter");
    await sourceFilter.selectOption("linear");

    await page.waitForTimeout(100);

    const rows = page.locator("#debug-activities-table tbody tr");
    const count = await rows.count();
    // Fixture has 3 linear activities
    expect(count).toBe(3);

    // Count display should update
    await expect(page.locator("#debug-count")).toContainText("3 / 10 activities");
  });

  test("kind filter narrows the displayed rows", async ({ page }) => {
    await openDebugDrawer(page);

    const kindFilter = page.locator("#debug-kind-filter");
    await kindFilter.selectOption("pr_merged");

    await page.waitForTimeout(100);

    const rows = page.locator("#debug-activities-table tbody tr");
    const count = await rows.count();
    // Fixture has 2 pr_merged activities
    expect(count).toBe(2);
  });

  test("closes when clicking the close button", async ({ page }) => {
    await openDebugDrawer(page);
    await expect(page.locator("#debug-drawer-overlay")).toBeVisible();

    await page.click("#debug-drawer-close");
    await expect(page.locator("#debug-drawer-overlay")).not.toBeAttached();
  });

  test("closes when clicking the overlay backdrop", async ({ page }) => {
    await openDebugDrawer(page);
    await expect(page.locator("#debug-drawer-overlay")).toBeVisible();

    // Click on the overlay area (far left, outside the drawer which is aligned right)
    await page.locator("#debug-drawer-overlay").click({ position: { x: 10, y: 400 } });
    await expect(page.locator("#debug-drawer-overlay")).not.toBeAttached();
  });

  test("closes when pressing Escape", async ({ page }) => {
    await openDebugDrawer(page);
    await expect(page.locator("#debug-drawer-overlay")).toBeVisible();

    await page.keyboard.press("Escape");
    await expect(page.locator("#debug-drawer-overlay")).not.toBeAttached();
  });
});

test.describe("Debug drawer pagination", () => {
  test('"Load more" button appears when activities exceed page size', async ({
    page,
  }) => {
    // Generate 150 activities to exceed the 100-row page size
    const manyActivities = [];
    for (let i = 0; i < 150; i++) {
      manyActivities.push({
        id: `01J${String(i).padStart(8, "0")}`,
        source: i % 3 === 0 ? "github" : i % 3 === 1 ? "linear" : "slack",
        source_id: `src-${i}`,
        kind: "commit_pushed",
        title: `Activity number ${i}`,
        description: null,
        url: `https://example.com/${i}`,
        project: "test-project",
        occurred_at: new Date().toISOString(),
        metadata: {},
        synced_at: new Date().toISOString(),
      });
    }

    await loadApp(page, {
      overrides: {
        get_all_activities: manyActivities,
      },
    });

    await page.click("#action-settings");
    await expect(page.locator("#settings-overlay")).toBeVisible();
    await page.locator("#debug-show-activities-btn").click();
    await expect(page.locator("#debug-drawer-overlay")).toBeVisible();

    // Should show exactly 100 rows on the first page
    const rows = page.locator("#debug-activities-table tbody tr");
    await expect(rows).toHaveCount(100);

    // "Load more" button should be present
    const loadMoreBtn = page.locator("#debug-load-more");
    await expect(loadMoreBtn).toBeVisible();
    await expect(loadMoreBtn).toContainText("more");
    await expect(loadMoreBtn).toContainText("100 / 150");

    // Click "Load more" to show the remaining 50
    await loadMoreBtn.click();
    await expect(rows).toHaveCount(150);

    // "Load more" should no longer be present
    await expect(page.locator("#debug-load-more")).not.toBeAttached();

    // Should show "Showing all 150 activities"
    await expect(page.locator("#debug-activities-table")).toContainText(
      "Showing all 150 activities"
    );
  });
});
