//! 用户可见验证/提示消息集中管理。
//!
//! 当前仅中文；日后 i18n 时可改为按 locale key 查表，函数签名不变。

// ── 通用 ──────────────────────────────────────

pub const ALREADY_EXISTS: &str = "数据已存在，请检查输入";
pub const INVALID_REQUEST_BODY: &str = "无效的请求体";

pub fn unsupported_format(fmt: &str) -> String {
    format!("不支持的格式：{fmt}")
}

// ── 认证 · 注册 / 登录 ─────────────────────────

pub const EMAIL_INVALID: &str = "请填写有效邮箱";
pub const EMAIL_NOT_SUPPORT_CODE_LOGIN: &str = "该邮箱不支持验证码登录";
pub const EMAIL_ALREADY_REGISTERED: &str = "该邮箱已注册，请直接登录";
pub const EMAIL_NOT_REGISTERED: &str = "该邮箱未注册";
pub const EMAIL_BOUND_OTHER_GITHUB: &str = "该邮箱已绑定其他 GitHub 账号";
pub const EMAIL_ADDRESS_INVALID: &str = "邮箱地址无效";
pub const EMAIL_CODE_REQUIRED: &str = "请输入验证码";
pub const EMAIL_CODE_INVALID: &str = "验证码无效或已过期";
pub const EMAIL_PASSWORD_WRONG: &str = "邮箱或密码不正确";
pub const EMAIL_NO_PASSWORD_SET: &str = "该账号未设置密码，请用验证码登录";
pub const EMAIL_RESET_LINK_INVALID: &str = "链接无效或已过期，请重新获取";
pub const EMAIL_WEBHOOK_NOT_CONFIGURED: &str = "PROMPTSTDIO_EMAIL_WEBHOOK_URL 未配置";
pub const SMTP_NOT_CONFIGURED: &str = "SMTP 未配置";

pub const PHONE_INVALID: &str = "请填写有效的大陆手机号（11 位）";
pub const PHONE_NOT_REGISTERED: &str = "该手机号未注册";
pub const PHONE_NO_PASSWORD_SET: &str = "该账号未设置密码，请使用验证码登录后在个人设置中设置密码";
pub const PHONE_PASSWORD_WRONG: &str = "密码不正确";
pub const SMS_CODE_REQUIRED: &str = "请输入验证码";
pub const SMS_CODE_INVALID: &str = "验证码无效或已过期";
pub const SMS_WEBHOOK_NOT_CONFIGURED: &str = "PROMPTSTDIO_SMS_WEBHOOK_URL 未配置";
pub const ALIYUN_SMS_NOT_CONFIGURED: &str = "阿里云短信未配置";

pub const PASSWORD_MIN_LENGTH: &str = "密码至少 8 个字符";
pub const PASSWORD_REQUIRED: &str = "请输入密码";

pub fn sms_provider_unknown(provider: &str) -> String {
    format!("短信服务未配置（provider={provider}），请使用邮箱登录")
}
pub fn email_provider_unknown(provider: &str) -> String {
    format!("邮件服务未配置（provider={provider}），请使用密码登录")
}

// ── 认证 · GitHub OAuth ────────────────────────

pub const GITHUB_AUTH_FAILED: &str = "GitHub 授权失败，请重试";
pub const GITHUB_ACCOUNT_INVALID: &str = "GitHub 账号无效";
pub const GITHUB_NO_ACCESS_TOKEN: &str = "GitHub 未返回 access token";
pub const GITHUB_CANNOT_READ_USER: &str = "无法读取 GitHub 账号信息";

pub fn github_auth_error(err: &str, detail: &str) -> String {
    format!("GitHub 授权失败：{err} {detail}")
}

// ── 个人设置 ──────────────────────────────────

pub const NAME_INVALID: &str = "请填写有效昵称（最多 100 字）";
pub const PASSWORD_MISMATCH: &str = "两次输入的新密码不一致";
pub const CURRENT_PASSWORD_WRONG: &str = "当前密码不正确";
pub const PASSWORD_ALREADY_SET: &str = "已设置过密码，请使用密码修改功能";
pub const DELETE_WRONG_PASSWORD: &str = "密码不正确，无法删除账号";
pub const DELETE_NEED_PHONE_CODE: &str = "请提供手机号和验证码以验证身份";
pub const DELETE_NEED_CODE: &str = "请提供验证码以验证身份";
pub const NO_PASSWORD_CONTACT_SUPPORT: &str = "该账号未设置密码，请用手机验证码登录后联系支持";

// ── 提示词 ────────────────────────────────────

pub const PROMPT_NO_CONTENT_TO_APPLY: &str = "该提示词没有可写入的内容";
pub const API_KEY_NAME_INVALID: &str = "请填写有效名称（最多 100 字）";

pub fn prompt_content_too_long(max_chars: usize) -> String {
    format!("正文不得超过 {max_chars} 字")
}

// ── 剪贴板 / 入库 ─────────────────────────────

