use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// One saved LLM connection (provider + endpoint + key + default model).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LlmProfile {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default = "default_llm_provider")]
    pub provider: String,
    #[serde(default = "default_llm_api_base")]
    pub api_base: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_llm_model")]
    pub model: String,
}

/// Resolved credentials for one chat / agent call.
#[derive(Debug, Clone)]
pub struct ResolvedLlm {
    pub profile_id: Option<String>,
    pub provider: String,
    pub api_base: String,
    pub api_key: String,
    pub model: String,
}

/// One saved PromptStdio (cloud asset) connection — settings「账号」.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpProfile {
    pub id: String,
    #[serde(default)]
    pub label: String,
    #[serde(default = "default_api_base")]
    pub api_base: String,
    #[serde(default)]
    pub api_token: Option<String>,
}

/// Resolved PromptStdio credentials for suite / sediment / membership calls.
#[derive(Debug, Clone)]
pub struct ResolvedMcp {
    pub profile_id: Option<String>,
    pub api_base: String,
    pub api_token: String,
}

/// One standard MCP protocol server — settings「MCP」.
///
/// Field shape aligns with mainstream clients (Cursor / Claude Desktop `mcpServers`):
/// `command` + `args` + `env` + optional `cwd` for stdio; `url` + `headers` for remote.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpServerProfile {
    pub id: String,
    #[serde(default)]
    pub label: String,
    /// `"stdio"`, `"http"` (Streamable HTTP), or `"sse"` (legacy remote; connected via HTTP client).
    #[serde(default = "default_mcp_transport")]
    pub transport: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// stdio: executable
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// stdio: working directory (Cursor `cwd`)
    #[serde(default)]
    pub cwd: Option<String>,
    /// http/sse: MCP endpoint URL
    #[serde(default)]
    pub url: Option<String>,
    #[serde(default)]
    pub headers: HashMap<String, String>,
}

fn default_mcp_transport() -> String {
    "stdio".into()
}

fn default_true() -> bool {
    true
}

/// Stitch configuration stored at ~/.config/stitch/config.toml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StitchConfig {
    /// PromptStdio API base URL (mirrors active account profile).
    #[serde(default = "default_api_base")]
    pub api_base: String,

    /// API token for PromptStdio authentication (mirrors active account profile).
    #[serde(default)]
    pub api_token: Option<String>,

    /// Saved PromptStdio connections. Empty until first load seeds from flat fields.
    #[serde(default)]
    pub mcp_profiles: Vec<McpProfile>,

    /// Which account profile backs the flat `api_*` fields.
    #[serde(default)]
    pub active_mcp_id: Option<String>,

    /// Standard MCP protocol servers (tools injected into the agent).
    #[serde(default)]
    pub mcp_servers: Vec<McpServerProfile>,

    /// LLM provider (e.g. "openai", "anthropic").
    #[serde(default = "default_llm_provider")]
    pub llm_provider: String,

    /// LLM API base URL. Defaults to OpenAI, can be overridden for proxies.
    #[serde(default = "default_llm_api_base")]
    pub llm_api_base: String,

    /// LLM API key. Falls back to `STITCH_LLM_API_KEY` or `OPENAI_API_KEY` env vars.
    #[serde(default)]
    pub llm_api_key: Option<String>,

    /// LLM model name.
    #[serde(default = "default_llm_model")]
    pub llm_model: String,

    /// Saved LLM connections. Empty until first load seeds from flat fields.
    #[serde(default)]
    pub llm_profiles: Vec<LlmProfile>,

    /// Which profile backs the flat `llm_*` fields (CLI / new sessions).
    #[serde(default)]
    pub active_profile_id: Option<String>,

    /// Max agent loop iterations.
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,

    /// Last project working directory (desktop Agent tools root).
    #[serde(default)]
    pub work_dir: Option<String>,

    /// Sediment save target: personal library only, or also submit Explore review (ADR-033).
    #[serde(default = "default_sediment_visibility")]
    pub sediment_visibility: String,

    /// Local vision model used to describe images for text-only models
    /// (e.g. Ollama qwen3-vl). Enabled by default — the defaults point at a
    /// local Ollama, so a fresh install works out of the box; set
    /// `enabled = false` to turn the describe layer off.
    #[serde(default)]
    pub local_vision: LocalVisionConfig,

    /// 权限模式（default / accept_edits / plan / bypass），CLI 启动默认值。
    #[serde(default)]
    pub permission_mode: Option<String>,

    /// deny 规则：禁用的工具名列表（始终生效，含 bypass）。
    #[serde(default)]
    pub disallowed_tools: Vec<String>,

    /// statusLine：每回合结束执行的 shell 命令（stdout 显示为状态行）。
    #[serde(default)]
    pub statusline: Option<String>,
}

/// Local vision describe layer (DeepSeek + local VL as the "eyes").
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalVisionConfig {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default = "default_local_vision_api_base")]
    pub api_base: String,
    #[serde(default = "default_local_vision_model")]
    pub model: String,
    #[serde(default = "default_local_vision_timeout_secs")]
    pub timeout_secs: u64,
}

impl Default for LocalVisionConfig {
    fn default() -> Self {
        Self {
            enabled: default_true(),
            api_base: default_local_vision_api_base(),
            model: default_local_vision_model(),
            timeout_secs: default_local_vision_timeout_secs(),
        }
    }
}

fn default_local_vision_api_base() -> String {
    "http://127.0.0.1:11434/v1".into()
}

fn default_local_vision_model() -> String {
    "qwen3-vl:8b".into()
}

fn default_local_vision_timeout_secs() -> u64 {
    30
}

fn default_sediment_visibility() -> String {
    "explore".into()
}

fn default_api_base() -> String {
    "https://www.promptstdio.com".into()
}

/// Apex `promptstdio.com` 301 → www；reqwest 跨主机跳转会丢掉 Authorization，探测会误报 Token 无效。
pub fn normalize_promptstdio_api_base(base: &str) -> Option<&'static str> {
    let t = base.trim().trim_end_matches('/');
    match t.to_ascii_lowercase().as_str() {
        "https://promptstdio.com" | "http://promptstdio.com" | "http://www.promptstdio.com" => {
            Some("https://www.promptstdio.com")
        }
        _ => None,
    }
}

fn default_llm_provider() -> String {
    "deepseek".into()
}

fn default_llm_api_base() -> String {
    // Official OpenAI-compatible base_url (api-docs.deepseek.com): no /v1.
    // Client appends `/chat/completions`.
    "https://api.deepseek.com".into()
}

fn default_llm_model() -> String {
    // DeepSeek retired deepseek-chat / deepseek-reasoner after 2026-07-24 UTC.
    "deepseek-v4-flash".into()
}

