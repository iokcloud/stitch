---
name: stitch-desktop-smoke
description: >-
  Run and extend Stitch Desktop layered acceptance: Playwright mock IPC (A),
  WebdriverIO real exe (B), Agent screenshot review (V), plus rich desktop
  scenarios. Use when the user asks for Stitch smoke, desktop E2E, UI regression,
  验收/门禁, after frontend/IPC/chat chrome changes, to add/promote probe specs,
  or before claiming any Stitch Desktop change done (must run scripts/accept.sh).
---

# Stitch Desktop Smoke（验收门禁）

**禁止**引入外部 GitHub / `npx` 验收 Skill 包（含泛用 QA Skills）。本仓已有分层 harness；
模式对齐 Cursor 官方 `create-verification-skill`（drive → evidence → PASS/FAIL），实现落在本 crate。

## 自动验收（Agent 强制）

改完 Stitch 相关代码、**向用户宣称完成前**，必须跑：

```bash
# 默认交付门禁（强制）：Playwright A + 真机 exe B
cd rust/crates/stitch-desktop && sh scripts/accept.sh --layers A,B
# 成熟 SCENE：   sh scripts/accept.sh --layers A,B,mature
# 更新 U0：      sh scripts/accept.sh --layers A,B,updater
# 更新发现 U1：  sh scripts/smoke-updater-discover.sh   # 本地假清单，勿改生产
```

- **禁止只跑 A 就交差**（用户要求：真机 B 默认必跑，勿等提醒）。  
- 脚本写 `e2e/artifacts/ACCEPTANCE-REPORT.md`（gitignore 下），打印勾选块。  
- **退出码非 0 = 未交付**，禁止只口头勾选。  
- 报告里列出的 PNG → Agent **Read** 做 Layer V（场景侧栏/壳/聊天 chrome）。  
- 细则仍见 `references/acceptance-gate.md`。

三层证明不同问题。**日常交付默认 A+B**；壳与聊天观感加 **V**；加长/成熟/updater 按变更面追加。细则见 `references/`。

| Layer | Tool | 证明什么 | 命令 |
| ----- | ---- | -------- | ---- |
| **A · UI** | Playwright + mock IPC | Svelte 路由/组件 + **可见 hygiene** | `sh scripts/smoke-ui.sh` |
| **B · Desktop** | WDIO + Tauri webdriver | 真 `.exe` 启动/导航 + hygiene | `sh scripts/smoke-desktop.sh` |
| **V · Visual** | Agent **Read 截图** | 无泄漏、无塌陷、无挡点击、无盖字 | 无单独脚本；读 `e2e/artifacts/**` |
| **加长** | WDIO + DeepSeek | 多轮/长文/落盘/确认/无 cmd 窗 | `cd e2e && npm run …` |
| **成熟场景** | WDIO + DeepSeek | 官方成熟 SCENE 全自动交付 | `mature-debug-recover` · `mature-checkpoint-resume` · `mature-merge-ready` · `mature-scope-lock` |
| **Updater** | WDIO + 清单 | U0 生产最新 · U1 本地假清单发现更高版 · U2 安装半自动 | `accept.sh --layers updater` · `smoke-updater-discover.sh` · 见 `references/updater-upgrade.md` |

**硬规则：改完必须真跑对应层再交差。** 只改代码不测 = 未完成。  
**交差前**填 [acceptance-gate.md](references/acceptance-gate.md) 勾选块。  
失败先 [failure-triage.md](references/failure-triage.md)；新用例走 [handover-promote.md](references/handover-promote.md)。

Crate root: `rust/crates/stitch-desktop/`。Shell：**Git Bash**。

## Why A alone is not enough

| 漏检 | 例 | 谁抓 |
| ---- | -- | ---- |
| 节点在、壳脏 | `__sveltekit_` 画在顶栏（S-008） | A/B hygiene |
| Chromium OK、WebView2 坏 | splash / hit-test | **B** |
| 自动化绿、观感丑 | 盖字、空主区、确认弹窗挡视线 | **V** |

`data-testid` ≠ 用户看见干净 UI。

## When to run which（速查）

- `frontend/src/**` 组件 → **A**（聊天壳/确认卡/工具卡/clamp → +**V**）  
- `app.html` · 全局 CSS · splash · `#app-root` → **A+B+V**  
- Rust / tray / `tauri.conf` / `stitch` 工具 → **A+B**（壳相关 +V）  
- 确认/计划/落盘/命令窗 → A + 对应加长（`run-command-no-console` · `agent-rich` · …）  
- 用户贴 UI 图 → **V** 必读图  

完整表：`references/acceptance-gate.md`。

## Layout

```
stitch-desktop/
├── frontend/e2e/browser/     # Layer A specs（含 *-probe 草稿）
├── frontend/e2e/helpers/     # mock-tauri · ui-hygiene
├── e2e/specs/                # B + 加长（desktop-smoke · agent-rich · …）
├── e2e/helpers/              # chat-desktop · ui-hygiene（与 A 同步禁词）
└── scripts/smoke-ui.sh · smoke-desktop.sh · clean-e2e-artifacts.sh
```

## Artifact hygiene

