/**
 * Programming scenario against real exe + LLM:
 * set sandbox workdir → ask agent to create code → file must land inside workdir.
 * Artifacts → e2e/artifacts/coding-workdir/
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
const outDir = path.resolve(__dirname, "../artifacts/coding-workdir");
const MARKER = "stitch-workdir-probe-ok";
const REL_FILE = path.join("stitch_probe", "hello.py");

async function workDirBarText(): Promise<string> {
  return browser.execute(() => {
    const active = document.querySelector(
      '[data-testid="workspace-row"][data-active="1"] [data-testid="workspace-label"]',
    );
    const any = document.querySelector('[data-testid="workspace-label"]');
    const el = active || any;
    return (el?.textContent || "").replace(/\s+/g, " ").trim();
  });
}

describe("Stitch coding workdir scenario", () => {
  it("creates project file under the sandbox working directory", async () => {
    fs.rmSync(outDir, { recursive: true, force: true });
    fs.mkdirSync(outDir, { recursive: true });

    const sandbox = fs.mkdtempSync(path.join(os.tmpdir(), "stitch-coding-"));
    const expectedFile = path.join(sandbox, REL_FILE);
    // Clean any leftover from prior runs with same name pattern
    fs.writeFileSync(path.join(outDir, "sandbox-path.txt"), sandbox, "utf8");

    await bootChat();
    await setPlanMode(false);
    await newSession();

    const setResult = await setWorkDir(sandbox);
    await browser.pause(400);
    await shot(outDir, "01-workdir-set");

    const bar = await workDirBarText();
    expect(bar.length).toBeGreaterThan(3);
    // Store / IPC must point at sandbox (UI may truncate with …)
    expect(setResult.applied.toLowerCase()).toContain("stitch-coding");
    expect(setResult.shown.toLowerCase()).toContain("stitch-coding");

    await fillChat(
      [
        "这是编程任务。请只在当前工作目录内操作。",
        `创建文件 ${REL_FILE.replace(/\\/g, "/")}，完整内容如下（不要改路径、不要写到工作目录外）：`,
        "",
        "```python",
        `def hello():`,
        `    return "${MARKER}"`,
        "",
        `if __name__ == "__main__":`,
        `    print(hello())`,
        "```",
        "",
        "写完后用一句话确认相对路径。不要运行会改动工作目录外的命令。",
      ].join("\n"),
    );
    await clickSend();
    await shot(outDir, "02-streaming");

    // Auto-approve write_file / create_directory confirms; allow long agent loop
    await waitIdle(300_000);
    await shot(outDir, "03-idle");

    const stats = await chatStats();
    expect(stats.bodyHasSveltekit).toBe(false);
    expect(await findVisibleUiLeak()).toBeNull();

    // Primary acceptance: file on disk under sandbox
    const exists = fs.existsSync(expectedFile);
    let content = "";
    if (exists) {
      content = fs.readFileSync(expectedFile, "utf8");
    }

    // Fallback: agent may have used slightly different relative path — search sandbox
    let foundPath = exists ? expectedFile : "";
    if (!exists) {
      const walk = (dir: string): string | null => {
        for (const name of fs.readdirSync(dir)) {
          const p = path.join(dir, name);
          const st = fs.statSync(p);
          if (st.isDirectory()) {
            const hit = walk(p);
            if (hit) return hit;
          } else if (name.endsWith(".py") && fs.readFileSync(p, "utf8").includes(MARKER)) {
            return p;
          }
        }
        return null;
      };
      foundPath = walk(sandbox) || "";
      if (foundPath) content = fs.readFileSync(foundPath, "utf8");
    }

    const report = [
      "Stitch coding workdir scenario",
      `sandbox: ${sandbox}`,
      `set_work_dir applied: ${setResult.applied}`,
      `store shown: ${setResult.shown}`,
      `workdir bar: ${bar}`,
      `expected: ${expectedFile}`,
      `found: ${foundPath || "(none)"}`,
      `marker in file: ${content.includes(MARKER)}`,
      `chat last assistant: ${stats.lastAssistant.slice(0, 300)}`,
      exists || foundPath ? "RESULT: PASS" : "RESULT: FAIL — no file under workdir",
    ].join("\n");
    fs.writeFileSync(path.join(outDir, "REPORT.txt"), report, "utf8");
    await shot(outDir, "04-done");

    if (!foundPath) {
      throw new Error(report);
    }
    expect(content).toContain(MARKER);
    // Must be under sandbox (normalized)
    const sandCanon = fs.realpathSync(sandbox);
    const fileCanon = fs.realpathSync(foundPath);
    expect(fileCanon.startsWith(sandCanon)).toBe(true);
  });
});
