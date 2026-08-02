# Stitch Desktop

PromptStdio 桌面 Agent 客户端（Tauri v2）。发现在 Web、执行在桌面。

| 文档 | 用途 |
| ---- | ---- |
| [ADR-029](../../../docs/DECISIONS.md) | 栈与分层决策（已拍板） |
| [DESIGN.md](./DESIGN.md) | 桌面视觉 / 布局薄规范（独立于网站 design-system） |
| [验收清单](../../../docs/STITCH-DESKTOP-ACCEPTANCE.md) | Package 人工点验 |
| Skill：开发 | `.agents/skills/stitch-desktop/SKILL.md` |
| Skill：冒烟 | `.agents/skills/stitch-desktop-smoke/SKILL.md` |
| Skill：启动 | `.agents/skills/tauri-splash/SKILL.md` |

## 架构（ADR-029）

| 层 | 落点 | 职责 |
| -- | ---- | ---- |
| Domain | `rust/crates/stitch` | Agent、工具、MCP、Plan、LLM |
| Shell | `stitch-desktop/src` | 窗口、托盘、splash、单实例、IPC commands |
| UI | `stitch-desktop/frontend` | SvelteKit 展示与交互；经 IPC 调壳 |

| 选型 | 结论 |
| ---- | ---- |
| 壳 | Tauri v2 |
| UI | SvelteKit 2 + Svelte 5 + TypeScript + Tailwind |
| 静态导出 | `@sveltejs/adapter-static` **SSG**：`prerender = true` · `ssr = false` · **无** SPA `fallback` |
| 产物 | `frontend/build` → `tauri.conf.json` `frontendDist` |
| npm | **仅** `frontend/`（主站 Leptos 仍无 npm） |
| 运行时 | 本 crate 默认 feature `custom-protocol` → `tauri/custom-protocol`；`.exe` 不依赖 Vite |
| build-dep | `tauri-build` **允许** `features = []`（与运行时无关；见下） |

**`tauri` ≠ `tauri-build`**：`custom-protocol` 挂在运行时 `tauri`（经本 crate `[features]`）。`tauri-build` 的 `features = []` 只影响 JSON5/TOML 等编译期选项，**不会**关掉嵌前端协议。规范见 `rust/docs/RUST-DEVELOPMENT-GUIDE.md` §2.4。若 exe 报「localhost 拒绝连接」，先查是否弄丢了 crate 默认 `custom-protocol`，不要去「修」`tauri-build`。

**禁止**：在 Svelte 里绕过 IPC 写领域逻辑；在主站或 crate 根再装 Node；把桌面 UI 塞回网站 `design-system/`；改启动链路却不跑 Layer B。

排障诊断条默认隐藏（仅错误时显示）。需要常开：DevTools / 控制台执行 `localStorage.setItem('stitch-diag','1')` 后刷新。

```
stitch-desktop/
├── src/                 # Rust 壳：main · commands · tray · platform · splash
├── frontend/            # SvelteKit → frontend/build
│   ├── src/lib/         # ipc.ts · types.ts · stores · components
│   ├── e2e/             # Layer A Playwright（mock IPC）
│   └── build/           # 嵌入 Tauri（勿手改）
├── e2e/                 # Layer B WebdriverIO（真 exe）
├── scripts/             # build-ui · smoke-ui · smoke-desktop · clean-e2e-artifacts
├── DESIGN.md
└── tauri.conf.json
```

## 启动链路（KEEP）

Win32 splash → `app.html` 内 `#app-loader` → 双 `rAF` → `finish_startup` → 交叉淡入。  
Rust 侧有安全超时。改 splash / 可见性 / hit-test **必须**跑 Layer B。

## 构建

收工须产出 exe，不要只 `cargo check`：

```sh
unset TAURI_CONFIG   # 避免 e2e webdriver 配置污染默认构建
cd rust/crates/stitch-desktop && sh scripts/build-ui.sh
cd ../../ && cargo build -p stitch-desktop
# → rust/target/debug/stitch-desktop.exe
```

- 改了 `frontend/` → 必须先 `build-ui.sh`（会 touch `build.rs`，避免 exe 嵌旧包）
- 只改 Rust → 可跳过 UI，但仍须 `cargo build -p stitch-desktop`
- 若 exe 报「localhost 拒绝连接」：用默认 `custom-protocol` 重建，不要指望本地起 Vite