产物：`e2e/artifacts/**` · `frontend/e2e/artifacts/**`（gitignore）。  
默认删 **mtime > 1h**；测前脚本会 clean；**测后 / Layer V 读完再 clean 一次**。禁提交大 PNG。

## Layer A — Playwright

```bash
cd rust/crates/stitch-desktop && sh scripts/smoke-ui.sh
```

- 需先有 `frontend` build（脚本内会 build）。  
- 默认系统 Chrome；`PLAYWRIGHT_CHROMIUM=1` 用自带。  
- 新 `invoke` → 扩 `mock-tauri.ts`。  
- 稳定页 → `assertUiHygiene`。  
- **禁止** Playwright 打真 `.exe`（那是 B）。

## Layer B — WebdriverIO

```bash
cd rust/crates/stitch-desktop && sh scripts/smoke-desktop.sh
```

- `cargo build -p stitch-desktop --features webdriver` + `e2e/tauri.webdriver.json`。  
- 端口默认 **17445**（避开 4445 残留）。  
- `STITCH_APP_BINARY` 可覆盖。结束：`findVisibleUiLeak() === null`。  
- 跑前清单实例：`stitch-desktop` / `msedgedriver`。  
- **默认不改主题**（主题回归见 `npm run theme-smoke`，须 `STITCH_THEME_SMOKE=1`）。  
- **默认测完不弹资源管理器**（需时 `STITCH_KEEP_DEBUG_DIR=1`）。

加长（需 webdriver 编过的 exe + Key）：

```bash
cd e2e
export STITCH_APP_BINARY=.../rust/target/debug/stitch-desktop.exe
export WDIO_EMBEDDED_PORT=17445
npm run agent-rich              # 多行 / 长输出 / 复杂写文件
npm run chat-core-human         # 停止 / 计划 / 切会话
npm run coding-workdir          # 落盘沙箱
npm run run-command-no-console  # S-009 无可见 cmd 窗
npm run updater-check           # U0 生产清单
# U1 发现更高版（勿改生产 JSON）：
#   sh ../scripts/smoke-updater-discover.sh
```

## Updater 全链路

见 [updater-upgrade.md](references/updater-upgrade.md)。摘要：

| 步 | 期望 | 自动化 |
| -- | ---- | ------ |
| U0 | 「已是最新版本」 | `accept.sh --layers updater` |
| U1 | 「发现新版本」+「安装更新」 | Layer A mock + `smoke-updater-discover.sh` |
| U2 | 签名安装并重启 | 半自动（发版后手验） |

**禁止**为测 U1 篡改生产 `stitch-update.json`。

## Layer V — 读图

壳/CSS/聊天 chrome 或用户贴图时必做：

1. 取整窗或 `e2e/artifacts/**/*.png`  
2. **Read 图片**（禁止只看 DOM）  
3. 对照：无 `__sveltekit_`、无 loader 挡屏、主区有内容、无叠层挡点击、长文/工具不盖字  
4. **顺手扫**：乱码/矛盾文案/喧宾横幅/侧栏全文 prompt/无 Token 却露出账号控件 → 能修则修（见 stitch-desktop Skill「测中顺手提质」）  
5. 读完 `sh scripts/clean-e2e-artifacts.sh`

## Mature 层前置

`accept.sh --layers mature` **不会**自动编 webdriver exe。跑前须：

```bash
# 清残留
taskkill //F //IM stitch-desktop.exe; taskkill //F //IM msedgedriver.exe
cd rust/crates/stitch-desktop && sh scripts/build-ui.sh
cd ../../ && TAURI_CONFIG="$(node -e "…读 e2e/tauri.webdriver.json…")" \
  cargo build -p stitch-desktop --features webdriver
# 或直接 smoke-desktop.sh（含上述 build）后再 npm run mature-*
```

端口 **17445** 超时 = 环境类（未编 webdriver / 残留 driver），见 `failure-triage.md`。

## 写新用例 / 晋升

见 `references/handover-promote.md`。摘要：

1. 先 `*-probe.spec.ts` + 独立 npm script，勿贸然进默认 smoke。  
2. 绿 ≥2 次再升入 `smoke-ui` / `smoke-desktop` / 加长 script。  
3. Prefer `data-testid`；双份 hygiene 同步禁词。  
4. 无 emoji 断言文案。

## Agent 执行清单

```
- [ ] 读 acceptance-gate：定层 A/B/V/加长
- [ ] Git Bash 跑 `sh scripts/accept.sh --layers …`（首选，勿只跑零散 npx）
- [ ] 报告 verdict=PASS；粘贴 ACCEPTANCE-REPORT 验收块
- [ ] 失败 → failure-triage 分类后再改
- [ ] 需 V → Read 报告列出的 PNG（mature-entry / theme-visual 等）
- [ ] clean-e2e-artifacts（读图后）
- [ ] 更新 docs/STATUS.md
- [ ] 禁止未跑 accept / 未贴报告就声称完成
- [ ] 禁止改用外部 GitHub QA Skill 包替代本门禁
```

## Related

- Dev: `.agents/skills/stitch-desktop/SKILL.md`  
- Hub: `rust/crates/stitch-desktop/README.md` · `DESIGN.md`  
- PITFALLS: S-008 · S-009  
- Splash: `.agents/skills/tauri-splash/SKILL.md`
