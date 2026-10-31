//! # KUPOL engine — entry point
//!
//! KUPOL is a local security scanner for a code repository. This binary is the
//! KUPOL engine (internal crate name `duo-agents`); the public `kupol` command
//! wraps it. The engine runs a plugin-based scan pipeline over a source tree and
//! reports findings, and also hosts the optional Web UI, MCP server, and the
//! multi-agent demo scenarios.
//!
//! ## Module layout
//!
//! ```text
//! src/
//! ├── main.rs           — entry point + CLI dispatch
//! ├── actor.rs          — Actor trait (2-phase Execute/Complete model)
//! ├── actors/           — domain actors (AST, security, drift, git host, MCP, swarm)
//! ├── orchestrator.rs   — DAG-style multi-actor flow + WebSocket
//! ├── server/           — Axum REST + WebSocket
//! ├── mcp_server.rs     — Model Context Protocol server (stdio)
//! └── demos/            — multi-agent demo scenarios
//! ```

// ─────────────────────────────────────────────────────────────────────────────
// § Декларации модулей
// ─────────────────────────────────────────────────────────────────────────────

/// Семантический движок (HIR Call Graph через syn)
pub use duo_kan::semantic_engine;
/// Модели сущностей: Entity, EntityGraph, AstPatch, MergeRequestEvent
pub use duo_kan::models;
/// YDB-протоколы: Message, GraphDelta, DynBitMap, NodeInfo, ActorLifecycle
pub use duo_kan::protocol;
/// Actor trait + spawn_actor_2phase
pub mod actor;
/// Доменные акторы (7 подмодулей)
pub mod actors;
/// Flow Orchestrator + WebSocket
pub mod orchestrator;

/// --- НОВЫЕ МОДУЛИ (Hackathon) ---
pub mod mcp_server;
pub use duo_kan::kan;
/// CLI интерфейс (clap)
pub mod cli;
/// Конфигурация `.kupol.yml`
pub mod config;
pub mod server;
pub mod demos;


/// Scan Pipeline (promptfoo-стиль)
pub use duo_kan::scan;

/// Тесты API
pub mod test_api;
/// Тесты базы данных
pub mod test_database;
/// Тесты фронтенда
pub mod test_frontend;
/// Движок политик безопасности
pub use duo_kan::policy_engine;
/// Провайдеры контекста (GitLab, LocalFs)
pub use duo_kan::fetcher;
/// Миграция конфигурации безопасности
pub mod migration;
/// Структурированная телеметрия
pub mod telemetry;
/// AgentEdge стратегия (Explore ↔ Exploit)
pub use duo_kan::strategy;
/// Babylonian 60-Head сканирование
pub use duo_kan::babylonian;
/// Context Crossover (подавление ложных срабатываний)
pub use duo_kan::crossover;
/// A2A телеком-примитивы (Circuit Breaker, ETS, WAL)
pub mod a2a;
/// CycloneDX Blast-Radius и Trust Decay
pub use duo_kan::blast_radius;
/// KAN Intelligence (12 модулей из Clojure KAN)
pub use duo_kan::kan_intelligence;



// ─────────────────────────────────────────────────────────────────────────────
// § Реэкспорт ключевых типов для обратной совместимости
// ─────────────────────────────────────────────────────────────────────────────

pub use models::*;
pub use protocol::*;
pub use actor::*;
pub use actors::drift_detector::cosine_similarity;

// ─────────────────────────────────────────────────────────────────────────────
// § Импорты для main()
// ─────────────────────────────────────────────────────────────────────────────

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::broadcast;
use tracing::{info, warn, error};
use clap::Parser;

