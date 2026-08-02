# 贡献指南

## 仓库结构

```
crates/
├── promptstdio-core/   # 共享领域模型（无服务端依赖）
├── stitch/             # Agent 核心：ReAct 循环、工具、MCP、计划、会话持久化
└── stitch-desktop/     # Tauri 桌面壳 + SvelteKit 前端 + e2e
scripts/                # 构建 / 冒烟 / 验收脚本
.agents/skills/         # 官方 Skill 预设
```

## 开发环境

见 [BUILDING.md](docs/BUILDING.md)。桌面设计规范见 `crates/stitch-desktop/DESIGN.md`。

## 开发流程

1. **改前端**（`crates/stitch-desktop/frontend/`）：先 `npm run build` 再跑测试（产物嵌入 exe，不构建会吃到旧包）
2. **改 Rust 壳 / IPC**：跑 `cargo test --workspace` + `bash scripts/smoke-ui.sh`（Layer A，mock IPC）
3. **改 UI/UX 后**：`bash scripts/accept.sh --layers A,B`（Layer A mock + Layer B 真 exe）
4. 提交信息用 conventional commits（`feat(stitch): ...` / `fix(stitch-desktop): ...`）

## 新增 IPC 命令

三处同步：`frontend/src/lib/ipc/ipc.ts`（类型）→ `frontend/src/lib/types.ts`（事件）→ `src/commands.rs`（Rust 实现），另加 mock（Layer A 测试注入）。

## 文案约定

用户可见文案遵守项目规范（`crates/stitch-desktop/DESIGN.md` 顶部）：

- 无 emoji，图标用 SVG
- 中性陈述：只写「做什么、用户得到什么」，不写对比式表述
- 通俗短句，能不写就不写

## 提交 PR

1. fork 仓库并创建分支
2. 通过上述测试
3. 描述改动与验证方式（附 Layer A 结果）
4. 维护者会在 3 个工作日内回复
