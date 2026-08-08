import { expect, test } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

// 窗口标题跟随会话名：发送消息后会话有标题 → set_window_title(标题)；
// 「新会话」/空标题 → 还原默认（空字符串）。

async function lastTitle(page: import("@playwright/test").Page): Promise<string> {
  return page.evaluate(() => {
    const i = (window as unknown as { __TAURI_INTERNALS__?: Record<string, unknown> })
      .__TAURI_INTERNALS__;
    return typeof i?.lastWindowTitle === "string" ? i.lastWindowTitle : "";
  });
}

test.describe("窗口标题跟随会话", () => {
  test("新会话 → 空标题（默认）；发送后 → 会话标题", async ({ page }) => {
    await mockTauri(page, { apiKeySet: true, streamChat: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    // 新会话（无标题）→ 还原默认。
    await expect.poll(() => lastTitle(page)).toBe("");

    // 发送消息 → 会话自动命名（summarizeSessionTitle）→ 窗口标题同步。
    await page.getByTestId("chat-input").fill("列出当前工作目录的顶层结构");
    await page.getByTestId("chat-input").press("Enter");
    await expect.poll(() => lastTitle(page), { timeout: 10_000 }).not.toBe("");
    const title = await lastTitle(page);
    expect(title).not.toBe("新会话");

    await assertUiHygiene(page);
  });
});
