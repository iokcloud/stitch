import { expect, test } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

// 失败工具卡：失败自动展开（观察错误），手动折叠后状态写回 store——
// 视图重建（设置↔聊天往返，等价虚拟化卸载/重建）不还原展开。

test.describe("失败工具卡展开状态", () => {
  test("自动展开 → 手动折叠 → 视图重建后保持折叠", async ({ page }) => {
    await mockTauri(page, {
      apiKeySet: true,
      apiTokenSet: true,
      streamChat: true,
      streamFailTool: true,
    });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    await page.getByTestId("chat-input").fill("跑一个失败的命令");
    await page.getByTestId("chat-input").press("Enter");

    // 失败 → 卡片自动展开（原行为，可观察错误）。
    const toolCard = page.getByTestId("tool-status");
    await expect(toolCard).toBeVisible({ timeout: 10_000 });
    await expect(toolCard).toHaveAttribute("data-running", "false");
    await expect(toolCard.locator(".tool-call-main")).toHaveAttribute(
      "aria-expanded",
      "true",
      { timeout: 5_000 },
    );

    // 手动折叠 → 收起（aria-expanded 翻转）。
    await toolCard.locator(".tool-call-main").click();
    await expect(toolCard.locator(".tool-call-main")).toHaveAttribute(
      "aria-expanded",
      "false",
    );

    // 视图重建（设置 ↔ 聊天往返 = ChatView 卸载重建，等价虚拟化重建）——
    // 折叠状态来自 store，不还原展开。
    await page.getByTestId("open-settings").click();
    await expect(page.getByTestId("settings-view")).toBeVisible();
    await page.getByTestId("settings-back-chat").click();
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    const rebuilt = page.getByTestId("tool-status");
    await expect(rebuilt).toHaveCount(1);
    await expect(rebuilt.locator(".tool-call-main")).toHaveAttribute(
      "aria-expanded",
      "false",
    );

    await assertUiHygiene(page);
  });

  test("失败卡展开后重建仍保持展开（写回双向）", async ({ page }) => {
    await mockTauri(page, {
      apiKeySet: true,
      apiTokenSet: true,
      streamChat: true,
      streamFailTool: true,
    });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    await page.getByTestId("chat-input").fill("跑一个失败的命令");
    await page.getByTestId("chat-input").press("Enter");

    const toolCard = page.getByTestId("tool-status");
    await expect(toolCard.locator(".tool-call-main")).toHaveAttribute(
      "aria-expanded",
      "true",
      { timeout: 10_000 },
    );

    // 展开态往返重建 → 仍展开。
    await page.getByTestId("open-settings").click();
    await expect(page.getByTestId("settings-view")).toBeVisible();
    await page.getByTestId("settings-back-chat").click();
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    await expect(page.getByTestId("tool-status").locator(".tool-call-main")).toHaveAttribute(
      "aria-expanded",
      "true",
    );
  });
});
