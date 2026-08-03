//! Shell command execution tool.
//!
//! Runs commands in the working directory. Requires user confirmation
//! for safety since commands can be destructive.

use super::{ToolDef, ToolResult};
use std::path::PathBuf;
use std::time::Duration;

/// Max bytes of combined stdout+stderr to capture.
const MAX_OUTPUT_BYTES: usize = 100_000;

/// Per-command timeout to prevent indefinite hangs (e.g. broken SSH).
const COMMAND_TIMEOUT: Duration = Duration::from_secs(120);

#[derive(Clone)]
pub struct RunCommand {
    work_dir: PathBuf,
}

impl RunCommand {
    pub fn new(work_dir: impl Into<PathBuf>) -> Self {
        Self {
            work_dir: work_dir.into(),
        }
    }

    pub fn definition(&self) -> ToolDef {
        ToolDef {
            name: "run_command".into(),
            description: "Run a shell command in the working directory. \
                 Returns stdout and stderr. Supports optional stdin input, \
                 custom timeout, and streaming progress output. \
                 Use for: building, testing, git operations, installing dependencies, \
                 long-running tasks."
                .into(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The shell command to execute"
                    },
                    "timeout_secs": {
                        "type": "integer",
                        "description": "Optional timeout in seconds (max 3600). Defaults to 120."
                    },
                    "input": {
                        "type": "string",
                        "description": "Optional stdin to pipe to the command"
                    },
                    "stream": {
                        "type": "boolean",
                        "description": "If true, return partial output every ~2s while command runs. Default false (return only final output)."
                    }
                },
                "required": ["command"]
            }),
        }
    }

    pub async fn execute(&self, arguments: serde_json::Value) -> anyhow::Result<ToolResult> {
        self.execute_with_progress(arguments, None, None).await
    }

    /// Execute, optionally pushing each output line to `progress_tx` as it
    /// arrives (ADR-037). The final `ToolResult` is unchanged either way.
    pub async fn execute_with_progress(
        &self,
        arguments: serde_json::Value,
        progress_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
        cancel_flag: Option<&std::sync::atomic::AtomicBool>,
    ) -> anyhow::Result<ToolResult> {
        let command = arguments["command"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("Missing 'command' argument"))?;

        let timeout = arguments
            .get("timeout_secs")
            .and_then(|v| v.as_u64())
            .map(|s| Duration::from_secs(s.min(3600)))
            .unwrap_or(COMMAND_TIMEOUT);

        let input = arguments
            .get("input")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        let stream = arguments
            .get("stream")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let output = run_shell(
            command,
            &self.work_dir,
            Some(timeout),
            input,
            stream,
            progress_tx,
            cancel_flag,
        )
        .await?;

        if output.success {
            Ok(ToolResult::ok(format_output(
                &output.stdout,
                &output.stderr,
            )))
        } else {
            Ok(ToolResult::fail(format!(
                "Exit code {}\n{}",
                output.code,
                format_output(&output.stdout, &output.stderr)
            )))
        }
    }
}

#[derive(Debug)]
struct CmdOutput {
    success: bool,
    code: i32,
    stdout: String,
    stderr: String,
}

/// Entry logged by streaming reader tasks.
type LogEntry = (std::time::Duration, String, String); // (elapsed, stream_label, line)

/// All commands read stdout/stderr line-by-line concurrently with the
/// process (ADR-037): every line is pushed to `progress_tx` live, while the
/// full capture is kept for the final result. `stream` only changes the
/// *returned* format (timestamped progress log vs plain stdout/stderr) and
/// the timeout semantics (partial output vs error).
async fn run_shell(
    command: &str,
    work_dir: &PathBuf,
    timeout: Option<Duration>,
    stdin_input: Option<String>,
    stream: bool,
    progress_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    cancel_flag: Option<&std::sync::atomic::AtomicBool>,
) -> anyhow::Result<CmdOutput> {
    let mut cmd = build_cmd(command, work_dir, stdin_input.is_some());
    let mut child = cmd.spawn()?;
    let pid = child.id();

    // Write stdin if provided
    if let Some(ref input) = stdin_input {
        use tokio::io::AsyncWriteExt;
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(input.as_bytes()).await;
        }
    }

    run_shell_streaming(child, pid, timeout, stream, progress_tx, cancel_flag).await
}

