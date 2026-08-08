use clap::{Parser, Subcommand, ValueEnum};

/// `stitch run` 输出格式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum OutputFormat {
    /// Human-readable text (default)
    Text,
    /// Single JSON document (response, iterations, cost, cache)
    Json,
    /// NDJSON event stream (tokens, tool calls, done, errors)
    StreamJson,
}

/// PromptStdio Agent CLI — promptstdio.com/stitch
#[derive(Parser)]
#[command(
    name = "stitch",
    about = "PromptStdio Agent CLI",
    version,
    long_about = "An AI coding agent that turns your prompt workflows into executable actions.\n\nExamples:\n  stitch chat              Start an interactive session\n  stitch chat --continue   Continue your last session\n  stitch run \"fix the bug\"  Run a one-shot task\n  stitch sessions          List your saved sessions\n\nDocs: https://promptstdio.com/docs/stitch"
)]
pub struct Cli {
    /// 无参数时默认进入交互对话（claude 语义）
    #[command(subcommand)]
    pub command: Option<Command>,

    /// 权限模式：default / accept_edits / plan / bypass（全局，优先级高于 config）
    #[arg(long, global = true)]
    pub permission_mode: Option<String>,

    /// 详细输出：debug 日志 + 回合级明细（模型/API/迭代/tokens）
    #[arg(long, global = true)]
    pub verbose: bool,

    /// 附加工作目录（可重复）：与主目录同为工作区，模型可读写其中的文件
    #[arg(long, global = true, value_name = "PATH")]
    pub add_dir: Vec<String>,

    /// 会话成本预算（元 ¥）：累计达到后提示并停止，防长任务跑飞超支
    #[arg(long, global = true, value_name = "YUAN")]
    pub budget: Option<f64>,

