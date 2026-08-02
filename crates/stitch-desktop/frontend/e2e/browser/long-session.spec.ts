import { expect, test, type Page } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

// Long-session rendering: virtualization kicks in for large timelines,
// keeps DOM bounded, and the composer / scroll flows still work.

async function seedLongSession(page: Page, count: number) {
  await page.addInitScript((n) => {
    const sessionsKey = "stitch-sessions";
    const sid = "long-perf-seed";
    const messages = Array.from({ length: n }, (_, i) => {
      const user = i % 2 === 0;
      return {
        id: `perf-${i}`,
        type: "message",
        role: user ? "user" : "assistant",
        content: `${user ? "用户" : "助手"}第 ${i} 条：${"这是一段用于长会话渲染性能测试的内容。".repeat(user ? 3 : 8)}`,
        error: false,
        stopped: false,
      };
    });
    const now = Date.now();
    const store = {
      current: sid,
      sessions: {
        [sid]: {
          id: sid,
          title: "长会话性能测试",
          createdAt: now - n * 1000,
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
  }, count);
}

async function boot(page: Page) {
  await mockTauri(page, { apiKeySet: true, apiTokenSet: true, streamChat: true });
  await page.goto("/");
  await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
}

test.describe("long-session rendering", () => {
  test("virtualizes a 200-turn timeline and keeps the DOM bounded", async ({ page }) => {
    await seedLongSession(page, 200);
    await boot(page);

    // With virtualization, only a window of blocks should be mounted.
    const mounted = await page.locator("[data-block-key]").count();
    expect(mounted).toBeGreaterThan(10);
    expect(mounted).toBeLessThan(120);

    // Jump straight to the tail — virtualization should follow and mount it.
    const chatLog = page.locator(".chat-log");
    await chatLog.evaluate((el) => {
      el.scrollTop = el.scrollHeight;
    });
    // Wait for the tail block to mount before asserting visibility.
    await page.waitForTimeout(500);
    await expect(
      page.getByTestId("chat-view").getByText("助手第 199 条", { exact: false }),
    ).toBeVisible({ timeout: 5_000 });

    // Composer still works: send a new message (mock stream).
    await page.getByTestId("chat-input").fill("收尾消息");
    await page.getByTestId("chat-input").press("Enter");
    await expect(
      page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" }),
    ).toBeVisible({ timeout: 10_000 });
    await expect(page.getByTestId("chat-send")).toHaveAttribute("aria-label", "发送", {
      timeout: 5_000,
    });
    await assertUiHygiene(page);
  });

  test("scroll up renders earlier blocks via spacers", async ({ page }) => {
    await seedLongSession(page, 200);
    await boot(page);

    const chatLog = page.locator(".chat-log");
    // Jump to the very top — earliest user turn must mount.
    await chatLog.evaluate((el) => {
      el.scrollTop = 0;
    });
    await expect(
      page.getByTestId("chat-view").getByText("用户第 0 条", { exact: false }),
    ).toBeVisible({ timeout: 3_000 });

    // And the tail bubble unmounts while scrolled to the top.
    await expect(
      page.getByTestId("chat-view").getByText("助手第 199 条", { exact: false }),
    ).toHaveCount(0);

    // 回到底部 pill restores the tail.
    await page.getByTestId("scroll-bottom").click();
    await page.waitForTimeout(500);
    await page.locator(".chat-log").evaluate((el) => {
      el.scrollTop = el.scrollHeight;
    });
    await page.waitForTimeout(300);
    await expect(
      page.getByTestId("chat-view").getByText("助手第 199 条", { exact: false }),
    ).toBeVisible({ timeout: 5_000 });
    await assertUiHygiene(page);
  });

  test("short sessions do not virtualize (no spacers)", async ({ page }) => {
    await boot(page);
    await page.getByTestId("chat-input").fill("你好");
    await page.getByTestId("chat-input").press("Enter");
    await expect(
      page.locator(".msg-assistant .md-content").filter({ hasText: "流式回复完成" }),
    ).toBeVisible({ timeout: 10_000 });
    await expect(page.getByTestId("chat-send")).toHaveAttribute("aria-label", "发送", {
      timeout: 5_000,
    });

    // Tiny session: no spacer divs, everything mounted.
    const spacers = await page.evaluate(
      () => document.querySelectorAll(".chat-log [style*='height:']").length,
    );
    expect(spacers).toBe(0);
    await assertUiHygiene(page);
  });
});
