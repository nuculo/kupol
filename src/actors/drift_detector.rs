//! # 🧠 Drift Detector Actor (KQP-оптимизатор + KMeans Vector ANN)
//!
//! Обнаружение архитектурного дрифта через два механизма:
//!
//! 1. **KMeans Vector ANN** — Семантический поиск ближайшего правила в дереве
//!    кластеров Level 1 (Root) → Level 0 (Leaf) по косинусному сходству.
//!
//! 2. **KQP Optimizer Pipeline** — Структурная проверка рёберных правил:
//!    - Peephole Rewrite: удаление самопетлей
//!    - PushOlapFilter: O(1) проверка через инвертированный индекс
//!
//! Вдохновлено YDB KQP-оптимизатором запросов.

use async_trait::async_trait;
use std::collections::{HashMap, HashSet};
use tracing::info;

use crate::protocol::*;
use crate::actor::*;

// ─────────────────────────────────────────────────────────────────────────────
// § 1. Математические утилиты
// ─────────────────────────────────────────────────────────────────────────────

/// Косинусное сходство двух 8-мерных векторов (для ANN-поиска правил).
pub fn cosine_similarity(a: &[f32; 8], b: &[f32; 8]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 { 0.0 } else { dot / (na * nb) }
}

// ─────────────────────────────────────────────────────────────────────────────
// § 2. Архитектурные правила и кластеры (KMeans)
// ─────────────────────────────────────────────────────────────────────────────

/// Архитектурное правило с эмбеддингом для ANN-поиска.
#[derive(Clone)]
pub struct ArchRule {
    pub id: &'static str,
    pub desc: &'static str,
    pub emb: [f32; 8],
}

/// KMeans-кластер правил. Level 1 (корень) содержит Level 0 (листья).
#[derive(Clone)]
pub struct KMeansCluster {
    pub name: &'static str,
    pub centroid_emb: [f32; 8],
    pub children: Vec<ArchRule>,
}

// ─────────────────────────────────────────────────────────────────────────────
// § 3. KQP Optimizer Pipeline (из YDB)
// ─────────────────────────────────────────────────────────────────────────────

/// Логическое правило: допустимые цели для данного источника.
#[derive(Clone)]
pub struct LogicalRule {
    pub source: &'static str,
    pub allowed_targets: Vec<&'static str>,
}

/// Физический OLAP-фильтр (инвертированный индекс для O(1) проверки рёбер).
pub struct PhyOlapFilter {
    pub compiled_rules: HashMap<&'static str, HashSet<&'static str>>,
}

impl PhyOlapFilter {
    /// Компиляция логических правил в инвертированный индекс.
    pub fn compile(logical_rules: &[LogicalRule]) -> Self {
        let mut map = HashMap::new();
        for rule in logical_rules {
            let set: HashSet<&str> = rule.allowed_targets.iter().copied().collect();
            map.insert(rule.source, set);
        }
        info!("   ⚙️ [KQP:Opt] Скомпилировано {} LogicalRules в PhyOlapFilter (O(1) исполнение)", logical_rules.len());
        Self { compiled_rules: map }
    }

    /// Проверить ребро: если цель не в списке допустимых → нарушение.
    pub fn execute(&self, delta: &GraphDelta) -> Option<(String, String)> {
        if let GraphDelta::EdgeAdded(c, t) = delta {
            if let Some(allowed) = self.compiled_rules.get(c.as_str()) {
                if !allowed.contains(t.as_str()) {
                    return Some((c.clone(), t.clone()));
                }
            }
        }
        None
    }
}

/// KQP-оптимизатор графовых правил (2-фазный конвейер).
pub struct KqpRuleOptimizer {
    pub physical_plan: PhyOlapFilter,
}

impl KqpRuleOptimizer {
    /// Создать оптимизатор из набора логических правил.
    pub fn new(rules: &[LogicalRule]) -> Self {
        Self { physical_plan: PhyOlapFilter::compile(rules) }
    }

