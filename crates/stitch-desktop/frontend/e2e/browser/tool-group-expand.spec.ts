import { expect, test, type Page } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

// 工具组展开状态保持：虚拟化重建（设置↔聊天往返等价卸载重建）后
// 已展开的「已执行 N 步」组不还原收起（与失败卡 expanded 同模式）。

async function seedLongSessionWithGroup(page: Page) {
  await page.addInitScript(() => {
    const sessionsKey = "stitch-sessions";
    const sid = "group-expand-seed";
    const messages: unknown[] = Array.from({ length: 44 }, (_, i) => {
      const user = i % 2 === 0;
      return {
        id: `perf-${i}`,
        type: "message",
        role: user ? "user" : "assistant",
        content: `${user ? "用户" : "助手"}第 ${i} 条：${"这是一段用于长会话渲染性能测试的内容。".repeat(4)}`,
        error: false,
        stopped: false,
      };
    });
    // 末尾两个连续「可折叠进程工具」→ 渲染为工具组（已执行 2 步）。
    // 注意：write/delete 类不折叠（保持单卡），须用白名单工具。
    messages.push(
      {
        id: "grp-t1",
        type: "tool",
        name: "read_file",
        done: true,
        error: false,
        expanded: false,
        summary: "3 lines",
        detail: "line1\nline2\nline3",
      },
      {
        id: "grp-t2",
        type: "tool",
        name: "run_command",
        done: true,
        error: false,
        expanded: false,
        summary: "完成：echo ok",
        detail: "ok",
      },
    );
    const now = Date.now();
    const store = {
      current: sid,
      sessions: {
        [sid]: {
          id: sid,
          title: "工具组展开保持",
          createdAt: now - 44 * 1000,
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

test.describe("工具组展开状态保持", () => {
  test("展开组 → 视图重建后仍展开（虚拟化不还原收起）", async ({ page }) => {
    await seedLongSessionWithGroup(page);
    await mockTauri(page, { apiKeySet: true, apiTokenSet: true, streamChat: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    // 组在末尾（stickToBottom 初始 true → 挂载可见），默认收起。
    const group = page.getByTestId("tool-group");
    await expect(group).toBeVisible({ timeout: 10_000 });
    await expect(group.locator(".tool-group-head")).toHaveAttribute("aria-expanded", "false");
    await expect(page.getByTestId("tool-group-body")).toHaveCount(0);

    // 展开组 → 组内工具可见。
    await group.locator(".tool-group-head").click();
    await expect(group.locator(".tool-group-head")).toHaveAttribute("aria-expanded", "true");
    await expect(page.getByTestId("tool-group-body")).toBeVisible();
    await expect(page.getByTestId("tool-status")).toHaveCount(2);

    // 视图重建（设置 ↔ 聊天往返）→ 组仍展开。
    await page.getByTestId("open-settings").click();
    await expect(page.getByTestId("settings-view")).toBeVisible();
    await page.getByTestId("settings-back-chat").click();
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    await expect(page.getByTestId("tool-group")).toHaveCount(1);
    await expect(page.getByTestId("tool-group").locator(".tool-group-head")).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    await expect(page.getByTestId("tool-group-body")).toBeVisible();

    // 收起 → 重建后仍收起（双向写回）。
    await page.getByTestId("tool-group").locator(".tool-group-head").click();
    await expect(page.getByTestId("tool-group-body")).toHaveCount(0);
    await page.getByTestId("open-settings").click();
    await expect(page.getByTestId("settings-view")).toBeVisible();
    await page.getByTestId("settings-back-chat").click();
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    await expect(page.getByTestId("tool-group-body")).toHaveCount(0);

    await assertUiHygiene(page);
  });
});
