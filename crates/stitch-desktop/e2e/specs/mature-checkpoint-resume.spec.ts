/**
 * Mature scene: checkpoint-resume — multi-step task with on-disk checkpoint + report.
 * Artifacts → e2e/artifacts/mature-checkpoint-resume/
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
const outDir = path.resolve(__dirname, "../artifacts/mature-checkpoint-resume");
const MARKER = "stitch-checkpoint-probe-ok";
const PROMPT = [
  "按「长任务检查点续跑」执行。约束：",
  "1) 在工作目录读写 stitch-checkpoint.json（没有则按目标拆步并初始化；有则续跑，跳过 status=done）。",
  "2) 预算 max_steps 默认 8；每完成或失败一步立刻回写 JSON；used_steps 每步 +1。",
  "3) 一次会话尽量推进；触达预算或某步 failed 就停。",
  "4) 结束时写 stitch-checkpoint-report.md：任务、已完成步、失败步、产物路径、如何继续。",
  "5) 危险命令先确认；不改工作目录外文件。",
  "我的目标：在工作目录创建 out/a.txt，内容仅一行 " + MARKER + "；再创建 out/b.txt，内容仅一行 step-two-ok。两步都做完并落盘 checkpoint 与报告。",
].join("\n");

describe("Stitch mature scene checkpoint-resume", () => {
  it("writes checkpoint json, step files, and report under workdir", async () => {
    fs.rmSync(outDir, { recursive: true, force: true });
    fs.mkdirSync(outDir, { recursive: true });

    const sandbox = fs.mkdtempSync(path.join(os.tmpdir(), "stitch-mature-ckpt-"));
    fs.writeFileSync(path.join(outDir, "sandbox-path.txt"), sandbox, "utf8");

    await bootChat();
    await setPlanMode(false);
    await newSession();

    const setResult = await setWorkDir(sandbox);
    await browser.pause(400);
    await shot(outDir, "01-workdir-set");
    expect(setResult.applied.toLowerCase()).toContain("stitch-mature-ckpt");

    await fillChat(PROMPT);
    await clickSend();
    await shot(outDir, "02-streaming");
    await waitIdle(420_000);
    await shot(outDir, "03-idle");

    const stats = await chatStats();
    expect(stats.bodyHasSveltekit).toBe(false);
    expect(await findVisibleUiLeak()).toBeNull();

    const ckptPath = path.join(sandbox, "stitch-checkpoint.json");
    const reportPath = path.join(sandbox, "stitch-checkpoint-report.md");
    const aPath = path.join(sandbox, "out", "a.txt");
    const bPath = path.join(sandbox, "out", "b.txt");

    const ckptExists = fs.existsSync(ckptPath);
    const reportExists = fs.existsSync(reportPath);
    const a = fs.existsSync(aPath) ? fs.readFileSync(aPath, "utf8") : "";
    const b = fs.existsSync(bPath) ? fs.readFileSync(bPath, "utf8") : "";
    const ckpt = ckptExists ? fs.readFileSync(ckptPath, "utf8") : "";
    const report = reportExists ? fs.readFileSync(reportPath, "utf8") : "";

    fs.writeFileSync(
      path.join(outDir, "REPORT.txt"),
      [
        `sandbox=${sandbox}`,
        `ckptExists=${ckptExists}`,
        `reportExists=${reportExists}`,
        `aOk=${a.includes(MARKER)}`,
        `bOk=${b.includes("step-two-ok")}`,
        `ckptPreview=${ckpt.slice(0, 300).replace(/\n/g, "\\n")}`,
        `reportPreview=${report.slice(0, 300).replace(/\n/g, "\\n")}`,
      ].join("\n"),
      "utf8",
    );

    expect(ckptExists).toBe(true);
    expect(reportExists).toBe(true);
    expect(a.includes(MARKER)).toBe(true);
    expect(b.includes("step-two-ok")).toBe(true);
  });
});
