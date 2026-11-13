// Slack tab: message text is hidden until explicitly revealed.
const { test, expect } = require("@playwright/test");
const { loadApp } = require("./helpers");

const now = new Date().toISOString();
const slackActivity = (project, description) => ({
  id: `s-${project}`, source: "slack", source_id: `s-${project}`, kind: "message_sent",
  title: `Message in ${project}`, description, url: "https://slack.example/msg",
  project, occurred_at: now, metadata: {}, synced_at: now,
});

test("hides Slack message text until Show messages is clicked", async ({ page }) => {
  await loadApp(page, {
    overrides: {
      get_digest: {
        period: { Week: now.slice(0, 10) },
        activities: [slackActivity("@Sam", "secret dm text"), slackActivity("general", "channel text")],
        stats: { total_activities: 2, by_source: { slack: 2 }, by_kind: { message_sent: 2 } },
      },
    },
  });
  await page.click('.nav-tab[data-view="slack"]');
  const table = page.locator("#view-slack .activity-table");

  await expect(table).toContainText("Open in Slack");
  await expect(table).not.toContainText("secret dm text");
  // DMs aren't prefixed with '#'; channels are.
  await expect(table).toContainText("@Sam");
  await expect(table).not.toContainText("#@Sam");
  await expect(table).toContainText("#general");

  await page.click("#slack-toggle-text");
  await expect(table).toContainText("secret dm text");
  await expect(page.locator("#slack-toggle-text")).toHaveText("Hide messages");

  await page.click("#slack-toggle-text");
  await expect(table).not.toContainText("secret dm text");
});
