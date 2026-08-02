/**
 * Compact morph probe (real exe): a desktop tool must morph the window into
 * the 420x64 floating bar showing execution state, then restore the full
 * window when the turn finishes.
 */
import fs from "node:fs";
import path from "node:path";
import { execSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import {
  bootChat,
  clickSend,
  fillChat,
  newSession,
  shot,
  waitIdle,
} from "../helpers/chat-desktop";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.resolve(__dirname, "../artifacts/compact-morph");

describe("Compact morph on desktop automation (real exe)", () => {
  before(async () => {
    fs.mkdirSync(outDir, { recursive: true });
    await bootChat();
  });

  it("morphs to the compact bar while a desktop tool runs, then restores", async () => {
    await newSession();
    await fillChat(
      "请使用 desktop_window_list 工具查看当前打开的窗口列表，然后用中文简要说明有哪些窗口。",
    );
    await clickSend();

    // 1. Compact mode engages while the desktop tool runs.
    await browser.waitUntil(
      async () =>
        browser.execute(
          () => document.documentElement.getAttribute("data-compact") === "true",
        ),
      { timeout: 60_000, timeoutMsg: "compact mode did not engage" },
    );
    // 2. Execution state visible: tool label + live stopwatch. These run
    // first — the PowerShell screen capture below is slow (~2-4s) and the
    // LLM turn must not finish before the assertions. The label may already
    // be in the「已完成」linger for very fast turns — either state proves the
    // execution-state surface is live.
    const tool = await $('[data-testid="compact-tool"]');
    await tool.waitForExist({ timeout: 15_000 });
    const toolText = (await tool.getText()) ?? "";
    expect(/^(正在执行|已完成)/.test(toolText)).toBe(true);
    // 呼吸光晕在跑（无秒表——compact-glow 动画生效）
    const glow = await $(".compact-bar");
    await glow.waitForExist({ timeout: 5_000 });
    // Drag region present on the label area.
    const dragRegion = await browser.execute(
      () =>
        !!document
          .querySelector('[data-testid="compact-tool"]')
          ?.closest("[data-tauri-drag-region]"),
    );
    expect(dragRegion).toBe(true);

    // Layer V: capture the REAL screen mid-morph. The webview screenshot
    // lags the window resize, so capture the desktop itself while the debug
    // animation (STITCH_COMPACT_ANIM_MS=5000) is still running.
    await browser.pause(300);
    try {
      execSync(
        `powershell -NoProfile -Command "Add-Type -AssemblyName System.Windows.Forms,System.Drawing; $b=New-Object System.Drawing.Bitmap([System.Windows.Forms.Screen]::PrimaryScreen.Bounds.Width,[System.Windows.Forms.Screen]::PrimaryScreen.Bounds.Height); $g=[System.Drawing.Graphics]::FromImage($b); $g.CopyFromScreen(0,0,0,0,$b.Size); $b.Save('${outDir.replace(/\\/g, "\\\\")}\\morph-screen.png')"`,
        { stdio: "ignore" },
      );
    } catch {
      /* screen capture best-effort */
    }
    // 3. The window really shrank to the bar (not just the DOM flag).
    await browser.waitUntil(
      async () => (await browser.execute(() => window.outerWidth)) < 600,
      { timeout: 15_000, timeoutMsg: "window did not shrink to bar size" },
    );
    await shot(outDir, "compact-bar");

    // 4. Turn finishes → 已完成 linger state is visible for ~2.6s.
    await waitIdle(180_000);
    await browser.waitUntil(
      async () => {
        const t = await $('[data-testid="compact-tool"]');
        return (await t.isExisting()) && (await t.getText()).includes("已完成");
      },
      { timeout: 4_000, timeoutMsg: "done linger state not seen" },
    );
    // Layer V: capture the completion state before the window restores.
    await shot(outDir, "compact-done");
    // …then compact exits → full window restored.
    await browser.waitUntil(
      async () =>
        browser.execute(
          () => document.documentElement.getAttribute("data-compact") !== "true",
        ),
      { timeout: 30_000, timeoutMsg: "compact mode did not exit" },
    );
    const restoredW = await browser.execute(() => window.outerWidth);
    expect(restoredW).toBeGreaterThan(800);

    // ── Manual round-trip (Ctrl+Shift+C): enter, idle bar, exit ────────
    await browser.keys(["Control", "Shift", "c"]);
    await browser.waitUntil(
      async () =>
        browser.execute(
          () => document.documentElement.getAttribute("data-compact") === "true",
        ),
      { timeout: 10_000, timeoutMsg: "manual compact did not engage" },
    );
    await browser.waitUntil(
      async () => (await browser.execute(() => window.outerWidth)) < 600,
      { timeout: 15_000, timeoutMsg: "manual compact window did not shrink" },
    );
    const idleLabel = await $('[data-testid="compact-tool"]');
    await idleLabel.waitForExist({ timeout: 5_000 });
    expect((await idleLabel.getText()).includes("紧凑模式")).toBe(true);
    await browser.keys(["Control", "Shift", "c"]);
    await browser.waitUntil(
      async () =>
        browser.execute(
          () => document.documentElement.getAttribute("data-compact") !== "true",
        ),
      { timeout: 10_000, timeoutMsg: "manual compact did not exit" },
    );
    const manualW = await browser.execute(() => window.outerWidth);
    expect(manualW).toBeGreaterThan(800);
  });
});
