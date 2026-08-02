/**
 * Desktop automation benchmark — standard Windows task success-rate evaluation.
 *
 * Drives the real binary (webdriver exe) + real LLM through five standard
 * Windows tasks, records pass/fail + wall time + per-tool duration_ms
 * (ToolResult.metrics, exposed on the tool card as data-metrics), then writes
 * artifacts/desktop-benchmark/REPORT.md.
 *
 * Run: npm run benchmark-desktop (needs `cargo build -p stitch-desktop
 * --features webdriver` first).
 */
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { execSync, spawn, type ChildProcess } from "node:child_process";
import { fileURLToPath } from "node:url";
import {
  bootChat,
  chatStats,
  clickSend,
  fillChat,
  newSession,
  setPlanMode,
  setWorkDir,
  shot,
  waitIdle,
} from "../helpers/chat-desktop";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.resolve(__dirname, "../artifacts/desktop-benchmark");

interface ToolRun {
  name: string;
  duration_ms: number | null;
}

interface TaskResult {
  id: string;
  title: string;
  pass: boolean;
  failReason: string;
  durationMs: number;
  tools: ToolRun[];
}

const results: TaskResult[] = [];
const notes: string[] = [];

function note(s: string) {
  notes.push(s);
  console.log(`[benchmark] ${s}`);
}

/** tool cards currently in the DOM → (name, duration_ms) list. */
async function collectToolMetrics(): Promise<ToolRun[]> {
  return browser.execute(() => {
    return [...document.querySelectorAll('[data-testid="tool-status"]')].map((el) => {
      let duration_ms: number | null = null;
      const raw = el.getAttribute("data-metrics");
      if (raw) {
        try {
          const m = JSON.parse(raw) as Record<string, number>;
          duration_ms = typeof m.duration_ms === "number" ? m.duration_ms : null;
        } catch {
          /* unparseable — leave null */
        }
      }
      return { name: el.getAttribute("data-tool") ?? "?", duration_ms };
    });
  });
}

function tasklistHas(proc: string): boolean {
  try {
    const out = execSync(`tasklist /FI "IMAGENAME eq ${proc}.exe" /NH`, {
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    });
    return out.toLowerCase().includes(proc);
  } catch {
    return false;
  }
}

const EDGE_CANDIDATES = [
  "C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe",
  "C:/Program Files/Microsoft/Edge/Application/msedge.exe",
];
const CHROME_CANDIDATES = [
  "C:/Program Files/Google/Chrome/Application/chrome.exe",
  "C:/Program Files (x86)/Google/Chrome/Application/chrome.exe",
];