pub const CLIPBOARD_EMPTY: &str = "请粘贴包含入库 JSON 的内容";
pub const CLIPBOARD_NO_MARKER: &str =
    "内容中未找到 promptstdio-harvest 标记，请确认已从 AI 复制正确格式";
pub const CLIPBOARD_JSON_INVALID: &str = "JSON 解析失败，请确认格式正确";
pub const CLIPBOARD_TITLE_CONTENT_REQUIRED: &str = "JSON 中 title 与 content 为必填";
pub const CLIPBOARD_TITLE_TOO_LONG: &str = "title 超过 255 字符上限";
pub const CLIPBOARD_CONTENT_TOO_LONG: &str = "content 超过 5000 字上限";

// ── 复利规则 ──────────────────────────────────

pub const HARVEST_TITLE_CONTENT_REQUIRED: &str = "title 与 content 为必填";
pub const HARVEST_CONTENT_TOO_LONG: &str = "content 超过字数限制";
pub const HARVEST_STEPS_REQUIRED: &str = "steps 为必填";
pub const HARVEST_STEPS_NOT_ARRAY: &str = "steps 须为 JSON 数组";
pub const HARVEST_STEP_PROMPT_ID_REQUIRED: &str = "每步须含 prompt_id";
pub const HARVEST_STEP_USER_PROMPT_ONLY: &str = "MCP 套件步骤仅可关联个人提示词";
pub const HARVEST_MIN_STEPS: &str = "提示词采集器套件至少需 2 个步骤";
pub const HARVEST_RULES_DETECTION_REQUIRED: &str = "请填写识别标准";
pub const HARVEST_RULES_FREQUENCY_REQUIRED: &str = "请填写询问频率";
pub const HARVEST_RULES_ASK_STYLE_INVALID: &str = "询问方式无效";
pub const HARVEST_RULES_LOW_SCORE_RANGE: &str = "低分提示阈值须在 0 到 100 之间";
pub const HARVEST_RULES_TAG_COUNT_RANGE: &str = "标签数量上限须在 1 到 10 之间";
pub const HARVEST_RULES_SAME_TAG_RANGE: &str = "同标签提示数阈值须在 1 到 50 之间";

pub fn harvest_score_range(key: &str) -> String {
    format!("{key} 评分须为 0–100 整数")
}
pub fn harvest_tags_max(max_count: usize) -> String {
    format!("标签最多 {max_count} 个")
}
pub fn harvest_tags_invalid(invalid: &str) -> String {
    format!("无效的标签：{invalid}")
}

// ── 任务套件 ──────────────────────────────────

pub const SUITE_MIN_STEPS: &str = "提示词采集器套件至少需 2 个步骤";
pub const SUITE_NO_COPYABLE_STEPS: &str = "套件没有可复制的步骤";
pub const SUITE_MEMBER_ONLY_SAVE: &str = "须开通会员才能保存此工具包。请前往会员专区。";
pub const SUITE_STEP_NOT_FOUND: &str = "步骤不存在";
pub const SUITE_SYSTEM_STEPS_IMMUTABLE: &str =
    "含官方快照步骤的套件不可在 Web 改步骤，仅可改名称与标签";

// ── 项目记忆 ──────────────────────────────────

pub const PROJECT_MEMORY_HEADING_BODY_REQUIRED: &str = "heading 与 body 为必填";

pub fn project_memory_file_write_denied(file: &str) -> String {
    format!("当前智能体配置不允许写入文件，请在 PromptStdio Web 中调整 agent 权限（文件：{file}）")
}
pub fn project_memory_file_too_long(file: &str, limit: usize) -> String {
    format!("{file} 超过 {limit} KB 上限")
}

// ── 导出 ──────────────────────────────────────

pub const EXPORT_TOO_MANY_PROMPTS: &str = "批量导出提示词过多，请减少选择";

// ── Explore ──────────────────────────────────

pub const EXPLORE_SAVE_ONLY_SYSTEM: &str = "仅可保存官方系统提示词";
pub const EXPLORE_AGENT_SUITE_NOT_FOUND: &str = "未找到对应的官方系统套件";

// ── MCP 集成 ──────────────────────────────────

pub const MCP_CLIENT_UNSUPPORTED: &str = "不支持的 MCP 客户端。";
pub const MCP_TOKEN_INVALID: &str = "无效的 Token";

// ── API Key ───────────────────────────────────

pub const API_KEY_NAME_INVALID_2: &str = "请填写有效名称（最多 100 字）";

// ── 回收站 ────────────────────────────────────

pub const RECYCLE_SUITE_HAS_ACTIVE_AGENT: &str = "该套件存在活跃的智能体，请先删除智能体";
pub const RECYCLE_PROMPT_IN_USE_BY_SUITE: &str = "该提示词仍被以下套件使用（不会影响删除操作）";

// ── 删除 / 防冲突 ─────────────────────────────

pub fn suite_has_active_agent(name: &str) -> String {
    format!("套件「{name}」存在活跃的智能体，请先删除智能体再删除套件")
}
pub fn prompt_in_use_by_suites(names: &str) -> String {
    format!("该提示词被以下套件引用：{names}")
}