/// Map retired DeepSeek aliases to current V4 model ids.
///
/// Official cutoff: 2026-07-24 15:59 UTC — legacy names become inaccessible.
pub fn migrate_llm_model(model: &str) -> Option<&'static str> {
    // Official mapping: both aliases → deepseek-v4-flash (thinking is a
    // separate request flag; agent path keeps thinking disabled).
    match model {
        "deepseek-chat" | "deepseek-reasoner" => Some("deepseek-v4-flash"),
        _ => None,
    }
}

/// Official DeepSeek OpenAI base is `https://api.deepseek.com` (not `…/v1`).
pub fn migrate_llm_api_base(base: &str) -> Option<&'static str> {
    let t = base.trim().trim_end_matches('/');
    if t.eq_ignore_ascii_case("https://api.deepseek.com/v1") {
        Some("https://api.deepseek.com")
    } else {
        None
    }
}

fn default_max_iterations() -> usize {
    25
}

fn default_profile_label(provider: &str) -> String {
    match provider {
        "openai" => "OpenAI".into(),
        "deepseek" => "DeepSeek".into(),
        "anthropic" => "Anthropic".into(),
        "zhipu" => "智谱".into(),
        "kimi" => "Kimi".into(),
        "minimax" => "MiniMax".into(),
        "ollama" => "Ollama 本地".into(),
        "custom" => "自定义".into(),
        _ => "自定义".into(),
    }
}

/// Strip `/chat/completions` / `/v1/responses` (and trailing slashes) so pasted
/// full URLs become API roots.
pub fn normalize_openai_compatible_base(raw: &str) -> String {
    let mut s = raw.trim().trim_end_matches('/').to_string();
    for suffix in [
        "/chat/completions",
        "/completions",
        "/v1/responses",
        "/responses",
    ] {
        if s.to_ascii_lowercase().ends_with(suffix) {
            s.truncate(s.len() - suffix.len());
            s = s.trim_end_matches('/').to_string();
            break;
        }
    }
    s
}

impl Default for StitchConfig {
    fn default() -> Self {
        Self {
            api_base: default_api_base(),
            api_token: None,
            mcp_profiles: Vec::new(),
            active_mcp_id: None,
            mcp_servers: Vec::new(),
            llm_provider: default_llm_provider(),
            llm_api_base: default_llm_api_base(),
            llm_api_key: None,
            llm_model: default_llm_model(),
            llm_profiles: Vec::new(),
            active_profile_id: None,
            local_vision: LocalVisionConfig::default(),
            max_iterations: default_max_iterations(),
            work_dir: None,
            sediment_visibility: default_sediment_visibility(),
            permission_mode: None,
            disallowed_tools: Vec::new(),
            statusline: None,
        }
    }
}

impl StitchConfig {
    /// Load config from the standard XDG path, falling back to defaults.
    pub fn load() -> anyhow::Result<Self> {
        let path = config_path();
        if path.exists() {
            let content = std::fs::read_to_string(&path)?;
            let mut cfg: Self = toml::from_str(&content)?;
            cfg.apply_env_overrides();
            // After env overrides — legacy aliases in file or STITCH_LLM_MODEL.
            let mut dirty = cfg.apply_model_migration();
            dirty |= cfg.apply_api_base_migration();
            dirty |= cfg.ensure_profiles_seeded();
            dirty |= cfg.ensure_mcp_profiles_seeded();
            // Older provision scripts wrote `…/stitch/config.toml` (missing Windows
            // `directories` extra `config/` segment). Pull Token so red-dots clear.
            dirty |= cfg.migrate_legacy_provision_token();
            // Persist alias → V4 rewrite / profile seed so desktop settings stay current.
            if dirty && let Err(e) = cfg.save() {
                tracing::warn!(error = %e, "failed to persist migrated config");
            }
            Ok(cfg)
        } else if let Some(legacy) = load_raw_config_file(&legacy_config_path()) {
            // First run after path fix: adopt legacy file as the active config.
            let mut cfg = legacy;
            cfg.apply_env_overrides();
            let _ = cfg.apply_model_migration();
            let _ = cfg.apply_api_base_migration();
            let _ = cfg.ensure_profiles_seeded();
            let _ = cfg.ensure_mcp_profiles_seeded();
            cfg.sync_active_mcp_from_flat();
            if let Err(e) = cfg.save() {
                tracing::warn!(error = %e, "failed to persist migrated legacy config");
            }
            Ok(cfg)
        } else {
            let mut cfg = Self::default();
            cfg.apply_env_overrides();
            let _ = cfg.apply_model_migration();
            let _ = cfg.apply_api_base_migration();
            let _ = cfg.ensure_profiles_seeded();
            let _ = cfg.ensure_mcp_profiles_seeded();
            Ok(cfg)
        }
    }

    /// If active config has no account Token, copy from legacy provision path.
    fn migrate_legacy_provision_token(&mut self) -> bool {
        let flat_missing = self
            .api_token
            .as_deref()
            .map(|t| t.trim().is_empty())
            .unwrap_or(true);
        let profiles_missing = self.mcp_profiles.iter().all(|p| {
            p.api_token
                .as_deref()
                .map(|t| t.trim().is_empty())
                .unwrap_or(true)
        });
        if !flat_missing && !profiles_missing {
            return false;
        }
        let legacy_path = legacy_config_path();
        if legacy_path == config_path() || !legacy_path.exists() {
            return false;
        }
        let Some(legacy) = load_raw_config_file(&legacy_path) else {
            return false;
        };
        let Some(tok) = legacy.api_token.filter(|t| !t.trim().is_empty()) else {
            return false;
        };
        if flat_missing {
            self.api_token = Some(tok);
            if self.api_base.trim().is_empty() && !legacy.api_base.trim().is_empty() {
                self.api_base = legacy.api_base;
            }
        }
        self.sync_active_mcp_from_flat();
        tracing::info!(
            from = %legacy_path.display(),
            "migrated api_token from legacy provision path"
        );
        true
    }

    /// Rewrite retired DeepSeek model aliases and legacy API bases. Returns `true` if changed.
    pub fn apply_model_migration(&mut self) -> bool {
        let mut changed = false;
        if let Some(next) = migrate_llm_model(&self.llm_model) {
            tracing::info!(
                from = %self.llm_model,
                to = next,
                "migrated retired DeepSeek model alias"
            );
            self.llm_model = next.to_string();
            changed = true;
        }
        if let Some(next) = migrate_llm_api_base(&self.llm_api_base) {
            tracing::info!(
                from = %self.llm_api_base,
                to = next,
                "migrated DeepSeek API base to official root"
            );
            self.llm_api_base = next.to_string();
            changed = true;
        }
        for p in &mut self.llm_profiles {
            if let Some(next) = migrate_llm_model(&p.model) {
                p.model = next.to_string();
                changed = true;
            }
            if let Some(next) = migrate_llm_api_base(&p.api_base) {
                p.api_base = next.to_string();
                changed = true;
            }
        }
        changed
    }

