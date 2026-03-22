// @ts-check
const { defineConfig } = require("@playwright/test");
const path = require("path");

module.exports = defineConfig({
  testDir: "./specs",
  timeout: 30_000,
  retries: 0,
  reporter: "list",
  use: {
    baseURL: "http://localhost:5174",
    headless: true,
    viewport: { width: 1280, height: 800 },
    actionTimeout: 5_000,
  },
  webServer: {
    // Serve the ui/ directory on port 5174.
    // npx serve is listed as a dependency-free way to do this;
    // we use python3 as a zero-install fallback that works on macOS and Linux CI.
    command: `python3 -m http.server 5174 --directory ${path.resolve(__dirname, "../../ui")}`,
    port: 5174,
    reuseExistingServer: !process.env.CI,
    timeout: 10_000,
  },
  projects: [
    {
      name: "chromium",
      use: { browserName: "chromium" },
    },
  ],
});
