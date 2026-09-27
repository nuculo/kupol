//! # 🔐 Security Analyzer Actor
//!
//! DFS taint-анализ по EntityGraph для обнаружения SQL-инъекций.
//! Использует AgentEdge стратегию и Ring All-Reduce для агрегации
//! находок от нескольких виртуальных сканеров.

use async_trait::async_trait;
use std::collections::HashSet;
use tracing::info;

use crate::models::*;
use crate::protocol::*;
use crate::actor::*;

// ─────────────────────────────────────────────────────────────────────────────
// § SecurityAnalyzerActor — DFS taint-анализ + AgentEdge стратегия
// ─────────────────────────────────────────────────────────────────────────────

/// Актор анализа безопасности: DFS-обход EntityGraph от Endpoint к DBQuery.
/// Если на пути нет Sanitizer — фиксируется уязвимость (SQL Injection).
pub struct SecurityAnalyzerActor {
    lifecycle: ActorLifecycle,
    /// AgentEdge стратегия: Explore ↔ Exploit мутация при каждом шаге
    agent_edge: crate::strategy::AgentEdge,
}

impl SecurityAnalyzerActor {
    pub fn new() -> Self {
        Self {
            lifecycle: ActorLifecycle::Active,
            agent_edge: crate::strategy::AgentEdge::new(),
        }
    }
}

#[async_trait]
impl Actor for SecurityAnalyzerActor {
    fn name(&self) -> &'static str { "SecurityAnalyzerActor" }
    fn lifecycle(&self) -> &ActorLifecycle { &self.lifecycle }
    fn set_lifecycle(&mut self, s: ActorLifecycle) { self.lifecycle = s; }

    async fn execute(&mut self, msg: Message) -> TxResult {
        if let Message::GraphReady { mr_id, files: _, graph } = msg {
            info!("🔐 [Sec:Execute] Активная стратегия: '{}' (Режим: {:?})",
                self.agent_edge.engine.name(), self.agent_edge.mode);

            // 1. Запуск Schema Migration Engine
            let legacy_yaml = r#"
banned_functions:
  - raw_query
  - eval
strict_layering: true
"#;
            let policy_v2 = crate::migration::parse_and_migrate(legacy_yaml);
            let max_depth = policy_v2.max_taint_depth.unwrap_or(5);

            // DFS от Endpoint к DBQuery: если нет Sanitizer на пути → уязвимость
            let mut vulns = Vec::new();
            let mut tainted_nodes = Vec::new();
            let mut plantuml = String::new();

            for idx in graph.graph.node_indices() {
                if let Some(ent) = graph.graph.node_weight(idx) {
                    if ent.kind == EntityKind::Endpoint {
                        let mut stack = vec![(idx, vec![ent.name.clone()])];
                        let mut visited = HashSet::new();

                        while let Some((node, path)) = stack.pop() {
                            if !visited.insert(node) { continue; }
                            if path.len() > max_depth { continue; } // Ограничение глубины из политики

                            let current_ent = graph.graph.node_weight(node).unwrap();
                            if current_ent.kind == EntityKind::Sanitizer { continue; }
                            if current_ent.kind == EntityKind::DBQuery {
                                vulns.push(format!("Graph Walk Violation (SQL Injection): {}", path.join(" -> ")));
                                tainted_nodes.push(current_ent.clone());

                                // Генерация PlantUML для tainted-пути
                                plantuml.push_str("```plantuml\n@startuml\nskinparam linetype ortho\nskinparam componentStyle uml2\n");
                                for (i, p) in path.iter().enumerate() {
                                    let color = if i == 0 { "#pink" } else if i == path.len() - 1 { "#red" } else { "#ffcccc" };
                                    plantuml.push_str(&format!("component \"{}\" as n{} {}\n", p, i, color));
                                }
                                for i in 0..path.len()-1 {
                                    let edge_label = if i == path.len() -2 { "Tainted (SQLi)" } else { "DataFlow" };
                                    plantuml.push_str(&format!("n{} --> n{} : {}\n", i, i+1, edge_label));
                                }
                                plantuml.push_str(&format!("note right of n{}\n  AST Patch:\n  format!(...) -> prepare_query(...)\nend note\n", path.len()-1));
                                plantuml.push_str("@enduml\n```\n");
                            }

                            for neighbor in graph.graph.neighbors(node) {
                                let mut new_path = path.clone();
                                new_path.push(graph.graph.node_weight(neighbor).unwrap().name.clone());
                                stack.push((neighbor, new_path));
                            }
                        }
                    }
                }
            }

            if vulns.is_empty() {
                // 🐜 AgentEdge: 0 уязвимостей → возможная мутация стратегии
                self.agent_edge.step(0);
                return TxResult::SecurityPass { mr_id };
            }

            // Упаковка tainted entities в patches для совместимости со Swarm
            let mut patches = Vec::new();
            for n in &tainted_nodes { patches.push(serde_json::to_string(&n).unwrap()); }

            // 🐜 AgentEdge: продуктивное сканирование → режим Exploit
            self.agent_edge.step(vulns.len());
            return TxResult::SecurityResult { mr_id, vulns, patches, plantuml };
        }
        TxResult::Ignored
    }

    async fn complete(&mut self, result: TxResult, ctx: &mut ActorContext) {
        match result {
            TxResult::SecurityResult { mr_id, vulns, patches, plantuml } => {
                // 📡 Ring All-Reduce: агрегация находок от 3 виртуальных сканеров
                let mut ring = crate::kan_intelligence::infrastructure::ring_reduce::FindingsRing::new(3);
                // Узел 0: DFS-находки от этого актора
                for v in &vulns {
                    ring.nodes[0].add_finding("SQLi", v.as_str(), 9);
                }
                // Узел 1: Имитация Pattern Matching сканера
                ring.nodes[1].add_finding("XSS", "Unescaped user input in template", 7);
                // Узел 2: Имитация LLM-сканера
                ring.nodes[2].add_finding("SQLi", vulns.first().map(|s| s.as_str()).unwrap_or("unknown"), 10);

                let merged = ring.all_reduce();
                info!("🔐 [Sec:Complete] Ring-агрегировано {} уникальных находок от 3 виртуальных сканеров", merged.len());

                let _ = ctx.orchestrator_tx.send(Message::SecurityVulnFound { mr_id, vulns, patches, plantuml }).await;
            }
            TxResult::SecurityPass { mr_id } => {
                let _ = ctx.orchestrator_tx.send(Message::SecurityScanPassed { mr_id }).await;
            }
            _ => {}
        }
    }
}
