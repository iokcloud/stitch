# 交付验收门禁（Verify gate）

改完代码后、宣称完成前，按下表填齐并真正执行。缺任一项 = 未交付。

## 变更 → 必跑层

| 变更面 | A | B | V | 真机加长（按需） |
| ------ | - | - | - | ---------------- |
| 仅 `frontend/` 组件（无壳） | 必 | **必** | 聊天壳/工具卡/确认卡/clamp 时必 | — |
| `app.css` / `app.html` / splash / `#app-root` | 必 | **必** | 必 | — |
| Rust IPC / tray / `tauri.conf` / `stitch` 工具 | 必 | **必** | 壳相关再加 | `run-command-no-console` 等 |
| 确认流 / 计划流 / 工作目录落盘 | 必 | **必** | 确认卡/计划卡 UI 时必 | `agent-rich` / `coding-workdir` / `chat-core-human` |
| 更新 UI / `check_update` / 发版清单 | 必（含 U1 mock） | **必** | — | `updater` 层 U0；发版前 U1 脚本 + U2 半自动（见 `updater-upgrade.md`） |
| 用户贴截图报 UI | 按定位补 | 按定位补 | **必**（读用户图 + 复现图） | 按需 |

**硬规则（2026-07-27）**：凡 Stitch Desktop 功能改动，交付默认 **`accept.sh --layers A,B`**。禁止只跑 Layer A 声称完成；勿等用户提醒再跑真机。

## 自动跑（首选）

```bash
cd rust/crates/stitch-desktop && sh scripts/accept.sh --layers A,B        # 默认交付（强制）
cd rust/crates/stitch-desktop && sh scripts/accept.sh --layers A,B,mature # +成熟 SCENE
cd rust/crates/stitch-desktop && sh scripts/accept.sh --layers A,B,updater # +更新 U0
# U1 本地假清单（勿改生产）: sh scripts/smoke-updater-discover.sh
# 仅调试 mock UI 时可临时 --layers A，但交差前仍须补 B
```

产物：`e2e/artifacts/ACCEPTANCE-REPORT.md`。Agent 把其中「验收块」贴进回复；需 V 时 Read 报告列出的 PNG。

## 交差前勾选（粘贴到回复）

优先用 `accept.sh` 打印的块；手填时用：

```
验收:
- [ ] 层: A | A+B | A+B+V | +加长名
- [ ] 命令退出码 0（粘贴关键一行 passed / Spec Files）
- [ ] hygiene 绿（无 __sveltekit_ / 可见 script）
- [ ] Layer V: 已 Read 的截图路径（若要求）
- [ ] 产物: 已 clean-e2e-artifacts（测后/读图后）
- [ ] STATUS「当前焦点」已更新
```

## 禁止

- 只 `cargo check` / 只改 mock 用例就说桌面验过  
- **只跑 Layer A 就交差**（缺真机 B = 未交付）  
- Layer A 绿就声称「真机 OK」  
- 有截图目录却不 Read 图片做 V  
- 把大 PNG 提交进 git 

## 与交接

长对话/压缩前：门禁未勾完不得写「已完成」进 CONTINUE；缺口写进 STATUS「下一项」。
