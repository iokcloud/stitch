pub const MCP_DEFAULT_LIMIT: i64 = 15;
pub const MCP_MAX_LIMIT: i64 = 50;
pub const RECOMMENDED_MDC_TARGET: &str = ".cursor/rules/promptstdio-applied.mdc";

/// Web 登录/注册：每 IP 每分钟（单进程；生产 nginx 叠加 `ps_auth` zone）。
pub const WEB_AUTH_RATE_PER_MINUTE: usize = 10;
/// 短信验证码发送：每 IP 每分钟。
pub const WEB_SMS_RATE_PER_MINUTE: usize = 5;
/// 邮箱验证码发送：每 IP 每分钟。
pub const WEB_EMAIL_RATE_PER_MINUTE: usize = 5;
/// usage-logs track：每用户每分钟（API 层）。
pub const USAGE_LOG_TRACK_RATE_PER_MINUTE: usize = 60;

/// HTTP JSON body 上限（字节）；与 `content_max_chars` 协调，含 JSON 字段开销。
/// 512 KB 为 MCP `create_task_suite`（多步骤内联提示词）与 agent 配置留出余量。
pub const HTTP_MAX_BODY_BYTES: usize = 512 * 1024;

/// 每用户最多保留的 API Key 数量。
pub const MAX_API_TOKENS: usize = 5;

/// ── 会员配额 ──
/// 免费用户：个人提示词上限
pub const FREE_MAX_PROMPTS: i64 = 50;
/// 免费用户：个人任务套件上限
pub const FREE_MAX_SUITES: i64 = 5;
/// 免费用户：任务智能体上限
pub const FREE_MAX_AGENTS: i64 = 3;
/// 会员用户：个人提示词上限
pub const MEMBER_MAX_PROMPTS: i64 = 500;
/// 会员用户：个人任务套件上限
pub const MEMBER_MAX_SUITES: i64 = 50;
/// 会员用户：任务智能体上限
pub const MEMBER_MAX_AGENTS: i64 = 30;

/// 到期前 N 天显示续费提醒
pub const EXPIRY_REMINDER_DAYS: i64 = 7;
/// 到期后宽限期（天）
pub const GRACE_PERIOD_DAYS: i64 = 7;