    /// Apex host → www so REST clients keep Authorization across redirects.
    pub fn apply_api_base_migration(&mut self) -> bool {
        let mut changed = false;
        if let Some(next) = normalize_promptstdio_api_base(&self.api_base) {
            tracing::info!(
                from = %self.api_base,
                to = next,
                "migrated PromptStdio API base to www"
            );
            self.api_base = next.to_string();
            changed = true;
        }
        for p in &mut self.mcp_profiles {
            if let Some(next) = normalize_promptstdio_api_base(&p.api_base) {
                p.api_base = next.to_string();
                changed = true;
            }
        }
        for s in &mut self.mcp_servers {
            if let Some(url) = s.url.as_mut() {
                let trimmed = url.trim().trim_end_matches('/');
                if let Some(rest) = trimmed
                    .strip_prefix("https://promptstdio.com")
                    .or_else(|| trimmed.strip_prefix("http://promptstdio.com"))
                    .or_else(|| trimmed.strip_prefix("http://www.promptstdio.com"))
                {
                    *url = format!("https://www.promptstdio.com{rest}");
                    changed = true;
                }
            }
        }
        changed
    }

    /// If `llm_profiles` is empty, seed one profile from flat `llm_*` fields.
    /// Returns `true` if the in-memory config changed.
    /// If `llm_profiles` is empty, seed one profile from flat `llm_*` fields.
    /// Returns `true` if the in-memory config changed.
    pub fn ensure_llm_profiles_seeded(&mut self) -> bool {
        self.ensure_profiles_seeded()
    }

    pub fn ensure_profiles_seeded(&mut self) -> bool {
        if !self.llm_profiles.is_empty() {
            // Keep active id valid when possible.
            if self.active_profile_id.is_none() {
                self.active_profile_id = Some(self.llm_profiles[0].id.clone());
                return true;
            }
            let active = self.active_profile_id.as_deref().unwrap_or("");
            if !self.llm_profiles.iter().any(|p| p.id == active) {
                self.active_profile_id = Some(self.llm_profiles[0].id.clone());
                return true;
            }
            return false;
        }
        let id = "default".to_string();
        let label = default_profile_label(&self.llm_provider);
        self.llm_profiles.push(LlmProfile {
            id: id.clone(),
            label,
            provider: self.llm_provider.clone(),
            api_base: self.llm_api_base.clone(),
            api_key: self.llm_api_key.clone(),
            model: self.llm_model.clone(),
        });
        self.active_profile_id = Some(id);
        true
    }

    /// Look up a saved profile by id.
    pub fn profile(&self, id: &str) -> Option<&LlmProfile> {
        self.llm_profiles.iter().find(|p| p.id == id)
    }