fn build_cmd(command: &str, work_dir: &PathBuf, need_stdin: bool) -> tokio::process::Command {
    #[cfg(target_os = "windows")]
    {
        let mut c = tokio::process::Command::new("cmd");
        c.args(["/C", command])
            .current_dir(work_dir)
            .kill_on_drop(true);
        c.env("PYTHONUTF8", "1");
        c.env("PYTHONIOENCODING", "utf-8");
        super::process_win::hide_console(&mut c);
        if need_stdin {
            c.stdin(std::process::Stdio::piped());
        }
        c.stdout(std::process::Stdio::piped());
        c.stderr(std::process::Stdio::piped());
        c
    }

    #[cfg(not(target_os = "windows"))]
    {
        let mut c = tokio::process::Command::new("sh");
        c.args(["-c", command])
            .current_dir(work_dir)
            .kill_on_drop(true);
        if need_stdin {
            c.stdin(std::process::Stdio::piped());
        }
        c.stdout(std::process::Stdio::piped());
        c.stderr(std::process::Stdio::piped());
        c
    }
}

/// Read stdout/stderr line-by-line concurrently with the process, pushing
/// each line to `progress_tx` live (ADR-037). With `stream`, returns a
/// timestamped progress log and preserves partial output on timeout;
/// otherwise returns plain stdout/stderr and timeout is an error.
async fn run_shell_streaming(
    mut child: tokio::process::Child,
    pid: Option<u32>,
    timeout: Option<Duration>,
    stream: bool,
    progress_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    cancel_flag: Option<&std::sync::atomic::AtomicBool>,
) -> anyhow::Result<CmdOutput> {
    use std::sync::atomic::Ordering;
    use std::sync::{Arc, Mutex};

    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| anyhow::anyhow!("no stdout pipe"))?;
    let stderr = child
        .stderr
        .take()
        .ok_or_else(|| anyhow::anyhow!("no stderr pipe"))?;

    let log: Arc<Mutex<Vec<LogEntry>>> = Arc::new(Mutex::new(Vec::new()));
    let start = std::time::Instant::now();
    let cancel_addr = cancel_flag.map(|f| std::ptr::from_ref(f) as usize);

    // ── stdout reader ──
    let log_out = log.clone();
    let prog_out = progress_tx.clone();
    let mut out_task = tokio::spawn(async move {
        read_lines(stdout, "stdout", start, log_out, prog_out, cancel_addr, pid).await;
    });

    // ── stderr reader ──
    let log_err = log.clone();
    let mut err_task = tokio::spawn(async move {
        read_lines(
            stderr,
            "stderr",
            start,
            log_err,
            progress_tx,
            cancel_addr,
            pid,
        )
        .await;
    });

    // ── wait for process (poll cancel between short waits) ──
    let wait_start = std::time::Instant::now();
    let (exit_code, timed_out, cancelled) = loop {
        if cancel_flag.is_some_and(|f| f.load(Ordering::SeqCst)) {
            kill_process_tree(pid);
            break (-1, false, true);
        }

        let remaining = timeout.map(|t| t.saturating_sub(wait_start.elapsed()));
        if remaining.is_some_and(|r| r.is_zero()) {
            kill_process_tree(pid);
            break (-1, true, false);
        }

        let poll = remaining
            .map(|r| r.min(Duration::from_millis(200)))
            .unwrap_or(Duration::from_millis(200));
        match tokio::time::timeout(poll, child.wait()).await {
            Ok(Ok(status)) => break (status.code().unwrap_or(-1), false, false),
            Ok(Err(e)) => {
                return Err(anyhow::anyhow!("Command failed to complete: {e}"));
            }
            Err(_) => continue,
        }
    };

    // Give readers a short window to drain any remaining buffered output
    // after the pipes close. Abort afterwards: a grandchild process may have
    // inherited the pipes and keep them open — a leaked reader would hold the
    // progress channel (and thus the UI forwarder) open forever.
    let _ = tokio::time::timeout(Duration::from_secs(2), async {
        let _ = tokio::join!(&mut out_task, &mut err_task);
    })
    .await;
    out_task.abort();
    err_task.abort();

    let collected = log.lock().unwrap();

    if !stream {
        // Plain path: stdout and stderr kept separate (wait_with_output
        // semantics); timeout discards partial output as an error.
        if cancelled {
            return Err(anyhow::anyhow!("Command cancelled"));
        }
        if timed_out {
            return Err(anyhow::anyhow!(
                "Command timed out after {} seconds",
                timeout.unwrap_or_default().as_secs()
            ));
        }
        let mut out = String::new();
        let mut err = String::new();
        for (_elapsed, label, line) in collected.iter() {
            if label == "stderr" {
                err.push_str(line);
            } else {
                out.push_str(line);
            }
        }
        return Ok(CmdOutput {
            success: exit_code == 0,
            code: exit_code,
            stdout: out,
            stderr: err,
        });
    }

    // Format the timestamped log
    let mut buf = String::new();
    for (elapsed, stream, line) in collected.iter() {
        let ts = format!("[{:3}.{:03}s]", elapsed.as_secs(), elapsed.subsec_millis());
        if stream == "stderr" {
            buf.push_str(&format!("{ts} [stderr] {line}"));
        } else {
            buf.push_str(&format!("{ts} {line}"));
        }
    }

    if timed_out {
        buf.push_str(&format!(
            "\n\n[stderr]\nCommand timed out after {} seconds\n",
            timeout.unwrap_or_default().as_secs()
        ));
    } else if cancelled {
        buf.push_str("\n\n[stderr]\nCommand cancelled\n");
    }

    let success = !timed_out && !cancelled && exit_code == 0;
    let stdout = truncate_str(&buf, MAX_OUTPUT_BYTES);
    Ok(CmdOutput {
        success,
        code: exit_code,
        stdout,
        stderr: String::new(),
    })
}

