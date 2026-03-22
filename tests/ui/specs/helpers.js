// helpers.js -- Shared test utilities
const path = require("path");
const fs = require("fs");

const MOCK_SCRIPT = fs.readFileSync(
  path.resolve(__dirname, "../tauri-mock.js"),
  "utf-8"
);

/**
 * Navigate to the app with the Tauri mock injected.
 * The mock must run before app.js, so we inject it via addInitScript.
 */
async function loadApp(page, options = {}) {
  // Inject the Tauri mock before any page script runs
  await page.addInitScript({ content: MOCK_SCRIPT });

  // Optionally override specific command handlers before navigating
  if (options.overrides) {
    await page.addInitScript({
      content: `
        (() => {
          const overrides = ${JSON.stringify(options.overrides)};
          for (const [cmd, value] of Object.entries(overrides)) {
            window.__FIXTURES__.COMMANDS[cmd] = () => value;
          }
        })();
      `,
    });
  }

  // Navigate and wait for the DOM to settle
  await page.goto("/");
  await page.waitForLoadState("domcontentloaded");
  // Wait for the app to finish its initial load cycle
  await page.waitForSelector("#header", { state: "visible" });
  // Give the async DOMContentLoaded handler time to complete
  await page.waitForTimeout(200);
}

module.exports = { loadApp, MOCK_SCRIPT };
