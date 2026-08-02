/**
 * Real-exe chat core paths that Layer A mocks cannot prove:
 * stop mid-stream · plan approve+execute · switch session mid-stream.
 * Screenshots → e2e/artifacts/chat-core-human/
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { findVisibleUiLeak } from "../helpers/ui-hygiene";
import {
  activeSessionId,
  bootChat,
  chatStats,
  clickSend,
  fillChat,
  newSession,
  planPhase,
  sendChat,
  setPlanMode,
  shot,
  switchToSession,
  waitIdle,
} from "../helpers/chat-desktop";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.resolve(__dirname, "../artifacts/chat-core-human");

describe("Stitch chat core human", () => {
  before(async () => {
    fs.rmSync(outDir, { recursive: true, force: true });
    fs.mkdirSync(outDir, { recursive: true });
    await bootChat();
  });

  it("stops mid-generation and unlocks composer", async () => {
    await setPlanMode(false);
    await newSession();
    await shot(outDir, "01-stop-ready");

    await fillChat(
      "请从 1 连续写到 300，每个数字单独一行，不要省略，不要总结，写完为止。",
    );
    await clickSend();
    // Stop ASAP — cancel must work even before the first token arrives.
    await browser.pause(400);
    await shot(outDir, "02-stop-streaming");
    const stopBtn = await $('[data-testid="chat-send"][aria-label="停止生成"]');
    if (await stopBtn.isExisting()) {
      await stopBtn.click();
    }
    await waitIdle(60_000);
    await shot(outDir, "03-stop-done");

    const after = await chatStats();
    expect(after.hasStopped).toBe(true);
    expect(await $('button[aria-label="发送"]').isExisting()).toBe(true);
    expect(await findVisibleUiLeak()).toBeNull();
  });

  it("plan mode: propose → approve → execute → done", async () => {
    await newSession();
    await setPlanMode(true);
    await shot(outDir, "04-plan-ready");

    await fillChat(
      "请用计划模式完成：1) 列出当前工作目录的文件名 2) 用一句话总结目录内容。不要写文件、不要改文件、不要执行 shell 命令。",
    );
    await clickSend();

    await $('[data-testid="plan-card"]').waitForExist({
      timeout: 120_000,
      timeoutMsg: "plan card did not appear",
    });
    await $('[data-testid="plan-approve"]').waitForExist({ timeout: 10_000 });
    expect(await planPhase()).toMatch(/待批准/);
    await shot(outDir, "05-plan-proposed");

    await $('[data-testid="plan-approve"]').click();
    await browser.waitUntil(async () => (await planPhase()).includes("已批准"), {
      timeout: 30_000,
      timeoutMsg: "plan did not become approved",
    });
    await shot(outDir, "06-plan-approved");

    await waitIdle(240_000);
    await shot(outDir, "07-plan-done");

    const after = await chatStats();
    expect(await planPhase()).toMatch(/已批准/);
    expect(after.assistantCount + after.userCount).toBeGreaterThanOrEqual(1);
    // Execution should leave some assistant or tool summary text.
    expect(after.logText.length).toBeGreaterThan(10);
    expect(await findVisibleUiLeak()).toBeNull();

    await setPlanMode(false);
  });

  it("switching session mid-stream cancels and does not leak into the other session", async () => {
    await setPlanMode(false);

    await newSession();
    await sendChat("会话甲标记：alpha-marker-91。只回复：甲就绪。");
    const sessionA = await activeSessionId();
    expect(sessionA).toBeTruthy();

    await newSession();
    await sendChat("会话乙标记：beta-marker-92。只回复：乙就绪。");
    const sessionB = await activeSessionId();
    expect(sessionB).toBeTruthy();
    expect(sessionB).not.toBe(sessionA);

    await switchToSession(sessionA!);
    await shot(outDir, "08-switch-on-a");

    const marker = `stream-switch-probe-${Date.now()}`;
    await fillChat(
      `请从 1 连续写到 400，每个数字单独一行。本段唯一标记：${marker}。不要省略。`,
    );
    await clickSend();
    await browser.waitUntil(
      async () => {
        const s = await chatStats();
        return s.assistantCount >= 1 || s.logText.includes("1");
      },
      { timeout: 45_000, timeoutMsg: "stream on A never started", interval: 300 },
    );
    await shot(outDir, "09-switch-streaming-a");

    await switchToSession(sessionB!);
    await waitIdle(60_000);
    await shot(outDir, "10-switch-on-b");

    const onB = await chatStats();
    expect(onB.logText).toContain("beta-marker-92");
    expect(onB.logText).not.toContain(marker);
    expect(await $('button[aria-label="发送"]').isExisting()).toBe(true);

    await switchToSession(sessionA!);
    await browser.pause(300);
    await shot(outDir, "11-switch-back-a");

    const onA = await chatStats();
    expect(onA.logText).toContain("alpha-marker-91");
    expect(onA.logText).toContain(marker);
    expect(onA.hasStopped).toBe(true);
    expect(await findVisibleUiLeak()).toBeNull();

    const report = [
      "Stitch chat core human — PASS",
      "stop: 已停止生成 + composer unlocked",
      "plan: propose → approve → execute → idle",
      "switch: mid-stream cancel; marker isolated to session A",
      `artifacts: ${outDir}`,
    ].join("\n");
    fs.writeFileSync(path.join(outDir, "REPORT.txt"), report, "utf8");
  });
});
