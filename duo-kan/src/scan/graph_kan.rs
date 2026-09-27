//! Graph KAN Routing (Supply Chain Code Intelligence)
//!
//! Инспирировано логистической подсистемой Haskell KAN (PetGraph + KAN pёбра).
//! Вместо плоского списка файлов мы строим Направленный Граф Зависимостей (Directed Graph).
//! Функция активации (KAN-модель) работает НА РЁБРАХ:
//! `φ(src_vulns, tgt_vulns, dependency_weight) -> scan_priority`.
//!
//! Когда в базовом модуле происходит "шок" (найдена критическая уязвимость),
//! KAN на рёбрах графа автоматически перемножает и усиливает приоритет сканирования
//! для всех модулей, ЗАВИСЯЩИХ от скомпрометированного узла.

use std::collections::HashMap;

#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub struct NodeId(pub String);

/// Узел графа (Микросервис или Модуль)
#[derive(Debug, Clone)]
pub struct ScanNode {
    pub id: NodeId,
    /// Количество найденных уязвимостей (Дефицит / Шок)
    pub vuln_count: u32,  
    /// Накопленный приоритет сканирования (от всех входящих рёбер)
    pub scan_priority: f64,
}

/// Ребро графа (Зависимость: from зависит от to)
#[derive(Debug, Clone)]
pub struct ScanEdge {
    pub from: NodeId,
    pub to: NodeId,
    /// Сила зависимости (например, сколько раз вызывается API: 0.0 - 1.0)
    pub dependency_weight: f64,
}

pub struct DependencyGraph {
    nodes: HashMap<NodeId, ScanNode>,
    edges: Vec<ScanEdge>,
}

impl DependencyGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: Vec::new(),
        }
    }

    pub fn add_node(&mut self, id: &str) {
        let nid = NodeId(id.to_string());
        self.nodes.insert(nid.clone(), ScanNode {
            id: nid,
            vuln_count: 0,
            scan_priority: 1.0, // Базовый приоритет
        });
    }

    pub fn add_edge(&mut self, from: &str, to: &str, weight: f64) {
        self.edges.push(ScanEdge {
            from: NodeId(from.to_string()),
            to: NodeId(to.to_string()),
            dependency_weight: weight,
        });
    }

    /// Впрыскивает "шок" (найдена уязвимость в модуле)
    pub fn inject_shock(&mut self, node: &str, vulns: u32) {
        if let Some(n) = self.nodes.get_mut(&NodeId(node.to_string())) {
            n.vuln_count += vulns;
        }
    }

    /// SiLU (Sigmoid Linear Unit) активация
    fn silu(x: f64) -> f64 {
        x / (1.0 + (-x).exp())
    }

    /// KAN Forward Pass на рёбрах графа (Supply Chain Routing)
    pub fn compute_routing_priorities(&mut self) {
        // 1. Сброс приоритетов до базового уровня
        for node in self.nodes.values_mut() {
            node.scan_priority = 1.0 + (node.vuln_count as f64 * 0.5); // Если сам болен - приоритет выше
        }

        // 2. Распространение "гравитации" по рёбрам
        // В реальном KAN здесь B-spline, для прототипа используем SiLU и нелинейность.
        let mut priority_deltas: HashMap<NodeId, f64> = HashMap::new();

        for edge in &self.edges {
            let src_vulns = self.nodes.get(&edge.to).map(|n| n.vuln_count).unwrap_or(0) as f64;
            // Если модуль, от которого мы зависим, имеет уязвимости, 
            // мы пропускаем этот "шок" через KAN ребра.
            
            // φ_edge = weight * SiLU(src_vulns * 2.0)
            let phi_edge = edge.dependency_weight * Self::silu(src_vulns * 2.0);

            // Добавляем этот импульс модулю-потребителю (from)
            *priority_deltas.entry(edge.from.clone()).or_insert(0.0) += phi_edge;
        }

        // 3. Применяем импульсы
        for (id, delta) in priority_deltas {
            if let Some(node) = self.nodes.get_mut(&id) {
                // Экспоненциальное взрывание приоритета при транзитивном шоке
                node.scan_priority += delta * 15.0; 
            }
        }
    }

    /// Возвращает очередь модулей на сканирование, отсортированную по приоритету
    pub fn get_scan_queue(&self) -> Vec<(&String, f64, u32)> {
        let mut queue: Vec<_> = self.nodes.values()
            .map(|n| (&n.id.0, n.scan_priority, n.vuln_count))
            .collect();
        
        // Сортировка по убыванию приоритета
        queue.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        queue
    }
}
