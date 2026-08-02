//! 统一 tracing 初始化（R6 可观测）。

use std::env;

use tracing_subscriber::{EnvFilter, fmt};

/// 读取 `RUST_LOG` / `PROMPTSTDIO_LOG_FORMAT`（`json` 启用结构化 JSON 日志）。
pub fn init_tracing() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    let json = env::var("PROMPTSTDIO_LOG_FORMAT")
        .map(|v| v.eq_ignore_ascii_case("json"))
        .unwrap_or(false);

    if json {
        fmt().json().with_env_filter(filter).init();
    } else {
        fmt().with_env_filter(filter).init();
    }
}