/// Read all lines from an async reader, pushing each into the shared log with
/// elapsed time since `start`, and forwarding it to the live progress
/// channel (if any) so the UI can render output as it happens.
async fn read_lines<R>(
    reader: R,
    label: &str,
    start: std::time::Instant,
    log: std::sync::Arc<std::sync::Mutex<Vec<LogEntry>>>,
    progress_tx: Option<tokio::sync::mpsc::UnboundedSender<String>>,
    cancel_addr: Option<usize>,
    pid: Option<u32>,
) where
    R: tokio::io::AsyncRead + Unpin,
{
    use std::sync::atomic::Ordering;
    use tokio::io::AsyncBufReadExt;
    let mut buf_reader = tokio::io::BufReader::new(reader);
    let mut line = Vec::new();
    loop {
        if cancel_addr.is_some_and(|addr| unsafe {
            (*(addr as *const std::sync::atomic::AtomicBool)).load(Ordering::SeqCst)
        }) {
            kill_process_tree(pid);
            break;
        }
        line.clear();
        let read_fut = buf_reader.read_until(b'\n', &mut line);
        match tokio::time::timeout(Duration::from_millis(200), read_fut).await {
            Ok(Ok(0)) => break, // EOF
            Ok(Ok(_)) => {
                let s = decode_console_bytes(&line);
                if let Some(tx) = &progress_tx {
                    // Receiver may be gone (turn cancelled) — never block.
                    let _ = tx.send(s.clone());
                }
                if let Ok(mut guard) = log.lock() {
                    guard.push((start.elapsed(), label.to_string(), s));
                }
            }
            Ok(Err(_)) => break,
            Err(_) => continue,
        }
    }
}

