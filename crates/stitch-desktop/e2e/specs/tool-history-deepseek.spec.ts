/**
 * Real-exe regression: screenshot bug (list_directory \\?\ path + DeepSeek tool 400).
 * Two-turn chat against debug stitch-desktop.exe + live LLM.
 * Artifacts → e2e/artifacts/tool-history-deepseek/
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { findVisibleUiLeak } from "../helpers/ui-hygiene";
import {
  assertNoGlyphIcons,
  bootChat,
  chatStats,
  newSession,
  sendChat,
  setPlanMode,
  setWorkDir,
  shot,
  uiSnapshot,
} from "../helpers/chat-desktop";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.resolve(__dirname, "../artifacts/tool-history-deepseek");
const repoRoot = path.resolve(__dirname, "../../../../../");

const lines: string[] = [];
function note(s: string) {
  lines.push(s);
}

async function toolDump() {
  return browser.execute(() => {
    const tools = [...document.querySelectorAll('[data-testid="tool-status"]')];
    return tools.map((el) => {
      const name = el.getAttribute("data-tool") || "";
      const headline =
        el.querySelector(".tool-call-headline")?.textContent?.replace(/\s+/g, " ").trim() || "";
      const sub =
        el.querySelector(".tool-call-sub")?.textContent?.replace(/\s+/g, " ").trim() || "";
      const detail =
        el.querySelector(".tool-call-detail, .tool-listing")?.textContent?.replace(/\s+/g, " ").trim() ||
        "";
      const err = el.classList.contains("is-error");
      return { name, headline: headline.slice(0, 200), sub: sub.slice(0, 120), detail: detail.slice(0, 400), err };
    });
  });
}

async function chatHasApi400(): Promise<string | null> {
  return browser.execute(() => {
    const log = document.querySelector('[role="log"]')?.textContent || "";
    if (/API error 400/i.test(log)) {
      const m = log.match(/API error 400[\s\S]{0,240}/i);
      return m?.[0]?.replace(/\s+/g, " ").trim() ?? "API error 400";
    }
    if (/tool_calls/i.test(log) && /role ['"]tool['"]/i.test(log)) {
      return "tool/tool_calls pairing error in chat";
    }
    const errBubbles = [...document.querySelectorAll(".message.is-error, .msg-error, [data-error='true']")];
    for (const b of errBubbles) {
      const t = (b.textContent || "").replace(/\s+/g, " ").trim();
      if (/400|tool_calls/i.test(t)) return t.slice(0, 280);
    }
    // Error assistant bubbles often use .message-error or red border without those classes
    for (const b of document.querySelectorAll(".msg-assistant, .message")) {
      const t = (b.textContent || "").replace(/\s+/g, " ").trim();
      if (/API error 400/i.test(t)) return t.slice(0, 280);
    }
    return null;
  });
}

describe("Stitch tool history DeepSeek (debug exe)", () => {
  before(async () => {
    fs.rmSync(outDir, { recursive: true, force: true });
    fs.mkdirSync(outDir, { recursive: true });
    await bootChat();
  });

  after(() => {
    fs.writeFileSync(
      path.join(outDir, "REPORT.txt"),
      ["Stitch tool-history DeepSeek (debug exe)", ...lines, `artifacts: ${outDir}`].join("\n"),
      "utf8",
    );
  });

  it("screenshot prompts: list workdir then 审查本次改动 (no tool 400)", async () => {
    await setPlanMode(false);
    await newSession();

    const set = await setWorkDir(repoRoot);
    note(`workdir applied=${set.applied}`);
    expect(set.applied.toLowerCase()).toMatch(/promptstdio/);

    // Exact user wording from the bug screenshots (turn 1)
    await sendChat("先列出当前工作目录的顶层结构", 240_000);
    await shot(outDir, "01-list-top");

    const err1 = await chatHasApi400();
    expect(err1).toBeNull();

    const tools1 = await toolDump();
    note(`turn1 tools=${JSON.stringify(tools1)}`);
    const listTools = tools1.filter((t) => t.name === "list_directory");
    expect(listTools.length).toBeGreaterThan(0);
    for (const t of listTools) {
      const blob = `${t.headline}\n${t.sub}\n${t.detail}`;
      expect(blob).not.toMatch(/\\\\\?\\/);
      expect(blob).not.toMatch(/\/\/\?\//);
    }
    const snap1 = await uiSnapshot();
    expect(`${snap1.lastAssistant}\n${JSON.stringify(tools1)}`.length).toBeGreaterThan(20);
    note("turn1 先列出当前工作目录的顶层结构: PASS");

    // Exact user wording from the bug screenshots (turn 2)
    await sendChat("审查本次改动", 420_000);
    await shot(outDir, "02-审查本次改动");

    const err2 = await chatHasApi400();
    if (err2) {
      note(`FAIL turn2: ${err2}`);
      await shot(outDir, "03-error");
    }
    expect(err2).toBeNull();

    const tools2 = await toolDump();
    note(`turn2 tools=${JSON.stringify(tools2.map((t) => t.name))}`);
    expect(tools2.length).toBeGreaterThan(0);

    const stats = await chatStats();
    expect(stats.lastAssistant.length).toBeGreaterThan(40);
    expect(stats.hasStopped).toBe(false);
    await assertNoGlyphIcons();
    expect(await findVisibleUiLeak()).toBeNull();
    note("turn2 审查本次改动: PASS (no API 400)");
  });
});
