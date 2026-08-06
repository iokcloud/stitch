use clap::{Parser, Subcommand};

/// PromptStdio Agent CLI — stitch prompts into workflows
#[derive(Parser)]
#[command(
    name = "stitch",
    about = "PromptStdio Agent CLI",
    version,
    long_about = "An AI coding agent that turns your prompt workflows into executable actions.\n\nDocs: https://promptstdio.com/docs/stitch"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    /// Run a single task with the agent
    #[command(visible_alias = "r")]
    Run {
        /// Task description in natural language
        prompt: Vec<String>,

        /// Override the LLM model for this run
        #[arg(short = 'm', long)]
        model: Option<String>,

        /// Skip confirmation prompts for file writes and commands
        #[arg(short = 'y', long)]
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