NSIS 安装包：`cargo tauri build` → `rust/target/release/bundle/nsis/`

## 冒烟测哪层

| 变更 | 跑 |
| ---- | -- |
| 仅 `frontend/src/**` 组件 | `sh scripts/smoke-ui.sh`（Layer A）；聊天壳再加 Layer V |
| `app.html` · 全局 CSS · splash · 主壳 · 长文折叠 | A + B + **Layer V**（Agent 读整窗截图，见 smoke Skill） |
| `src/**/*.rs` · tray · `tauri.conf.json` · IPC 名 | A + `sh scripts/smoke-desktop.sh`（Layer B） |
| 真机多场景（多行/长文/写文件） | webdriver exe 后 `cd e2e && npm run agent-rich` |
| 发版 / Package 交付 | `bash scripts/e2e-stitch-desktop-delivery.sh`（仓库根） |

**改完必须跑上表对应命令，禁止只改代码交差。**  
截图：`sh scripts/clean-e2e-artifacts.sh`（冒烟 / `e2e` npm 入口会自动跑）默认清掉 **1 小时前** 的截图与临时报告。

新 UI 须加 `data-testid`；新 `invoke` 须同步 `frontend/e2e/helpers/mock-tauri.ts` + Layer A。  
导航与启动路径须在 Layer B 断言（Chromium mock 抓不住 WebView2 问题）。  
可见层泄漏（如 `__sveltekit_` 源码上屏）用 `e2e/helpers/ui-hygiene.ts`（A/B 各一份，模式须同步）——仅靠 `data-testid` 抓不到（S-008）。

## IPC 契约

前端薄封装：`frontend/src/lib/ipc.ts` · 类型：`frontend/src/lib/types.ts` · 实现：`src/commands.rs`。  
**新增命令 = 三处同步 + mock + Layer A**（涉及启动/导航再加 Layer B）。

### Commands

| Command | 说明 |
| ------- | ---- |
| `get_config` / `save_config` | 配置快照读写（含 `llm_profiles` · `active_profile_id`） |
| `upsert_llm_profile` / `delete_llm_profile` / `set_active_llm_profile` | 多基座配置 CRUD / 设默认 |
| `test_connection` | LLM 连通测试（可带 `profile_id` / 表单 override） |
| `send_message` | 发消息（history · planMode · 可选 `profile_id`/`model`）→ 流式 `agent-event` |
| `cancel_generation` | 停止生成 |
| `respond_confirmation` | 危险工具确认 |
| `respond_plan` | Plan 批准 / 拒绝 |
| `list_suites` / `list_agents` | L1 场景侧栏列表 |
| `create_prompt` | 会话沉淀：保存个人提示词（需账号 Token） |
| `track_usage` | 商业观测埋点（需 Token；无 Token 静默跳过）见 `docs/OBSERVABILITY-STITCH.md` |
| `run_suite` / `run_agent` | 跑套件 / 智能体；套件中途失败 emit 可读汇总（已完成 / 失败原因 / 未执行） |
| `get_work_dir` / `set_work_dir` / `browse_work_dir` / `open_folder_path` | 工作目录 · 选夹 · 打开文件夹 |
| `set_titlebar_theme` | 标题栏明暗 |
| `finish_startup` | 结束 splash / 揭窗 |
| `clear_taskbar_progress` | 清除任务栏进度 |
| `check_update` / `install_update` | 更新（全链路见 `docs/UPDATER.md`） |
| `list_skills` | 官方 Skill 目录（场景侧栏） |
| `frontend_log` | 前端 → Rust 诊断日志 |

### Events

| Event | Payload |
| ----- | ------- |
| `agent-event` | `AgentEvent`（token · tool_* · done · confirm_request · cancelled · error · plan_*） |

## 密钥（勿提交）

- 本机配置：`%APPDATA%\promptstdio\stitch\config.toml`
- 脚本：`scripts/local-stitch-secrets.path` · `scripts/provision-stitch-prod-token.sh`

## 与网站分工

| | Web（Leptos） | Desktop（本 crate） |
| - | ------------- | ------------------- |
| 发现 / 会员 / Explore | 是 | 否 |
| Agent 执行 / 本地工具 / Plan | 否 | 是 |
| 设计系统 | `design-system/` · `promptstdio-web` | 本目录 `DESIGN.md` + `frontend/src/app.css` |
