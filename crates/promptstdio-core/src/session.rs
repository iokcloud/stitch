//! Web 会话 cookie 名（登录后 HttpOnly · ADR-024）。

/// 浏览器登录后写入的 API Token cookie；`/api/v1` 同源 fetch 可携带。
pub const SESSION_TOKEN_COOKIE: &str = "promptstdio_token";
