/**
 * Local recommended (light) scenes — welcome + library「推荐场景».
 * Official mature scenes live in mature-scenes.ts (library only; fill composer).
 */

export type RecommendedScene = {
  id: string;
  title: string;
  /** One-line list blurb (library); welcome may still show prompt. */
  summary: string;
  prompt: string;
};

export const RECOMMENDED_SCENES: RecommendedScene[] = [
  {
    id: "structure",
    title: "了解项目结构",
    summary: "顶层结构与入口，三句话说明",
    prompt: "先列出当前工作目录的顶层结构，用三句话说明这是什么项目、主要入口在哪。",
  },
  {
    id: "explain",
    title: "解释关键代码",
    summary: "找核心入口，说明启动与模块",
    prompt: "找出本仓库最核心的入口文件，简要说明它如何启动、关键模块各做什么。",
  },
  {
    id: "fix",
    title: "修检查警告",
    summary: "修编译或静态检查警告",
    prompt: "在工作目录内检查并修复明显的编译或静态检查警告；改动前先说明计划，危险操作先确认。",
  },
  {
    id: "review",
    title: "快速代码审查",
    summary: "风险、可读性、可测性按严重度列",
    prompt: "对最近改动或我指定的文件做一次快速审查：风险、可读性、可测性，按严重程度列出。",
  },
  {
    id: "breakdown",
    title: "拆解一个改动",
    summary: "把改动拆成可执行步骤",
    prompt: "把下面要做的改动拆成可执行步骤（先计划再动手）。改动目标：",
  },
];
