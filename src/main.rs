//! # 🚀 Duo Agent Platform — Точка входа
//!
//! Модульная платформа анализа Merge Request на основе YDB-акторов.
//!
//! ## Архитектура модулей
//!
//! ```text
//! src/
//! ├── main.rs           — Точка входа + демо-сценарии
//! ├── models.rs         — Модели сущностей (Entity, EntityGraph, AstPatch)
//! ├── protocol.rs       — YDB-протоколы (Message, GraphDelta, DynBitMap, NodeInfo)
//! ├── actor.rs          — Actor trait (2-фазная модель Execute/Complete)
//! ├── actors/           — Доменные акторы
//! │   ├── ast_analyzer  — Построение EntityGraph
//! │   ├── drift_detector— KQP-оптимизатор + KMeans Vector ANN
//! │   ├── security      — DFS taint-анализ + Ring All-Reduce
//! │   ├── gitlab        — GitLab API клиент + авто-хилинг MR
//! │   ├── mcp_bridge    — JSON-RPC 2.0 → MCP Server
//! │   ├── swarm         — AstFixAgent + ReviewAgent + AggregatorActor
//! │   └── node_broker   — Регистрация узлов (YDB TTxRegisterNode)
//! ├── orchestrator.rs   — FlowOrchestratorActor + WebSocket
//! ├── semantic_engine/  — HIR Call Graph через syn
//! ├── kan_intelligence/ — 12 модулей из Clojure KAN
//! └── ...               — Вспомогательные модули
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
use tokio::sync::{mpsc, broadcast};
use tokio::time::Duration;
use tokio::net::TcpListener;
use tracing::{info, warn, error};
use clap::Parser;
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
    response::IntoResponse,
};
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;

