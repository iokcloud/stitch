# 失败分流（先分类再改码）

测试红了先判一类，避免乱改产品或乱改断言。

## 三类

| 类 | 信号 | 动作 |
| -- | ---- | ---- |
| **产品 bug** | 同步骤手工复现；截图/DOM 与预期不符；hygiene 抓到泄漏 | 修 `frontend/` 或 Rust；补/保留能钉住回归的用例 |
| **测试脆** | 偶发超时；依赖文案/动画；并行抢单实例；未等 IPC | 稳选择器（`data-testid`/`getByRole`）；`waitIdle`/`expect.poll`；加长合理 timeout；勿用固定 `sleep` 当主等待 |
| **环境** | 无 API Key；端口 17445/4445 被占；旧 exe 未重编；单实例锁；EdgeDriver 残留 | `taskkill` stitch/msedgedriver；`unset TAURI_CONFIG`；重跑 `build-ui` + `cargo build -p stitch-desktop --features webdriver`；检查 `STITCH_APP_BINARY` |

## 常见映射（Stitch）

| 现象 | 优先类 | 线索 |
| ---- | ------ | ---- |
| A 全绿、B 白屏/无 `view=chat` | 产品或环境 | splash / `finish_startup` / 旧 embed；看 diag |
| A 失败 `unhandled invoke` | 测试 | 补 `mock-tauri.ts` |
| `generation did not finish` | 环境或脆 | LLM/Key；计划模式未关；确认卡未点到 |
| `VISIBLE_CMD` / 黑窗 | 产品 | S-009 · `CREATE_NO_WINDOW` |
| `__sveltekit_` 可见 | 产品 | S-008 · hygiene |
| 确认卡找不到 / overlay | 产品或测试 | 内联 `confirm-card`；testid 是否过期 |
| WDIO `Binary Permissions` / sessionId | 环境 | 忽略噪音若最终 PASSED；反复失败则清 driver |
| Mature：Embedded WebDriver 17445 超时 | 环境 | 先 `build-ui` + `cargo build -p stitch-desktop --features webdriver`；清 stitch/msedgedriver |
| Playwright `getByText` strict：气泡与沉淀预览同字 | 测试 | 限定 `.msg-assistant .md-content`；或沉淀预览仅 playbook |
| **截图内容与源码矛盾**（面板文案/区块在 src 与 build 都搜不到） | 环境 | **4173 残留 preview server 喂旧包**：sirv 内存缓存旧 build，Playwright url 探测命中即复用。跑前 `netstat -ano \| grep 4173` 清干净；手动起的 preview 用完必须杀（S-014） |
| 新 testid 等待超时但 src 确有 | 环境 | **改动后忘了 `npm run build`**：Playwright webServer 吃的是 `build/` 产物不是 src。先构建再跑 spec；`accept.sh` / `smoke-ui.sh` 自带构建，直接 `npx playwright test` 不带 |
| 软提示文案像硬墙但发送仍可用 | 产品 | G1 须写「可先试用」类；禁「开通才能用」 |
| updater「尚未配置签名公钥」 | 配置 | `tauri.conf` pubkey |
| U1 仍「已是最新」 | 环境 | 假服务未起 / 未用 `tauri.updater-discover.json` 编 exe |
| U1 安装失败 signature | 预期 | 假清单只验发现；U2 用真签名包 |

## 调试顺序

1. 看失败是 **A / B / 加长** 哪一层  
2. 读 REPORT / Playwright error-context / 产物截图  
3. 归类 → 只改该类根因  
4. 同层复跑至绿，再补门禁要求的其它层  
5. 新失败模式写入本文件一行 + 必要时 PITFALLS `S-xxx`
