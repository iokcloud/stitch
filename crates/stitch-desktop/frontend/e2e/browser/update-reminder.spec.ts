import { expect, test } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

test.describe("启动更新提醒（升级提醒横幅）", () => {
  test("无更新：不打扰，聊天照常可用", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");

    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId("update-banner")).toHaveCount(0);
    await assertUiHygiene(page);
  });

  test("有更新：进入聊天后横幅可见（版本 + 说明）", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, updateAvailable: true });
    await page.goto("/");

    await expect(page.getByTestId("update-banner")).toBeVisible({ timeout: 15_000 });
    await expect(page.getByText("新版本 v0.1.3 可用")).toBeVisible();
    await expect(page.getByTestId("update-banner-notes")).toContainText("启动更新提醒");
    await assertUiHygiene(page);
  });

  test("首次启动向导期间不打扰；完成后横幅出现", async ({ page }) => {
    await mockTauri(page, { apiKeySet: false, updateAvailable: true });
    await page.goto("/");

    await expect(page.getByTestId("firstrun-wizard")).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId("update-banner")).toHaveCount(0);

    // 完成向导 → 进入聊天 → 升级提醒照常出现
    const wizard = page.getByTestId("firstrun-wizard");
    await wizard.getByTestId("fr-key").fill("sk-test-123");
    await wizard.getByTestId("fr-model").fill("deepseek-v4-flash");
    await wizard.getByRole("button", { name: "测试连接" }).click();
    await expect(page.getByText("连接成功")).toBeVisible({ timeout: 10_000 });
    await page.getByRole("button", { name: "下一步" }).click();
    await page.getByRole("button", { name: "开始使用" }).click();

    await expect(page.getByTestId("update-banner")).toBeVisible({ timeout: 15_000 });
  });

  test("点「更新」→ 进入安装中状态", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, updateAvailable: true });
    await page.goto("/");

    await expect(page.getByTestId("update-banner")).toBeVisible({ timeout: 15_000 });
    await page.getByTestId("update-banner-install").click();
    await expect(page.getByText("正在安装更新…")).toBeVisible();
  });

  test("「稍后」→ 横幅关闭（本次启动不再出现）", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, updateAvailable: true });
    await page.goto("/");

    await expect(page.getByTestId("update-banner")).toBeVisible({ timeout: 15_000 });
    await page.getByTestId("update-banner-later").click();
    await expect(page.getByTestId("update-banner")).toHaveCount(0);

    // 从设置返回聊天也不重查（store 自守卫）
    await page.getByTestId("open-settings").click();
    await expect(page.getByTestId("settings-view")).toBeVisible();
    await page.getByTestId("settings-back-chat").click();
    await expect(page.getByTestId("update-banner")).toHaveCount(0);
  });
});
