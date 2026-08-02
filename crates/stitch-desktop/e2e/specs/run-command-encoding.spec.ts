/**
 * Real-exe: run_command must decode Windows GBK stderr (no U+FFFD mojibake).
 * Plants a script that writes GBK bytes to stderr regardless of PYTHONUTF8.
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { findVisibleUiLeak } from "../helpers/ui-hygiene";
import {
  bootChat,
  chatStats,
  fillChat,
  clickSend,
  newSession,
  setPlanMode,
  setWorkDir,
  shot,
  waitIdle,
} from "../helpers/chat-desktop";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.resolve(__dirname, "../artifacts/run-command-encoding");
const MARKER_ZH = "断言失败";
const SCRIPT = "force_gbk_err.py";

describe("Stitch run_command console encoding", () => {
  it("shows readable Chinese stderr from GBK bytes (no replacement diamonds)", async () => {
    fs.rmSync(outDir, { recursive: true, force: true });
    fs.mkdirSync(outDir, { recursive: true });

    const sandbox = fs.mkdtempSync(path.join(os.tmpdir(), "stitch-enc-"));
    fs.writeFileSync(path.join(outDir, "sandbox-path.txt"), sandbox, "utf8");
    // Explicit GBK on stderr.buffer — not affected by PYTHONUTF8 text mode.
    fs.writeFileSync(
      path.join(sandbox, SCRIPT),
      [
        "import sys",
        `sys.stderr.buffer.write("${MARKER_ZH}：expected 5 got -1\\n".encode("gbk"))`,
        "sys.exit(1)",
        "",
      ].join("\n"),
      "utf8",
    );

    await bootChat();
    await setPlanMode(false);
    await newSession();
    await setWorkDir(sandbox);
    await shot(outDir, "01-workdir");

    await fillChat(
      [
        "关闭计划模式。不要写文件、不要修脚本。",
        `立刻调用工具 run_command，command 参数恰好是：python ${SCRIPT}`,
        "等命令结束后，用一句话说明工具输出里是否出现中文「断言失败」。不要使用表情符号。",
      ].join("\n"),
    );
    await clickSend();
    await shot(outDir, "02-streaming");
    await waitIdle(300_000);
    await shot(outDir, "03-idle");

    const tools = await browser.execute(() =>
      [...document.querySelectorAll('[data-testid="tool-status"]')].map((el) => ({
        tool: el.getAttribute("data-tool") || "",
        text: (el.textContent || "").replace(/\s+/g, " ").trim().slice(0, 500),
      })),
    );
    const body = await browser.execute(() => document.body.innerText || "");
    const stats = await chatStats();
    const toolBlob = tools.map((t) => t.text).join("\n");
    const combined = `${toolBlob}\n${body}`;

    const hasRun = tools.some((t) => t.tool === "run_command");
    const hasZh = combined.includes(MARKER_ZH);
    const hasReplacement = combined.includes("\uFFFD");

    fs.writeFileSync(
      path.join(outDir, "REPORT.txt"),
      [
        `sandbox=${sandbox}`,
        `hasRunCommand=${hasRun}`,
        `hasChineseMarker=${hasZh}`,
        `hasReplacementChar=${hasReplacement}`,
        `tools=${JSON.stringify(tools)}`,
        `assistant=${stats.lastAssistant.slice(0, 300)}`,
        hasZh && !hasReplacement ? "RESULT: PASS" : "RESULT: FAIL",
      ].join("\n"),
      "utf8",
    );

    expect(hasRun).toBe(true);
    expect(hasZh).toBe(true);
    expect(hasReplacement).toBe(false);
    expect(await findVisibleUiLeak()).toBeNull();
  });
});