describe("Desktop automation benchmark", () => {
  let workDir: string;
  let httpServer: http.Server | null = null;
  let browserProc: ChildProcess | null = null;

  before(async () => {
    fs.rmSync(outDir, { recursive: true, force: true });
    fs.mkdirSync(outDir, { recursive: true });
    await bootChat();
    await setPlanMode(false);
    workDir = fs.mkdtempSync(path.join(os.tmpdir(), "stitch-bench-"));
    note(`workDir=${workDir}`);
  });

  after(() => {
    // Close the benchmark browser window (best-effort).
    try {
      if (browserProc?.pid) {
        execSync(`taskkill /PID ${browserProc.pid} /T /F`, { stdio: "ignore" });
      }
    } catch {
      /* already closed */
    }
    httpServer?.close();
    writeReport();
    try {
      fs.rmSync(workDir, { recursive: true, force: true });
    } catch {
      /* locked by exe — ok */
    }
  });

  /** One task: fresh session → send → wait idle → verify → record. */
  async function runTask(
    id: string,
    title: string,
    prompt: string,
    verify: () => Promise<{ ok: boolean; reason?: string }>,
  ): Promise<TaskResult> {
    const t0 = Date.now();
    let pass = false;
    let failReason = "";
    let tools: ToolRun[] = [];
    try {
      await newSession();
      // session-new binds the session to the workspace dir (promptstdio
      // root), which overrides the isolated temp workDir. Re-apply.
      await setWorkDir(workDir);
      await fillChat(prompt);
      await clickSend();
      await waitIdle(240_000);
      await shot(outDir, `${id}-after`);
      tools = await collectToolMetrics();
      const v = await verify();
      pass = v.ok;
      if (!v.ok) failReason = v.reason ?? "verification failed";
    } catch (e) {
      failReason = `exception: ${String(e).slice(0, 300)}`;
    }
    const r: TaskResult = {
      id,
      title,
      pass,
      failReason,
      durationMs: Date.now() - t0,
      tools,
    };
    results.push(r);
    note(
      `${id} ${pass ? "PASS" : "FAIL"} (${(r.durationMs / 1000).toFixed(1)}s)${
        failReason ? ` — ${failReason}` : ""
      } — tools: ${tools.map((t) => t.name).join(", ") || "none"}`,
    );
    return r;
  }

  it("runs the standard Windows task battery", async () => {
    const benchMarker = `BMK${Math.floor(1000 + Math.random() * 9000)}`;

    // ── T1 notepad-write ─────────────────────────────────────────────
    const benchTxt = path.join(workDir, "bench.txt");
    const t1Marker = `BMK-NOTEPAD-${benchMarker}`;
    await runTask(
      "T1",
      "notepad-write",
      `请用桌面自动化完成以下任务：
1. 用 desktop_app_launch 打开记事本（app 填 notepad）。
2. 用 desktop_type 输入这一行文字（不含引号）：${t1Marker} desktop automation
3. 用 desktop_key 按 ctrl+s 保存，在弹出的保存对话框中用 desktop_type 把文件路径输入为：${benchTxt}，然后按回车确认保存。
4. 用 desktop_window_action 关闭记事本窗口（title 填「记事本」，action 填 close）。
完成后告诉我文件已保存到哪个路径。`,
      async () => {
        if (!fs.existsSync(benchTxt)) {
          return { ok: false, reason: `missing artifact ${benchTxt}` };
        }
        const body = fs.readFileSync(benchTxt, "utf8");
        if (!body.includes(t1Marker)) {
          return { ok: false, reason: `artifact lacks marker "${t1Marker}"` };
        }
        if (tasklistHas("notepad")) {
          return { ok: false, reason: "notepad still running after close" };
        }
        return { ok: true };
      },
    );

    // ── T2 browser-read ──────────────────────────────────────────────
    const t2Marker = `BMK-BROWSER-${benchMarker}`;
    const html = `<!doctype html><html><head><title>${t2Marker}</title></head>
<body style="background:#fff;font-family:system-ui,sans-serif">
<div style="font-size:64px;font-weight:700;color:#111;padding:48px">${t2Marker}</div>
</body></html>`;
    httpServer = http.createServer((_req, res) => {
      res.setHeader("content-type", "text/html; charset=utf-8");
      res.end(html);
    });
    await new Promise<void>((resolve) => httpServer!.listen(0, "127.0.0.1", resolve));
    const port = (httpServer.address() as { port: number }).port;
    const url = `http://127.0.0.1:${port}/`;
    const browserExe =
      EDGE_CANDIDATES.find(fs.existsSync) ?? CHROME_CANDIDATES.find(fs.existsSync);
    if (browserExe) {
      // App-mode window keeps the benchmark page isolated and closable.
      browserProc = spawn(browserExe, [`--app=${url}`], { detached: true, stdio: "ignore" });
      await new Promise((r) => setTimeout(r, 2500));
    }
    await runTask(
      "T2",
      "browser-read",
      `屏幕上已打开一个浏览器窗口，显示一个本地测试页面。请用 desktop_browser 工具读出页面上显示的大字标记文本（用 read_page 或截图 OCR 都行），然后告诉我页面上显示的标记是什么。`,
      async () => {
        const after = await chatStats();
        const reply = after.lastAssistant;
        if (!reply) return { ok: false, reason: "no assistant reply" };
        const norm = reply.toLowerCase();
        if (!norm.includes(t2Marker.toLowerCase())) {
          return { ok: false, reason: `reply lacks marker "${t2Marker}"` };
        }
        return { ok: true };
      },
    );

    // ── T3 run-write-combo ───────────────────────────────────────────
    const t3Py = path.join(workDir, "data.txt");
    const t3PyMarker = `BMK-PY-${benchMarker}`;
    const t3Write = path.join(workDir, "note.txt");
    const t3WriteMarker = `BMK-WRITE-${benchMarker}`;
    await runTask(
      "T3",
      "run-write-combo",
      `请在当前工作目录 ${workDir} 中完成：
1. 用 run_command 执行 python 命令，生成文件 ${t3Py}，文件内容为一行 ${t3PyMarker}（不含引号）。
2. 用 write_file 创建文件 ${t3Write}，内容为 ${t3WriteMarker}。
完成后用 list_directory 确认这两个文件都在，然后告诉我结果。`,
      async () => {
        const checks: string[] = [];
        if (!fs.existsSync(t3Py)) checks.push(`missing ${t3Py}`);
        else if (!fs.readFileSync(t3Py, "utf8").includes(t3PyMarker))
          checks.push(`py artifact lacks marker`);
        if (!fs.existsSync(t3Write)) checks.push(`missing ${t3Write}`);
        else if (!fs.readFileSync(t3Write, "utf8").includes(t3WriteMarker))
          checks.push(`write artifact lacks marker`);
        return checks.length
          ? { ok: false, reason: checks.join("; ") }
          : { ok: true };
      },
    );

    // ── T4 window-ops ────────────────────────────────────────────────
    await runTask(
      "T4",
      "window-ops",
      `请用桌面自动化完成：1. 用 desktop_app_launch 打开记事本（app 填 notepad）。2. 用 desktop_window_list 确认记事本窗口在列表中。3. 用 desktop_window_action 关闭标题包含「记事本」的窗口（action 填 close）。完成后告诉我窗口是否已关闭。`,
      async () => {
        if (tasklistHas("notepad")) {
          return { ok: false, reason: "notepad still running after close" };
        }
        return { ok: true };
      },
    );

    // ── T5 screenshot-ocr (soft — recorded, does not fail the suite) ──
    await runTask(
      "T5",
      "screenshot-ocr",
      `请用 desktop_screenshot 工具（把 ocr 参数设为 true）截取当前屏幕，然后用中文简要描述屏幕上能看到什么。`,
      async () => {
        const after = await chatStats();
        if (!after.lastAssistant) return { ok: false, reason: "no assistant reply" };
        return { ok: true };
      },
    );
  });
});

