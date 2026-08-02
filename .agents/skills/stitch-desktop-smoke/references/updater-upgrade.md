# Updater 全链路验收（本仓）

> 证明「旧客户端能发现更高版本并（可选）安装重启」。  
> **禁止**用外部 GitHub / `npx` QA Skill 替代。细则与发版见 `rust/crates/stitch-desktop/docs/UPDATER.md`。

## 层定义

| 代号 | 证明什么 | 命令 / 做法 | 是否改生产清单 |
| ---- | -------- | ----------- | -------------- |
| **U0 · 同版本最新** | 公钥 + 生产端点可达；当前版 = 清单 →「已是最新版本」 | `cd e2e && npm run updater-check` 或 `accept.sh --layers updater` | 否 |
| **U1 · 发现更高版** | 清单 version > app →「发现新版本」+ 按钮变「安装更新」 | Layer A mock；真机 `sh scripts/smoke-updater-discover.sh`（本地假清单，**勿**改生产） | 否 |
| **U2 · 安装重启** | 签名包可下载安装并启动 NSIS（会话常断开） | `sh scripts/smoke-updater-u2.sh`（本地真 `.sig` + 伪装 0.1.1 客户端；**勿**改生产）+ 正式发版后旧包手装验收 | 仅正式发版时 |

日常改设置/更新文案：先 **Layer A（含发现新版本 mock）** + **U0**。  
正式发版前：再跑 **U1**（本地假清单）+ **U2**（`smoke-updater-u2.sh` 本地真签名；发版后再做旧装包手验）。

## 硬规则

1. **勿**为了测「发现新版本」把生产 `stitch-update.json` 改成高于已发客户端的假版本（会误伤真实用户）。  
2. U1 真机必须用 **本地 HTTP 假清单** + `e2e/tauri.updater-discover.json`（含 `dangerousInsecureTransportProtocol`）。测完勿把该 stub 编进 release。  
3. U2 假清单的 dummy `.sig` **不能**完成安装；安装须用 `publish-update-manifest.sh` 签过的真包。  
4. 私钥仅本机 `~/.stitch/updater.key`，禁止入库 / 上生产机。

## U0 — 生产「已是最新」

前置：debug/webdriver exe 与生产清单同 major（当前 `0.1.0`）；公钥已写入 `tauri.conf.json`。

```bash
cd rust/crates/stitch-desktop
sh scripts/build-ui.sh
# 若尚无 webdriver 二进制：
# cargo build -p stitch-desktop --features webdriver  （TAURI_CONFIG=e2e/tauri.webdriver.json）
cd e2e && npm run updater-check
```

期望：设置 → 通用 → 检查更新 → 页脚「已是最新版本」（若清单已更高则「发现新版本」亦可，证明端点通）。

## U1 — 发现新版本

### A · Playwright（默认进 smoke-ui）

`mockTauri({ updateAvailable: true })` → 检查更新 →「发现新版本」→ 按钮「安装更新」。

### B · 真机本地假清单

```bash
cd rust/crates/stitch-desktop
sh scripts/smoke-updater-discover.sh
```

脚本会：起 `127.0.0.1:18765` 静态目录 → 用 updater-discover 配置编 webdriver exe → 跑 `updater-discover` → 停服务。

期望：页脚「发现新版本」；按钮文案「安装更新」。  
**不要**在此步点安装（dummy 签名会失败）。

## U2 — 安装（本地真签名 + 可选发版手验）

### A · 本地真签名（不改生产）

前置：`rust/data/downloads/Stitch_0.1.2_x64-setup.exe` + `.sig`（已 `publish-update-manifest`）。

```bash
cd rust/crates/stitch-desktop
sh scripts/smoke-updater-u2.sh
```

脚本会：用 **0.1.2 真签名包** 生成本地清单（JSON `version` 标更高如 0.1.3，URL 仍指向该 `.exe`）→ `tauri.updater-u2.json` 指本地端点 → 检查更新 → **安装更新**。  
期望：页脚「发现新版本」后进入安装；Windows 上进程常因 NSIS 退出导致 WDIO session 断开——计为通过（签名校验 + 安装已启动）。  
**测完**须用 `tauri.webdriver.json` 重编 webdriver exe，勿把 U2 stub 留在日常冒烟二进制里。

### B · 发版后手验（生产）

1. 按 `docs/UPDATER.md` 签名构建更高 `version`，跑 `publish-update-manifest.sh`，同步生产 downloads（用户明确发版时）。  
2. 本机保留/重装 **旧** 安装包。  
3. 设置 → 通用 → 检查更新 →「发现新版本」→「安装更新」→ 进程重启 → 关于/版本号为新版。  
4. 记录 sha256 与清单 version 到 STATUS。

半自动核验（不改生产清单）：

```bash
cd rust/crates/stitch-desktop
sh scripts/smoke-updater-prod-verify.sh
# CURRENT → exit 0；BEHIND → 按 REPORT 手点升级后重跑；NEED_INSTALL → 先装包
```

自动化缺口：重启后 WDIO session 断开；本地 U2 覆盖「下载+验签+启动安装」，完整「重启后版本号」用上表手验 + `smoke-updater-prod-verify.sh`。

## accept.sh

```bash
sh scripts/accept.sh --layers updater     # = U0
sh scripts/accept.sh --layers A,updater   # UI + U0
```

U1：`sh scripts/smoke-updater-discover.sh`（独立，会临时改编译端点）。  
U2：`sh scripts/smoke-updater-u2.sh`（独立，本地真签名；测完重编 webdriver）。

## 失败分流

| 现象 | 类 | 动作 |
| ---- | -- | ---- |
| 「更新尚未配置签名公钥」 | 产品/配置 | 查 `tauri.conf` pubkey |
| 「无法连接更新服务」 | 环境 | 网络 / 生产 404 |
| U0 意外「发现新版本」 | 环境 | 清单已高于本机；确认是否刚发版 |
| U1 仍「已是最新」 | 环境 | 假服务未起；或编了生产端点 exe（未用 updater-discover） |
| U2 安装失败 signature | 预期（假清单）或产品（真包） | 假清单勿验 U2；真包查 `.sig` 与私钥配对 |

## 与交接

U0/U1 绿才可在 STATUS 写「updater 发现链验收」；U2 本地脚本绿写「本地真签名安装链」；生产「重启后版本号」未手验须写明。