    /// 单次管道模式（claude -p 语义）：`stitch -p "prompt"` 或
    /// `echo "…" | stitch -p`（无值 → 读 stdin），跑完即退；
    /// 配 `--output-format` 出机器可读结果。与 `stitch run` 等价，仅语法不同。
    #[arg(
        short = 'p',
        long = "print",
        global = true,
        value_name = "PROMPT",
        num_args = 0..=1,
        default_missing_value = ""
    )]
    pub print: Option<String>,

    /// 最大回合数：自动模式（accept_edits/bypass）下达到上限自动停，防模型跑飞。
    /// 交互会话到数后提示退出；run 单次任务覆盖 max_iterations。
    #[arg(long, global = true, value_name = "N")]
    pub max_turns: Option<usize>,

    /// 机器可读 JSON 输出（仅 -p/--print 与 run 生效）：response / iterations /
    /// cost / cache。Shorthand for `--output-format json`。
    #[arg(long, global = true)]
    pub json: bool,

    /// 输出格式（仅 -p/--print 与 run 生效）：text（默认）、json（汇总）、
    /// stream-json（NDJSON 事件流，CI / 脚本用）
    #[arg(long, global = true, value_enum)]
    pub output_format: Option<OutputFormat>,

    /// 追加自定义系统提示（可重复）：角色设定 / 行为约束 / 个性注入，
    /// 追加在系统提示最末尾（最高优先），与项目规则叠加。
    #[arg(long = "append-system-prompt", global = true, value_name = "TEXT")]
    pub append_system_prompt: Vec<String>,

    /// 禁用工具列表（可重复，逗号分隔）：与 config 的 disallowed_tools 合并，
    /// 始终生效（bypass 模式下也不放行）——CI / 受限场景禁危险工具。
    #[arg(long, global = true, value_name = "TOOL", value_delimiter = ',')]
    pub disallowed_tools: Vec<String>,

    /// 工具白名单（可重复，逗号分隔）：非空时只允许列表内的工具，
    /// 其余直接拒绝（deny 规则仍优先）——最小权限执行环境。
    #[arg(long, global = true, value_name = "TOOL", value_delimiter = ',')]
    pub allowed_tools: Vec<String>,

    /// 会话级快速配置（可重复，`KEY=VALUE`）：permission_mode /
    /// disallowed_tools / append_system_prompt / statusline / model，
    /// 优先级高于 config 与 settings.json（不落盘，仅本次会话）。
    #[arg(long, global = true, value_name = "KEY=VALUE")]
    pub setting: Vec<String>,

    /// 模型参数配置（JSON 文件）：{"temperature": 0.7, "top_p": 0.9,
    /// "max_tokens": 4096} ——覆盖默认采样参数（DeepSeek V4 支持）。
    #[arg(long, global = true, value_name = "FILE")]
    pub model_config: Option<std::path::PathBuf>,

    /// 附加文件到上下文（可重复）：内容注入每次请求的系统提示末尾，
    /// 模型始终可见（Claude Code --include 语义）。
    #[arg(long, global = true, value_name = "PATH")]
    pub include: Vec<std::path::PathBuf>,

    /// 精确恢复指定会话 id（Claude Code --session-id 语义）：
    /// 跳过交互选择直接进入该会话；`stitch sessions` 查看 id。
    /// chat 子命令下优先 --resume，其次 --session-id。
    #[arg(long, global = true, value_name = "ID")]
    pub session_id: Option<String>,

    /// 外部 MCP 服务器配置文件（Claude Code --mcp-config 语义）：
    /// 兼容 Cursor / Claude Desktop `mcpServers` JSON，会话级加载合并
    /// （同 id 覆盖 config.toml 配置，不落盘）。
    #[arg(long, global = true, value_name = "FILE")]
    pub mcp_config: Option<std::path::PathBuf>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start an interactive chat session (REPL with history and slash commands).
    /// This is the default: running `stitch` with no arguments enters chat.
    #[command(visible_alias = "c")]
    Chat {
        /// Resume a saved session by id (use `stitch sessions` to list).
        /// With no value (`--resume`), pick interactively.
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        resume: Option<Option<String>>,

        /// Continue the most recent session in this workspace
        #[arg(long)]
        continue_: bool,

        /// Fork a saved session (Claude Code 语义): start a NEW session
        /// reusing the context up to its latest user message — or up to
        /// message `seq` with `id:seq` (1-based). The original session
        /// is preserved. With no value (`--fork`), pick interactively.
        #[arg(long, num_args = 0..=1, default_missing_value = "")]
        fork: Option<Option<String>>,

        /// Override the LLM model for this session (Claude Code 语义)
        #[arg(short = 'm', long)]
        model: Option<String>,
    },

    /// Manage saved sessions in this workspace (list / delete / rename)
    Sessions {
        #[command(subcommand)]
        action: Option<SessionAction>,
    },

    /// Show workspace session statistics (session count, messages, tokens)
    Stats,

    /// Run a single task with the agent (non-interactive)
    #[command(visible_alias = "r")]
    Run {
        /// Task description in natural language.
        /// Omitted when piped via stdin (`echo "…" | stitch run`).
        prompt: Vec<String>,

        /// Override the LLM model for this run
        #[arg(short = 'm', long)]
        model: Option<String>,

        /// Skip confirmation prompts for file writes and commands
        #[arg(short = 'y', long = "dangerously-skip-permissions")]
        yes: bool,
    },

    /// Run a task suite from PromptStdio
    Suite {
        /// Suite slug or ID from PromptStdio
        slug: String,
    },

    /// Start an interactive agent session with a Studio agent
    Agent {
        /// Agent slug or ID from PromptStdio
        slug: String,
    },

    /// Log in to PromptStdio
    Login,

    /// Log out and clear local credentials
    Logout,

    /// Show or edit stitch configuration
    Config {
        #[command(subcommand)]
        action: Option<ConfigAction>,
    },

    /// Diagnose the environment (config, LLM key, connectivity)
    Doctor,

    /// Initialize a CLAUDE.md project-memory file in this workspace
    Init,

    /// Manage MCP server connections (stdio command or HTTP URL)
    Mcp {
        #[command(subcommand)]
        action: Option<McpAction>,
    },

    /// Upgrade to the latest version (downloads from promptstdio.com)
    Upgrade,

    /// Generate shell completion scripts (bash / zsh / fish / powershell)
    Completions {
        /// Target shell (omit to list available)
        #[arg(value_enum)]
        shell: Option<clap_complete::Shell>,
    },
}

/// `stitch sessions` 子命令。
#[derive(Subcommand)]
pub enum SessionAction {
    /// List saved sessions (default when omitted)
    List,
    /// Delete a session by id (removes its directory and history)
    Delete {
        /// Session id (see `stitch sessions`)
        id: String,
    },
    /// Rename a session (overrides the auto-extracted title)
    Rename {
        /// Session id (see `stitch sessions`)
        id: String,
        /// New title (empty clears back to auto-extracted)
        title: String,
    },
}

#[derive(Subcommand)]
pub enum McpAction {
    /// List configured MCP servers
    List,
    /// Add an MCP server (stdio: `--command "npx -y pkg args"`, or `--url`)
    Add {
        /// Server name (id)
        name: String,
        /// stdio 启动命令（可含参数，空格拆分）
        #[arg(long)]
        command: Option<String>,
        /// http/sse 端点 URL
        #[arg(long)]
        url: Option<String>,
    },
    /// Remove an MCP server by name
    Remove { name: String },
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Set a config value
    Set { key: String, value: String },
    /// Get a config value
    Get { key: String },
    /// List all config values (secrets masked)
    List,
    /// Print the config file path
    Path,
}