/// Kill a process and all its children. On Windows uses taskkill /T;
/// on Unix sends SIGKILL to the process group.
fn kill_process_tree(pid: Option<u32>) {
    let Some(pid) = pid else { return };
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("taskkill")
            .args(["/F", "/T", "/PID", &pid.to_string()])
            .creation_flags(super::process_win::CREATE_NO_WINDOW)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
    #[cfg(not(target_os = "windows"))]
    {
        // Send SIGKILL to the entire process group
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
}

/// Decode cmd/tool output: UTF-8 first; on Windows fall back to GBK (cp936).
fn decode_console_bytes(bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    if let Ok(s) = std::str::from_utf8(bytes) {
        return s.to_owned();
    }
    #[cfg(windows)]
    {
        let (cow, _, _) = encoding_rs::GBK.decode(bytes);
        cow.into_owned()
    }
    #[cfg(not(windows))]
    {
        String::from_utf8_lossy(bytes).into_owned()
    }
}

fn format_output(stdout: &str, stderr: &str) -> String {
    let mut parts = Vec::new();

    let out = truncate_str(stdout, MAX_OUTPUT_BYTES);
    if !out.is_empty() {
        parts.push(out);
    }

    let err = truncate_str(stderr, MAX_OUTPUT_BYTES);
    if !err.is_empty() {
        parts.push(format!("[stderr]\n{err}"));
    }

    if parts.is_empty() {
        "(no output)".into()
    } else {
        parts.join("\n")
    }
}

/// Truncate a string to at most `max_bytes` **bytes**, preserving valid
/// UTF-8 character boundaries (no split multi-byte sequences).
fn truncate_str(s: &str, max_bytes: usize) -> String {
    if s.len() <= max_bytes {
        return s.to_string();
    }
    // Walk backwards from max_bytes to find a valid char boundary.
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    let truncated = &s[..end];
    format!(
        "{truncated}\n... [truncated at {} bytes, total {}]",
        end,
        s.len()
    )
}

// ── tests ──────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_str_ascii_under_limit() {
        let s = "hello world";
        assert_eq!(truncate_str(s, 20), "hello world");
    }

    #[test]
    fn truncate_str_ascii_over_limit() {
        let s = "hello world this is a long string";
        let result = truncate_str(s, 11);
        assert!(result.starts_with("hello world"));
        assert!(result.contains("truncated at 11 bytes"));
        assert!(result.contains(&format!("total {}", s.len())));
    }

    #[test]
    fn truncate_str_cjk_respects_char_boundaries() {
        // Each CJK char is 3 bytes in UTF-8; 12 chars = 36 bytes
        let s = "断言失败断言失败断言失败";
        let result = truncate_str(s, 20);
        // At 20 bytes, last valid boundary is 18 = 6 chars.
        let expected: String = s.chars().take(6).collect();
        assert!(
            result.starts_with(&expected),
            "expected prefix '{expected}', got '{result}'"
        );
        // Must not contain U+FFFD (replacement character from bad split)
        assert!(!result.contains('\u{FFFD}'));
    }

    #[test]
    fn truncate_str_exact_boundary() {
        let s = "abc"; // 3 bytes
        assert_eq!(truncate_str(s, 3), "abc");
    }

    #[test]
    fn decode_console_accepts_utf8() {
        assert_eq!(decode_console_bytes(b"hello"), "hello");
        assert_eq!(decode_console_bytes("断言失败".as_bytes()), "断言失败");
    }

    #[test]
    fn decode_console_gbk_chinese_on_windows() {
        // "断言错误" in GBK
        let gbk = [0xB6, 0xCF, 0xD1, 0xD4, 0xB4, 0xED, 0xCE, 0xF3];
        let s = decode_console_bytes(&gbk);
        #[cfg(windows)]
        assert_eq!(s, "断言错误");
        #[cfg(not(windows))]
        assert!(!s.is_empty());
    }

    #[tokio::test]
    async fn run_shell_echo_ok() {
        let dir = std::env::temp_dir();
        let out = run_shell(
            "echo stitch-hidden-console",
            &dir,
            None,
            None,
            false,
            None,
            None,
        )
        .await
        .expect("shell");
        assert!(out.success, "stderr={}", out.stderr);
        assert!(
            out.stdout.to_lowercase().contains("stitch-hidden-console"),
            "stdout={}",
            out.stdout
        );
    }

    #[tokio::test]
    async fn run_shell_timeout_kills_long_command() {
        let dir = std::env::temp_dir();
        #[cfg(target_os = "windows")]
        let cmd = "ping -n 10 127.0.0.1 >nul";
        #[cfg(not(target_os = "windows"))]
        let cmd = "sleep 10";
        let out = run_shell(
            cmd,
            &dir,
            Some(Duration::from_secs(2)),
            None,
            false,
            None,
            None,
        )
        .await;
        assert!(out.is_err(), "expected timeout error");
        let msg = out.unwrap_err().to_string();
        assert!(
            msg.contains("timed out"),
            "error should mention timeout: {msg}"
        );
    }

    #[tokio::test]
    async fn run_shell_stdin_input() {
        let dir = std::env::temp_dir();
        #[cfg(target_os = "windows")]
        let cmd = "sort";
        #[cfg(not(target_os = "windows"))]
        let cmd = "cat";
        let out = run_shell(
            cmd,
            &dir,
            None,
            Some("hello world\nfoo bar\n".into()),
            false,
            None,
            None,
        )
        .await
        .expect("shell");
        assert!(out.success, "exit code={}, stderr={}", out.code, out.stderr);
        assert!(out.stdout.contains("hello"), "stdout={}", out.stdout);
    }

    #[tokio::test]
    async fn run_shell_stream_echo_timestamped() {
        let dir = std::env::temp_dir();
        #[cfg(target_os = "windows")]
        let cmd = "echo hello && echo world";
        #[cfg(not(target_os = "windows"))]
        let cmd = "echo hello && echo world";
        let out = run_shell(cmd, &dir, None, None, true, None, None)
            .await
            .expect("shell");
        assert!(out.success, "stderr={}", out.stderr);
        // Streaming output should contain timestamp markers like "[  0."
        assert!(
            out.stdout.contains("[  ") || out.stdout.contains("[ "),
            "expected timestamp like '[  0.xxx]', got: {}",
            out.stdout
        );
        assert!(out.stdout.contains("hello"), "stdout={}", out.stdout);
        assert!(out.stdout.contains("world"), "stdout={}", out.stdout);
    }

    #[tokio::test]
    async fn run_shell_progress_channel_receives_lines_live() {
        let dir = std::env::temp_dir();
        #[cfg(target_os = "windows")]
        let cmd = "echo alpha && echo beta";
        #[cfg(not(target_os = "windows"))]
        let cmd = "echo alpha && echo beta";
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let out = run_shell(cmd, &dir, None, None, false, Some(tx), None)
            .await
            .expect("shell");
        assert!(out.success, "stderr={}", out.stderr);

        let mut live = String::new();
        while let Ok(line) = rx.try_recv() {
            live.push_str(&line);
        }
        assert!(live.contains("alpha"), "live stream missing alpha: {live}");
        assert!(live.contains("beta"), "live stream missing beta: {live}");
        // Final result still carries the full plain output.
        assert!(out.stdout.contains("alpha"), "stdout={}", out.stdout);
        assert!(out.stdout.contains("beta"), "stdout={}", out.stdout);
    }

    #[tokio::test]
    async fn run_shell_progress_arrives_before_process_exit() {
        // A command that prints then sleeps: the first line must reach the
        // channel well before the command finishes (no batching at the end).
        let dir = std::env::temp_dir();
        #[cfg(target_os = "windows")]
        let cmd = "echo early-line && ping -n 3 127.0.0.1 >nul";
        #[cfg(not(target_os = "windows"))]
        let cmd = "echo early-line && sleep 2";
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let handle =
            tokio::spawn(
                async move { run_shell(cmd, &dir, None, None, false, Some(tx), None).await },
            );
        let first = tokio::time::timeout(Duration::from_secs(1), rx.recv())
            .await
            .expect("first line should arrive within 1s")
            .expect("channel open");
        assert!(first.contains("early-line"), "first line: {first}");
        let out = handle.await.expect("join").expect("shell");
        assert!(out.success);
    }

    #[tokio::test]
    async fn run_shell_stream_timeout_preserves_partial() {
        let dir = std::env::temp_dir();
        #[cfg(target_os = "windows")]
        let cmd = "echo starting && ping -n 10 127.0.0.1 >nul && echo never-reached";
        #[cfg(not(target_os = "windows"))]
        let cmd = "echo starting && sleep 10 && echo never-reached";
        let out = run_shell(
            cmd,
            &dir,
            Some(Duration::from_secs(2)),
            None,
            true,
            None,
            None,
        )
        .await;
        // Should not be an Err — streaming returns partial output on timeout
        assert!(out.is_ok(), "expected partial output, got Err: {out:?}");
        let o = out.unwrap();
        assert!(!o.success, "should not be success after timeout");
        assert!(
            o.stdout.contains("starting"),
            "should contain partial output before timeout. stdout={}",
            o.stdout
        );
        assert!(
            o.stdout.to_lowercase().contains("timed out"),
            "should mention timeout. stdout={}",
            o.stdout
        );
        assert!(
            !o.stdout.contains("never-reached"),
            "should not contain output after timeout. stdout={}",
            o.stdout
        );
    }

    /// GUI hosts must not flash `system32\cmd.exe`. Poll visible console titles
    /// while a multi-second command runs under CREATE_NO_WINDOW.
    #[cfg(windows)]
    #[tokio::test]
    async fn run_shell_no_visible_system32_cmd_window() {
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};
        use std::time::Duration;

        let seen = Arc::new(AtomicBool::new(false));
        let seen_bg = Arc::clone(&seen);
        let stop = Arc::new(AtomicBool::new(false));
        let stop_bg = Arc::clone(&stop);

        let watcher = std::thread::spawn(move || {
            while !stop_bg.load(Ordering::Relaxed) {
                if visible_system32_cmd() {
                    seen_bg.store(true, Ordering::Relaxed);
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        });

        let dir = std::env::temp_dir();
        let out = run_shell(
            "ping -n 3 127.0.0.1 >nul",
            &dir,
            None,
            None,
            false,
            None,
            None,
        )
        .await
        .expect("shell");
        stop.store(true, Ordering::Relaxed);
        let _ = watcher.join();

        assert!(out.success, "stderr={}", out.stderr);
        assert!(
            !seen.load(Ordering::Relaxed),
            "visible system32\\cmd.exe window appeared during run_command"
        );
    }

    #[cfg(windows)]
    fn visible_system32_cmd() -> bool {
        use std::sync::atomic::{AtomicBool, Ordering};

        static FOUND: AtomicBool = AtomicBool::new(false);
        FOUND.store(false, Ordering::Relaxed);

        unsafe extern "system" {
            fn EnumWindows(
                cb: Option<unsafe extern "system" fn(isize, isize) -> i32>,
                lparam: isize,
            ) -> i32;
            fn IsWindowVisible(hwnd: isize) -> i32;
            fn GetClassNameW(hwnd: isize, lp_class_name: *mut u16, n_max_count: i32) -> i32;
            fn GetWindowTextW(hwnd: isize, lp_string: *mut u16, n_max_count: i32) -> i32;
        }

        unsafe extern "system" fn enum_cb(hwnd: isize, _: isize) -> i32 {
            unsafe {
                if IsWindowVisible(hwnd) == 0 {
                    return 1;
                }
                let mut class = [0u16; 64];
                if GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32) <= 0 {
                    return 1;
                }
                let class_name = String::from_utf16_lossy(
                    &class[..class.iter().position(|&c| c == 0).unwrap_or(class.len())],
                );
                if class_name != "ConsoleWindowClass" {
                    return 1;
                }
                let mut title = [0u16; 512];
                let n = GetWindowTextW(hwnd, title.as_mut_ptr(), title.len() as i32);
                if n <= 0 {
                    return 1;
                }
                let t = String::from_utf16_lossy(&title[..n as usize]).to_ascii_lowercase();
                if t.contains("system32\\cmd.exe") || t == "cmd.exe" {
                    FOUND.store(true, Ordering::Relaxed);
                    return 0; // stop enumeration
                }
                1
            }
        }

        unsafe {
            EnumWindows(Some(enum_cb), 0);
        }
        FOUND.load(Ordering::Relaxed)
    }
}
