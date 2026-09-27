//! What-If Scenario Modeling (Graph Incremental Cache)
//!
//! Инспирировано `petgraph.md` из Haskell KAN. Позволяет симулировать структурные
//! изменения в кодовой базе (например, замена одной библиотеки на другую)
//! *до* того, как будет написан код.
//!
//! Использует инкрементальное кеширование ("KV-Cache для графов"): при мутации графа
//! пересчитываются только смежные рёбра (O(E_adjacent)), а не весь проект (O(E_total)),
//! обеспечивая мгновенный Security Review для огромных монорепозиториев.

use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone)]
pub struct WhatIfNode {
    pub name: String,
    pub inherent_risk: f64, // Базовый риск модуля (например, история CVE)
}

#[derive(Debug, Clone)]
pub struct WhatIfEdge {
    pub from: String,
    pub to: String,
    pub weight: f64,
    /// KV-Cache: Расчитанный риск ребра. Сбрасывается при инвалидации.
    pub cached_risk_flow: Option<f64>,
}

pub struct ScenarioSimulatorGraph {
    nodes: HashMap<String, WhatIfNode>,
    edges: Vec<WhatIfEdge>,
}

impl ScenarioSimulatorGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: Vec::new(),
        }
    }

    pub fn add_node(&mut self, name: &str, risk: f64) {
        self.nodes.insert(name.to_string(), WhatIfNode {
            name: name.to_string(),
            inherent_risk: risk,
        });
        // Добавление узла напрямую ни на что не влияет, пока нет рёбер.
    }

    pub fn add_edge(&mut self, from: &str, to: &str, weight: f64) {
        self.edges.push(WhatIfEdge {
            from: from.to_string(),
            to: to.to_string(),
            weight,
            cached_risk_flow: None, // Ребро новое, нет кэша
        });
        
        // Инвалидируем соседей для честности, хотя ребро и так None
        self.invalidate_node_cache(from);
        self.invalidate_node_cache(to);
    }

    pub fn remove_node(&mut self, name: &str) {
        self.nodes.remove(name);
        
        // Находим все рёбра, связанные с узлом, удаляем их, 
        // и инвалидируем узлы, оставшиеся "на другом конце оборванного провода".
        let mut edges_to_keep = Vec::new();
        let mut affected_nodes = HashSet::new();

        for edge in &self.edges {
            if edge.from == name {
                affected_nodes.insert(edge.to.clone());
            } else if edge.to == name {
                affected_nodes.insert(edge.from.clone());
            } else {
                edges_to_keep.push(edge.clone());
            }
        }

        self.edges = edges_to_keep;

        for aff in affected_nodes {
            self.invalidate_node_cache(&aff);
        }
    }

    /// Инвалидация кэша O(E_adjacent)
    /// Сбрасывает `cached_risk_flow` только для тех рёбер, которые прилегают к `node`.
    fn invalidate_node_cache(&mut self, node: &str) {
        for edge in self.edges.iter_mut() {
            if edge.from == node || edge.to == node {
                edge.cached_risk_flow = None;
            }
        }
    }

    /// Выполняет Forward Pass по графу, вычисляя "напряжение риска" (Risk Flow)
    /// Использует инкрементальный кэш: вычисляет только `None` рёбра.
    pub fn compute_ecosystem_risk(&mut self) -> (f64, usize) {
        let mut total_ecosystem_risk = 0.0;
        let mut recomputed_edges_count = 0;

        // Временные данные для чтения базового риска
        let risk_map: HashMap<String, f64> = self.nodes.iter()
            .map(|(k, v)| (k.clone(), v.inherent_risk))
            .collect();

        for edge in self.edges.iter_mut() {
            if let Some(cached) = edge.cached_risk_flow {
                // ИСПОЛЬЗУЕМ КЭШ - Микросекундная задержка (O(1))
                total_ecosystem_risk += cached;
            } else {
                // Кэш промах: вычисляем Risk Flow на ребре
                recomputed_edges_count += 1;

                let src_risk = risk_map.get(&edge.from).copied().unwrap_or(0.0);
                let tgt_risk = risk_map.get(&edge.to).copied().unwrap_or(0.0);

                // Синтетическая нелинейная функция транзитивного риска:
                // Сильное напряжение возникает, если доверенный компонент обращается к компоненту с высоким риском.
                let risk_flow = edge.weight * (tgt_risk * 1.5 + src_risk * 0.5);
                
                edge.cached_risk_flow = Some(risk_flow);
                total_ecosystem_risk += risk_flow;
            }
        }

        (total_ecosystem_risk, recomputed_edges_count)
    }

    pub fn total_edges(&self) -> usize {
        self.edges.len()
    }
}
