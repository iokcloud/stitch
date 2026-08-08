import { expect, test } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

// 功能公告横幅：启动拉取 → 未读显示（标题/正文/查看详情）→
// 知道了（已读，重载不再出现）/ 稍后（本次不显示，下次照常）。
// 无公告 → 不打扰。

test.describe("功能公告横幅", () => {
  test("无公告：不打扰", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId("announce-banner")).toHaveCount(0);
    await assertUiHygiene(page);
  });

  test("有公告：横幅可见（标题/正文/查看详情）", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, announceAvailable: true });
    await page.goto("/");
    await expect(page.getByTestId("announce-banner")).toBeVisible({ timeout: 15_000 });
    await expect(page.getByText("Stitch 0.2.3 已发布")).toBeVisible();
    await expect(page.getByTestId("announce-banner-text")).toContainText("侧栏分割线");
    await expect(page.getByTestId("announce-banner-open")).toBeVisible();
    await assertUiHygiene(page);
  });

  test("知道了 → 记入已读，重载后不再出现", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, announceAvailable: true });
    await page.goto("/");
    await expect(page.getByTestId("announce-banner")).toBeVisible({ timeout: 15_000 });
    await page.getByTestId("announce-banner-read").click();
    await expect(page.getByTestId("announce-banner")).toHaveCount(0);

    await page.reload();
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId("announce-banner")).toHaveCount(0);
    await assertUiHygiene(page);
  });

  test("稍后 → 本次不显示，重载后（未读）照常出现", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, announceAvailable: true });
    await page.goto("/");
    await expect(page.getByTestId("announce-banner")).toBeVisible({ timeout: 15_000 });
    await page.getByTestId("announce-banner-later").click();
    await expect(page.getByTestId("announce-banner")).toHaveCount(0);

    await page.reload();
    await expect(page.getByTestId("announce-banner")).toBeVisible({ timeout: 15_000 });
    await assertUiHygiene(page);
  });
});
