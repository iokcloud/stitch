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

/** 长会话 + 第一条 assistant 消息为长文（>900 字符触发折叠）。 */
async function seedExpandSession(page: Page) {
  await page.addInitScript(() => {
    const sessionsKey = "stitch-sessions";
    const sid = "expand-seed";
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
          title: "展开跨虚拟化",
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

  test("expanded long message keeps state and clamp affordance across virtualization", async ({
    page,
  }) => {
    await seedExpandSession(page);
    await boot(page);

    const chatLog = page.locator(".chat-log");
    const expand = (label: string) =>
      page.locator(`[data-testid="message-expand"][aria-label="${label}"]`);

    // 回顶部固定视口（长会话打开时钉在底部），展开长文
    await chatLog.evaluate((el) => {
      el.scrollTop = 0;
    });
    await expect(expand("展开全文").first()).toBeVisible({ timeout: 5_000 });
    await expand("展开全文").first().click();
    await expect(expand("收起")).toHaveCount(1);

    // 展开块绝对几何（相对滚动容器；scrollTop=0 后先固定，防滚动锚定漂移）
    await chatLog.evaluate((el) => {
      el.scrollTop = 0;
    });
    await page.waitForTimeout(300);
    const absBottom = await page.evaluate(() => {
      const log = document.querySelector(".chat-log");
      const btn = [...document.querySelectorAll('[data-testid="message-expand"]')].find(
        (b) => b.getAttribute("aria-label") === "收起",
      );
      const block = btn?.closest(".message") as HTMLElement | null;
      if (!log || !block) return -1;
      return (
        block.getBoundingClientRect().bottom - log.getBoundingClientRect().top
      );
    });
    // 长文确实展开了（折叠阈值 320px，展开后应有 ~1000px+）
    expect(absBottom).toBeGreaterThan(500);

    // 小步下滚：展开块的真实高度必须被测量——块整块离开视口前不允许
    // 被虚拟化卸载（估算 ~58px/块时 ~1000px 处就提前卸载 → 按钮消失）
    // 注意：滚动会改变 spacer/总高（顶部块挂载后 scrollHeight 变大），
    // 上限必须每步动态取——用滚动前测的 maxScroll 会提前终止循环。
    let dropAt = -1;
    for (let st = 300; dropAt < 0 && st < 8000; st += 300) {
      await chatLog.evaluate((el, v) => {
        el.scrollTop = v;
      }, st);
      await page.waitForTimeout(60);
      const has = await expand("收起").count();
      const maxScroll = await chatLog.evaluate((el) => el.scrollHeight - el.clientHeight);
      if (has === 0) dropAt = st;
      if (st >= maxScroll) break;
    }
    expect(dropAt).toBeGreaterThan(absBottom + 500);

    // 滚到底再回顶：状态与折叠控件跨虚拟化卸载/重建保持（回归 5f0f5d6）
    await chatLog.evaluate((el) => {
      el.scrollTop = el.scrollHeight;
    });
    await page.waitForTimeout(500);
    await chatLog.evaluate((el) => {
      el.scrollTop = 0;
    });
    await page.waitForTimeout(500);
    await expect(expand("收起")).toHaveCount(1);

    // 收起可往返
    await expand("收起").click();
    await expect(expand("展开全文")).toHaveCount(1);
    await assertUiHygiene(page);
  });

  test("regenerate while expanded keeps view anchored and restores the clamp affordance", async ({
    page,
  }) => {
    // 长文在最后一条（regenerate 只在最后一条 assistant 上出现）。
    await page.addInitScript(() => {
      const sessionsKey = "stitch-sessions";
      const sid = "expand-regenerate-seed";
      const longBody = Array.from(
        { length: 40 },
        (_, i) =>
          `重生成段落 ${i + 1}：这是用于测试长内容折叠的句子，重复填充以保证超过折叠阈值。`,
      ).join("\n\n");
      const messages: unknown[] = [];
      for (let i = 0; i < 30; i++) {
        messages.push({ id: `u${i}`, type: "message", role: "user", content: `问题 ${i}` });
        messages.push({
          id: `a${i}`,
          type: "message",
          role: "assistant",
          content: i === 29 ? longBody : `回答 ${i}：普通内容。`,
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
            title: "展开重生成",
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
      localStorage.setItem(sessionsKey, JSON.stringify(store));
    });
    const longReply = Array.from(
      { length: 40 },
      (_, i) =>
        `重生成回复 ${i + 1}：这是用于测试长内容折叠的句子，重复填充以保证超过折叠阈值。`,
    ).join("\n\n");
    await mockTauri(page, {
      apiKeySet: true,
      apiTokenSet: true,
      streamChat: true,
      streamDoneReply: longReply,
    });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    const chatLog = page.locator(".chat-log");
    const expand = (label: string) =>
      page.locator(`[data-testid="message-expand"][aria-label="${label}"]`);

    // 会话打开钉在底部 → 最后一条（长文）可见；展开
    await expect(expand("展开全文").first()).toBeVisible({ timeout: 5_000 });
    await expand("展开全文").first().click();
    await expect(expand("收起")).toHaveCount(1);
    // 展开的块必须在 DOM 中（未被虚拟化重排裁出——锚定逻辑兜底）
    await expect(page.locator('[data-block-key="a29"]')).toHaveCount(1);

    // 展开中重新生成：重生成=删尾重建（removeItemsAfter+重发）——新消息新 id，
    // 从折叠开始——折叠控件必须正常出现（不卡死），长文开头可见
    await page.getByTestId("msg-regenerate").click();
    await expect(
      page.locator(".msg-assistant .md-content").filter({ hasText: "重生成回复 1" }),
    ).toBeVisible({ timeout: 10_000 });
    await expect(expand("展开全文").first()).toBeVisible();
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
