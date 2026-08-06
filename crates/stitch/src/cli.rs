use clap::{Parser, Subcommand};

/// PromptStdio Agent CLI — promptstdio.com/stitch
#[derive(Parser)]
#[command(
    name = "stitch",
    about = "PromptStdio Agent CLI",
    version,
    long_about = "An AI coding agent that turns your prompt workflows into executable actions.\n\nExamples:\n  stitch chat              Start an interactive session\n  stitch chat --continue   Continue your last session\n  stitch run \"fix the bug\"  Run a one-shot task\n  stitch sessions          List your saved sessions\n\nDocs: https://promptstdio.com/docs/stitch"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start an interactive chat session (REPL with history and slash commands)
    #[command(visible_alias = "c")]
    Chat {
        /// Resume a saved session by id (use `stitch sessions` to list)
        #[arg(long)]
        resume: Option<String>,

        /// Continue the most recent session in this workspace
        #[arg(long)]
        continue_: bool,
    },

    /// List saved sessions in this workspace
    Sessions,

    /// Run a single task with the agent (non-interactive)
    #[command(visible_alias = "r")]
    Run {
        /// Task description in natural language
        prompt: Vec<String>,

        /// Override the LLM model for this run
        #[arg(short = 'm', long)]
        model: Option<String>,

        /// Skip confirmation prompts for file writes and commands
        #[arg(short = 'y', long = "dangerously-skip-permissions")]
        yes: bool,

        /// Emit machine-readable JSON (response, iterations, cost, cache)
        #[arg(long)]
        json: bool,
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

    /// Upgrade to the latest version (downloads from promptstdio.com)
    Upgrade,
}

#[derive(Subcommand)]
pub enum ConfigAction {
    /// Set a config value
    Set { key: String, value: String },
    /// Get a config value
    Get { key: String },
    /// Print the config file path
    Path,
}
