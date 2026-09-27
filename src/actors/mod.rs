//! # 🧠 Доменные акторы платформы Duo Agent
//!
//! Реэкспорт всех специализированных акторов:
//!
//! | Модуль | Актор | Роль |
//! |--------|-------|------|
//! | `ast_analyzer` | `ASTAnalyzerActor` | Построение EntityGraph через semantic_engine |
//! | `drift_detector` | `DriftDetectorActor` | KQP-оптимизатор + KMeans Vector ANN |
//! | `security` | `SecurityAnalyzerActor` | DFS taint-анализ + Ring All-Reduce |
//! | `gitlab` | `GitLabMRActor` | API GitLab + авто-хилинг MR |
//! | `mcp_bridge` | `MCPBridgeActor` | JSON-RPC 2.0 → Jira MCP Server |
//! | `node_broker` | `NodeBrokerActor` | Регистрация узлов (YDB TTxRegisterNode) |
//! | `swarm/` | **Рой из 6 агентов** | Директория Swarm-агентов |
//! |  ├ `ast_fix_agent` | `AstFixAgent` | 🛠️ Семантические AST-патчи |
//! |  ├ `review_agent` | `ReviewAgent` | 👨‍💻 Код-ревью + телеметрия |
//! |  ├ `complexity_agent` | `ComplexityAgent` | 📏 Цикломатическая сложность |
//! |  ├ `dependency_agent` | `DependencyAgent` | 📦 Аудит зависимостей (CVE) |
//! |  ├ `doc_coverage_agent` | `DocCoverageAgent` | 📝 Покрытие документацией |
//! |  └ `aggregator` | `AggregatorActor` | 🐝 Fan-in агрегация |

pub mod ast_analyzer;
pub mod drift_detector;
pub mod security;
pub mod gitlab;
pub mod mcp_bridge;
pub mod swarm;
pub mod node_broker;