    /// Copy a profile into the flat `llm_*` fields.
    pub fn activate_profile(&mut self, id: &str) -> anyhow::Result<()> {
        let profile = self
            .llm_profiles
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("找不到模型配置：{id}"))?;
        self.llm_provider = profile.provider;
        self.llm_api_base = profile.api_base;
        self.llm_api_key = profile.api_key;
        self.llm_model = profile.model;
        self.active_profile_id = Some(profile.id);
        Ok(())
    }

    /// Insert or replace a profile by id. Empty `api_key` on update keeps the previous key.
    pub fn upsert_profile(&mut self, mut profile: LlmProfile) -> anyhow::Result<()> {
        let id = profile.id.trim();
        if id.is_empty() {
            anyhow::bail!("配置 id 不能为空");
        }
        profile.id = id.to_string();
        if profile.label.trim().is_empty() {
            profile.label = default_profile_label(&profile.provider);
        }
        if let Some(next) = migrate_llm_model(&profile.model) {
            profile.model = next.to_string();
        }
        profile.api_base = normalize_openai_compatible_base(&profile.api_base);
        if let Some(next) = migrate_llm_api_base(&profile.api_base) {
            profile.api_base = next.to_string();
        }
        if let Some(existing) = self.llm_profiles.iter().find(|p| p.id == profile.id)
            && profile
                .api_key
                .as_deref()
                .map(|k| k.trim().is_empty())
                .unwrap_or(true)
        {
            profile.api_key = existing.api_key.clone();
        }
        if let Some(slot) = self.llm_profiles.iter_mut().find(|p| p.id == profile.id) {
            *slot = profile.clone();
        } else {
            self.llm_profiles.push(profile.clone());
        }
        // Keep flat fields in sync when editing the active profile.
        let active = self.active_profile_id.clone().unwrap_or_default();
        if active.is_empty() || active == profile.id {
            self.activate_profile(&profile.id)?;
        }
        Ok(())
    }

    /// Remove a profile. If it was active, activate another or clear active.
    pub fn delete_profile(&mut self, id: &str) -> anyhow::Result<()> {
        let before = self.llm_profiles.len();
        self.llm_profiles.retain(|p| p.id != id);
        if self.llm_profiles.len() == before {
            anyhow::bail!("找不到模型配置：{id}");
        }
        if self.active_profile_id.as_deref() == Some(id) {
            if let Some(next) = self.llm_profiles.first().cloned() {
                self.activate_profile(&next.id)?;
            } else {
                self.active_profile_id = None;
            }
        }
        Ok(())
    }

    /// If `mcp_profiles` is empty, seed one profile from flat `api_*` fields.
    pub fn ensure_mcp_profiles_seeded(&mut self) -> bool {
        if !self.mcp_profiles.is_empty() {
            if self.active_mcp_id.is_none() {
                self.active_mcp_id = Some(self.mcp_profiles[0].id.clone());
                return true;
            }
            let active = self.active_mcp_id.as_deref().unwrap_or("");
            if !self.mcp_profiles.iter().any(|p| p.id == active) {
                self.active_mcp_id = Some(self.mcp_profiles[0].id.clone());
                return true;
            }
            return false;
        }
        let id = "default".to_string();
        self.mcp_profiles.push(McpProfile {
            id: id.clone(),
            label: "PromptStdio".into(),
            api_base: if self.api_base.trim().is_empty() {
                default_api_base()
            } else {
                self.api_base.clone()
            },
            api_token: self.api_token.clone(),
        });
        self.active_mcp_id = Some(id);
        true
    }

    pub fn mcp_profile(&self, id: &str) -> Option<&McpProfile> {
        self.mcp_profiles.iter().find(|p| p.id == id)
    }

    /// Copy an MCP profile into the flat `api_*` fields.
    pub fn activate_mcp_profile(&mut self, id: &str) -> anyhow::Result<()> {
        let profile = self
            .mcp_profiles
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("找不到账号配置：{id}"))?;
        self.api_base = profile.api_base;
        self.api_token = profile.api_token;
        self.active_mcp_id = Some(profile.id);
        Ok(())
    }

    /// Insert or replace an MCP profile. Empty `api_token` on update keeps the previous token.
    pub fn upsert_mcp_profile(&mut self, mut profile: McpProfile) -> anyhow::Result<()> {
        let id = profile.id.trim();
        if id.is_empty() {
            anyhow::bail!("配置 id 不能为空");
        }
        profile.id = id.to_string();
        if profile.label.trim().is_empty() {
            profile.label = "PromptStdio".into();
        }
        let base = profile.api_base.trim().trim_end_matches('/');
        profile.api_base = if base.is_empty() {
            default_api_base()
        } else if let Some(next) = normalize_promptstdio_api_base(base) {
            next.to_string()
        } else {
            base.to_string()
        };
        if let Some(existing) = self.mcp_profiles.iter().find(|p| p.id == profile.id)
            && profile
                .api_token
                .as_deref()
                .map(|k| k.trim().is_empty())
                .unwrap_or(true)
        {
            profile.api_token = existing.api_token.clone();
        }
        if let Some(slot) = self.mcp_profiles.iter_mut().find(|p| p.id == profile.id) {
            *slot = profile.clone();
        } else {
            self.mcp_profiles.push(profile.clone());
        }
        let active = self.active_mcp_id.clone().unwrap_or_default();
        if active.is_empty() || active == profile.id {
            self.activate_mcp_profile(&profile.id)?;
        }
        Ok(())
    }

    pub fn delete_mcp_profile(&mut self, id: &str) -> anyhow::Result<()> {
        let before = self.mcp_profiles.len();
        self.mcp_profiles.retain(|p| p.id != id);
        if self.mcp_profiles.len() == before {
            anyhow::bail!("找不到账号配置：{id}");
        }
        if self.active_mcp_id.as_deref() == Some(id) {
            if let Some(next) = self.mcp_profiles.first().cloned() {
                self.activate_mcp_profile(&next.id)?;
            } else {
                self.active_mcp_id = None;
            }
        }
        Ok(())
    }

    /// Clear Token on one MCP profile (and flat fields when it is active).
    pub fn clear_mcp_profile_token(&mut self, id: &str) -> anyhow::Result<()> {
        let _ = self.ensure_mcp_profiles_seeded();
        let profile = self
            .mcp_profiles
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| anyhow::anyhow!("找不到账号配置：{id}"))?;
        profile.api_token = None;
        if self.active_mcp_id.as_deref() == Some(id) {
            self.api_token = None;
        }
        Ok(())
    }

    pub fn mcp_server(&self, id: &str) -> Option<&McpServerProfile> {
        self.mcp_servers.iter().find(|p| p.id == id)
    }

    pub fn enabled_mcp_servers(&self) -> Vec<&McpServerProfile> {
        self.mcp_servers.iter().filter(|p| p.enabled).collect()
    }

    /// Insert or replace a protocol MCP server profile.
    pub fn upsert_mcp_server(&mut self, mut profile: McpServerProfile) -> anyhow::Result<()> {
        let id = profile.id.trim();
        if id.is_empty() {
            anyhow::bail!("服务 id 不能为空");
        }
        profile.id = id.to_string();
        if profile.label.trim().is_empty() {
            profile.label = profile.id.clone();
        }
        let transport = profile.transport.trim().to_ascii_lowercase();
        if transport != "stdio" && transport != "http" && transport != "sse" {
            anyhow::bail!("传输方式须为 stdio、http 或 sse");
        }
        profile.transport = transport;
        if let Some(cwd) = profile
            .cwd
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
        {
            profile.cwd = Some(cwd.to_string());
        } else {
            profile.cwd = None;
        }
        if profile.transport == "stdio" {
            let cmd = profile
                .command
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("stdio 服务须填写命令"))?;
            profile.command = Some(cmd.to_string());
        } else {
            let url = profile
                .url
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .ok_or_else(|| anyhow::anyhow!("远程服务须填写地址"))?;
            profile.url = Some(url.trim_end_matches('/').to_string());
            // Keep prior headers (incl. Authorization) when UI omits them on update.
            if let Some(existing) = self.mcp_servers.iter().find(|p| p.id == profile.id) {
                for (k, v) in &existing.headers {
                    profile
                        .headers
                        .entry(k.clone())
                        .or_insert_with(|| v.clone());
                }
            }
        }
        if let Some(slot) = self.mcp_servers.iter_mut().find(|p| p.id == profile.id) {
            *slot = profile;
        } else {
            self.mcp_servers.push(profile);
        }
        Ok(())
    }

    pub fn delete_mcp_server(&mut self, id: &str) -> anyhow::Result<()> {
        let before = self.mcp_servers.len();
        self.mcp_servers.retain(|p| p.id != id);
        if self.mcp_servers.len() == before {
            anyhow::bail!("找不到 MCP 服务：{id}");
        }
        Ok(())
    }

    pub fn set_mcp_server_enabled(&mut self, id: &str, enabled: bool) -> anyhow::Result<()> {
        let p = self
            .mcp_servers
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or_else(|| anyhow::anyhow!("找不到 MCP 服务：{id}"))?;
        p.enabled = enabled;
        Ok(())
    }

    /// Resolve PromptStdio credentials without mutating active fields.
    pub fn resolve_mcp(&self, profile_id: Option<&str>) -> anyhow::Result<ResolvedMcp> {
        if let Some(pid) = profile_id.map(|s| s.trim()).filter(|s| !s.is_empty())
            && let Some(p) = self.mcp_profiles.iter().find(|p| p.id == pid)
        {
            let token = p
                .api_token
                .as_deref()
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "账号配置「{}」未设置 Token。请在设置中补全。",
                        if p.label.is_empty() { &p.id } else { &p.label }
                    )
                })?;
            return Ok(ResolvedMcp {
                profile_id: Some(p.id.clone()),
                api_base: p.api_base.clone(),
                api_token: token.to_string(),
            });
        }
        let token = self
            .api_token
            .as_deref()
            .map(str::trim)
            .filter(|k| !k.is_empty())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "请先在设置中填写 PromptStdio API Token（当前服务地址：{}）",
                    {
                        if self.api_base.trim().is_empty() {
                            "（未设置服务地址）"
                        } else {
                            self.api_base.as_str()
                        }
                    }
                )
            })?;
        Ok(ResolvedMcp {
            profile_id: self.active_mcp_id.clone(),
            api_base: if self.api_base.trim().is_empty() {
                default_api_base()
            } else {
                self.api_base.clone()
            },
            api_token: token.to_string(),
        })
    }

    fn sync_active_mcp_from_flat(&mut self) {
        let _ = self.ensure_mcp_profiles_seeded();
        let Some(active) = self.active_mcp_id.clone() else {
            return;
        };
        if let Some(p) = self.mcp_profiles.iter_mut().find(|p| p.id == active) {
            p.api_base = self.api_base.clone();
            p.api_token = self.api_token.clone();
            if p.label.trim().is_empty() {
                p.label = "PromptStdio".into();
            }
        }
    }

    /// Resolve credentials for a chat call without mutating active fields.
    pub fn resolve_llm(
        &self,
        profile_id: Option<&str>,
        model_override: Option<&str>,
    ) -> anyhow::Result<ResolvedLlm> {
        let model_override = model_override
            .map(|m| m.trim())
            .filter(|m| !m.is_empty())
            .map(|m| migrate_llm_model(m).unwrap_or(m).to_string());

        if let Some(pid) = profile_id.map(|s| s.trim()).filter(|s| !s.is_empty())
            && let Some(p) = self.llm_profiles.iter().find(|p| p.id == pid)
        {
            let key = p
                .api_key
                .as_deref()
                .map(str::trim)
                .filter(|k| !k.is_empty())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "模型配置「{}」未设置 API Key。请在设置中补全密钥。",
                        if p.label.is_empty() { &p.id } else { &p.label }
                    )
                })?;
            let model = model_override.unwrap_or_else(|| p.model.clone());
            return Ok(ResolvedLlm {
                profile_id: Some(p.id.clone()),
                provider: p.provider.clone(),
                api_base: p.api_base.clone(),
                api_key: key.to_string(),
                model,
            });
        }
        // Unknown id → fall through to flat fields.

        let key = self.require_llm_key()?.to_string();
        let model = model_override.unwrap_or_else(|| self.llm_model.clone());
        Ok(ResolvedLlm {
            profile_id: self.active_profile_id.clone(),
            provider: self.llm_provider.clone(),
            api_base: self.llm_api_base.clone(),
            api_key: key,
            model,
        })
    }

    /// When flat `llm_*` keys are set via CLI/desktop, mirror into the active profile.
    fn sync_active_profile_from_flat(&mut self) {
        let _ = self.ensure_profiles_seeded();
        let Some(active) = self.active_profile_id.clone() else {
            return;
        };
        if let Some(p) = self.llm_profiles.iter_mut().find(|p| p.id == active) {
            p.provider = self.llm_provider.clone();
            p.api_base = self.llm_api_base.clone();
            p.api_key = self.llm_api_key.clone();
            p.model = self.llm_model.clone();
            if p.label.trim().is_empty() {
                p.label = default_profile_label(&p.provider);
            }
        }
    }

    /// Save config to the standard XDG path.
    pub fn save(&self) -> anyhow::Result<()> {
        let path = config_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let content = toml::to_string_pretty(self)?;
        std::fs::write(&path, content)?;
        tracing::info!(path = %path.display(), "config saved");
        Ok(())
    }

    /// Apply environment variable overrides (higher priority than config file).
    fn apply_env_overrides(&mut self) {
        if let Ok(key) = std::env::var("STITCH_LLM_API_KEY") {
            self.llm_api_key = Some(key);
        } else if let Ok(key) = std::env::var("OPENAI_API_KEY") {
            self.llm_api_key = Some(key);
        }
        if let Ok(base) = std::env::var("STITCH_LLM_API_BASE") {
            self.llm_api_base = base;
        } else if let Ok(base) = std::env::var("OPENAI_API_BASE") {
            self.llm_api_base = base;
        }
        if let Ok(model) = std::env::var("STITCH_LLM_MODEL") {
            self.llm_model = model;
        }
    }

    /// Resolve the effective LLM API key, returning an error if not set.
    pub fn require_llm_key(&self) -> anyhow::Result<&str> {
        self.llm_api_key.as_deref().ok_or_else(|| {
            anyhow::anyhow!(
                "No LLM API key configured.\n\
                 Set it with:\n  \
                 stitch config set llm_api_key <your-key>\n  \
                 or export STITCH_LLM_API_KEY=<your-key>"
            )
        })
    }

    /// Check if the user is authenticated with PromptStdio.
    pub fn is_logged_in(&self) -> bool {
        self.api_token.is_some()
    }

    /// Set a config value by key name.
    pub fn set(&mut self, key: &str, value: &str) -> anyhow::Result<()> {
        match key {
            "api_base" => {
                let base = value.trim().trim_end_matches('/');
                self.api_base = if base.is_empty() {
                    default_api_base()
                } else {
                    base.to_string()
                };
                self.sync_active_mcp_from_flat();
            }
            "api_token" => {
                // Empty string clears the token (logout).
                let t = value.trim();
                self.api_token = if t.is_empty() {
                    None
                } else {
                    Some(t.to_string())
                };
                self.sync_active_mcp_from_flat();
            }
            "llm_provider" => {
                self.llm_provider = value.to_string();
                self.sync_active_profile_from_flat();
            }
            "llm_api_base" => {
                let mut base = normalize_openai_compatible_base(value);
                if let Some(next) = migrate_llm_api_base(&base) {
                    base = next.to_string();
                }
                self.llm_api_base = base;
                self.sync_active_profile_from_flat();
            }
            "llm_api_key" => {
                let k = value.trim();
                self.llm_api_key = if k.is_empty() {
                    None
                } else {
                    Some(k.to_string())
                };
                self.sync_active_profile_from_flat();
            }
            "llm_model" => {
                let migrated = migrate_llm_model(value).unwrap_or(value);
                self.llm_model = migrated.to_string();
                self.sync_active_profile_from_flat();
            }
            "max_iterations" => {
                self.max_iterations = value
                    .parse()
                    .map_err(|_| anyhow::anyhow!("max_iterations must be a number"))?;
            }
            "local_vision_enabled" => {
                self.local_vision.enabled = value.trim() == "true";
            }
            "local_vision_api_base" => {
                let base = normalize_openai_compatible_base(value);
                self.local_vision.api_base = base;
            }
            "local_vision_model" => {
                let m = value.trim();
                if !m.is_empty() {
                    self.local_vision.model = m.to_string();
                }
            }
            "work_dir" => {
                let p = value.trim();
                self.work_dir = if p.is_empty() {
                    None
                } else {
                    Some(p.to_string())
                };
            }
            "sediment_visibility" => {
                let v = value.trim().to_ascii_lowercase();
                self.sediment_visibility = match v.as_str() {
                    "personal" => "personal".into(),
                    "explore" | "public" => "explore".into(),
                    _ => anyhow::bail!(
                        "sediment_visibility must be personal or explore (got {value})"
                    ),
                };
            }
            "statusline" => {
                let s = value.trim();
                self.statusline = if s.is_empty() {
                    None
                } else {
                    Some(s.to_string())
                };
            }
            "permission_mode" => {
                let v = value.trim().to_ascii_lowercase();
                if !["default", "accept_edits", "plan", "bypass"].contains(&v.as_str()) {
                    anyhow::bail!(
                        "permission_mode must be default / accept_edits / plan / bypass (got {value})"
                    );
                }
                self.permission_mode = Some(v);
            }
            "disallowed_tools" => {
                // 逗号分隔工具名；空串清空
                self.disallowed_tools = value
                    .split(',')
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(str::to_string)
                    .collect();
            }
            _ => anyhow::bail!(
                "Unknown config key: {key}. Valid keys: api_base, api_token, llm_provider, \
                 llm_api_base, llm_api_key, llm_model, max_iterations, work_dir, \
                 sediment_visibility, statusline, permission_mode, disallowed_tools"
            ),
        }
        Ok(())
    }

    /// Get a config value by key name.
    pub fn get(&self, key: &str) -> anyhow::Result<String> {
        match key {
            "api_base" => Ok(self.api_base.clone()),
            "api_token" => Ok(self.api_token.clone().unwrap_or_default()),
            "llm_provider" => Ok(self.llm_provider.clone()),
            "llm_api_base" => Ok(self.llm_api_base.clone()),
            "llm_api_key" => Ok(self
                .llm_api_key
                .as_deref()
                .unwrap_or("(not set)")
                .to_string()),
            "llm_model" => Ok(self.llm_model.clone()),
            "max_iterations" => Ok(self.max_iterations.to_string()),
            "local_vision_enabled" => Ok(self.local_vision.enabled.to_string()),
            "local_vision_api_base" => Ok(self.local_vision.api_base.clone()),
            "local_vision_model" => Ok(self.local_vision.model.clone()),
            "work_dir" => Ok(self.work_dir.clone().unwrap_or_default()),
            "sediment_visibility" => Ok(self.sediment_visibility.clone()),
            "statusline" => Ok(self.statusline.clone().unwrap_or_default()),
            "permission_mode" => Ok(self.permission_mode.clone().unwrap_or_default()),
            "disallowed_tools" => Ok(self.disallowed_tools.join(", ")),
            _ => anyhow::bail!(
                "Unknown config key: {key}. Valid keys: api_base, api_token, llm_provider, \
                 llm_api_base, llm_api_key, llm_model, max_iterations, work_dir, \
                 sediment_visibility, statusline, permission_mode, disallowed_tools"
            ),
        }
    }

    /// Whether sediment should submit Explore review after personal save.
    pub fn sediment_submits_explore(&self) -> bool {
        self.sediment_visibility.eq_ignore_ascii_case("explore")
    }
}

