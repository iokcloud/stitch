/** Debug probe: where does a simple tool turn hang? */
import { bootChat, newSession, setWorkDir, fillChat, clickSend } from "../helpers/chat-desktop";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "../../../../../");

describe("debug stream hang", () => {
  it("dump state every 5s during a list-directory turn", async () => {
    await bootChat();
    await newSession();
    await setWorkDir(repoRoot);
    await fillChat("先列出当前工作目录的顶层结构");
    await clickSend();

    for (let i = 0; i < 24; i++) {
      await browser.pause(5000);
      const state = await browser.execute(() => {
        const stop = document.querySelector('button[aria-label="停止生成"]');
        const send = document.querySelector('button[aria-label="发送"]');
        const tools = [...document.querySelectorAll('[data-testid="tool-status"]')].map(
          (el) => ({
            tool: el.getAttribute("data-tool"),
            running: el.getAttribute("data-running"),
            text: (el.textContent || "").replace(/\s+/g, " ").slice(0, 120),
          }),
        );
        const confirm = document.querySelector('[data-testid="confirm-allow"]');
        const log = document.querySelector('[role="log"]')?.textContent || "";
        return {
          streaming: !!stop,
          sendVisible: !!send,
          confirmOpen: !!confirm,
          tools,
          logTail: log.slice(-300),
        };
      });
      console.log(`[PROBE ${i * 5}s]`, JSON.stringify(state));
      if (!state.streaming) {
        console.log("[PROBE] generation finished");
        break;
      }
    }
  });
});
