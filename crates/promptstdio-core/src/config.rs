use std::env;

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub bind_addr: String,
    pub surrealdb_endpoint: String,
    pub surrealdb_ns: String,
    pub surrealdb_db: String,
    /// 外部 SurrealDB 用户名（ws:// / wss:// 时生效）
    pub surrealdb_user: Option<String>,
    /// 外部 SurrealDB 密码（ws:// / wss:// 时生效）
    pub surrealdb_pass: Option<String>,
    pub meilisearch_url: String,
    pub meilisearch_api_key: Option<String>,
    pub cache_l1_max_capacity: u64,
    pub cache_l1_ttl_secs: u64,
    pub cache_l2_max_capacity: u64,
    pub cache_l2_ttl_secs: u64,
    pub worker_poll_interval_secs: u64,
    pub usage_log_retention_days: i64,
    pub mcp_session_ttl_secs: u64,
    pub mcp_sse_heartbeat_secs: u64,
    /// `POST/GET/DELETE /mcp` 专用访问日志目录（`MCP_ACCESS_LOG_DIR`）
    pub mcp_access_log_dir: String,
    /// MCP 访问日志保留天数（`MCP_ACCESS_LOG_RETENTION_DAYS`，默认 7）
    pub mcp_access_log_retention_days: i64,
    /// 是否启用 MCP 访问日志（`MCP_ACCESS_LOG_ENABLED`，默认开）
    pub mcp_access_log_enabled: bool,
    pub content_max_chars: usize,
    pub dev_api_token: Option<String>,
    pub dev_sms_code: String,
    pub sms_code_ttl_secs: u64,
    /// `dev` 固定码 · `log` staging 随机码打日志 · `webhook` POST · `aliyun` 阿里云 Dysmsapi
    pub sms_provider: String,
    pub sms_webhook_url: Option<String>,
    /// `PROMPTSTDIO_SMS_PROVIDER=aliyun` 时必填
    pub aliyun_sms: Option<crate::aliyun_sms::AliyunSmsConfig>,
    pub dev_email_code: String,
    pub email_code_ttl_secs: u64,
    /// `dev` 固定码 · `log` staging · `webhook` POST · `smtp` SMTP 发信
    pub email_provider: String,
    pub email_webhook_url: Option<String>,
    /// `PROMPTSTDIO_EMAIL_PROVIDER=smtp` 时必填
    pub smtp: Option<crate::smtp::SmtpConfig>,
    pub garage_endpoint: String,
    pub garage_bucket: String,
    pub garage_access_key: String,
    pub garage_secret_key: String,
    /// `local` · `staging` · `production`（见 `PROMPTSTDIO_ENV`）
    pub app_env: String,
    /// API Token HMAC pepper（`APP_KEY`）；生产必填
    pub app_key: Option<String>,
    /// 启动时导入 SurrealQL（Laravel ETL 产物 · `PROMPTSTDIO_ETL_SURQL`）
    pub etl_surql_path: Option<String>,
    /// 跳过 dev 用户/演示 seed（ETL 导入场景 · `PROMPTSTDIO_SKIP_DEV_SEED=1`）
    pub skip_dev_seed: bool,
    /// ETL 模式下将 `DEV_API_TOKEN` 绑定到指定邮箱用户（`PROMPTSTDIO_ETL_DEV_TOKEN_EMAIL`）
    pub etl_dev_token_email: Option<String>,
    /// 切流期 Web 公告：提示用户在 `/api-keys` 重签 Token（`PROMPTSTDIO_CUTOVER_NOTICE=1`）
    pub cutover_notice: bool,
    /// 运营后台允许登录的邮箱（逗号分隔 · `PROMPTSTDIO_PLATFORM_OPERATOR_EMAILS`）
    pub platform_operator_emails: Vec<String>,
    /// 运营后台允许登录的手机号（逗号分隔 · `PROMPTSTDIO_PLATFORM_OPERATOR_PHONES`）
    pub platform_operator_phones: Vec<String>,
    /// 对外 Web/MCP 根 URL（`PROMPTSTDIO_APP_URL`；OAuth 回调等）
    pub app_public_url: String,
    pub github_oauth: Option<crate::github_oauth::GitHubOAuthConfig>,
    /// 邮箱密码注册/登录（生产默认关，dev/staging 默认开；`PROMPTSTDIO_EMAIL_PASSWORD_AUTH`）。
    pub email_password_auth_enabled: bool,
    /// 微信支付是否启用
    pub wechat_pay_enabled: bool,
    /// 微信支付商户号
    pub wechat_pay_mch_id: Option<String>,
    /// 微信支付 API v3 密钥
    pub wechat_pay_api_v3_key: Option<String>,
    /// 微信支付商户证书序列号
    pub wechat_pay_serial_no: Option<String>,
    /// 微信支付商户私钥路径（PEM）
    pub wechat_pay_private_key_path: Option<String>,
    /// 微信支付回调通知地址（默认 `{app_public_url}/api/v1/payments/wechat/notify`）
    pub wechat_pay_notify_url: Option<String>,
    /// 微信支付 AppID（Native 下单必填）
    pub wechat_pay_app_id: Option<String>,
    /// 微信支付平台公钥证书路径（PEM，用于回调验签）
    pub wechat_pay_platform_cert_path: Option<String>,
}