// ─────────────────────────────────────────────────────────────────────────────
// § Shared state for the API server
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AppState {
    pub scan_results: Arc<Mutex<Vec<scan::ScanResult>>>,
    pub telemetry_tx: broadcast::Sender<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// § Точка входа + CLI Router
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli_args = cli::Cli::parse();

    // Logs go to stderr so that stdout stays clean for reports and JSON-RPC.
    let default_level = if cli_args.verbose { "debug" } else { "info" };
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(default_level));
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_thread_ids(false)
        .without_time()
        .with_writer(std::io::stderr)
        .init();

    match cli_args.command {
        Some(cli::Commands::Scan { path, format, output, min_severity, mr, project, fail_on }) => {
            // Load .kupol.yml for defaults (ignore rules are applied inside the engine).
            let config = config::KupolConfig::load_for(&path);

            info!("Scanning {}", path);
            let result = scan::run_scan(&path);

            // Filter by minimum severity (CLI flag wins over config).
            let min_sev_str = config.min_severity.clone().unwrap_or(min_severity);
            let min_sev = scan::Severity::from_str(&min_sev_str);
            let filtered_findings = scan::graders::filter_by_severity(&result.findings, &min_sev);
            let filtered_result = scan::ScanResult {
                summary: scan::ScanSummary::from_findings(&filtered_findings, result.summary.total_files),
                findings: filtered_findings,
                ..result
            };

            match format.as_str() {
                "json" => {
                    let json = scan::report::to_json(&filtered_result);
                    if let Some(path) = &output {
                        std::fs::write(path, &json)?;
                        info!("JSON report written to {}", path);
                    } else {
                        println!("{}", json);
                    }
                }
                "markdown" | "md" => {
                    let md = scan::report::to_markdown(&filtered_result);
                    if let Some(path) = &output {
                        std::fs::write(path, &md)?;
                        info!("Markdown report written to {}", path);
                    } else {
                        println!("{}", md);
                    }
                }
                other => {
                    if other != "table" {
                        warn!("unknown format '{}', falling back to table", other);
                    }
                    scan::report::print_table(&filtered_result);
                    if let Some(path) = &output {
                        let json = scan::report::to_json(&filtered_result);
                        std::fs::write(path, &json)?;
                        info!("JSON report also written to {}", path);
                    }
                }
            }

            // Optional git-host integration: comment the report on a merge/pull request.
            if let Some(mr_id) = mr {
                let project_id = project.or_else(|| std::env::var("GITLAB_PROJECT_ID").ok()).unwrap_or_default();
                let token = std::env::var("GITLAB_TOKEN").ok().unwrap_or_default();
                let host_url = std::env::var("GITLAB_URL").unwrap_or_else(|_| "https://gitlab.com".to_string());

                if !project_id.is_empty() && !token.is_empty() {
                    let client = actors::gitlab::GitLabClient::new(&host_url, &token, &project_id);
                    let md_report = scan::report::to_markdown(&filtered_result);

                    info!("Posting report to merge request #{} on {} (project {})", mr_id, host_url, project_id);
                    match client.comment_mr(mr_id, &md_report).await {
                        Ok(_) => info!("Report posted to the merge request"),
                        Err(e) => error!("Could not post the report: {}", e),
                    }
                } else {
                    warn!("GITLAB_TOKEN or GITLAB_PROJECT_ID not set; skipping merge-request comment");
                }
            }

            // CI gate: --fail-on flag, or fail_on from .kupol.yml.
            if let Some(threshold) = fail_on.or(config.fail_on) {
                let threshold_sev = scan::Severity::from_str(&threshold);
                let tripped = filtered_result
                    .findings
                    .iter()
                    .any(|f| f.severity >= threshold_sev);
                if tripped {
                    error!("findings at or above severity '{}' — failing", threshold);
                    std::process::exit(1);
                }
            }

            return Ok(());
        }

        Some(cli::Commands::Serve { port, .. }) => {
            return crate::server::run_server(port).await;
        }

        Some(cli::Commands::Report { format, output }) => {
            info!("Exporting scan results");
            let demo_result = scan::run_scan("src/");
            match format.as_str() {
                "json" => {
                    let json = scan::report::to_json(&demo_result);
                    if let Some(p) = output { std::fs::write(p, json)?; }
                    else { println!("{}", scan::report::to_json(&demo_result)); }
                }
                "markdown" | "md" => {
                    let md = scan::report::to_markdown(&demo_result);
                    if let Some(p) = output { std::fs::write(p, md)?; }
                    else { println!("{}", scan::report::to_markdown(&demo_result)); }
                }
                _ => scan::report::print_table(&demo_result),
            }
            return Ok(());
        }

        Some(cli::Commands::Info) => {
            println!();
            println!("  KUPOL v{}", env!("CARGO_PKG_VERSION"));
            println!("  Local security scanner — https://kupol.app");
            println!("  ----------------------------------------");
            println!("  Plugins:    {} security checks", scan::plugins::all_plugins().len());
            println!("  Formats:    table, json, markdown");
            println!("  Runtime:    Tokio + Axum + WebSocket");
            println!("  Dashboard:  React + Tailwind CSS");
            println!("  Engine:     duo-agents (internal crate name)");
            println!();
            return Ok(());
        }

        Some(cli::Commands::Init { dir, force }) => {
            info!("Generating CI pipeline template");
            let path = std::path::Path::new(&dir).join(".gitlab-ci.yml");

            if path.exists() && !force {
                warn!("{} already exists; pass --force to overwrite", path.display());
                return Ok(());
            }

            let ci_template = r#"# KUPOL — security scan on merge requests
#
# Fails the pipeline when a finding is at or above the --fail-on severity.
# Install the engine (crate: duo-agents) however you prefer; this template
# builds it from source. For production, download a release binary instead.

stages:
  - security

kupol-scan:
  stage: security
  image: rust:1.80
  only:
    - merge_requests
  script:
    - cargo build --release
    - ./install.sh ./target/release/duo-agents
    - kupol scan . -f markdown -o kupol-report.md --fail-on high
  artifacts:
    when: always
    paths:
      - kupol-report.md
  variables:
    RUST_LOG: "info"
"#;

            std::fs::write(&path, ci_template)?;
            info!("Wrote {}", path.display());
            println!("CI pipeline ready. Commit '.gitlab-ci.yml' to enable KUPOL scans on merge requests.");
            return Ok(());
        }

        Some(cli::Commands::Mcp) => {
            return mcp_server::run_stdio_server().await;
        }

        Some(cli::Commands::Demo { number: _ }) | None => {
            // Run the multi-agent demo scenarios.
            crate::demos::run_demos().await?;
            return Ok(());
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// § API Server (Axum + React Dashboard)
// ─────────────────────────────────────────────────────────────────────────────

