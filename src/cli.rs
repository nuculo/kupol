//! CLI module — clap-based command-line interface for the KUPOL engine.
//!
//! Commands:
//! - `scan`   — scan a codebase for security findings
//! - `serve`  — start the Web UI + API server
//! - `report` — export the results of a scan
//! - `init`   — generate a CI pipeline template
//! - `mcp`    — run as a Model Context Protocol server
//! - `info`   — print engine information
//! - `demo`   — run the multi-agent demo scenarios

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "kupol",
    version = env!("CARGO_PKG_VERSION"),
    about = "KUPOL — a local security scanner for your repository",
    long_about = "KUPOL scans a local codebase for security findings and prints a report.\n\nThis binary is the KUPOL engine. The public command `kupol` wraps it and adds\nthe --fail-on CI gate. See https://kupol.app for details."
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,

    /// Enable verbose output
    #[arg(short, long, global = true)]
    pub verbose: bool,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Scan a codebase for security findings
    Scan {
        /// Path to the directory or file to scan
        #[arg(short, long, default_value = ".")]
        path: String,

        /// Merge/pull request ID to comment on (optional git host integration)
        #[arg(long)]
        mr: Option<u64>,

        /// Project ID for the git host integration (optional)
        #[arg(long)]
        project: Option<String>,

        /// Output format: table, json, or markdown
        #[arg(short, long, default_value = "table")]
        format: String,

        /// Write the report to a file instead of stdout
        #[arg(short, long)]
        output: Option<String>,

        /// Minimum severity to report: info, low, medium, high, critical
        #[arg(long, default_value = "low")]
        min_severity: String,

        /// Exit with code 1 if any finding is at least this severe
        /// (info, low, medium, high, critical)
        #[arg(long)]
        fail_on: Option<String>,
    },

    /// Start the Web UI + API server
    Serve {
        /// Port to bind the server to
        #[arg(short, long, default_value = "3000")]
        port: u16,

        /// Do not open a browser automatically
        #[arg(long)]
        no_open: bool,
    },

    /// Export the results of the last scan
    Report {
        /// Output format: json, markdown, or table
        #[arg(short, long, default_value = "json")]
        format: String,

        /// Write the report to a file instead of stdout
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Run the multi-agent demo scenarios
    Demo {
        /// Demo scenario number (empty = run all)
        #[arg(short, long)]
        number: Option<u32>,
    },

    /// Generate a CI pipeline template (.gitlab-ci.yml)
    Init {
        /// Directory to generate the file in (default: current directory)
        #[arg(short, long, default_value = ".")]
        dir: String,

        /// Overwrite an existing file
        #[arg(short, long)]
        force: bool,
    },

    /// Print engine information
    Info,

    /// Run as a Model Context Protocol (MCP) server over stdio
    Mcp,
}