// ─────────────────────────────────────────────────────────────────────────────
// § Shared State для API
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
    tracing_subscriber::fmt().with_target(false).with_thread_ids(false).without_time().init();

    let cli_args = cli::Cli::parse();

    match cli_args.command {
        Some(cli::Commands::Scan { path, format, output, min_severity, mr, project }) => {
            info!("🔍 Запуск сканирования: {}", path);
            let result = scan::run_scan(&path);

            // Filter by severity
            let min_sev = scan::Severity::from_str(&min_severity);
            let filtered_findings = scan::graders::filter_by_severity(&result.findings, &min_sev);
            let filtered_result = scan::ScanResult {
                findings: filtered_findings,
                summary: scan::ScanSummary::from_findings(&result.findings, result.summary.total_files),
                ..result
            };

            match format.as_str() {
                "json" => {
                    let json = scan::report::to_json(&filtered_result);
                    if let Some(path) = output {
                        std::fs::write(&path, &json)?;
                        info!("📄 JSON-отчёт сохранён: {}", path);
                    } else {
                        println!("{}", json);
                    }
                }
                "markdown" | "md" => {
                    let md = scan::report::to_markdown(&filtered_result);
                    if let Some(path) = output {
                        std::fs::write(&path, &md)?;
                        info!("📄 Markdown-отчёт сохранён: {}", path);
                    } else {
                        println!("{}", md);
                    }
                }
                _ => {
                    scan::report::print_table(&filtered_result);
                    if let Some(path) = output {
                        let json = scan::report::to_json(&filtered_result);
                        std::fs::write(&path, &json)?;
                        info!("📄 Отчёт также сохранён: {}", path);
                    }
                }
            }

            // GitLab MR Integration
            if let Some(mr_id) = mr {
                let project_id = project.or_else(|| std::env::var("GITLAB_PROJECT_ID").ok()).unwrap_or_default();
                let token = std::env::var("GITLAB_TOKEN").ok().unwrap_or_default();
                let gitlab_url = std::env::var("GITLAB_URL").unwrap_or_else(|_| "https://gitlab.example.com".to_string());
                
                if !project_id.is_empty() && !token.is_empty() {
                    let client = actors::gitlab::GitLabClient::new(&gitlab_url, &token, &project_id);
                    // Мы всегда генерируем Markdown для GitLab
                    let md_report = scan::report::to_markdown(&filtered_result);
                    
                    info!("📝 Публикация отчёта в GitLab MR #{} на {} (Project: {})", mr_id, gitlab_url, project_id);
                    match client.comment_mr(mr_id, &md_report).await {
                        Ok(_) => info!("✅ Отчёт успешно опубликован в MR!"),
                        Err(e) => error!("❌ Ошибка публикации в GitLab: {}", e),
                    }
                } else {
                    warn!("⚠️ Не задан GITLAB_TOKEN или GITLAB_PROJECT_ID (флагом или env). Отчёт в MR не опубликован.");
                }
            }

            return Ok(());
        }

        Some(cli::Commands::Serve { port, .. }) => {
            return crate::server::run_server(port).await;
        }

        Some(cli::Commands::Report { format, output }) => {
            info!("📊 Экспорт последних результатов...");
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
            println!("  🛡️  Duo Architecture Guardian v{}", env!("CARGO_PKG_VERSION"));
            println!("  ──────────────────────────────────");
            println!("  Actors:     12 (AST, Security, Drift, GitLab, MCP, Swarm×6, Broker)");
            println!("  Plugins:    {} security plugins", scan::plugins::all_plugins().len());
            println!("  Formats:    table, json, markdown");
            println!("  Runtime:    Tokio + Axum + WebSocket");
            println!("  Dashboard:  React + TailwindCSS");
            println!();
            return Ok(());
        }

        Some(cli::Commands::Init { dir, force }) => {
            info!("🛠️ Генерация CI/CD пайплайна...");
            let path = std::path::Path::new(&dir).join(".gitlab-ci.yml");
            
            if path.exists() && !force {
                warn!("⚠️ Файл {} уже существует. Используйте --force для его перезаписи.", path.display());
                std::println!("Pipeline generation skipped.");
                return Ok(());
            }

            let ci_template = r#"
# 🛡️ Платформа Duo Architecture Guardian
# Автоматически сгенерированный пайплайн для проверки Merge Requests.
# 
# Требования:
# 1. Установите GITLAB_TOKEN в Settings -> CI/CD -> Variables.
# 2. GITLAB_PROJECT_ID и CI_MERGE_REQUEST_IID будут переданы автоматически.
#
# Этот job запускается ТОЛЬКО на Merge Requests.

stages:
  - security-review

duo-agents-scan:
  stage: security-review
  image: rust:1.80 # Используем актуальный образ Rust
  only:
    - merge_requests
  script:
    - echo "🚀 Установка Duo Agents..."
    # Для production рекомендуется скачивать релизный бинарник:
    # - wget https://example.com/duo-agents-x86_64-linux -O duo-agents && chmod +x duo-agents
    # В демо устанавливаем из исходников:
    - cargo install --path . || true 
    - echo "🔍 Запуск сканирования и публикация в MR..."
    - duo-agents scan src/ --mr $CI_MERGE_REQUEST_IID --project $CI_PROJECT_ID
  variables:
    # Задайте GITLAB_URL, если используете self-hosted GitLab (по умолчанию: https://gitlab.com)
    # GITLAB_URL: "https://gitlab.example.com"
    RUST_LOG: "info"
        "#;

            std::fs::write(&path, ci_template.trim())?;
            info!("✅ Файл {} успешно сгенерирован!", path.display());
            std::println!("GitLab CI/CD Pipeline ready. Make sure to commit '.gitlab-ci.yml' and set GITLAB_TOKEN in your project variables.");
            return Ok(());
        }

        Some(cli::Commands::Mcp) => {
            return mcp_server::run_stdio_server().await;
        }

        Some(cli::Commands::Demo { number: _ }) | None => {
            // Запуск демо-сценариев (оригинальный код)
            crate::demos::run_demos().await?;
            return Ok(());
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// § API Server (Axum + React Dashboard)
// ─────────────────────────────────────────────────────────────────────────────

