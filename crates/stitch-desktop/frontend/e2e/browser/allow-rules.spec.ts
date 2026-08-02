import { expect, test } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

const SEED = [
  { tool: "read_file", scope: "path", value: "C:\\work\\src" },
  { tool: "run_command", scope: "command", value: "npm run build" },
];

async function openSystemTab(page: import("@playwright/test").Page) {
  await page.getByTestId("open-settings").click();
  await expect(page.getByTestId("settings-tab-system")).toBeVisible({ timeout: 5_000 });
  await page.getByTestId("settings-tab-system").click();
}

test("allow rules list renders tool/scope/value rows", async ({ page }) => {
  await mockTauri(page, { apiKeySet: true, allowRules: SEED });
  await page.goto("/");
  await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
  await openSystemTab(page);
  await expect(page.getByTestId("allow-rule-row-0")).toBeVisible({ timeout: 5_000 });
  await expect(page.getByTestId("allow-rule-row-1")).toBeVisible();
  await expect(page.getByTestId("allow-rule-row-0")).toContainText("read_file");
  await expect(page.getByTestId("allow-rule-row-0")).toContainText("path");
  await expect(page.getByTestId("allow-rule-row-0")).toContainText("C:\\work\\src");
  await expect(page.getByTestId("allow-rule-row-1")).toContainText("run_command");
  await expect(page.getByTestId("allow-rule-row-1")).toContainText("npm run build");
  await expect(page.getByTestId("allow-rules-clear")).toBeVisible();
  await assertUiHygiene(page);
});

test("removing a row drops it from the list", async ({ page }) => {
  await mockTauri(page, { apiKeySet: true, allowRules: SEED });
  await page.goto("/");
  await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
  await openSystemTab(page);
  await expect(page.getByTestId("allow-rule-row-0")).toBeVisible({ timeout: 5_000 });
  await page.getByTestId("allow-rule-remove-0").click();
  await expect(page.getByTestId("allow-rule-row-0")).toContainText("run_command");
  await expect(page.getByTestId("allow-rule-row-1")).toHaveCount(0);
  await expect(page.getByTestId("settings-footer-status")).toHaveText(/已删除/, { timeout: 5_000 });
  await assertUiHygiene(page);
});

test("clearing requires the two-step inline confirm", async ({ page }) => {
  await mockTauri(page, { apiKeySet: true, allowRules: SEED });
  await page.goto("/");
  await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
  await openSystemTab(page);
  await expect(page.getByTestId("allow-rule-row-0")).toBeVisible({ timeout: 5_000 });
  // First click arms the confirm; list stays.
  await page.getByTestId("allow-rules-clear").click();
  await expect(page.getByTestId("allow-rules-clear-confirm")).toBeVisible();
  await expect(page.getByTestId("allow-rule-row-0")).toBeVisible();
  // Second click clears.
  await page.getByTestId("allow-rules-clear-confirm").click();
  await expect(page.getByTestId("allow-rules-empty")).toBeVisible({ timeout: 5_000 });
  await expect(page.getByTestId("allow-rule-row-0")).toHaveCount(0);
  await expect(page.getByTestId("settings-footer-status")).toHaveText(/已清除/, { timeout: 5_000 });
  await assertUiHygiene(page);
});

test("no rules shows the empty state without a clear button", async ({ page }) => {
  await mockTauri(page, { apiKeySet: true });
  await page.goto("/");
  await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
  await openSystemTab(page);
  await expect(page.getByTestId("allow-rules-empty")).toBeVisible({ timeout: 5_000 });
  await expect(page.getByTestId("allow-rules-clear")).toHaveCount(0);
  await assertUiHygiene(page);
});

test("settings search jumps to the allow-rules group", async ({ page }) => {
  await mockTauri(page, { apiKeySet: true, allowRules: SEED });
  await page.goto("/");
  await expect(page.getByTestId("chat-input")).toBeVisible({ timeout: 15_000 });
  await page.getByTestId("open-settings").click();
  await expect(page.getByTestId("settings-tab-system")).toBeVisible({ timeout: 5_000 });
  await page.getByTestId("settings-search").fill("允许规则");
  await page.getByTestId("settings-search-hit").first().click();
  await expect(page.getByTestId("allow-rule-row-0")).toBeVisible({ timeout: 5_000 });
  await assertUiHygiene(page);
});
