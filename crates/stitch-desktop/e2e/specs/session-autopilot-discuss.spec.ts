/**
 * Design discussion with Stitch (debug exe): Session Autopilot → consensus.
 * Artifacts → e2e/artifacts/session-autopilot-discuss/
 */
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  bootChat,
  newSession,
  sendChat,
  setPlanMode,
  setWorkDir,
  shot,
} from "../helpers/chat-desktop";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const outDir = path.resolve(__dirname, "../artifacts/session-autopilot-discuss");
const repoRoot = path.resolve(__dirname, "../../../../../");

async function dumpChat(): Promise<{
  lastAssistant: string;
  allAssistants: string[];
  logText: string;
  api400: string | null;
}> {
  return browser.execute(() => {
    const log = document.querySelector('[role="log"]');
    const assistants = [...(log?.querySelectorAll(".msg-assistant") ?? [])].map(
      (el) => el.textContent?.trim() ?? "",
    );
    const lastAssistant = assistants.at(-1) ?? "";
    const logText = (log?.textContent || "").trim();
    let api400: string | null = null;
    if (/API error 400/i.test(logText)) {
      const m = logText.match(/API error 400[\s\S]{0,240}/i);
      api400 = m?.[0]?.replace(/\s+/g, " ").trim() ?? "API error 400";
    }
    return { lastAssistant, allAssistants: assistants, logText, api400 };
  });
}

function writeRound(n: number, prompt: string, assistant: string) {
  fs.writeFileSync(path.join(outDir, `round-${n}-prompt.txt`), prompt, "utf8");
  fs.writeFileSync(path.join(outDir, `round-${n}-assistant.txt`), assistant, "utf8");
}

const ROUND1 = `我们要为 Stitch 定稿「单会话变长 · Session Autopilot」方案，目标是全自动无人干预闭环（危险工具确认除外）。请审查并挑刺，指出还缺什么才能叫「完美自主」。先读现有压缩实现再评：rust/crates/stitch/src/agent/context.rs、rust/crates/stitch-desktop/src/commands.rs（AgentSessionStore）、rust/crates/stitch-desktop/frontend/src/lib/stores/sessions.ts。

当前草案（已吸收一轮工程审查）：

A. 权威历史 vs UI 投影分离
- Agent Session（含 tool_calls）为本机权威，按 session_id 落盘；UI localStorage 只存投影。
- 对外会话 id / 标题 / 工作区绑定守恒；禁止静默拆新侧栏会话。

B. Committed Epoch（预压缩竞态）
- 预压缩只产候选 checkpoint-vN+1，不覆盖 vN；fsync 后原子切换 committed 指针。
- 同 session 单写者；用户发送优先：作废基于过期世代的预压缩。
- 预压缩默认纯计算，不改正在跑的 ReAct 内存 Session；仅空闲/回合间隙 commit。

C. 压缩世代
- ~70% 预压，~85% 硬压；Checkpoint 保留目标/决策/路径/未完成/最近结论。
- 再压只压「上次 checkpoint 之后」的明细；远古 checkpoint 合并，保留最近 2～3 代。

D. 自愈边界
- 协议级缺 tool result → 可 stub（DeepSeek 角色规则）。
- tool id 对不上 / 工具组损坏 → 不猜 id，丢弃该组，收成「用户原意 + 已丢弃工具链」写入摘要，本机记自愈日志（不弹窗）。
- JSON 不可解析 → 回退上一 committed；再不行降级纯文本 history。

E. Checkpoint.artifacts 强约束
- 沉淀草稿等产物进结构化 artifacts[]（local_id/title/content_hash；日后可挂 cloud id）。
- 硬压缩时 artifacts / 关键路径 / 未完成项不可丢。

F. 投影瘦身
- 工具输出权威可全文、投影截断；DOM 窗口化；大块外置文件；写失败自动降采样，无「请清理」弹窗。

G. 收敛（可选）
- 空闲/计划完成 → 静默预填沉淀候选；不自动提交云端；用户点「保存」即可。

请输出：
1) 同意 / 不同意（逐条 A–G）
2) 为实现「完美自主」还必须补的硬约束（若有）
3) 明确「不要做」的清单
4) 若已接近共识，给一版可写入 ADR 的终稿提纲（短）
文案中性、无 emoji。`;

const ROUND2 = `上一轮你的意见已收到。请在不推翻「会话 id 守恒 + Committed Epoch + 权威/投影分离」的前提下，把剩余分歧收敛成最终共识。

请特别裁定这几条（同意就写「采纳」，不同意就给替代条文，禁止含糊）：
1) 预压缩是否允许与 ReAct 并行计算（只禁止 commit 进内存）？还是必须整段串行？
2) Checkpoint 是否必须结构化（JSON 字段），还是允许纯自然语言摘要？
3) 投影外置失败时，是否允许静默丢弃最旧工具正文（权威仍保留）？
4) 沉淀候选预填是否进 v1 Autopilot，还是必须后置？
5) 重启后 Agent 落盘缺失时，降级到 historyForSend 是否可接受为唯一降级？

最后输出固定标题「## 共识终稿」，下列条目必须齐全且可直接当 ADR 正文：
- 目标与非目标
- 数据模型（权威 / 投影 / Epoch / Checkpoint）
- 运行时规则（发送优先、单写者、预压/硬压、自愈）
- 用户可见行为（零干预边界）
- 验收标准（真机可测）
- 分期（S0–S3）与明确不做项
不要再列开放问题；必须拍板。`;

describe("Session Autopilot design discussion (debug exe)", function () {
  this.timeout(900_000);

  before(async () => {
    fs.rmSync(outDir, { recursive: true, force: true });
    fs.mkdirSync(outDir, { recursive: true });
    await bootChat();
  });

  it("discuss until consensus dump", async () => {
    await setPlanMode(false);
    await newSession();

    const set = await setWorkDir(repoRoot);
    expect(set.applied.toLowerCase()).toMatch(/promptstdio/);

    await sendChat(ROUND1, 600_000);
    await shot(outDir, "01-round1");
    let dump = await dumpChat();
    expect(dump.api400).toBeNull();
    expect(dump.lastAssistant.length).toBeGreaterThan(120);
    writeRound(1, ROUND1, dump.lastAssistant);

    await sendChat(ROUND2, 600_000);
    await shot(outDir, "02-round2");
    dump = await dumpChat();
    expect(dump.api400).toBeNull();
    expect(dump.lastAssistant.length).toBeGreaterThan(120);
    writeRound(2, ROUND2, dump.lastAssistant);

    const report = [
      "# Session Autopilot discussion (debug exe)",
      "",
      `workdir: ${set.applied}`,
      `rounds: 2`,
      "",
      "## Round 1 assistant",
      "",
      fs.readFileSync(path.join(outDir, "round-1-assistant.txt"), "utf8"),
      "",
      "## Round 2 assistant (expect 共识终稿)",
      "",
      dump.lastAssistant,
      "",
    ].join("\n");
    fs.writeFileSync(path.join(outDir, "DISCUSSION.md"), report, "utf8");
    fs.writeFileSync(path.join(outDir, "consensus.txt"), dump.lastAssistant, "utf8");

    expect(/共识终稿/.test(dump.lastAssistant)).toBe(true);
  });
});
