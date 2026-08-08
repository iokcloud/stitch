import { expect, test } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

test.describe("Stitch UI smoke (browser / mock IPC)", () => {
  test("first-run shows wizard when API key unset and completes setup", async ({ page }) => {
    await mockTauri(page, { apiKeySet: false });
    await page.goto("/");

    await expect(page.getByTestId("firstrun-wizard")).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId("chat-view")).toHaveCount(0);
    // 填密钥 + 模型 → 测试连接 → 下一步 → 开始使用
    // （向导悬浮层与设置页同挂，交互一律限定在向导内）
    const wizard = page.getByTestId("firstrun-wizard");
    await wizard.getByTestId("fr-key").fill("sk-test-123");
    await wizard.getByTestId("fr-model").fill("deepseek-v4-flash");
    await wizard.getByRole("button", { name: "测试连接" }).click();
    await expect(wizard.getByText("连接成功")).toBeVisible({ timeout: 10_000 });
    await wizard.getByRole("button", { name: "下一步" }).click();
    await wizard.getByRole("button", { name: "开始使用" }).click();
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await assertUiHygiene(page);
  });

  test("configured user lands on chat", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, streamChat: true, streamToolOutput: true });
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
    await mockTauri(page, { apiKeySet: true, streamChat: true, streamToolOutput: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    await page.getByTestId("library-tab").click();
    await page.getByTestId("library-tab-skills").click();
    await expect(page.getByTestId("library-skill-pm-prd-demo")).toBeVisible({ timeout: 5_000 });
    await page.getByTestId("library-skill-pm-prd-demo").click();
    await expect(page.getByTestId("chat-input")).toHaveValue(/安装 PromptStdio/);

    // run_command 不再自动弹出终端（用户决策——输出留在消息流工具卡内），
    // 终端经命令面板手动打开。
    await page.getByTestId("chat-input").fill("跑一个命令");
    await page.getByTestId("chat-send").click();
    await expect(page.getByTestId("stream-rail")).toBeVisible({ timeout: 10_000 });
    await expect(page.getByTestId("terminal-panel")).toHaveCount(0);

    await page.keyboard.press("Control+k");
    const termItem = page.getByTestId("palette-item").filter({ hasText: "打开终端" });
    await expect(termItem).toBeVisible();
    await termItem.click();
    await expect(page.getByTestId("terminal-panel")).toBeVisible({ timeout: 10_000 });
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
