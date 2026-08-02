# Stitch 自动更新全链路

> 客户端已接 `tauri-plugin-updater`（设置 → 通用 → 检查更新）。

## 链路

```
发版构建 (签名私钥 + cargo tauri build)
  → NSIS + .sig
  → publish-update-manifest.sh → rust/data/downloads/
  → 同步 stitch-update.json + 安装包到生产 /downloads
  → 客户端 check_update → download_and_install → 重启
```

## 当前配置

| 项 | 值 |
| -- | -- |
| 公钥 | `tauri.conf.json` → `plugins.updater.pubkey`（已写入） |
| 端点 | 静态 JSON：`https://www.promptstdio.com/downloads/stitch-update.json` |
| 产物 | `bundle.createUpdaterArtifacts: true` → NSIS + `.sig` |
| 私钥 | 本机 `~/.stitch/updater.key`（**禁止入库**） |

> 曾规划子域 `updates.promptstdio.com`；现改挂官网已有 `/downloads`，免新 DNS/反代。

## 一次性：密钥（已完成于发版机）

```bash
mkdir -p ~/.stitch
cargo tauri signer generate -w ~/.stitch/updater.key -p "" --ci
# 公钥 → tauri.conf.json plugins.updater.pubkey（单行，无换行）
# 私钥 → 仅本机 / CI 环境变量，勿提交
```

备份私钥到离线安全位置。丢失则无法继续为已安装客户端签更新。

## 发版步骤

```bash
# 1) 如需发新版本：先改 tauri.conf.json version
# 2) UI
cd rust/crates/stitch-desktop && sh scripts/build-ui.sh

# 3) 签名构建（私钥路径或内容）
# 须用密钥*内容*（PATH 变量当前 CLI 不认）
export TAURI_SIGNING_PRIVATE_KEY="$(cat "$HOME/.stitch/updater.key")"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
unset TAURI_CONFIG
cd rust/crates/stitch-desktop
# 若 beforeBuild cwd 异常：先 build-ui，再清空 beforeBuildCommand
cargo tauri build --config '{"build":{"beforeBuildCommand":""}}'

# 4) 生成本地清单 + 复制到 data/downloads
cd crates/stitch-desktop && sh scripts/publish-update-manifest.sh

# 5) 同步生产（与换安装包相同路径；勿把私钥带上服务器）
#    rust/data/downloads/Stitch_*_x64-setup.exe
#    rust/data/downloads/Stitch_*_x64-setup.exe.sig
#    rust/data/downloads/stitch-update.json
```

清单格式（Tauri 静态 JSON）：

```json
{
  "version": "0.1.0",
  "notes": "",
  "pub_date": "2026-07-26T12:00:00Z",
  "platforms": {
    "windows-x86_64": {
      "signature": "<.sig 文件内容>",
      "url": "https://www.promptstdio.com/downloads/Stitch_0.1.0_x64-setup.exe"
    }
  }
}
```

## 验收

| 步骤 | 期望 |
| ---- | ---- |
| 已配公钥 + 清单 version ≤ 当前 | 「已是最新版本」 |
| 已配公钥 + 清单 version 更高 | 显示新版本，二次点击安装并重启 |
| 清单 404 / 网络失败 | 友好错误，不崩溃 |

自动化门禁（本仓 smoke，**勿**引入外部 QA Skill）：

| 代号 | 命令 | 说明 |
| ---- | ---- | ---- |
| U0 | `accept.sh --layers updater` / `npm run updater-check` | 打生产清单 |
| U1 | Layer A mock + `sh scripts/smoke-updater-discover.sh` | 本地假清单；**勿**改生产 JSON |
| U2 | `sh scripts/smoke-updater-u2.sh` + 发版后旧包手验 | 本地真 `.sig` 安装链；生产重启手验 |
| 生产版本核验 | `sh scripts/smoke-updater-prod-verify.sh` | 本机已装 ProductVersion vs 生产清单（不改清单） |

细则：`.agents/skills/stitch-desktop-smoke/references/updater-upgrade.md`。

当前生产清单版本见 `https://www.promptstdio.com/downloads/stitch-update.json`。  
**无**启动时主动弹窗/系统推送；须用户点「检查更新」。若要启动静默检查 + 非阻断提示，另开任务改前端。
