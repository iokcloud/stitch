# 用例晋升：probe → smoke / rich

借鉴 handover→regression：新场景先**探针**，稳定后再进默认门禁。

## 目录约定（不拆大改目录名）

| 阶段 | Layer A（Playwright） | Layer B / 真机（WDIO） |
| ---- | --------------------- | ---------------------- |
| **probe（临时）** | `frontend/e2e/browser/*-probe.spec.ts` 或 `agent-ux` 内 `test.skip` 关掉的草稿 | `e2e/specs/*-probe.spec.ts`；`package.json` 单独 script，**不**进 `smoke` |
| **smoke（常驻）** | `smoke.spec.ts` / `settings` / `agent-ux` / `chat-core`…；由 `smoke-ui.sh` 跑全量 | `desktop-smoke.spec.ts`；由 `smoke-desktop.sh` 跑 |
| **rich（加长）** | — | `agent-rich` · `chat-core-human` · `coding-workdir` · `run-command-no-console` |

命名：`{topic}-probe.spec.ts`（如 `confirm-inline-probe.spec.ts`）。票号可选前缀 `S009-`。

## 何时 probe

- 新交互（内联确认、无窗口命令、计划失败汇总）  
- 依赖真 LLM / 真文件系统、耗时长  
- 断言策略未定（怕 flake）

## 晋升清单（probe → 常驻）

- [ ] 本地连续绿 ≥ 2 次（同机）  
- [ ] 选择器以 `data-testid` / role 为主，无脆弱纯文案  
- [ ] mock（A）覆盖新 IPC；真机 script 写清 `STITCH_APP_BINARY`  
- [ ] 失败信息可定位（REPORT / 截图路径）  
- [ ] 挂入 `smoke-ui` / `smoke-desktop` 或 `package.json` 的加长 script  
- [ ] 本 Skill「When to run which」补一行  
- [ ] 删掉或改名去掉 `-probe`

## 降级 / 删除

长期红且判定为环境/产品未就绪 → 标 `test.skip` + STATUS 缺口，或移回 probe；禁止默默删断言装绿。

## 写新用例要点（摘要）

1. 交互走 helpers（`chat-desktop` / `mock-tauri`），spec 保持短  
2. 稳定页必 `assertUiHygiene` / `findVisibleUiLeak`  
3. 截图只进 `e2e/artifacts/<suite>/`  
4. 用户可见断言文案无 emoji（ADR-025）  
5. 脆等待：优先条件等待；短 `expect.poll` / `toPass` 可；长 `sleep` 禁止当主路径  
