import { defineConfig, devices } from "@playwright/test";

const port = 4173;
const baseURL = `http://127.0.0.1:${port}`;

// Prefer system Chrome (no Playwright browser download). Set
// PLAYWRIGHT_CHROMIUM=1 to use bundled Chromium after `npx playwright install`.
const useBundled = process.env.PLAYWRIGHT_CHROMIUM === "1";

export default defineConfig({
  testDir: "./e2e/browser",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  workers: process.env.CI ? 1 : undefined,
  reporter: [["list"]],
  use: {
    ...devices["Desktop Chrome"],
    ...(useBundled ? {} : { channel: "chrome" }),
    baseURL,
    trace: "on-first-retry",
  },
  webServer: {
    command: "npm run preview -- --host 127.0.0.1 --port 4173",
    url: baseURL,
    // After `npm run build`, prefer a fresh preview so UI tokens are not stale.
    reuseExistingServer: process.env.PLAYWRIGHT_REUSE === "1",
    timeout: 120_000,
  },
});