/// Returns the config file path (`directories` ProjectDirs `config_dir` + `config.toml`).
/// On Windows this is typically `%APPDATA%/promptstdio/stitch/config/config.toml`.
/// 配置根目录（config.toml/窗口状态/allow_rules/crash.log 所在）。
/// - `STITCH_CONFIG_DIR` 环境变量优先（真机探针注入正式目录用）；
/// - debug 构建（cargo build）用 `stitch-dev`——与正式安装包完全隔离，
///   避免调试数据（会话/窗口状态/规则）污染正式包（用户要求）；
/// - release 用 `stitch`。
pub fn config_dir() -> std::path::PathBuf {
    if let Ok(dir) = std::env::var("STITCH_CONFIG_DIR")
        && !dir.trim().is_empty()
    {
        return std::path::PathBuf::from(dir);
    }
    let app = if cfg!(debug_assertions) {
        "stitch-dev"
    } else {
        "stitch"
    };
    directories::ProjectDirs::from("com", "promptstdio", app)
        .map(|d| d.config_dir().to_path_buf())
        .unwrap_or_else(|| {
            let home = dirs_fallback();
            home.join(".config").join(app)
        })
}

pub fn config_path() -> std::path::PathBuf {
    config_dir().join("config.toml")
}

/// Mistaken path used by early provision scripts: `…/stitch/config.toml`
/// (Windows `directories` config_dir already ends with `…/stitch/config`).
pub fn legacy_config_path() -> std::path::PathBuf {
    config_path()
        .parent()
        .and_then(|config_dir| config_dir.parent())
        .map(|project| project.join("config.toml"))
        .unwrap_or_else(config_path)
}

