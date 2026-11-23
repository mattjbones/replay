// Work Day Types card on the Trends view
const { test, expect } = require("@playwright/test");
const { loadApp } = require("./helpers");

test("day types show share-of-days bars and a labelled activity mix", async ({ page }) => {
  await loadApp(page);
  await page.click('.nav-tab[data-view="trends"]');

  const cards = page.locator("#cluster-container .cluster-card");
  await expect(cards).toHaveCount(3);

  // Fixture: 5 + 3 + 2 = 10 days, so Coding Day fills half the track
  const coding = cards.filter({ hasText: "Coding Day" });
  await expect(coding.locator(".cluster-count")).toHaveText("5 days");
  await expect(coding.locator(".cluster-bar-fill")).toHaveAttribute("style", /width:50\.0%/);
  // Top dimensions by per-day average, labelled; sub-0.5 values are dropped
  await expect(coding.locator(".cluster-mix")).toContainText("8 Coding · 1 PRs · 0.5 Reviews");
});
