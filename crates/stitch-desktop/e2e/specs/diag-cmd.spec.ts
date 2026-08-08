/** 诊断：run_command 失败 + 终端展示区 */
import { bootChat, setWorkDir, fillChat, clickSend } from "../helpers/chat-desktop";

describe("diag cmd", () => {
  it("sends a command and dumps tool state", async () => {
    await bootChat();
    await setWorkDir("C:\Users\ava_t\Desktop\promptstdio");
    await fillChat("运行命令 dir 看当前目录");
    await clickSend();
    // 等回合结束（最多 90s）
    for (let i = 0; i < 18; i++) {
      await browser.pause(5000);
      const state = await browser.execute(() => {
        const tools = [...document.querySelectorAll('[data-testid="tool-status"]')].map((el) => ({
          tool: el.getAttribute("data-tool"),
          running: el.getAttribute("data-running"),
          text: (el.textContent || "").replace(/\s+/g, " ").slice(0, 200),
        }));
        const stop = !!document.querySelector('button[aria-label="停止生成"]');
        const errs = [...document.querySelectorAll('[role="alert"], .toast, [data-testid*="error"]')]
          .map((e) => (e.textContent || "").replace(/\s+/g, " ").slice(0, 120));
        // 终端区是否存在（消息上方）
        const term = !!document.querySelector('[data-testid="terminal-panel"], .terminal, [class*="term-"]');
        return { streaming: stop, tools, errs, term };
      });
      console.log(`[CMD ${i * 5}s]`, JSON.stringify(state));
      if (!state.streaming) break;
    }
  });
});
