//! statusLine——每回合结束显示一条自定义状态行（Claude Code 语义）。
//!
//! 配置：`config.toml` 的 `statusline` = 一条 shell 命令（如
//! `git rev-parse --abbrev-ref HEAD`）。每回合结束后执行，stdout 显示在
//! 提示符上方。输出支持：
//! - 纯文本（逐行 trim 后合并）
//! - JSON `{"text": "…"}`（Claude Code statusline 格式）
//!
//! 超时 10 秒、输出上限 2KB——状态行绝不阻塞会话。

use std::sync::Mutex;
use std::time::Duration;

/// 会话级覆盖（--setting statusline=…）：非空时优先于 settings/config。
static OVERRIDE: Mutex<Option<String>> = Mutex::new(None);

/// 设置会话级 statusline 覆盖（--setting，不落盘）。
pub fn set_override(cmd: Option<String>) {
    if let Ok(mut g) = OVERRIDE.lock() {
        *g = cmd;
    }
}

/// 解析最终 statusline：--setting > settings.json > config。
pub fn resolved<'a>(settings: Option<&'a str>, cfg: Option<&'a str>) -> Option<String> {
    OVERRIDE
        .lock()
        .ok()
        .and_then(|g| g.clone())
        .or_else(|| settings.map(str::to_string))
        .or_else(|| cfg.map(str::to_string))
}

/// 解析命令输出：JSON `{"text": …}` 或纯文本。
pub fn parse_output(stdout: &str) -> String {
    let trimmed = stdout.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    let text = serde_json::from_str::<serde_json::Value>(trimmed)
        .ok()
        .and_then(|v| v.get("text").and_then(|t| t.as_str()).map(str::to_string));
    if let Some(text) = text {
        return text.trim().to_string();
    }
    // 纯文本：逐行 trim，跳过空行
    trimmed
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// 执行 statusline 命令。失败 / 超时 / 空输出 → None（静默）。
pub async fn run(command: &str) -> Option<String> {
    #[cfg(target_os = "windows")]
    let spawn = tokio::process::Command::new("cmd")
        .args(["/C", command])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn();

    #[cfg(not(target_os = "windows"))]
    let spawn = tokio::process::Command::new("sh")
        .args(["-c", command])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn();

    let mut child = match spawn {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(command, error = %e, "statusline spawn failed");
            return None;
        }
    };

    // 轮询退出状态，超时即 kill——try_wait 借用即时结束，之后可安全 kill
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => {}
            Err(e) => {
                tracing::warn!(command, error = %e, "statusline wait failed");
                return None;
            }
        }
        if tokio::time::Instant::now() >= deadline {
            let _ = child.kill().await;
            tracing::warn!(command, "statusline timed out");
            return None;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    };
    if !status.success() {
        return None;
    }

    // 读 stdout（进程已退出，管道缓冲完整）
    use tokio::io::AsyncReadExt;
    let mut bytes = Vec::new();
    if let Some(stdout) = child.stdout.as_mut() {
        let _ = stdout.read_to_end(&mut bytes).await;
    }

    // 截断到 2KB（不拆 UTF-8 码点）
    let stdout = String::from_utf8_lossy(&bytes);
    let text = parse_output(&stdout);
    if text.is_empty() {
        return None;
    }
    let mut truncated = text;
    if truncated.len() > 2048 {
        let mut end = 2048;
        while !truncated.is_char_boundary(end) {
            end -= 1;
        }
        truncated.truncate(end);
    }
    Some(truncated)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_joins_lines() {
        assert_eq!(
            parse_output("  branch: main\n  dirty\n"),
            "branch: main · dirty"
        );
        assert_eq!(parse_output("  \n\n  "), "");
    }

    #[test]
    fn json_text_field() {
        assert_eq!(parse_output(r#"{"text": "hello"}"#), "hello");
        // 带缩进/换行的 JSON 也解析
        assert_eq!(parse_output("{\n  \"text\": \"hi\"\n}\n"), "hi");
    }

    #[test]
    fn invalid_json_falls_back_to_plain() {
        // 以 { 开头但不是合法 JSON → 纯文本
        assert_eq!(parse_output("{not json"), "{not json");
    }

    #[tokio::test]
    async fn run_echo_returns_stdout() {
        #[cfg(target_os = "windows")]
        let cmd = "echo statusline-test";
        #[cfg(not(target_os = "windows"))]
        let cmd = "echo statusline-test";
        let out = run(cmd).await;
        assert!(out.is_some());
        assert!(out.unwrap().contains("statusline-test"));
    }

    #[tokio::test]
    async fn run_failing_command_returns_none() {
        #[cfg(target_os = "windows")]
        let cmd = "exit /b 1";
        #[cfg(not(target_os = "windows"))]
        let cmd = "exit 1";
        // 非零退出码：stdout 为空 → None
        assert!(run(cmd).await.is_none());
    }
}
