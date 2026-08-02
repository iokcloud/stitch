/**
 * Mature scene: debug-recover-auto — broken sandbox → agent recovers → report on disk.
 * Artifacts → e2e/artifacts/mature-debug-recover/
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
const outDir = path.resolve(__dirname, "../artifacts/mature-debug-recover");
const REPORT = "stitch-debug-recover-report.md";
const PROMPT = [
  "按「改崩后停线复原」跑完，少问我。约束：",
  "1) 先停线：不新增功能；三句话写清症状与范围。",
  "2) 在当前工作目录复现失败：运行 python check.py；摘录关键错误。",
  "3) 给 1～3 条假设并选最可能的一条做最小修复；危险命令先确认；不顺手重构无关文件。",
  "4) 用同一命令再验证。",
  "5) 在工作目录写入 stitch-debug-recover-report.md：症状、复现命令、根因、改动文件列表、验证结果、若仍失败则停止建议。",
  "若无法复现：写明已检查什么，仍输出报告后结束。",
].join("\n");

function seedBrokenSandbox(sandbox: string) {
  fs.writeFileSync(
    path.join(sandbox, "buggy.py"),
    [
      "def add(a, b):",
      "    # intentional bug for mature-scene probe",
      "    return a - b",
      "",
    ].join("\n"),
    "utf8",
  );
  fs.writeFileSync(
    path.join(sandbox, "check.py"),
    [
      "from buggy import add",
      "",
      "got = add(2, 3)",
      "assert got == 5, f'expected 5, got {got}'",
      "print('check ok')",
      "",
    ].join("\n"),
    "utf8",
  );
}

describe("Stitch mature scene debug-recover-auto", () => {
  it("reproduces, minimal-fixes, and writes recover report under workdir", async () => {
    fs.rmSync(outDir, { recursive: true, force: true });
    fs.mkdirSync(outDir, { recursive: true });

    const sandbox = fs.mkdtempSync(path.join(os.tmpdir(), "stitch-mature-debug-"));
    seedBrokenSandbox(sandbox);
    fs.writeFileSync(path.join(outDir, "sandbox-path.txt"), sandbox, "utf8");

    await bootChat();
    await setPlanMode(false);
    await newSession();

    const setResult = await setWorkDir(sandbox);
    await browser.pause(400);
    await shot(outDir, "01-workdir-set");
    expect(setResult.applied.toLowerCase()).toContain("stitch-mature-debug");

    await fillChat(PROMPT);
    await clickSend();
    await shot(outDir, "02-streaming");
    await waitIdle(420_000);
    await shot(outDir, "03-idle");

    const stats = await chatStats();
    expect(stats.bodyHasSveltekit).toBe(false);
    expect(await findVisibleUiLeak()).toBeNull();

    const reportPath = path.join(sandbox, REPORT);
    const buggyPath = path.join(sandbox, "buggy.py");
    const reportExists = fs.existsSync(reportPath);
    const buggy = fs.existsSync(buggyPath) ? fs.readFileSync(buggyPath, "utf8") : "";
    const report = reportExists ? fs.readFileSync(reportPath, "utf8") : "";
    const fixed = buggy.includes("a + b") || buggy.includes("a+b");

    fs.writeFileSync(
      path.join(outDir, "REPORT.txt"),
      [
        `sandbox=${sandbox}`,
        `reportExists=${reportExists}`,
        `fixedAdd=${fixed}`,
        `buggyPreview=${buggy.slice(0, 200).replace(/\n/g, "\\n")}`,
        `reportPreview=${report.slice(0, 400).replace(/\n/g, "\\n")}`,
        `assistant=${stats.lastAssistant.slice(0, 300)}`,
      ].join("\n"),
      "utf8",
    );

    expect(reportExists).toBe(true);
    expect(report.length).toBeGreaterThan(40);
    expect(fixed).toBe(true);
  });
});
