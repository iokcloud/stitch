/**
 * Real-exe: run_command must not flash a visible system32\cmd.exe window (S-009).
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { spawn, type ChildProcessWithoutNullStreams } from "node:child_process";
import { findVisibleUiLeak } from "../helpers/ui-hygiene";
import {
  assertNoGlyphIcons,
  bootChat,
  clickSend,
  fillChat,
  newSession,
  setPlanMode,
  setWorkDir,
  waitIdle,
} from "../helpers/chat-desktop";

const outDir = path.join(process.cwd(), "artifacts", "run-command-no-console");

describe("Stitch run_command no console flash", () => {
  let watcher: ChildProcessWithoutNullStreams | null = null;
  let logPath = "";
  let markerPath = "";

  before(async () => {
    fs.mkdirSync(outDir, { recursive: true });
    await bootChat();
  });

  after(async () => {
    if (markerPath) {
      try {
        fs.writeFileSync(markerPath, "stop", "utf8");
      } catch {
        /* ignore */
      }
    }
    if (watcher && !watcher.killed) {
      watcher.kill();
    }
  });

  it("echo via run_command leaves no visible cmd.exe window", async () => {
    const sandbox = fs.mkdtempSync(path.join(os.tmpdir(), "stitch-nocmd-"));
    logPath = path.join(outDir, "cmd-windows.log");
    markerPath = path.join(outDir, "watch-stop.marker");
    try {
      fs.unlinkSync(markerPath);
    } catch {
      /* ignore */
    }

    const ps1 = path.join(process.cwd(), "helpers", "watch-cmd-windows.ps1");
    watcher = spawn(
      "powershell.exe",
      [
        "-NoProfile",
        "-ExecutionPolicy",
        "Bypass",
        "-File",
        ps1,
        "-LogPath",
        logPath,
        "-MarkerPath",
        markerPath,
        "-TimeoutSec",
        "120",
      ],
      { stdio: "ignore", windowsHide: true },
    );

    await newSession();
    await setPlanMode(false);
    await setWorkDir(sandbox);
    await fillChat(
      [
        "关闭计划模式。不要写文件，不要列目录。",
        "立刻调用工具 run_command，command 参数恰好是：echo stitch-no-cmd-window",
        "收到输出后用一句话回复含 stitch-no-cmd-window。不要使用表情符号。",
      ].join("\n"),
    );
    await clickSend();
    await waitIdle(300_000);

    fs.writeFileSync(markerPath, "stop", "utf8");
    await browser.pause(400);
    if (watcher && !watcher.killed) watcher.kill();

    const tools = await browser.execute(() =>
      [...document.querySelectorAll('[data-testid="tool-status"]')].map(
        (el) => el.getAttribute("data-tool") || "",
      ),
    );
    const body = await browser.execute(() => document.body.innerText || "");

    const log = fs.existsSync(logPath) ? fs.readFileSync(logPath, "utf8") : "";
    fs.writeFileSync(
      path.join(outDir, "REPORT.txt"),
      [
        `sandbox=${sandbox}`,
        `tools=${tools.join(",") || "(none)"}`,
        `log=${log.trim() || "(empty)"}`,
        `hasMarkerText=${body.includes("stitch-no-cmd-window")}`,
        "expect: run_command used + no VISIBLE_CMD",
      ].join("\n"),
      "utf8",
    );

    expect(tools.includes("run_command")).toBe(true);
    expect(log).not.toMatch(/VISIBLE_CMD/);
    await assertNoGlyphIcons();
    expect(await findVisibleUiLeak()).toBeNull();
  });
});
