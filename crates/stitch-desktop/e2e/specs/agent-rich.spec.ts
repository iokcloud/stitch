/**
 * Rich real-exe scenarios: multiline input, long output, complex multi-file task.
 * Screenshots → e2e/artifacts/agent-rich/
 */
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { findVisibleUiLeak } from "../helpers/ui-hygiene";
import {
  assertNoGlyphIcons,
  bootChat,
  chatStats,
  clickSend,
  composerHeight,
  fillChat,
  newSession,
  sendChat,
  setPlanMode,
  setWorkDir,
  shot,
  uiSnapshot,
  waitIdle,
} from "../helpers/chat-desktop";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.resolve(__dirname, "../artifacts/agent-rich");

const lines: string[] = [];
function note(s: string) {
  lines.push(s);
}

describe("Stitch agent rich scenarios", () => {
  before(async () => {
    fs.rmSync(outDir, { recursive: true, force: true });
    fs.mkdirSync(outDir, { recursive: true });
    await bootChat();
  });

  after(() => {
    fs.writeFileSync(
      path.join(outDir, "REPORT.txt"),
      ["Stitch agent rich scenarios", ...lines, `artifacts: ${outDir}`].join("\n"),
      "utf8",
    );
  });

  it("multiline composer grows and sends preserved content", async () => {
    await setPlanMode(false);
    await newSession();

    const multi = [
      "请确认你收到了多行输入。",
      "第二行：alpha-line",
      "第三行：beta-line",
      "请只回复：多行已收到，并逐行列出 alpha-line 与 beta-line。",
    ].join("\n");

    const h0 = await composerHeight();
    await fillChat(multi);
    await browser.pause(200);
    const h1 = await composerHeight();
    expect(h1).toBeGreaterThan(h0 + 12);
    await shot(outDir, "01-multiline-composer");

    await clickSend();
    await waitIdle(120_000);
    await shot(outDir, "02-multiline-sent");

    const snap = await uiSnapshot();
    expect(snap.lastUserText).toContain("alpha-line");
    expect(snap.lastUserText).toContain("beta-line");
    expect(snap.lastAssistant.toLowerCase()).toMatch(/alpha-line/);
    expect(snap.lastAssistant.toLowerCase()).toMatch(/beta-line/);
    await assertNoGlyphIcons();
    expect(await findVisibleUiLeak()).toBeNull();
    note("multiline: PASS (composer grew + lines preserved in reply)");
  });

  it("long assistant output is usable (clamp or substantial length)", async () => {
    await newSession();
    await sendChat(
      [
        "不要调用任何工具，不要写文件。",
        "请直接在对话里用中文写一篇很长的结构化说明，主题是「如何在本地调试桌面 Agent」。",
        "硬性要求：至少 10 个二级标题（markdown ##）；每个标题下至少 5 句完整话；",
        "全文纯文字不少于 1200 字；不要省略、不要用列表敷衍成短句。",
        "不要使用表情符号。写满再结束。",
      ].join(""),
      240_000,
    );
    await shot(outDir, "03-long-output");

    const snap = await uiSnapshot();
    // uiSnapshot truncates lastAssistant to 800; also check expand + raw length
    const rawLen = await browser.execute(() => {
      const el = [...document.querySelectorAll(".msg-assistant")].at(-1);
      return (el?.textContent || "").trim().length;
    });
    const hasExpand = snap.expandBtns.some((t) => /展开/.test(t));
    expect(rawLen).toBeGreaterThan(900);
    expect(hasExpand).toBe(true);
    // Click the assistant expand control (not a clamped user bubble).
    await browser.execute(() => {
      const assistant = [...document.querySelectorAll(".msg-assistant")].at(-1);
      const btn = assistant?.querySelector(".message-expand") as HTMLButtonElement | null;
      btn?.click();
    });
    await browser.pause(250);
    await shot(outDir, "04-long-expanded");
    const after = await uiSnapshot();
    expect(after.expandBtns.some((t) => /收起/.test(t))).toBe(true);
    await assertNoGlyphIcons();
    expect(await findVisibleUiLeak()).toBeNull();
    note(`long-output: PASS (rawLen=${rawLen}, expand=${hasExpand})`);
  });

  it("complex coding task writes multiple files under workdir without glyph icons", async () => {
    const sandbox = fs.mkdtempSync(path.join(os.tmpdir(), "stitch-rich-"));
    fs.writeFileSync(path.join(outDir, "sandbox-path.txt"), sandbox, "utf8");

    await newSession();
    const set = await setWorkDir(sandbox);
    expect(set.applied.toLowerCase()).toContain("stitch-rich");
    await shot(outDir, "05-complex-workdir");

    await setPlanMode(true);
    await fillChat(
      [
        "复杂编程任务（计划模式）：只在当前工作目录内操作。",
        "1) 创建 util/math_ops.py，实现 add(a,b) 与 mul(a,b)，返回数值。",
        "2) 创建 tests/test_math_ops.py，用 assert 测 add(2,3)==5 与 mul(3,4)==12。",
        "3) 创建 README_PROBE.md，用三行说明这两个文件做什么。",
        "文件内容里请包含标记：rich-probe-42",
        "不要写到工作目录以外；完成后用列表汇报创建的相对路径。不要使用表情符号。",
      ].join("\n"),
    );
    await clickSend();

    // Wait for plan card then approve
    await $('[data-testid="plan-card"]').waitForExist({
      timeout: 180_000,
      timeoutMsg: "plan card missing for complex task",
    });
    await shot(outDir, "06-complex-plan");
    const approve = await $('[data-testid="plan-approve"]');
    if (await approve.isExisting()) {
      await approve.click();
    }
    await waitIdle(360_000);
    await shot(outDir, "07-complex-done");

    await assertNoGlyphIcons();
    const snap = await uiSnapshot();
    expect(snap.toolCallCount + snap.planCount).toBeGreaterThan(0);

    const must = ["util/math_ops.py", "tests/test_math_ops.py", "README_PROBE.md"];
    const found: string[] = [];
    const walk = (dir: string) => {
      for (const name of fs.readdirSync(dir)) {
        const p = path.join(dir, name);
        if (fs.statSync(p).isDirectory()) walk(p);
        else found.push(path.relative(sandbox, p).replace(/\\/g, "/"));
      }
    };
    walk(sandbox);

    const missing = must.filter((m) => !found.includes(m));
    // Allow slight path variance but require marker somewhere under sandbox
    let markerHit = false;
    const checkMarker = (dir: string) => {
      for (const name of fs.readdirSync(dir)) {
        const p = path.join(dir, name);
        if (fs.statSync(p).isDirectory()) checkMarker(p);
        else if (fs.readFileSync(p, "utf8").includes("rich-probe-42")) markerHit = true;
      }
    };
    checkMarker(sandbox);

    const report = [
      `sandbox files: ${found.join(", ") || "(none)"}`,
      `missing exact paths: ${missing.join(", ") || "(none)"}`,
      `markerHit: ${markerHit}`,
      `tools: ${snap.tools.join(" | ") || "(none)"}`,
    ].join("\n");
    fs.writeFileSync(path.join(outDir, "complex-files.txt"), report, "utf8");

    expect(markerHit || found.length >= 2).toBe(true);
    expect(found.length).toBeGreaterThanOrEqual(2);
    // At least two of the three expected files (LLM may rename slightly)
    const hitCount = must.filter((m) => found.some((f) => f.endsWith(path.basename(m)))).length;
    expect(hitCount).toBeGreaterThanOrEqual(2);
    expect(await findVisibleUiLeak()).toBeNull();

    const stats = await chatStats();
    expect(stats.bodyHasSveltekit).toBe(false);
    note(`complex-coding: PASS (files=${found.length}, marker=${markerHit}, tools=${snap.toolCallCount})`);
    note(report);
  });
});
