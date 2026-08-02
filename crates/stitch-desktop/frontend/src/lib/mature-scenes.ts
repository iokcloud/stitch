/**
 * Official mature scenes — productized prompts from tmp/stitch-mature-scenes/.
 * Shown only in the library「精选」sub-tab. Not welcome-area light hints (scenes.ts).
 */

/** free_hook = always open; paid_pool = G1 soft tip / G2 hard gate target. */
export type MatureTier = "free_hook" | "paid_pool";

export type MatureScene = {
  id: string;
  title: string;
  /** One-line list blurb; full prompt goes into the composer. */
  summary: string;
  tier: MatureTier;
  /** Primary report / state files the scene must leave in workdir. */
  deliverables: string[];
  /** Allowed verdict tokens in the report (empty = n/a). */
  verdicts: string[];
  prompt: string;
};

export const MATURE_SCENES: MatureScene[] = [
  {
    id: "debug-recover-auto",
    title: "改崩后停线复原",
    summary: "复现失败、最小修复、写出复原报告",
    tier: "free_hook",
    deliverables: ["stitch-debug-recover-report.md"],
    verdicts: [],
    prompt: `按「改崩后停线复原」跑完，少问我。约束：
1) 先停线：不新增功能；三句话写清症状与范围。
2) 在当前工作目录复现失败（编译/测试/静态检查，选最贴症状的一条）；摘录关键错误。
3) 给 1～3 条假设并选最可能的一条做最小修复；危险命令先确认；不顺手重构无关文件。
4) 用同一命令再验证。
5) 在工作目录写入 stitch-debug-recover-report.md：症状、复现命令、根因、改动文件列表、验证结果、若仍失败则停止建议。
若无法复现：写明已检查什么，仍输出报告后结束。`,
  },
  {
    id: "checkpoint-resume",
    title: "长任务检查点续跑",
    summary: "读写检查点，从安全点接着做并写报告",
    tier: "paid_pool",
    deliverables: ["stitch-checkpoint.json", "stitch-checkpoint-report.md"],
    verdicts: [],
    prompt: `按「长任务检查点续跑」执行。约束：
1) 在工作目录读写 stitch-checkpoint.json（没有则按我的目标拆 3～6 步并初始化；有则续跑，跳过 status=done）。
2) 预算 max_steps 默认 8；每完成或失败一步立刻回写 JSON；used_steps 每步 +1。
3) 一次会话尽量推进，但触达预算或某步 failed 就停，不要死循环重试超过 2 次。
4) 结束时写 stitch-checkpoint-report.md：任务、已完成步、失败步、产物路径、用户再说「继续检查点任务」时应发生什么。
5) 危险命令先确认；不改工作目录外文件。
我的目标：（在此写清要完成的事）`,
  },
  {
    id: "merge-ready-auto",
    title: "合并前审查自动",
    summary: "四轴审查、轻量检查、写出能否合并结论",
    tier: "paid_pool",
    deliverables: ["stitch-merge-ready-report.md"],
    verdicts: ["MERGE_OK", "MERGE_BLOCK"],
    prompt: `按「合并前审查自动」跑完，少问我。约束：
1) 只审当前工作目录；先列审查范围（文件/目录），不改目录外文件。
2) 跑一条最贴合的轻量检查（测试/静态检查/仓库脚本）；摘录关键结果；没有则写明未执行原因。
3) 按四轴写发现：正确性、安全/密钥、可测性、破坏性/回滚；标严重度。
4) 默认不改业务代码；若发现硬编码密钥或明显会坏的逻辑，结论必须 MERGE_BLOCK。
5) 在工作目录写入 stitch-merge-ready-report.md，须含：范围、检查命令与结果、四轴发现、结论（仅 MERGE_OK 或 MERGE_BLOCK）、合并前必做 1～3 条。`,
  },
  {
    id: "scope-lock-audit",
    title: "工作区硬边界巡检",
    summary: "查越界路径与危险命令，写出 PASS/FAIL 报告",
    tier: "paid_pool",
    deliverables: ["stitch-scope-lock-report.md"],
    verdicts: ["SCOPE_PASS", "SCOPE_FAIL"],
    prompt: `按「工作区硬边界巡检」跑完，少问我。约束：
1) 以当前工作目录为唯一允许范围；先写出绝对路径，并声明不得读写目录外。
2) 检查近期改动、脚本、配置中的路径：是否出现 ..、盘符绝对路径、家目录、云盘/文档等越界迹象。
3) 若发现对目录外的删除或全盘遍历倾向，标严重度；默认不改业务代码。
4) 在工作目录写入 stitch-scope-lock-report.md，须含：边界路径、发现列表、结论（仅 SCOPE_PASS 或 SCOPE_FAIL）、建议 1～3 条。
5) 文案无 emoji；结论词只用 SCOPE_PASS / SCOPE_FAIL。`,
  },
];

/** Match a filled composer / sent user message to an official mature scene. */
export function matchMatureScene(userText: string): MatureScene | null {
  const t = userText.trim();
  if (!t) return null;
  for (const scene of MATURE_SCENES) {
    if (t.includes(`「${scene.title}」`) || t.includes(scene.title)) return scene;
    if (scene.deliverables.some((d) => t.includes(d))) return scene;
  }
  return null;
}

/** Resolve mature scene from a sediment title (exact). */
export function matureSceneByTitle(title: string): MatureScene | null {
  const t = title.trim();
  if (!t) return null;
  return MATURE_SCENES.find((s) => s.title === t) ?? null;
}

/** Rewrite legacy playbook paths so old sessions stay readable. */
export function normalizeSedimentPlaybook(content: string): string {
  return content
    .replace(/在入口打开本场景，核对目标后发送。/g, "在「场景」→「精选」打开本项，核对目标后发送。")
    .replace(/在「场景」打开本项，核对目标后发送。/g, "在「场景」→「精选」打开本项，核对目标后发送。")
    .replace(/在入口打开本项，核对目标后发送。/g, "在「场景」→「精选」打开本项，核对目标后发送。");
}

function extractVerdict(scene: MatureScene, assistantText: string): string {
  for (const v of scene.verdicts) {
    if (assistantText.includes(v)) return v;
  }
  return "";
}

/**
 * Short reusable playbook for sediment — not a dump of the full chat.
 */
export function buildMatureSediment(
  scene: MatureScene,
  assistantText: string,
): { title: string; content: string } {
  const verdict = extractVerdict(scene, assistantText);
  const summaryLine = assistantText
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 280);
  const lines = [
    `# ${scene.title}`,
    scene.summary,
    "",
    "## 交付物",
    ...scene.deliverables.map((d) => `- ${d}`),
    "",
    "## 下次怎么跑",
    "1. 选好工作目录。",
    "2. 在「场景」→「精选」打开本项，核对目标后发送。",
    "3. 按工作区报告结论处理。",
  ];
  if (verdict) {
    lines.push("", "## 本次结论", verdict);
  } else if (summaryLine.length >= 40) {
    // Keep preview short; omit long free-form assistant echoes
    lines.push("", "## 本次摘要", summaryLine.slice(0, 160));
  }
  return {
    title: scene.title,
    content: lines.join("\n").slice(0, 5000),
  };
}
