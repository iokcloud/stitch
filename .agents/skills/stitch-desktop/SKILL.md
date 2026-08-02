---
name: stitch-desktop
description: Develop the Stitch Tauri v2 desktop client (rust/crates/stitch-desktop). Use when adding features, fixing bugs, or modifying the desktop app's UI, IPC commands, or build configuration.
---

# Stitch Desktop Development

**Doc hub（先读）**：`rust/crates/stitch-desktop/README.md`（架构 · 构建 · IPC · 冒烟）· `DESIGN.md`（视觉/布局薄规范，含聊天壳）· ADR-029。

**视觉怎么协作**：桌面 UI **不要**套用网站 `.cursor/skills/ui-design`（那是 Leptos + `design-system/`）。改聊天/设置观感时读并更新本 crate 的 `DESIGN.md`，实现落在 `frontend/src/app.css` + 组件；交付跑 smoke Skill 的 Layer A/B/V。

## Target architecture (ADR-029)

| Layer | Choice |
| ----- | ------ |
| Shell | Tauri v2 |
| UI | **SvelteKit 2 + Svelte 5 + TypeScript + Tailwind** |
| Static export | `@sveltejs/adapter-static` **SSG** (`prerender = true`, `ssr = false`, no SPA `fallback`) |
| Domain | `stitch` crate; desktop is IPC + chrome |
| npm | Only under `stitch-desktop/frontend/` |
| Design | Desktop-only `DESIGN.md` — do **not** use website `design-system/` |

Official refs:

- https://v2.tauri.app/start/frontend/sveltekit/
- https://svelte.dev/docs/kit/adapter-static

## Layout

```
stitch-desktop/
├── src/                 # Rust: main, commands, tray, splash_win
├── frontend/            # SvelteKit → builds to frontend/build
│   ├── src/routes/      # +layout.ts (SSG flags) · +page.svelte (shell)
│   ├── src/lib/         # ipc, stores, components
│   ├── static/          # fonts, icons
│   └── build/           # adapter-static output → Tauri frontendDist
├── icons/
├── scripts/build-ui.sh
└── tauri.conf.json      # frontendDist: ./frontend/build · devUrl :5173
```

## Startup (KEEP)

Win32 splash → CSS `#app-loader` in `app.html` → double rAF → `finish_startup` → cross-fade. Rust safety timeout + `custom-protocol` (crate default feature). **No Vite server needed** for the `.exe`.

## Build（收工必做）

**桌面端功能做完 / 修完后，Agent 必须编译，不要只 `cargo check` 就交差。**

```sh
unset TAURI_CONFIG   # 避免残留 e2e webdriver 配置污染默认构建
cd rust/crates/stitch-desktop && sh scripts/build-ui.sh
cd ../../ && cargo build -p stitch-desktop
# → rust/target/debug/stitch-desktop.exe
```

改了 `frontend/` 必须先 `build-ui.sh`；只改 Rust 可跳过 UI 构建，但仍须 `cargo build -p stitch-desktop`。

If「localhost 拒绝连接」: rebuild with crate default `custom-protocol` (`tauri/custom-protocol`); do not expect a web server. Do **not** “fix” by changing `tauri-build` `features = []` — that build-dep is unrelated (see README · RUST-DEVELOPMENT-GUIDE §2.4).

## SSG vs SPA note

Tauri docs also show SPA mode (`fallback: 'index.html'`). Stitch uses **SSG** (no fallback, `prerender = true`) per product choice. Keep Tauri IPC in `onMount` / client handlers — not in `load` during build.

## Smoke / 验收（必跑，禁止只改不测）

分层门禁见 **`.agents/skills/stitch-desktop-smoke/SKILL.md`**（含 `references/acceptance-gate.md` · 失败分流 · probe 晋升）。  
改完**先**跑自动验收（写报告 + 退出码），再向用户交差：

```bash
sh scripts/accept.sh --layers A,B            # 默认强制：A + 真机 exe B
sh scripts/accept.sh --layers A,B,mature     # +成熟 SCENE 真机
# 等价底层：smoke-ui.sh / smoke-desktop.sh；加长 npm run …
# Layer V：Read 报告列出的 PNG；读完再 sh scripts/clean-e2e-artifacts.sh
```

**禁止只跑 A 交差**（默认含真机 B，勿等用户提醒）。  
**禁止**用外部 GitHub/`npx` 泛用 QA Skill 包替代本仓 harness。

截图在 `e2e/artifacts/` 与 `frontend/e2e/artifacts/`（gitignore）。测完 / 读图后再 clean。  
聊天壳 / 确认卡 / 工具卡 / clamp 改完须 A + B + V。

## 测中顺手提质（Layer V / 真机时）

跑验收或读图时，若发现下列问题**顺手修**（最小改动），并回写 `DESIGN.md` 一两句，勿只记在 Chat：

| 类 | 看什么 | 常见落点 |
| -- | ------ | -------- |
| 乱码 / 泄漏 | `__sveltekit_`、半截源码、方框字；`run_command` stderr 菱形问号 | `app.css` · hygiene；控制台解码见 `stitch` `cmd.rs`（UTF-8→GBK） |
| 主题 | 默认应跟随系统，勿写死 light | `theme.ts` · `ThemePreference` |
| 文案 | 说明书腔、与真实行为矛盾（如软提示写「不能用」但可发送） | 组件文案 · ADR-025 |
| 设计 | 促销感横幅、侧栏塞全文 prompt、双层边框 composer | `DESIGN.md` · `app.css` |
| 逻辑 | 无 Token 仍露出套件 ID 栏；沉淀 dump 整段官方 prompt | `LibraryPanel` · `buildMatureSediment` |

成熟场景：`tmp/stitch-mature-scenes/FRICTION.md` 记摩擦；产品副本在 `mature-scenes.ts`。  
验收细节与环境坑见 **stitch-desktop-smoke** Skill。
