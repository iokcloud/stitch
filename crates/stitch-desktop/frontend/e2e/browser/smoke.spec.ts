import { expect, test } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

test.describe("Stitch UI smoke (browser / mock IPC)", () => {
  test("first-run shows settings when API key unset", async ({ page }) => {
    await mockTauri(page, { apiKeySet: false });
    await page.goto("/");

    await expect(page.getByTestId("settings-view")).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId("settings-save")).toBeVisible();
    await expect(page.getByTestId("chat-view")).toHaveCount(0);
    await expect(page.getByTestId("diag-view")).toHaveText(/view=settings/);
    await assertUiHygiene(page);
  });

  test("configured user lands on chat", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");

    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId("chat-input")).toBeVisible();
    await expect(page.getByTestId("settings-view")).toHaveCount(0);
    await expect(page.getByTestId("diag-view")).toHaveText(/view=chat/);
    await assertUiHygiene(page);
  });

  test("library tab lists suites with token; plan mode toggle visible", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, apiTokenSet: true });
    await page.goto("/");

    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId("plan-mode-toggle")).toBeVisible();
    await page.getByTestId("library-tab").click();
    await expect(page.getByTestId("library-panel")).toBeVisible();
    await expect(page.getByTestId("library-mature-scenes")).toBeVisible();
    await page.getByTestId("library-tab-suites").click();
    await expect(page.getByText("演示套件")).toBeVisible({ timeout: 5_000 });
    await assertUiHygiene(page);
  });

  test("Skill tab fills composer; terminal drawer toggles", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    await page.getByTestId("library-tab").click();
    await page.getByTestId("library-tab-skills").click();
    await expect(page.getByTestId("library-skill-pm-prd-demo")).toBeVisible({ timeout: 5_000 });
    await page.getByTestId("library-skill-pm-prd-demo").click();
    await expect(page.getByTestId("chat-input")).toHaveValue(/安装 PromptStdio/);

    await page.getByTestId("toggle-terminal").click();
    await expect(page.getByTestId("terminal-panel")).toBeVisible();
    await page.getByTestId("terminal-close").click();
    await expect(page.getByTestId("terminal-panel")).toHaveCount(0);
    await assertUiHygiene(page);
  });

  test("need-token CTA opens account settings tab", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, apiTokenSet: false });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    await page.getByTestId("library-tab").click();
    await page.getByTestId("library-tab-agents").click();
    await expect(page.getByTestId("library-need-token")).toBeVisible();
    await page.getByTestId("library-open-account-settings").click();

    await expect(page.getByTestId("settings-view")).toBeVisible({ timeout: 10_000 });
    await expect(page.getByTestId("settings-tab-account")).toHaveClass(/settings-nav-item-active/);
    await expect(page.getByTestId("settings-tab-model")).not.toHaveClass(/settings-nav-item-active/);
    await assertUiHygiene(page);
  });

  test("suite 401 stays in library panel with L1 copy (no raw JSON in chat)", async ({ page }) => {
    await mockTauri(page, {
      apiKeySet: true,
      apiTokenSet: true,
      suiteAuthFail: true,
    });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await page.getByTestId("library-tab").click();
    await page.getByTestId("library-tab-suites").click();
    await expect(page.getByText("演示套件")).toBeVisible({ timeout: 5_000 });
    await page.getByText("演示套件").click();

    await expect(page.getByTestId("library-run-error")).toBeVisible({ timeout: 5_000 });
    await expect(page.getByTestId("library-run-error")).toContainText("账号未连接或已失效");
    await expect(page.getByRole("log")).not.toContainText("API error 401");
    await expect(page.getByRole("log")).not.toContainText("Unauthorized");
    await page.getByTestId("library-run-error-settings").click();
    await expect(page.getByTestId("settings-view")).toBeVisible({ timeout: 10_000 });
    await assertUiHygiene(page);
  });
});
