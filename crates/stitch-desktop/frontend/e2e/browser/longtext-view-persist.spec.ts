import { expect, test, type Page } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

// 长文展开状态跨视图保持：展开长文 → 切设置↔聊天（ChatView 卸载重建）
// → 展开保持（与失败卡/工具组同模式；重启即折叠仍是有意为之）。

async function seedLongTextSession(page: Page) {
  await page.addInitScript(() => {
    const sessionsKey = "stitch-sessions";
    const sid = "longtext-persist-seed";
    const longBody = Array.from(
      { length: 40 },
      (_, i) =>
        `段落 ${i + 1}：这是用于测试长内容折叠的句子，重复填充以保证超过折叠阈值。`,
    ).join("\n\n");
    const messages: unknown[] = [];
    for (let i = 0; i < 30; i++) {
      messages.push({ id: `u${i}`, type: "message", role: "user", content: `问题 ${i}` });
      messages.push({
        id: `a${i}`,
        type: "message",
        role: "assistant",
        content: i === 0 ? longBody : `回答 ${i}：普通内容。`,
        error: false,
        stopped: false,
      });
    }
    const now = Date.now();
    const store = {
      current: sid,
      sessions: {
        [sid]: {
          id: sid,
          title: "长文跨视图保持",
          createdAt: now - 60_000,
          updatedAt: now,
          workDirPath: null,
          llmProfileId: null,
          llmModel: null,
          messages,
          sedimentCandidate: null,
        },
      },
    };
    try {
      localStorage.setItem(sessionsKey, JSON.stringify(store));
    } catch {
      /* ignore */
    }
  });
}

test.describe("长文展开跨视图保持", () => {
  test("展开长文 → 设置↔聊天往返后仍展开；收起同样保持", async ({ page }) => {
    await seedLongTextSession(page);
    await mockTauri(page, { apiKeySet: true, apiTokenSet: true, streamChat: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    // 滚到第一条长文（会话顶部）。
    await page.evaluate(() => {
      const el = document.querySelector(".chat-log") as HTMLElement | null;
      if (el) el.scrollTop = 0;
    });
    await page.waitForTimeout(300);

    // 默认折叠（message-clamp 生效）。
    const longBubble = page.locator('[data-block-key="a0"] .message-clamp');
    await expect(longBubble).toBeVisible({ timeout: 10_000 });

    // 展开全文（用 testid——getByRole name 子串会误匹配「收起侧栏」）。
    await page.getByTestId("message-expand").first().click();
    await expect(longBubble).toHaveCount(0);

    // 设置 ↔ 聊天往返 → 展开保持。
    await page.getByTestId("open-settings").click();
    await expect(page.getByTestId("settings-view")).toBeVisible();
    await page.getByTestId("settings-back-chat").click();
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    // 重建后视口锚定底部（stickToBottom 初始 true）——滚回顶部让 a0 挂载。
    await page.evaluate(() => {
      const el = document.querySelector(".chat-log") as HTMLElement | null;
      if (el) el.scrollTop = 0;
    });
    await page.waitForTimeout(300);
    await expect(page.locator('[data-block-key="a0"] .message-clamp')).toHaveCount(0);
    // 按钮仍在（needsClamp 保持 true，label=收起——真展开态）。
    await expect(page.getByTestId("message-expand").first()).toHaveAttribute("aria-label", "收起");

    // 收起 → 往返后仍收起（双向写回）。
    await page.getByTestId("message-expand").first().click();
    await expect(page.locator('[data-block-key="a0"] .message-clamp')).toBeVisible();
    await page.getByTestId("open-settings").click();
    await expect(page.getByTestId("settings-view")).toBeVisible();
    await page.getByTestId("settings-back-chat").click();
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await page.evaluate(() => {
      const el = document.querySelector(".chat-log") as HTMLElement | null;
      if (el) el.scrollTop = 0;
    });
    await page.waitForTimeout(300);
    await expect(page.locator('[data-block-key="a0"] .message-clamp')).toBeVisible();

    await assertUiHygiene(page);
  });
});