fn load_raw_config_file(path: &std::path::Path) -> Option<StitchConfig> {
    let content = std::fs::read_to_string(path).ok()?;
    toml::from_str(&content).ok()
}

fn dirs_fallback() -> std::path::PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::PathBuf::from("."))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrates_retired_deepseek_aliases() {
        assert_eq!(
            migrate_llm_model("deepseek-chat"),
            Some("deepseek-v4-flash")
        );
        assert_eq!(
            migrate_llm_model("deepseek-reasoner"),
            Some("deepseek-v4-flash")
        );
        assert_eq!(migrate_llm_model("deepseek-v4-pro"), None);
    }

    #[test]
    fn set_llm_model_rewrites_alias() {
        let mut cfg = StitchConfig::default();
        cfg.set("llm_model", "deepseek-chat").unwrap();
        assert_eq!(cfg.llm_model, "deepseek-v4-flash");
    }

    #[test]
    fn clear_api_token_with_empty_string() {
        let mut cfg = StitchConfig::default();
        cfg.set("api_token", "pts-secret").unwrap();
        assert!(cfg.api_token.is_some());
        cfg.set("api_token", "").unwrap();
        assert!(cfg.api_token.is_none());
    }

    #[test]
    fn sediment_visibility_defaults_explore() {
        let cfg = StitchConfig::default();
        assert_eq!(cfg.sediment_visibility, "explore");
        assert!(cfg.sediment_submits_explore());
        let mut cfg = StitchConfig::default();
        cfg.set("sediment_visibility", "personal").unwrap();
        assert!(!cfg.sediment_submits_explore());
        cfg.set("sediment_visibility", "public").unwrap();
        assert!(cfg.sediment_submits_explore());
    }

    #[test]
    fn legacy_config_path_is_sibling_of_config_dir() {
        let active = config_path();
        let legacy = legacy_config_path();
        assert_ne!(active, legacy);
        assert_eq!(
            legacy.file_name().and_then(|s| s.to_str()),
            Some("config.toml")
        );
        // …/stitch/config/config.toml → …/stitch/config.toml
        if active
            .parent()
            .and_then(|p| p.file_name())
            .and_then(|s| s.to_str())
            == Some("config")
        {
            assert_eq!(
                legacy.parent().map(|p| p.to_path_buf()),
                active
                    .parent()
                    .and_then(|p| p.parent())
                    .map(|p| p.to_path_buf())
            );
        }
    }

    #[test]
    fn set_work_dir_roundtrip() {
        let mut cfg = StitchConfig::default();
        cfg.set("work_dir", "C:/tmp/project").unwrap();
        assert_eq!(cfg.work_dir.as_deref(), Some("C:/tmp/project"));
        cfg.set("work_dir", "  ").unwrap();
        assert!(cfg.work_dir.is_none());
    }

    #[test]
    fn seed_profiles_from_flat_fields() {
        let mut cfg = StitchConfig::default();
        cfg.llm_api_key = Some("sk-test".into());
        cfg.llm_model = "deepseek-v4-pro".into();
        assert!(cfg.ensure_profiles_seeded());
        assert_eq!(cfg.llm_profiles.len(), 1);
        assert_eq!(cfg.active_profile_id.as_deref(), Some("default"));
        assert_eq!(cfg.llm_profiles[0].model, "deepseek-v4-pro");
        assert_eq!(cfg.llm_profiles[0].api_key.as_deref(), Some("sk-test"));
        assert!(!cfg.ensure_profiles_seeded());
    }

    #[test]
    fn upsert_activate_delete_profile() {
        let mut cfg = StitchConfig::default();
        cfg.llm_api_key = Some("sk-a".into());
        cfg.ensure_profiles_seeded();

        cfg.upsert_profile(LlmProfile {
            id: "openai".into(),
            label: "OpenAI".into(),
            provider: "openai".into(),
            api_base: "https://api.openai.com/v1".into(),
            api_key: Some("sk-b".into()),
            model: "gpt-4o".into(),
        })
        .unwrap();
        assert_eq!(cfg.llm_profiles.len(), 2);

        cfg.activate_profile("openai").unwrap();
        assert_eq!(cfg.llm_model, "gpt-4o");
        assert_eq!(cfg.llm_api_key.as_deref(), Some("sk-b"));
        assert_eq!(cfg.active_profile_id.as_deref(), Some("openai"));

        cfg.delete_profile("openai").unwrap();
        assert_eq!(cfg.llm_profiles.len(), 1);
        assert_eq!(cfg.active_profile_id.as_deref(), Some("default"));
        assert_eq!(cfg.llm_api_key.as_deref(), Some("sk-a"));
    }

    #[test]
    fn upsert_keeps_previous_key_when_empty() {
        let mut cfg = StitchConfig::default();
        cfg.ensure_profiles_seeded();
        cfg.upsert_profile(LlmProfile {
            id: "default".into(),
            label: "DeepSeek".into(),
            provider: "deepseek".into(),
            api_base: "https://api.deepseek.com".into(),
            api_key: Some("sk-keep".into()),
            model: "deepseek-v4-flash".into(),
        })
        .unwrap();
        cfg.upsert_profile(LlmProfile {
            id: "default".into(),
            label: "DeepSeek".into(),
            provider: "deepseek".into(),
            api_base: "https://api.deepseek.com".into(),
            api_key: None,
            model: "deepseek-v4-pro".into(),
        })
        .unwrap();
        assert_eq!(cfg.llm_profiles[0].api_key.as_deref(), Some("sk-keep"));
        assert_eq!(cfg.llm_model, "deepseek-v4-pro");
    }

    #[test]
    fn resolve_llm_uses_profile_without_mutating_active() {
        let mut cfg = StitchConfig::default();
        cfg.llm_api_key = Some("sk-active".into());
        cfg.ensure_profiles_seeded();
        cfg.upsert_profile(LlmProfile {
            id: "other".into(),
            label: "Other".into(),
            provider: "openai".into(),
            api_base: "https://api.openai.com/v1".into(),
            api_key: Some("sk-other".into()),
            model: "gpt-4o-mini".into(),
        })
        .unwrap();

        let resolved = cfg.resolve_llm(Some("other"), Some("gpt-4o")).unwrap();
        assert_eq!(resolved.api_key, "sk-other");
        assert_eq!(resolved.model, "gpt-4o");
        assert_eq!(resolved.api_base, "https://api.openai.com/v1");
        // Active flat fields unchanged.
        assert_eq!(cfg.active_profile_id.as_deref(), Some("default"));
        assert_eq!(cfg.llm_api_key.as_deref(), Some("sk-active"));
    }

    #[test]
    fn set_flat_mirrors_active_profile() {
        let mut cfg = StitchConfig::default();
        cfg.ensure_profiles_seeded();
        cfg.set("llm_model", "deepseek-v4-pro").unwrap();
        assert_eq!(cfg.llm_profiles[0].model, "deepseek-v4-pro");
    }

    #[test]
    fn normalizes_chat_completions_url() {
        assert_eq!(
            normalize_openai_compatible_base("https://api.openai.com/v1/chat/completions"),
            "https://api.openai.com/v1"
        );
        assert_eq!(
            normalize_openai_compatible_base(
                "https://open.bigmodel.cn/api/paas/v4/chat/completions/"
            ),
            "https://open.bigmodel.cn/api/paas/v4"
        );
        let mut cfg = StitchConfig::default();
        cfg.ensure_profiles_seeded();
        cfg.upsert_profile(LlmProfile {
            id: "z".into(),
            label: "智谱".into(),
            provider: "zhipu".into(),
            api_base: "https://open.bigmodel.cn/api/paas/v4/chat/completions".into(),
            api_key: Some("sk".into()),
            model: "glm-4-flash".into(),
        })
        .unwrap();
        assert_eq!(
            cfg.profile("z").unwrap().api_base,
            "https://open.bigmodel.cn/api/paas/v4"
        );
    }

    #[test]
    fn migrates_deepseek_v1_base_to_official_root() {
        assert_eq!(
            migrate_llm_api_base("https://api.deepseek.com/v1"),
            Some("https://api.deepseek.com")
        );
        assert_eq!(migrate_llm_api_base("https://api.deepseek.com"), None);
        assert_eq!(default_llm_api_base(), "https://api.deepseek.com");

        let mut cfg = StitchConfig::default();
        cfg.llm_api_base = "https://api.deepseek.com/v1".into();
        cfg.ensure_profiles_seeded();
        cfg.llm_profiles[0].api_base = "https://api.deepseek.com/v1".into();
        assert!(cfg.apply_model_migration());
        assert_eq!(cfg.llm_api_base, "https://api.deepseek.com");
        assert_eq!(cfg.llm_profiles[0].api_base, "https://api.deepseek.com");

        cfg.upsert_profile(LlmProfile {
            id: "ds".into(),
            label: "DeepSeek".into(),
            provider: "deepseek".into(),
            api_base: "https://api.deepseek.com/v1".into(),
            api_key: Some("sk".into()),
            model: "deepseek-v4-flash".into(),
        })
        .unwrap();
        assert_eq!(
            cfg.profile("ds").unwrap().api_base,
            "https://api.deepseek.com"
        );
    }

    #[test]
    fn mcp_profiles_seed_and_crud() {
        let mut cfg = StitchConfig::default();
        cfg.api_token = Some("tok-a".into());
        assert!(cfg.ensure_mcp_profiles_seeded());
        assert_eq!(cfg.mcp_profiles.len(), 1);
        assert_eq!(cfg.active_mcp_id.as_deref(), Some("default"));
        assert_eq!(cfg.mcp_profiles[0].api_token.as_deref(), Some("tok-a"));

        cfg.upsert_mcp_profile(McpProfile {
            id: "staging".into(),
            label: "预发".into(),
            api_base: "https://staging.example.com/".into(),
            api_token: Some("tok-b".into()),
        })
        .unwrap();
        assert_eq!(cfg.mcp_profiles.len(), 2);
        cfg.activate_mcp_profile("staging").unwrap();
        assert_eq!(cfg.api_base, "https://staging.example.com");
        assert_eq!(cfg.api_token.as_deref(), Some("tok-b"));

        let resolved = cfg.resolve_mcp(Some("default")).unwrap();
        assert_eq!(resolved.api_token, "tok-a");
        assert_eq!(resolved.api_base, "https://www.promptstdio.com");

        cfg.upsert_mcp_profile(McpProfile {
            id: "staging".into(),
            label: "预发".into(),
            api_base: "https://staging.example.com".into(),
            api_token: None,
        })
        .unwrap();
        assert_eq!(
            cfg.mcp_profile("staging").unwrap().api_token.as_deref(),
            Some("tok-b")
        );

        cfg.delete_mcp_profile("staging").unwrap();
        assert_eq!(cfg.active_mcp_id.as_deref(), Some("default"));
        assert_eq!(cfg.api_token.as_deref(), Some("tok-a"));
    }

    #[test]
    fn set_api_token_mirrors_active_mcp() {
        let mut cfg = StitchConfig::default();
        cfg.ensure_mcp_profiles_seeded();
        cfg.set("api_token", "tok-new").unwrap();
        assert_eq!(cfg.mcp_profiles[0].api_token.as_deref(), Some("tok-new"));
        cfg.set("api_base", "https://custom.example.com/").unwrap();
        assert_eq!(cfg.api_base, "https://custom.example.com");
        assert_eq!(cfg.mcp_profiles[0].api_base, "https://custom.example.com");
    }

    #[test]
    fn apex_promptstdio_api_base_migrates_to_www() {
        assert_eq!(
            normalize_promptstdio_api_base("https://promptstdio.com/"),
            Some("https://www.promptstdio.com")
        );
        let mut cfg = StitchConfig::default();
        cfg.api_base = "https://promptstdio.com".into();
        cfg.mcp_profiles.push(McpProfile {
            id: "default".into(),
            label: "PromptStdio".into(),
            api_base: "https://promptstdio.com".into(),
            api_token: Some("tok".into()),
        });
        cfg.mcp_servers.push(McpServerProfile {
            id: "promptstdio".into(),
            label: "PromptStdio".into(),
            transport: "http".into(),
            enabled: true,
            command: None,
            args: vec![],
            env: Default::default(),
            cwd: None,
            url: Some("https://promptstdio.com/mcp".into()),
            headers: Default::default(),
        });
        assert!(cfg.apply_api_base_migration());
        assert_eq!(cfg.api_base, "https://www.promptstdio.com");
        assert_eq!(cfg.mcp_profiles[0].api_base, "https://www.promptstdio.com");
        assert_eq!(
            cfg.mcp_servers[0].url.as_deref(),
            Some("https://www.promptstdio.com/mcp")
        );
        assert!(!cfg.apply_api_base_migration());
    }

    #[test]
    fn mcp_server_crud() {
        let mut cfg = StitchConfig::default();
        cfg.upsert_mcp_server(McpServerProfile {
            id: "fs".into(),
            label: "Filesystem".into(),
            transport: "stdio".into(),
            enabled: true,
            command: Some("npx".into()),
            args: vec![
                "-y".into(),
                "@modelcontextprotocol/server-filesystem".into(),
            ],
            env: {
                let mut e = HashMap::new();
                e.insert("FOO".into(), "bar".into());
                e
            },
            cwd: Some("/tmp/ws".into()),
            url: None,
            headers: Default::default(),
        })
        .unwrap();
        assert_eq!(cfg.mcp_servers.len(), 1);
        assert_eq!(
            cfg.mcp_server("fs")
                .unwrap()
                .env
                .get("FOO")
                .map(String::as_str),
            Some("bar")
        );
        assert_eq!(
            cfg.mcp_server("fs").unwrap().cwd.as_deref(),
            Some("/tmp/ws")
        );
        cfg.set_mcp_server_enabled("fs", false).unwrap();
        assert!(!cfg.mcp_server("fs").unwrap().enabled);
        assert!(cfg.enabled_mcp_servers().is_empty());

        cfg.upsert_mcp_server(McpServerProfile {
            id: "remote".into(),
            label: "Remote".into(),
            transport: "http".into(),
            enabled: true,
            command: None,
            args: vec![],
            env: Default::default(),
            cwd: None,
            url: Some("https://example.com/mcp/".into()),
            headers: {
                let mut h = HashMap::new();
                h.insert("Authorization".into(), "Bearer tok".into());
                h
            },
        })
        .unwrap();
        assert_eq!(
            cfg.mcp_server("remote").unwrap().url.as_deref(),
            Some("https://example.com/mcp")
        );
        // Update without Authorization keeps prior token.
        cfg.upsert_mcp_server(McpServerProfile {
            id: "remote".into(),
            label: "Remote".into(),
            transport: "http".into(),
            enabled: true,
            command: None,
            args: vec![],
            env: Default::default(),
            cwd: None,
            url: Some("https://example.com/mcp".into()),
            headers: Default::default(),
        })
        .unwrap();
        assert_eq!(
            cfg.mcp_server("remote")
                .unwrap()
                .headers
                .get("Authorization")
                .map(String::as_str),
            Some("Bearer tok")
        );
        cfg.delete_mcp_server("fs").unwrap();
        assert_eq!(cfg.mcp_servers.len(), 1);
    }
}