function writeReport() {
  const passed = results.filter((r) => r.pass).length;
  const soft = results.filter((r) => r.id === "T5");
  const lines: string[] = [];
  lines.push("# 桌面自动化 benchmark");
  lines.push("");
  lines.push(`- 日期: ${new Date().toISOString().slice(0, 10)}`);
  lines.push(`- 二进制: debug/webdriver stitch-desktop.exe`);
  lines.push(`- 模型: 会话配置（真 LLM）`);
  lines.push(`- 通过率: ${passed}/${results.length}（${Math.round((passed / Math.max(results.length, 1)) * 100)}%）`);
  lines.push("");
  lines.push("## 任务结果");
  lines.push("");
  lines.push("| 任务 | 结果 | 耗时(s) | 失败原因 |");
  lines.push("|---|---|---|---|");
  for (const r of results) {
    const mark = r.id === "T5" ? (r.pass ? "PASS*" : "FAIL*") : r.pass ? "PASS" : "FAIL";
    lines.push(`| ${r.id} ${r.title} | ${mark} | ${(r.durationMs / 1000).toFixed(1)} | ${r.failReason || "-"} |`);
  }
  lines.push("");
  if (soft.length) {
    lines.push("*T5 为软任务：只记录不阻断套件。");
    lines.push("");
  }
  lines.push("## 工具耗时 (duration_ms)");
  lines.push("");
  const perTool = new Map<string, number[]>();
  for (const r of results) {
    for (const t of r.tools) {
      if (t.duration_ms == null) continue;
      if (!perTool.has(t.name)) perTool.set(t.name, []);
      perTool.get(t.name)!.push(t.duration_ms);
    }
  }
  if (perTool.size === 0) {
    lines.push("（无带 duration_ms 的工具指标——桌面工具未运行或 metrics 未透出）");
  } else {
    lines.push("| 工具 | 次数 | min | avg | max |");
    lines.push("|---|---|---|---|---|");
    for (const [name, ms] of [...perTool.entries()].sort()) {
      const min = Math.min(...ms);
      const max = Math.max(...ms);
      const avg = ms.reduce((a, b) => a + b, 0) / ms.length;
      lines.push(`| ${name} | ${ms.length} | ${min.toFixed(0)} | ${avg.toFixed(0)} | ${max.toFixed(0)} |`);
    }
  }
  lines.push("");
  if (notes.length) {
    lines.push("## 运行日志");
    lines.push("");
    for (const n of notes) lines.push(`- ${n}`);
    lines.push("");
  }
  const reportPath = path.join(outDir, "REPORT.md");
  fs.writeFileSync(reportPath, lines.join("\n"), "utf8");
  console.log(`[benchmark] report written: ${reportPath}`);
}
