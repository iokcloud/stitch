//! 自更新：版本检查与升级（`stitch upgrade` 与会话内 `/upgrade` 共用）。
//!
//! 版本清单：https://www.promptstdio.com/downloads/stitch-cli-version.json
//! （官网 /downloads 直链，国内可直连；GitHub Release 为国际镜像）
//! 完整性：清单带 sha256，下载后比对（不匹配即中止）；防回滚：仅接受
//! 高于当前版本的更新（语义版本比较）。

use std::io::Write;

const VERSION_URL: &str = "https://www.promptstdio.com/downloads/stitch-cli-version.json";
const BASE_URL: &str = "https://www.promptstdio.com/downloads/";

/// `stitch upgrade`：从官网版本清单拉最新版，下载对应平台二进制并覆盖自身。
pub async fn run() -> anyhow::Result<()> {
    use sha2::{Digest, Sha256};

    let current = env!("CARGO_PKG_VERSION");
    let client = reqwest::Client::new();
    let manifest: serde_json::Value = client
        .get(VERSION_URL)
        .timeout(std::time::Duration::from_secs(20))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("无法连接更新服务：{e}（请检查网络）"))?
        .json()
        .await
        .map_err(|e| anyhow::anyhow!("更新清单解析失败：{e}"))?;
    let latest = manifest
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if latest.is_empty() {
        anyhow::bail!("更新清单缺少 version 字段");
    }
    // 防回滚：仅允许升级到更高版本
    if !version_newer(&latest, current) {
        println!("已是最新版本 v{current}。");
        return Ok(());
    }
    println!("发现新版本 v{latest}（当前 v{current}），开始下载…");

    // 平台 → 文件名 + 清单 sha256 key
    let (file, hash_key): (&str, &str) = if cfg!(target_os = "windows") {
        ("stitch-x86_64-pc-windows-msvc.exe", "windows")
    } else if cfg!(target_os = "macos") {
        if cfg!(target_arch = "aarch64") {
            ("stitch-aarch64-apple-darwin", "macos-arm")
        } else {
            ("stitch-x86_64-apple-darwin", "macos-x64")
        }
    } else {
        ("stitch-x86_64-unknown-linux-musl", "linux")
    };
    let expected_sha = manifest
        .get("sha256")
        .and_then(|m| m.get(hash_key))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if expected_sha.is_empty() {
        anyhow::bail!("更新清单缺少 sha256.{hash_key} 字段");
    }

    let exe = std::env::current_exe().map_err(|e| anyhow::anyhow!("无法定位当前程序路径：{e}"))?;
    let tmp = exe.with_file_name(format!(
        "{}.upgrade",
        exe.file_name().and_then(|n| n.to_str()).unwrap_or("stitch")
    ));

    // 下载到临时文件，同时计算 sha256
    let url = format!("{BASE_URL}{file}");
    let mut resp = client
        .get(&url)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| anyhow::anyhow!("下载失败：{e}"))?;
    let mut out = std::fs::File::create(&tmp)
        .map_err(|e| anyhow::anyhow!("无法写入临时文件 {tmp:?}：{e}"))?;
    let mut hasher = Sha256::new();
    while let Some(chunk) = resp
        .chunk()
        .await
        .map_err(|e| anyhow::anyhow!("下载中断：{e}"))?
    {
        hasher.update(&chunk);
        out.write_all(&chunk)?;
    }
    drop(out);

    // 完整性校验：sha256 不匹配则丢弃并中止（防止损坏/被篡改的二进制）
    let actual = format!("{:x}", hasher.finalize());
    if !actual.eq_ignore_ascii_case(&expected_sha) {
        let _ = std::fs::remove_file(&tmp);
        anyhow::bail!("下载文件校验失败（sha256 不匹配），已中止升级。");
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&tmp, std::fs::Permissions::from_mode(0o755));
    }

    // 覆盖自身：Unix 直接 rename；Windows 运行中的 exe 被锁 → 生成延迟替换
    // 脚本（exe 退出后自动替换，无需手动 move——0.5.1 及以前要手动执行）
    match std::fs::rename(&tmp, &exe) {
        Ok(()) => {
            println!("已升级到 v{latest}。");
        }
        Err(_) if cfg!(windows) => {
            let exe_name = exe
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("stitch.exe");
            let tmp_name = tmp
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("stitch.exe.upgrade");
            let bat = exe.with_file_name(format!("{exe_name}.upgrade.bat"));
            let script = format!(
                "@echo off\r\n\
                 rem Stitch 延迟替换：等 exe 退出后自动升级（临时脚本，结束后自删）\r\n\
                 :wait\r\n\
                 tasklist /fi \"IMAGENAME eq {exe_name}\" | findstr /i \"{exe_name}\" >nul 2>&1\r\n\
                 if not errorlevel 1 (\r\n\
                 \x20 timeout /t 2 /nobreak >nul\r\n\
                 \x20 goto :wait\r\n\
                 )\r\n\
                 move /y \"%~dp0{tmp_name}\" \"%~dp0{exe_name}\" >nul\r\n\
                 if exist \"%~dp0{tmp_name}\" (\r\n\
                 \x20 echo [stitch] 替换失败，请稍后手动执行: move /y \"%~dp0{tmp_name}\" \"%~dp0{exe_name}\"\r\n\
                 \x20 pause\r\n\
                 )\r\n\
                 del \"%~f0\" >nul 2>&1\r\n"
            );
            std::fs::write(&bat, script).map_err(|e| anyhow::anyhow!("无法写入升级脚本：{e}"))?;
            // 隐藏窗口静默执行延迟替换（PowerShell Start-Process -WindowStyle Hidden）
            let bat_path = bat.display().to_string().replace('\'', "''");
            let _ = std::process::Command::new("powershell")
                .args([
                    "-NoProfile",
                    "-WindowStyle",
                    "Hidden",
                    "-Command",
                    &format!("Start-Process -FilePath '{bat_path}' -WindowStyle Hidden"),
                ])
                .spawn();
            println!("已下载 v{latest}（校验通过）。退出本会话后自动完成替换，无需手动操作。");
        }
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            anyhow::bail!("升级失败：{e}");
        }
    }
    Ok(())
}

