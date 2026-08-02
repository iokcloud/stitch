/**
 * Mature scene: merge-ready-auto — review sandbox with planted secret → MERGE_BLOCK report.
 * Artifacts → e2e/artifacts/mature-merge-ready/
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
const outDir = path.resolve(__dirname, "../artifacts/mature-merge-ready");
const REPORT = "stitch-merge-ready-report.md";
const SECRET_MARKER = "sk-stitch-merge-probe-secret";
const PROMPT = [
  "按「合并前审查自动」跑完，少问我。约束：",
  "1) 只审当前工作目录；先列审查范围；不改目录外文件。",
  "2) 运行 python check.py 作为轻量检查；摘录结果。",
  "3) 按四轴写发现：正确性、安全/密钥、可测性、破坏性/回滚。",
  "4) 默认不改业务代码；发现硬编码密钥则结论必须 MERGE_BLOCK。",
  "5) 写入 stitch-merge-ready-report.md：范围、检查结果、四轴发现、结论（MERGE_OK 或 MERGE_BLOCK）、合并前必做 1～3 条。",
].join("\n");

function seedMergeCandidate(sandbox: string) {
  fs.writeFileSync(
    path.join(sandbox, "app.py"),
    [
      "# merge candidate — intentional issues for mature-scene probe",
      `API_KEY = "${SECRET_MARKER}"`,
      "",
      "def greet(name):",
      "    return f'hi {name}'",
      "",
    ].join("\n"),
    "utf8",
  );
  fs.writeFileSync(
    path.join(sandbox, "check.py"),
    [
      "from app import greet",
      "",
      "assert greet('x') == 'hi x'",
      "print('check ok')",
      "",
    ].join("\n"),
    "utf8",
  );
  fs.writeFileSync(
    path.join(sandbox, "README.md"),
    "Probe sandbox for Stitch merge-ready-auto.\n",
    "utf8",
  );
}

describe("Stitch mature scene merge-ready-auto", () => {
  it("reviews sandbox, blocks on planted secret, writes merge report", async () => {
    fs.rmSync(outDir, { recursive: true, force: true });
    fs.mkdirSync(outDir, { recursive: true });

    const sandbox = fs.mkdtempSync(path.join(os.tmpdir(), "stitch-mature-merge-"));
    seedMergeCandidate(sandbox);
    fs.writeFileSync(path.join(outDir, "sandbox-path.txt"), sandbox, "utf8");

    await bootChat();
    await setPlanMode(false);
    await newSession();

    const setResult = await setWorkDir(sandbox);
    await browser.pause(400);
    await shot(outDir, "01-workdir-set");
    expect(setResult.applied.toLowerCase()).toContain("stitch-mature-merge");

    await fillChat(PROMPT);
    await clickSend();
    await shot(outDir, "02-streaming");
    await waitIdle(420_000);
    await shot(outDir, "03-idle");

    const stats = await chatStats();
    expect(stats.bodyHasSveltekit).toBe(false);
    expect(await findVisibleUiLeak()).toBeNull();

    const reportPath = path.join(sandbox, REPORT);
    const appPath = path.join(sandbox, "app.py");
    const reportExists = fs.existsSync(reportPath);
    const report = reportExists ? fs.readFileSync(reportPath, "utf8") : "";
    const app = fs.existsSync(appPath) ? fs.readFileSync(appPath, "utf8") : "";
    const blocked = /MERGE_BLOCK/i.test(report);
    const mentionsSecret =
      report.includes(SECRET_MARKER) ||
      /API_KEY|硬编码|密钥|secret/i.test(report);

    fs.writeFileSync(
      path.join(outDir, "REPORT.txt"),
      [
        `sandbox=${sandbox}`,
        `reportExists=${reportExists}`,
        `blocked=${blocked}`,
        `mentionsSecret=${mentionsSecret}`,
        `appStillHasSecret=${app.includes(SECRET_MARKER)}`,
        `reportPreview=${report.slice(0, 500).replace(/\n/g, "\\n")}`,
        `assistant=${stats.lastAssistant.slice(0, 300)}`,
      ].join("\n"),
      "utf8",
    );

    expect(reportExists).toBe(true);
    expect(report.length).toBeGreaterThan(80);
    expect(blocked).toBe(true);
    expect(mentionsSecret).toBe(true);
  });
});