impl AppConfig {
    pub fn is_production(&self) -> bool {
        self.app_env.eq_ignore_ascii_case("production")
    }

    pub fn is_local(&self) -> bool {
        self.app_env.eq_ignore_ascii_case("local")
    }

    pub fn http_max_body_bytes(&self) -> usize {
        self.content_max_chars
            .saturating_mul(4)
            .saturating_add(64 * 1024)
            .clamp(64 * 1024, crate::limits::HTTP_MAX_BODY_BYTES)
    }

    pub fn garage_enabled(&self) -> bool {
        !self.garage_endpoint.is_empty()
            && !self.garage_bucket.is_empty()
            && !self.garage_access_key.is_empty()
            && !self.garage_secret_key.is_empty()
    }

    pub fn github_oauth_enabled(&self) -> bool {
        self.github_oauth.is_some()
    }

    /// 启动前校验；生产环境 unsafe 配置 fail-fast。
    pub fn validate_for_startup(&self) -> Result<(), String> {
        if self.is_production() {
            if self
                .dev_api_token
                .as_deref()
                .is_some_and(|t| t == "dev-token-change-me")
            {
                return Err(
                    "PROMPTSTDIO_ENV=production 不得使用默认 DEV_API_TOKEN dev-token-change-me"
                        .into(),
                );
            }
            if self.sms_provider.eq_ignore_ascii_case("dev") {
                return Err(
                    "PROMPTSTDIO_ENV=production 不得使用 PROMPTSTDIO_SMS_PROVIDER=dev".into(),
                );
            }
            if self.email_provider.eq_ignore_ascii_case("dev") {
                return Err(
                    "PROMPTSTDIO_ENV=production 不得使用 PROMPTSTDIO_EMAIL_PROVIDER=dev".into(),
                );
            }
            let key = self.app_key.as_deref().unwrap_or("").trim();
            if key.len() < 32 {
                return Err(
                    "PROMPTSTDIO_ENV=production 须设置 APP_KEY（≥32 字符，用于 API Token HMAC）"
                        .into(),
                );
            }
        }
        if self.sms_provider.eq_ignore_ascii_case("webhook") {
            validate_webhook_url(
                self.sms_webhook_url.as_deref(),
                self.is_local(),
                self.is_production(),
                "PROMPTSTDIO_SMS_WEBHOOK_URL",
            )?;
        }
        if self.sms_provider.eq_ignore_ascii_case("aliyun") {
            let cfg = self.aliyun_sms.as_ref().ok_or_else(|| {
                "PROMPTSTDIO_SMS_PROVIDER=aliyun 须配置阿里云短信环境变量".to_string()
            })?;
            cfg.validate()?;
        }
        if self.email_provider.eq_ignore_ascii_case("webhook") {
            validate_webhook_url(
                self.email_webhook_url.as_deref(),
                self.is_local(),
                self.is_production(),
                "PROMPTSTDIO_EMAIL_WEBHOOK_URL",
            )?;
        }
        if self.email_provider.eq_ignore_ascii_case("smtp") {
            let cfg = self.smtp.as_ref().ok_or_else(|| {
                "PROMPTSTDIO_EMAIL_PROVIDER=smtp 须配置 SMTP 环境变量".to_string()
            })?;
            cfg.validate()?;
        }
        if let Some(cfg) = &self.github_oauth {
            cfg.validate()?;
        }
        if self.wechat_pay_enabled && !self.is_local() {
            self.validate_wechat_pay_for_startup()?;
        }
        Ok(())
    }

