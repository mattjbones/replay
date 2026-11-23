// "This week's pace" card on the Trends view
const { test, expect } = require("@playwright/test");
const { loadApp } = require("./helpers");

test("pace card compares this week so far with the same point in past weeks", async ({ page }) => {
  await loadApp(page);
  await page.click('.nav-tab[data-view="trends"]');

  await expect(page.locator("#pace-title")).toHaveText("This week's pace · Wednesday (day 3 of 7)");
  const rows = page.locator("#pace-content tbody tr");
  await expect(rows).toHaveCount(4); // 3 metrics + overall

  const merged = rows.filter({ hasText: "PRs merged" });
  await expect(merged.locator("td")).toHaveText(["PRs merged", "6", "4", "3.5", /Ahead/]);
  await expect(rows.filter({ hasText: "Issues done" }).locator(".pace-chip")).toHaveClass(/pace-behind/);
  await expect(rows.last()).toHaveClass(/pace-overall/);
});
