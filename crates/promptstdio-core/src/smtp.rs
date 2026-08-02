use std::env;

/// SMTP 发信配置（`PROMPTSTDIO_EMAIL_PROVIDER=smtp` 时必填）。
#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    /// 完整 From 头，如 `PromptStdio <noreply@promptstdio.com>`
    pub from: String,
}

impl SmtpConfig {
    pub fn from_env() -> Option<Self> {
        let host = env::var("PROMPTSTDIO_SMTP_HOST")
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        let username = env::var("PROMPTSTDIO_SMTP_USERNAME")
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        let password = env::var("PROMPTSTDIO_SMTP_PASSWORD")
            .ok()
            .filter(|s| !s.is_empty())?;
        let from = env::var("PROMPTSTDIO_SMTP_FROM")
            .ok()
            .filter(|s| !s.trim().is_empty())?;
        let port = env::var("PROMPTSTDIO_SMTP_PORT")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(587);
        Some(Self {
            host: host.trim().to_string(),
            port,
            username: username.trim().to_string(),
            password,
            from: from.trim().to_string(),
        })
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.host.is_empty() {
            return Err("PROMPTSTDIO_SMTP_HOST 未配置".into());
        }
        if self.username.is_empty() {
            return Err("PROMPTSTDIO_SMTP_USERNAME 未配置".into());
        }
        if self.password.is_empty() {
            return Err("PROMPTSTDIO_SMTP_PASSWORD 未配置".into());
        }
        if self.from.is_empty() {
            return Err(
                "PROMPTSTDIO_SMTP_FROM 未配置（如 PromptStdio <noreply@example.com>）".into(),
            );
        }
        if self.port == 0 {
            return Err("PROMPTSTDIO_SMTP_PORT 无效".into());
        }
        Ok(())
    }
}