    fn validate_wechat_pay_for_startup(&self) -> Result<(), String> {
        let required = [
            (
                "PROMPTSTDIO_WECHAT_PAY_MCH_ID（或 PROMPTSTDIO_WECHAT_PAY_MCHID）",
                self.wechat_pay_mch_id.as_deref(),
            ),
            (
                "PROMPTSTDIO_WECHAT_PAY_API_V3_KEY",
                self.wechat_pay_api_v3_key.as_deref(),
            ),
            (
                "PROMPTSTDIO_WECHAT_PAY_SERIAL_NO",
                self.wechat_pay_serial_no.as_deref(),
            ),
            (
                "PROMPTSTDIO_WECHAT_PAY_APPID",
                self.wechat_pay_app_id.as_deref(),
            ),
        ];
        for (name, value) in required {
            if value.is_none_or(|s| s.trim().is_empty()) {
                return Err(format!(
                    "{name} 未配置（PROMPTSTDIO_WECHAT_PAY_ENABLED=1 时必填）"
                ));
            }
        }

        let api_v3_key = self.wechat_pay_api_v3_key.as_deref().unwrap_or("");
        if api_v3_key.len() != 32 {
            return Err(format!(
                "PROMPTSTDIO_WECHAT_PAY_API_V3_KEY 须为 32 字符，当前为 {}",
                api_v3_key.len()
            ));
        }

        let private_key_path = self
            .wechat_pay_private_key_path
            .as_deref()
            .unwrap_or("")
            .trim();
        if private_key_path.is_empty() {
            return Err(
                "PROMPTSTDIO_WECHAT_PAY_PRIVATE_KEY_PATH 未配置（PROMPTSTDIO_WECHAT_PAY_ENABLED=1 时必填）"
                    .into(),
            );
        }
        std::fs::read_to_string(private_key_path).map_err(|e| {
            format!("无法读取 PROMPTSTDIO_WECHAT_PAY_PRIVATE_KEY_PATH ({private_key_path}): {e}")
        })?;

        let platform_cert_path = self
            .wechat_pay_platform_cert_path
            .as_deref()
            .unwrap_or("")
            .trim();
        if platform_cert_path.is_empty() {
            return Err(
                "PROMPTSTDIO_WECHAT_PAY_PLATFORM_CERT_PATH 未配置（PROMPTSTDIO_WECHAT_PAY_ENABLED=1 时必填）"
                    .into(),
            );
        }
        std::fs::read_to_string(platform_cert_path).map_err(|e| {
            format!(
                "无法读取 PROMPTSTDIO_WECHAT_PAY_PLATFORM_CERT_PATH ({platform_cert_path}): {e}"
            )
        })?;

        Ok(())
    }
}

fn validate_webhook_url(
    url: Option<&str>,
    is_local: bool,
    is_production: bool,
    env_key: &str,
) -> Result<(), String> {
    let url = url.unwrap_or("").trim();
    if url.is_empty() {
        return Err(format!("{env_key} 未配置"));
    }
    if !(url.starts_with("https://") || (is_local && url.starts_with("http://"))) {
        return Err(format!("{env_key} 须为 https（本地可用 http）"));
    }
    let lower = url.to_ascii_lowercase();
    if is_production
        && (lower.contains("127.0.0.1") || lower.contains("localhost") || lower.contains("[::1]"))
    {
        return Err(format!("生产 {env_key} 不得指向 localhost/内网"));
    }
    Ok(())
}

