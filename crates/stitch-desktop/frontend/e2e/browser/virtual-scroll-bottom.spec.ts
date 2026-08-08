import { expect, test, type Page } from "@playwright/test";
import { mockTauri } from "../helpers/mock-tauri";
import { assertUiHygiene } from "../helpers/ui-hygiene";

// 虚拟化滚动可达性：① 底部有高内容块（展开的失败工具卡）时 spacer 估算
// 不低估——否则滚动条到顶也看不到完整内容；② 聊天内部滚动容器（工具卡
// 输出区等）滚到底后必须链到外层聊天（overscroll 不拦截滚轮）。

async function seedLongSession(page: Page, cardIndex: number) {
  await page.addInitScript(({ idx }) => {
    const sessionsKey = "stitch-sessions";
    const sid = "virtual-bottom-seed";
    const messages: unknown[] = [];
    const pushMsg = (i: number) => {
      const user = i % 2 === 0;
      messages.push({
        id: `perf-${i}`,
        type: "message",
        role: user ? "user" : "assistant",
        content: `${user ? "用户" : "助手"}第 ${i} 条：${"这是一段用于长会话渲染性能测试的内容。".repeat(4)}`,
        error: false,
        stopped: false,
      });
    };
    for (let i = 0; i < 44; i++) {
      if (i === idx) {
        messages.push({
          id: "fail-card",
          type: "tool",
          name: "run_command",
          done: true,
          error: true,
          expanded: true,
          summary: "安装失败：网络连接被拒绝（ECONNREFUSED）",
          detail: Array.from(
            { length: 30 },
            (_, j) => `输出行 ${j + 1}: 错误详情 ${"x".repeat(40)}`,
          ).join("\n"),
        });
      }
      pushMsg(i);
    }
    const now = Date.now();
    const store = {
      current: sid,
      sessions: {
        [sid]: {
          id: sid,
          title: "虚拟化底部可达性",
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
  }, { idx: cardIndex });
}

test.describe("虚拟化滚动底部可达性", () => {
  test("底部展开失败卡：滚动到顶后卡片完整可见（spacer 不低估）", async ({ page }) => {
    await seedLongSession(page, 43);
    await mockTauri(page, { apiKeySet: true, apiTokenSet: true, streamChat: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    const failCard = page.getByTestId("tool-status");
    await expect(failCard).toBeVisible({ timeout: 10_000 });

    // 滚到底（多次，模拟用户反复滚动；虚拟化窗口移动会重测）。
    for (let i = 0; i < 12; i++) {
      await page.evaluate(() => {
        const el = document.querySelector(".chat-log") as HTMLElement | null;
        if (el) el.scrollTop = el.scrollHeight;
      });
      await page.waitForTimeout(120);
    }

    const reach = await page.evaluate(() => {
      const el = document.querySelector(".chat-log") as HTMLElement | null;
      if (!el) return null;
      return {
        atMax: el.scrollTop + el.clientHeight >= el.scrollHeight - 2,
        scrollTop: el.scrollTop,
        scrollHeight: el.scrollHeight,
        clientHeight: el.clientHeight,
      };
    });
    expect(reach).not.toBeNull();
    expect(reach!.atMax).toBe(true);

    // 失败卡完整内容在视口内（底部未被 spacer 低估截断）。
    const cardBox = await failCard.boundingBox();
    const viewport = await page.evaluate(() => {
      const el = document.querySelector(".chat-log") as HTMLElement | null;
      return el ? el.getBoundingClientRect().bottom : 0;
    });
    expect(cardBox).not.toBeNull();
    expect(cardBox!.y + cardBox!.height).toBeLessThanOrEqual(viewport + 2);

    // 最后一行输出可见（完整内容没被吃掉）。
    await expect(page.getByText("输出行 30:")).toBeVisible({ timeout: 5_000 });

    await assertUiHygiene(page);
  });

  test("滚轮悬停失败卡输出区：内部滚到底后链到外层聊天（不拦截）", async ({ page }) => {
    await seedLongSession(page, 20);
    await mockTauri(page, { apiKeySet: true, apiTokenSet: true, streamChat: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });

    // 中部卡片初始在视口外（虚拟化未挂载）——先滚到其所在区域。
    await page.evaluate(() => {
      const el = document.querySelector(".chat-log") as HTMLElement | null;
      if (el) el.scrollTop = el.scrollHeight * 0.45;
    });
    await page.waitForTimeout(400);
    const failCard = page.getByTestId("tool-status");
    await expect(failCard).toBeVisible({ timeout: 10_000 });

    // 悬停失败卡输出区 + 滚轮向下：内部滚到底后必须链到外层聊天。
    const shell = failCard.locator(".tool-shell");
    await shell.scrollIntoViewIfNeeded();
    // force：虚拟化重排期间元素不稳定，但只需鼠标位置落在输出区上。
    await shell.hover({ force: true });
    const before = await page.evaluate(() => {
      const el = document.querySelector(".chat-log") as HTMLElement | null;
      return el ? el.scrollTop : -1;
    });
    for (let i = 0; i < 8; i++) {
      await page.mouse.wheel(0, 300);
      await page.waitForTimeout(60);
    }
    const after = await page.evaluate(() => {
      const el = document.querySelector(".chat-log") as HTMLElement | null;
      return el ? el.scrollTop : -1;
    });
    // 外层聊天确实滚动过（修复前 overscroll contain 拦截：8 次滚轮外层
    // 纹丝不动；修复后内部到底链出——scrollHeight 随虚拟化重排变化，
    // 数值方向不定，只要位置变动即链出生效）。
    expect(after).not.toBe(before);

    await assertUiHygiene(page);
  });

  test("滚动中无未捕获异常（measureClamp 卸载竞态已修）", async ({ page }) => {
    await seedLongSession(page, 20);
    const cdp = await page.context().newCDPSession(page);
    await cdp.send("Runtime.enable");
    const exceptions: string[] = [];
    cdp.on("Runtime.exceptionThrown", (ev: unknown) => {
      const d = (ev as { exceptionDetails?: { text?: string } }).exceptionDetails;
      if (d?.text) exceptions.push(d.text);
    });
    await mockTauri(page, { apiKeySet: true, apiTokenSet: true, streamChat: true });
    await page.goto("/");
    await expect(page.getByTestId("chat-view")).toBeVisible({ timeout: 15_000 });
    // 滚到中部让失败卡挂载（虚拟化窗口移动触发测量/卸载循环）。
    await page.evaluate(() => {
      const el = document.querySelector(".chat-log") as HTMLElement | null;
      if (el) el.scrollTop = el.scrollHeight * 0.45;
    });
    await page.waitForTimeout(400);
    await expect(page.getByTestId("tool-status")).toBeVisible({ timeout: 10_000 });

    // 高频滚动 + 展开/折叠长文（触发 measureClamp 重跑）——卸载竞态窗口。
    for (let r = 0; r < 3; r++) {
      for (let i = 0; i < 20; i++) {
        await page.evaluate(({ odd }) => {
          const el = document.querySelector(".chat-log") as HTMLElement | null;
          if (el) el.scrollTop = odd ? 0 : el.scrollHeight;
        }, { odd: i % 2 === 1 });
        await page.waitForTimeout(60);
      }
    }
    await page.waitForTimeout(500);
    expect(exceptions).toEqual([]);
  });
});