/// 启动时检查新版本并打印提示（主线程同步调用 · 3s 短超时 · 失败静默）。
///
/// 不能用后台线程 + println：rustyline 接管终端后，后台输出与主循环
/// 渲染竞争会被吞/错位（真机复现：0.5.1 启动提示不显示）。同步检查
/// 正常网络 <100ms 无感；3s 超时上限保证坏网络下不卡启动。
pub async fn check_update_and_hint() {
    let Ok(latest) = fetch_latest_version_short().await else {
        return; // 离线/慢网/清单异常：静默，不打扰
    };
    if let Some(text) = update_hint_text(&latest, env!("CARGO_PKG_VERSION")) {
        println!("\x1b[90m{text}\x1b[0m");
        let _ = std::io::stdout().flush();
    }
}

/// 拉取官网版本清单中的最新版本号（3s 短超时；失败返回 Err，调用方静默）。
async fn fetch_latest_version_short() -> anyhow::Result<String> {
    let client = reqwest::Client::new();
    let manifest: serde_json::Value = client
        .get(VERSION_URL)
        .timeout(std::time::Duration::from_secs(3))
        .send()
        .await?
        .json()
        .await?;
    Ok(manifest
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string())
}

/// 新版本提示文案；无新版本返回 None（纯函数，可测）。
pub fn update_hint_text(latest: &str, current: &str) -> Option<String> {
    if version_newer(latest, current) {
        Some(format!(
            "发现新版本 v{latest}（当前 v{current}）——运行 `stitch upgrade` 或会话内 `/upgrade` 更新"
        ))
    } else {
        None
    }
}

/// 语义版本比较：`a` 是否高于 `b`（x.y.z 三段；解析失败视为不高于）。
pub fn version_newer(a: &str, b: &str) -> bool {
    fn parse(v: &str) -> Option<(u64, u64, u64)> {
        let mut parts = v.trim_start_matches('v').split('.');
        let x = parts.next()?.parse().ok()?;
        let y = parts.next().unwrap_or("0").parse().ok()?;
        let z = parts.next().unwrap_or("0").parse().ok()?;
        Some((x, y, z))
    }
    match (parse(a), parse(b)) {
        (Some(av), Some(bv)) => av > bv,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::{update_hint_text, version_newer};

    #[test]
    fn update_hint_only_when_newer() {
        assert!(update_hint_text("0.5.0", "0.4.1").is_some());
        assert!(update_hint_text("0.4.1", "0.4.1").is_none(), "同版本无提示");
        assert!(update_hint_text("0.4.0", "0.4.1").is_none(), "低版本无提示");
        assert!(update_hint_text("abc", "0.4.1").is_none(), "非法版本无提示");
        let text = update_hint_text("0.5.0", "0.4.1").unwrap();
        assert!(text.contains("0.5.0") && text.contains("upgrade"));
    }

    #[test]
    fn version_compare_guards_rollback() {
        assert!(version_newer("0.4.0", "0.3.0"));
        assert!(version_newer("0.3.1", "0.3.0"));
        assert!(version_newer("1.0.0", "0.9.9"));
        assert!(!version_newer("0.3.0", "0.3.0"), "同版本不升级");
        assert!(!version_newer("0.2.9", "0.3.0"), "低版本不降级");
        assert!(!version_newer("abc", "0.3.0"), "非法版本不升级");
        assert!(!version_newer("0.3.0", "abc"));
    }
}