impl AppConfig {
    pub fn from_env() -> Self {
        let bind_addr = env_or("PROMPTSTDIO_BIND", "127.0.0.1:8090");
        let app_public_url = env::var("PROMPTSTDIO_APP_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| format!("http://{bind_addr}"));
        let github_oauth = crate::github_oauth::GitHubOAuthConfig::from_env(&app_public_url);
        let app_env = env_or("PROMPTSTDIO_ENV", "local");
        let email_password_enabled = email_password_auth_enabled(&app_env);
        Self {
            bind_addr,
            surrealdb_endpoint: env_or("SURREALDB_ENDPOINT", "mem://"),
            surrealdb_ns: env_or("SURREALDB_NS", "promptstdio"),
            surrealdb_db: env_or("SURREALDB_DB", "dev"),
            surrealdb_user: env::var("SURREALDB_USER")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            surrealdb_pass: env::var("SURREALDB_PASS")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            meilisearch_url: env_or("MEILISEARCH_URL", "http://127.0.0.1:7700"),
            meilisearch_api_key: env::var("MEILISEARCH_API_KEY").ok(),
            cache_l1_max_capacity: parse_u64("CACHE_L1_MAX_CAPACITY", 10_000),
            cache_l1_ttl_secs: parse_u64("CACHE_L1_TTL_SECS", 300),
            cache_l2_max_capacity: parse_u64("CACHE_L2_MAX_CAPACITY", 50_000),
            cache_l2_ttl_secs: parse_u64("CACHE_L2_TTL_SECS", 3600),
            worker_poll_interval_secs: parse_u64("WORKER_POLL_INTERVAL_SECS", 2),
            usage_log_retention_days: parse_i64("USAGE_LOG_RETENTION_DAYS", 90),
            mcp_session_ttl_secs: parse_u64("MCP_SESSION_TTL_SECS", 86_400),
            mcp_sse_heartbeat_secs: parse_u64("MCP_SSE_HEARTBEAT_SECS", 30),
            mcp_access_log_dir: env_or("MCP_ACCESS_LOG_DIR", "./data/logs/mcp"),
            mcp_access_log_retention_days: parse_i64("MCP_ACCESS_LOG_RETENTION_DAYS", 7),
            mcp_access_log_enabled: parse_bool("MCP_ACCESS_LOG_ENABLED", true),
            content_max_chars: parse_usize("CONTENT_MAX_CHARS", 5_000),
            dev_api_token: env::var("DEV_API_TOKEN").ok(),
            dev_sms_code: env_or("PROMPTSTDIO_DEV_SMS_CODE", "123456"),
            sms_code_ttl_secs: parse_u64("PROMPTSTDIO_SMS_CODE_TTL_SECS", 300),
            sms_provider: env_or("PROMPTSTDIO_SMS_PROVIDER", "dev"),
            sms_webhook_url: env::var("PROMPTSTDIO_SMS_WEBHOOK_URL")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            aliyun_sms: crate::aliyun_sms::AliyunSmsConfig::from_env(),
            dev_email_code: env::var("PROMPTSTDIO_DEV_EMAIL_CODE")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| env_or("PROMPTSTDIO_DEV_SMS_CODE", "123456")),
            email_code_ttl_secs: parse_u64("PROMPTSTDIO_EMAIL_CODE_TTL_SECS", 300),
            email_provider: env_or("PROMPTSTDIO_EMAIL_PROVIDER", "dev"),
            email_webhook_url: env::var("PROMPTSTDIO_EMAIL_WEBHOOK_URL")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            smtp: crate::smtp::SmtpConfig::from_env(),
            garage_endpoint: env_or("GARAGE_ENDPOINT", ""),
            garage_bucket: env_or("GARAGE_BUCKET", ""),
            garage_access_key: env_or("GARAGE_ACCESS_KEY", ""),
            garage_secret_key: env_or("GARAGE_SECRET_KEY", ""),
            app_env,
            app_key: env::var("APP_KEY").ok().filter(|s| !s.trim().is_empty()),
            etl_surql_path: env::var("PROMPTSTDIO_ETL_SURQL")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            skip_dev_seed: parse_bool("PROMPTSTDIO_SKIP_DEV_SEED", false),
            etl_dev_token_email: env::var("PROMPTSTDIO_ETL_DEV_TOKEN_EMAIL")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            cutover_notice: parse_bool("PROMPTSTDIO_CUTOVER_NOTICE", false),
            platform_operator_emails: parse_email_list("PROMPTSTDIO_PLATFORM_OPERATOR_EMAILS"),
            platform_operator_phones: parse_email_list("PROMPTSTDIO_PLATFORM_OPERATOR_PHONES"),
            app_public_url,
            github_oauth,
            email_password_auth_enabled: email_password_enabled,
            wechat_pay_enabled: parse_bool("PROMPTSTDIO_WECHAT_PAY_ENABLED", false),
            wechat_pay_mch_id: env_first_nonempty(&[
                "PROMPTSTDIO_WECHAT_PAY_MCH_ID",
                "PROMPTSTDIO_WECHAT_PAY_MCHID",
            ]),
            wechat_pay_api_v3_key: env::var("PROMPTSTDIO_WECHAT_PAY_API_V3_KEY")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            wechat_pay_serial_no: env::var("PROMPTSTDIO_WECHAT_PAY_SERIAL_NO")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            wechat_pay_private_key_path: env::var("PROMPTSTDIO_WECHAT_PAY_PRIVATE_KEY_PATH")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            wechat_pay_notify_url: env::var("PROMPTSTDIO_WECHAT_PAY_NOTIFY_URL")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            wechat_pay_app_id: env::var("PROMPTSTDIO_WECHAT_PAY_APPID")
                .ok()
                .filter(|s| !s.trim().is_empty()),
            wechat_pay_platform_cert_path: env::var("PROMPTSTDIO_WECHAT_PAY_PLATFORM_CERT_PATH")
                .ok()
                .filter(|s| !s.trim().is_empty()),
        }
    }
}

