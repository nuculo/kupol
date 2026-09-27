//! # 🐝 Рой Swarm-агентов (Swarm Agent Directory)
//!
//! Модульная архитектура роя: каждый агент — отдельный файл.
//! Новые агенты добавляются как файлы в эту директорию.
//!
//! ## Архитектура роя (Mermaid)
//!
//! ```mermaid
//! graph TB
//!     subgraph "🔱 FlowOrchestrator"
//!         ORCH["FlowOrchestratorActor<br/>DAG Controller"]
//!     end
//!
//!     subgraph "🐝 Swarm — Рой Агентов"
//!         direction LR
//!         subgraph "🔍 Анализ"
//!             FIX["🛠️ AstFixAgent<br/>Семантические AST-патчи"]
//!             REV["👨‍💻 ReviewAgent<br/>Код-ревью + телеметрия"]
//!             CMPL["📏 ComplexityAgent<br/>Цикломатическая сложность"]
//!         end
//!         subgraph "🔬 Аудит"
//!             DEP["📦 DependencyAgent<br/>Аудит зависимостей CVE"]
//!             DOC["📝 DocCoverageAgent<br/>Покрытие документацией"]
//!         end
//!         subgraph "⚙️ Инфраструктура"
//!             AGG["🐝 AggregatorActor<br/>Fan-in агрегация"]
//!         end
//!     end
//!
//!     subgraph "🌐 Внешние"
//!         GL["GitLabMRActor<br/>API + авто-хилинг"]
//!     end
//!
//!     ORCH -->|"MergeRequestCreated"| REV
//!     ORCH -->|"MergeRequestCreated"| CMPL
//!     ORCH -->|"MergeRequestCreated"| DEP
//!     ORCH -->|"MergeRequestCreated"| DOC
//!     ORCH -->|"SecurityVulnFound"| FIX
//!     REV -->|"ReviewReport"| AGG
//!     CMPL -->|"ReviewReport"| AGG
//!     DEP -->|"ReviewReport"| AGG
//!     DOC -->|"ReviewReport"| AGG
//!     FIX -->|"FixPatch"| AGG
//!     AGG -->|"AggregatedResult"| GL
//! ```
//!
//! ## Файлы агентов
//!
//! | Файл | Агент | Роль |
//! |------|-------|------|
//! | `ast_fix_agent.rs` | `AstFixAgent` | 🛠️ Семантические AST-патчи через ContextEngine |
//! | `review_agent.rs` | `ReviewAgent` | 👨‍💻 Код-ревью + структурированная телеметрия |
//! | `complexity_agent.rs` | `ComplexityAgent` | 📏 Цикломатическая сложность функций |
//! | `dependency_agent.rs` | `DependencyAgent` | 📦 Аудит crate-зависимостей (CVE) |
//! | `doc_coverage_agent.rs` | `DocCoverageAgent` | 📝 Покрытие документацией (pub fn/struct) |
//! | `aggregator.rs` | `AggregatorActor` | 🐝 Fan-in агрегация от всех агентов |

pub mod ast_fix_agent;
pub mod review_agent;
pub mod complexity_agent;
pub mod dependency_agent;
pub mod doc_coverage_agent;
pub mod aggregator;

// Реэкспорт для обратной совместимости
pub use ast_fix_agent::AstFixAgent;
pub use review_agent::ReviewAgent;
pub use complexity_agent::ComplexityAgent;
pub use dependency_agent::DependencyAgent;
pub use doc_coverage_agent::DocCoverageAgent;
pub use aggregator::AggregatorActor;