    /// Оптимизировать и исполнить проверку рёбер:
    /// 1. Peephole Rewrite — удалить самопетли (A→A)
    /// 2. PushOlapFilter — быстрая проверка через инвертированный индекс
    pub fn optimize_and_execute(&self, deltas: Vec<GraphDelta>) -> Vec<(String, String)> {
        let mut violations = Vec::new();

        // Фаза 1: Peephole Rewrite — удаление избыточных внутримодульных вызовов
        let mut optimized_deltas = Vec::new();
        for d in deltas {
            if let GraphDelta::EdgeAdded(c, t) = &d {
                if c == t {
                    info!("   ⚙️ [KQP:Peephole] Авто-удаление внутреннего ребра: {}→{}", c, t);
                    continue; // Полностью пропускаем физический фильтр!
                }
            }
            optimized_deltas.push(d);
        }

        // Фаза 2: PushOlapFilter — быстрая векторная оценка
        info!("   ⚙️ [KQP:Phy] Исполнение PushOlapFilter на {} оптимизированных дельтах...", optimized_deltas.len());
        for d in optimized_deltas {
            if let Some(violation) = self.physical_plan.execute(&d) {
                violations.push(violation);
            }
        }

        violations
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// § 4. DriftDetectorActor
// ─────────────────────────────────────────────────────────────────────────────

/// Актор обнаружения архитектурного дрифта.
/// Комбинирует KMeans ANN-поиск и KQP-оптимизатор для полного анализа.
pub struct DriftDetectorActor {
    lifecycle: ActorLifecycle,
    optimizer: KqpRuleOptimizer,
    root_clusters: Vec<KMeansCluster>,
}

impl DriftDetectorActor {
    /// Создать актор с предустановленными правилами и кластерами.
    pub fn new() -> Self {
        let logical_rules = vec![
            LogicalRule { source: "frontend", allowed_targets: vec!["api"] },
        ];
        let optimizer = KqpRuleOptimizer::new(&logical_rules);

        let c1 = KMeansCluster {
            name: "Arch_Guidelines",
            centroid_emb: [0.8, 0.1, 0.1, 0.9, 0.8, 0.1, 0.1, 0.9],
            children: vec![
                ArchRule { id: "GUIDE-01", desc: "Do not invent custom JWT. Use GitLab SSO/OmniAuth.", emb: [0.9, 0.0, 0.0, 0.8, 0.9, 0.0, 0.0, 0.8] },
                ArchRule { id: "GUIDE-03", desc: "API Gateway MUST sit behind Envoy sidecar.", emb: [0.7, 0.1, 0.2, 0.9, 0.7, 0.1, 0.2, 0.9] },
            ]
        };
        let c2 = KMeansCluster {
            name: "Performance_AntiPatterns",
            centroid_emb: [0.1, 0.9, 0.8, 0.1, 0.1, 0.9, 0.8, 0.1],
            children: vec![
                ArchRule { id: "GUIDE-02", desc: "Direct DB from UI prohibited. Use GraphQL.", emb: [0.1, 0.9, 0.8, 0.1, 0.1, 0.9, 0.8, 0.1] },
                ArchRule { id: "GUIDE-04", desc: "N+1 Queries strictly forbidden. Use batching.", emb: [0.2, 0.9, 0.9, 0.0, 0.2, 0.9, 0.9, 0.0] },
            ]
        };

        Self { lifecycle: ActorLifecycle::Active, optimizer, root_clusters: vec![c1, c2] }
    }

    /// Заглушка эмбеддинга: распознаёт "jwt"/"auth" темы.
    fn mock_embed(title: &str) -> [f32; 8] {
        if title.to_lowercase().contains("jwt") || title.to_lowercase().contains("auth") {
            [0.85, 0.05, 0.1, 0.9, 0.85, 0.05, 0.1, 0.9]
        } else {
            [0.1, 0.1, 0.9, 0.1, 0.1, 0.1, 0.9, 0.1]
        }
    }
}

#[async_trait]
impl Actor for DriftDetectorActor {
    fn name(&self) -> &'static str { "DriftDetectorActor" }
    fn lifecycle(&self) -> &ActorLifecycle { &self.lifecycle }
    fn set_lifecycle(&mut self, s: ActorLifecycle) { self.lifecycle = s; }

    async fn execute(&mut self, msg: Message) -> TxResult {
        if let Message::CheckDrift { mr_id, title, deltas } = msg {
            info!("🧠 [Drift:Execute] Семантический Vector ANN (KMeans по уровням) поиск...");
            let qe = Self::mock_embed(&title);

            // ═══════════════════════════════════════════════════════════════════
            // KMeans Cluster Route: Level 1 (корень) → Level 0 (листья)
            // ═══════════════════════════════════════════════════════════════════
            let root_cluster_names: Vec<&str> = self.root_clusters.iter().map(|c| c.name).collect();
            info!("   ⚙️ [KMeans:Level 1] Оценка {} корневых кластеров: {:?}", self.root_clusters.len(), root_cluster_names);

            let mut best_cluster = None;
            let mut best_root_sim = -1.0;

            for cluster in &self.root_clusters {
                let sim = cosine_similarity(&qe, &cluster.centroid_emb);
                if sim > best_root_sim {
                    best_root_sim = sim;
                    best_cluster = Some(cluster);
                }
            }

            let mut sem = None;
            if let Some(cluster) = best_cluster {
                info!("   ⚙️ [KMeans:Level 1] Ближайший корневой кластер: \"{}\" (cos:{:.2})", cluster.name, best_root_sim);
                info!("   ⚙️ [KMeans:Level 0] Спуск в кластер. Оценка {} листовых правил...", cluster.children.len());

                for rule in &cluster.children {
                    let sim = cosine_similarity(&qe, &rule.emb);
                    if sim > 0.85 {
                        info!("   ⚠️ ANN совпадение (cos={:.2}): {}", sim, rule.desc);
                        sem = Some(format!("Context [{}] -> {} (cos:{:.2}, {})", cluster.name, rule.desc, sim, rule.id));
                        break; // Найдено лучшее совпадение
                    }
                }
            }

            // ═══════════════════════════════════════════════════════════════════
            // KQP-фаза оптимизации
            // ═══════════════════════════════════════════════════════════════════
            let mut puml = String::from("@startuml\nskinparam linetype ortho\n");
            for d in &deltas {
                if let GraphDelta::EdgeAdded(c, t) = d {
                    if c == t { puml.push_str(&format!("[{}] --> [{}] : Internal\n", c, t)); }
                    else { puml.push_str(&format!("[{}] --> [{}]\n", c, t)); }
                }
                if let GraphDelta::EdgeRemoved(c, t) = d {
                    puml.push_str(&format!("[{}] -[#gray]-> [{}] : 🗑️ Removed\n", c, t));
                }
            }

            // Исполнение KQP-оптимизатора
            let violations = self.optimizer.optimize_and_execute(deltas);

            for (c, t) in &violations {
                puml = puml.replace(&format!("[{}] --> [{}]\n", c, t), &format!("[{}] -[#red]-> [{}] : 🚨 Drift\n", c, t));
            }
            puml.push_str("@enduml");

            if violations.is_empty() && sem.is_none() {
                return TxResult::DriftPass { mr_id };
            }
            return TxResult::DriftResult { mr_id, violations, semantic_drift: sem, plantuml: puml };
        }
        TxResult::Ignored
    }

    async fn complete(&mut self, result: TxResult, ctx: &mut ActorContext) {
        match result {
            TxResult::DriftResult { mr_id, violations, semantic_drift, plantuml } => {
                info!("🧠 [Drift:Complete] Продвижение нарушений в Committed");
                let _ = ctx.orchestrator_tx.send(Message::DriftDetected { mr_id, violations, semantic_drift, plantuml }).await;
            }
            TxResult::DriftPass { mr_id } => {
                let _ = ctx.orchestrator_tx.send(Message::DriftPassed { mr_id }).await;
            }
            _ => {}
        }
    }
}