fn email_password_auth_enabled(app_env: &str) -> bool {
    match env::var("PROMPTSTDIO_EMAIL_PASSWORD_AUTH").ok().as_deref() {
        Some(v) => matches!(v.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on"),
        None => !app_env.eq_ignore_ascii_case("production"),
    }
}

fn parse_email_list(key: &str) -> Vec<String> {
    env::var(key)
        .ok()
        .map(|raw| {
            raw.split(',')
                .map(|s| s.trim().to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default()
}

fn env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.into())
}

/// First non-empty env var among `keys` (supports legacy aliases).
fn env_first_nonempty(keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| env::var(key).ok().filter(|s| !s.trim().is_empty()))
}

fn parse_u64(key: &str, default: u64) -> u64 {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn parse_i64(key: &str, default: i64) -> i64 {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn parse_usize(key: &str, default: usize) -> usize {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

fn parse_bool(key: &str, default: bool) -> bool {
    match env::var(key).ok().map(|v| v.to_ascii_lowercase()) {
        Some(v) if matches!(v.as_str(), "1" | "true" | "yes" | "on") => true,
        Some(v) if matches!(v.as_str(), "0" | "false" | "no" | "off") => false,
        Some(_) => default,
        None => default,
    }
}

#[cfg(test)]
mod startup_tests {
    use super::AppConfig;

    #[test]
    fn production_rejects_default_dev_token() {
        let mut cfg = AppConfig::from_env();
        cfg.app_env = "production".into();
        cfg.dev_api_token = Some("dev-token-change-me".into());
        assert!(cfg.validate_for_startup().is_err());
    }

    #[test]
    fn production_requires_app_key() {
        let mut cfg = AppConfig::from_env();
        cfg.app_env = "production".into();
        cfg.dev_api_token = None;
        cfg.sms_provider = "log".into();
        cfg.email_provider = "log".into();
        cfg.app_key = None;
        assert!(cfg.validate_for_startup().is_err());
        cfg.app_key = Some("a".repeat(32));
        assert!(cfg.validate_for_startup().is_ok());
    }

    #[test]
    fn local_allows_dev_sms() {
        let mut cfg = AppConfig::from_env();
        cfg.app_env = "local".into();
        cfg.sms_provider = "dev".into();
        assert!(cfg.validate_for_startup().is_ok());
    }

    #[test]
    fn aliyun_provider_requires_config() {
        let mut cfg = AppConfig::from_env();
        cfg.app_env = "production".into();
        cfg.dev_api_token = None;
        cfg.app_key = Some("a".repeat(32));
        cfg.sms_provider = "aliyun".into();
        cfg.email_provider = "log".into();
        cfg.aliyun_sms = None;
        assert!(cfg.validate_for_startup().is_err());
        cfg.aliyun_sms = Some(crate::aliyun_sms::AliyunSmsConfig {
            access_key_id: "id".into(),
            access_key_secret: "secret".into(),
            sign_name: "签名".into(),
            template_code: "SMS_123".into(),
            region_id: "cn-hangzhou".into(),
        });
        assert!(cfg.validate_for_startup().is_ok());
    }

    #[test]
    fn smtp_provider_requires_config() {
        let mut cfg = AppConfig::from_env();
        cfg.app_env = "production".into();
        cfg.dev_api_token = None;
        cfg.app_key = Some("a".repeat(32));
        cfg.sms_provider = "log".into();
        cfg.email_provider = "dev".into();
        assert!(cfg.validate_for_startup().is_err());
        cfg.email_provider = "smtp".into();
        cfg.smtp = None;
        assert!(cfg.validate_for_startup().is_err());
        cfg.smtp = Some(crate::smtp::SmtpConfig {
            host: "smtp.example.com".into(),
            port: 587,
            username: "user".into(),
            password: "pass".into(),
            from: "PromptStdio <noreply@example.com>".into(),
        });
        assert!(cfg.validate_for_startup().is_ok());
    }
}
