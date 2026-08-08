/**
 * 真机滚动链验收（滚动修复 339c3cd）：
 * ① 失败工具卡输出区滚轮滚到底后链到外层聊天（overscroll 不再拦截）；
 * ② 外层可滚到真实底部（底部内容完整可见）。
 * 真实模型驱动：新会话 + 发一个必然失败的命令 → 失败卡自动展开（红色框）。
 */
import { ensureChat, chatStats, shot } from "../helpers/chat-desktop";

const OUT = "artifacts/scroll-chain-real";

/** 循环批准确认卡（run_command 等工具请求批准；勾记住规则减少打断）。 */
async function autoApproveUntil(until: () => Promise<boolean>, timeoutMs: number) {
  const deadline = Date.now() + timeoutMs;
  while (Date.now() < deadline) {
    if (await until()) return;
    const allow = await $('[data-testid="confirm-allow"]');
    if (await allow.isExisting()) {
      const remember = await $('[data-testid="confirm-remember"]');
      if (await remember.isExisting()) {
        const checked = await remember.isSelected();
        if (!checked) await remember.click();
      }
      await allow.click();
    }
    await browser.pause(500);
  }
  throw new Error("timeout waiting for condition");
}

async function sendAndWaitFailCard(prompt: string) {
  const input = await $('[data-testid="chat-input"]');
  await input.setValue(prompt);
  await $('[data-testid="chat-send"]').click();

  const card = await $('[data-testid="tool-status"].is-error');
  await autoApproveUntil(
    async () => await card.isExisting(),
    120_000,
  );
  await browser.waitUntil(
    async () => (await card.getAttribute("data-running")) === "false",
    { timeout: 30_000, timeoutMsg: "tool never finished" },
  );
  // 失败卡自动展开（可观察错误）——展开态高内容块。
  await browser.waitUntil(
    async () =>
      (await $('[data-testid="tool-status"].is-error .tool-call-main').getAttribute("aria-expanded")) ===
      "true",
    { timeout: 10_000, timeoutMsg: "fail card not auto-expanded" },
  );
  return card;
}

describe("Stitch 真机滚动链（滚动修复验收）", () => {
  it("失败卡输出区滚轮链出外层 + 底部可达", async () => {
    await ensureChat();
    await chatStats(); // 等待聊天就绪（含历史加载）

    // 开新会话，避免旧内容干扰断言。
    const newSession = await $('[data-testid="session-new"]');
    if (await newSession.isExisting()) {
      await newSession.click();
      await browser.pause(600);
    }

    const card = await sendAndWaitFailCard(
      "运行命令 `nonexistent-cmd-zzz-123` 并报告结果（这个命令必然失败，直接运行即可）",
    );
    await shot(OUT, "01-fail-card-expanded");

    // 外层滚到中段（可自由滚动区域）。
    await browser.execute(() => {
      const log = document.querySelector('[role="log"]') as HTMLElement;
      if (log) log.scrollTop = log.scrollHeight * 0.5;
    });
    await browser.pause(300);

    // 悬停失败卡输出区 + 滚轮向下多次。
    const shell = await $('[data-testid="tool-status"].is-error .tool-shell-body');
    await shell.waitForExist({ timeout: 15_000 });
    await browser.action("pointer").move({ origin: shell }).perform();
    const before = await browser.execute(() => {
      const log = document.querySelector('[role="log"]') as HTMLElement;
      return log ? log.scrollTop : -1;
    });
    for (let i = 0; i < 10; i++) {
      await browser.action("wheel").scroll({ deltaY: 400 }).perform();
    }
    const after = await browser.execute(() => {
      const log = document.querySelector('[role="log"]') as HTMLElement;
      return log ? log.scrollTop : -1;
    });

    // ① 链出生效：外层滚动位置变化（修复前 contain 拦截：纹丝不动）。
    expect(after).not.toBe(before);

    // ② 底部可达：滚到底后视口在真实底部。
    await browser.execute(() => {
      const log = document.querySelector('[role="log"]') as HTMLElement;
      if (log) log.scrollTop = log.scrollHeight;
    });
    await browser.pause(400);
    const reach = await browser.execute(() => {
      const log = document.querySelector('[role="log"]') as HTMLElement;
      return log ? log.scrollTop + log.clientHeight >= log.scrollHeight - 4 : false;
    });
    expect(reach).toBe(true);
    await shot(OUT, "02-bottom-reached");
  });
});
