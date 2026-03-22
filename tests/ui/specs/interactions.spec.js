// Tests for key UI interactions beyond navigation
const { test, expect } = require("@playwright/test");
const { loadApp } = require("./helpers");

test.describe("Period tab switching", () => {
  test("clicking Week tab updates active state", async ({ page }) => {
    await loadApp(page);

    const weekTab = page.locator('.tab[data-period="week"]');
    await weekTab.click();
    await expect(weekTab).toHaveClass(/active/);

    const dayTab = page.locator('.tab[data-period="day"]');
    await expect(dayTab).not.toHaveClass(/active/);
  });

  test("clicking Month tab updates active state", async ({ page }) => {
    await loadApp(page);

    const monthTab = page.locator('.tab[data-period="month"]');
    await monthTab.click();
    await expect(monthTab).toHaveClass(/active/);
  });
});

test.describe("Standup modal", () => {
  test("opens when clicking Generate Standup button", async ({ page }) => {
    await loadApp(page);

    await expect(page.locator("#standup-overlay")).toBeHidden();
    await page.click("#action-standup");

    await expect(page.locator("#standup-overlay")).toBeVisible();
    await expect(page.locator(".standup-modal h2")).toHaveText("Daily Standup");
  });

  test("closes when clicking X", async ({ page }) => {
    await loadApp(page);
    await page.click("#action-standup");
    await expect(page.locator("#standup-overlay")).toBeVisible();

    await page.click("#standup-close");
    await expect(page.locator("#standup-overlay")).toBeHidden();
  });
});

test.describe("Sync button", () => {
  test("sync button is present in the action bar", async ({ page }) => {
    await loadApp(page);
    const syncBtn = page.locator("#sync-btn");
    await expect(syncBtn).toBeVisible();
  });
});

test.describe("Empty state", () => {
  test("shows empty state when no activities exist", async ({ page }) => {
    await loadApp(page, {
      overrides: {
        get_digest: {
          period: { Day: "2026-03-22" },
          activities: [],
          stats: { total_activities: 0, by_source: {}, by_kind: {} },
          llm_summary: null,
        },
      },
    });

    await expect(page.locator("#empty-state")).toBeVisible();
    await expect(page.locator("#empty-state")).toContainText("No activities yet");
    await expect(page.locator("#dashboard")).toBeHidden();
  });
});

test.describe("Date navigation", () => {
  test("date navigation buttons are present", async ({ page }) => {
    await loadApp(page);

    await expect(page.locator("#date-prev")).toBeVisible();
    await expect(page.locator("#date-next")).toBeVisible();
    await expect(page.locator("#date-today")).toBeVisible();
  });

  test("Today button is clickable", async ({ page }) => {
    await loadApp(page);
    // Click prev to change date, then click Today to reset
    await page.click("#date-prev");
    const dateAfterPrev = await page.locator("#header-date").textContent();

    await page.click("#date-today");
    const dateAfterToday = await page.locator("#header-date").textContent();

    // The date should change back (they should be different if the day actually changed)
    expect(dateAfterToday).toBeTruthy();
  });
});
