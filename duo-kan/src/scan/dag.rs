//! Lazy DAG Pipeline — XLA-style Deferred Execution for Scan
//!
//! Вдохновлён `lazy_graph.clj` из Clojure KAN.
//! Вместо eager-выполнения (каждый плагин парсит файл заново),
//! плагины декларируют свои требования, pipeline строит DAG
//! и оптимизирует: CSE (Common Subexpression Elimination),
//! Dead Code Elimination, операция fusion.

use std::collections::{HashMap, HashSet};

// ── Требования плагинов ──────────────────────────────────────────────────────

/// Категория вычисления, которое может потребоваться плагину
#[derive(Debug, Clone, Hash, Eq, PartialEq)]
pub enum Requirement {
    /// Исходный текст файла (всегда доступен, стоимость 0)
    RawContent,
    /// Построение AST (дорогая операция)
    ParseAST,
    /// Извлечение паттернов из AST
    ExtractPatterns,
    /// Анализ control flow графа
    ControlFlowGraph,
    /// Data flow / taint tracking
    DataFlowAnalysis,
    /// Git blame информация
    GitBlame,
    /// Извлечение строковых литералов
    StringLiterals,
    /// Dependency graph (import/use)
    DependencyGraph,
}

impl Requirement {
    /// Стоимость вычисления (условные единицы CPU)
    pub fn cost(&self) -> usize {
        match self {
            Requirement::RawContent => 0,
            Requirement::StringLiterals => 1,
            Requirement::ParseAST => 10,
            Requirement::ExtractPatterns => 5,
            Requirement::ControlFlowGraph => 15,
            Requirement::DataFlowAnalysis => 20,
            Requirement::GitBlame => 8,
            Requirement::DependencyGraph => 12,
        }
    }

    /// Зависимости этого вычисления
    pub fn dependencies(&self) -> Vec<Requirement> {
        match self {
            Requirement::RawContent => vec![],
            Requirement::StringLiterals => vec![Requirement::RawContent],
            Requirement::ParseAST => vec![Requirement::RawContent],
            Requirement::ExtractPatterns => vec![Requirement::ParseAST],
            Requirement::ControlFlowGraph => vec![Requirement::ParseAST],
            Requirement::DataFlowAnalysis => vec![Requirement::ControlFlowGraph],
            Requirement::GitBlame => vec![Requirement::RawContent],
            Requirement::DependencyGraph => vec![Requirement::ParseAST],
        }
    }
}

// ── Декларация плагина ───────────────────────────────────────────────────────

/// Декларативное описание того, что плагин хочет от pipeline
#[derive(Debug, Clone)]
pub struct PluginDeclaration {
    pub name: String,
    pub requirements: Vec<Requirement>,
}

// ── DAG узел ─────────────────────────────────────────────────────────────────

/// Узел в DAG выполнения
#[derive(Debug, Clone)]
pub struct DagNode {
    pub requirement: Requirement,
    /// Количество плагинов, зависящих от этого узла
    pub consumers: usize,
    /// Стоимость вычисления
    pub cost: usize,
    /// Был ли уже вычислен (для CSE)
    pub computed: bool,
}

// ── Scan DAG ─────────────────────────────────────────────────────────────────

/// Lazy DAG — оптимизированный граф вычислений для scan pipeline
pub struct ScanDag {
    /// Зарегистрированные плагины
    plugins: Vec<PluginDeclaration>,
    /// Граф: requirement → DagNode
    nodes: HashMap<Requirement, DagNode>,
}

impl ScanDag {
    /// Создать пустой DAG
    pub fn new() -> Self {
        Self {
            plugins: Vec::new(),
            nodes: HashMap::new(),
        }
    }

    /// Зарегистрировать плагин с его декларацией требований
    pub fn register(&mut self, decl: PluginDeclaration) {
        // Добавить все требования плагина (+ транзитивные зависимости)
        for req in &decl.requirements {
            self.add_requirement_recursive(req);
            // Увеличить счётчик consumer'ов
            if let Some(node) = self.nodes.get_mut(req) {
                node.consumers += 1;
            }
        }
        self.plugins.push(decl);
    }

    /// Рекурсивно добавить requirement + все его зависимости
    fn add_requirement_recursive(&mut self, req: &Requirement) {
        if self.nodes.contains_key(req) {
            return; // CSE: уже есть в графе
        }

        // Сначала добавить зависимости
        for dep in req.dependencies() {
            self.add_requirement_recursive(&dep);
        }

        // Добавить сам узел
        self.nodes.insert(req.clone(), DagNode {
            requirement: req.clone(),
            consumers: 0,
            cost: req.cost(),
            computed: false,
        });
    }

    /// Оптимизация 1: Dead Code Elimination
    /// Удалить узлы, которые никому не нужны (consumers == 0 и не зависимость)
    pub fn eliminate_dead_code(&mut self) -> usize {
        let needed: HashSet<Requirement> = self.plugins.iter()
            .flat_map(|p| {
                let mut all = Vec::new();
                for req in &p.requirements {
                    all.push(req.clone());
                    Self::collect_deps(req, &mut all);
                }
                all
            })
            .collect();

        let before = self.nodes.len();
        self.nodes.retain(|k, _| needed.contains(k));
        before - self.nodes.len()
    }

    /// Собрать все транзитивные зависимости
    fn collect_deps(req: &Requirement, out: &mut Vec<Requirement>) {
        for dep in req.dependencies() {
            out.push(dep.clone());
            Self::collect_deps(&dep, out);
        }
    }

    /// Выполнить DAG в топологическом порядке
    pub fn execute(&mut self) -> ExecutionPlan {
        let mut order: Vec<Requirement> = Vec::new();
        let mut visited: HashSet<Requirement> = HashSet::new();

        // Topological sort (DFS)
        let keys: Vec<Requirement> = self.nodes.keys().cloned().collect();
        for req in &keys {
            self.topo_visit(req, &mut visited, &mut order);
        }

        // Пометить все как computed
        let mut total_cost = 0;
        let mut cse_savings = 0;

        for req in &order {
            if let Some(node) = self.nodes.get_mut(req) {
                if !node.computed {
                    total_cost += node.cost;
                    node.computed = true;
                    // CSE: если consumer > 1, мы сэкономили (consumers - 1) * cost
                    if node.consumers > 1 {
                        cse_savings += (node.consumers - 1) * node.cost;
                    }
                }
            }
        }

        // Наивная стоимость (без CSE): каждый плагин вычисляет всё сам
        let naive_cost: usize = self.plugins.iter()
            .flat_map(|p| {
                let mut all = Vec::new();
                for req in &p.requirements {
                    all.push(req.clone());
                    Self::collect_deps(req, &mut all);
                }
                all
            })
            .map(|r| r.cost())
            .sum();

        ExecutionPlan {
            execution_order: order,
            total_cost,
            naive_cost,
            cse_savings,
            plugin_count: self.plugins.len(),
            node_count: self.nodes.len(),
        }
    }

    /// Topological sort helper (DFS post-order)
    fn topo_visit(
        &self,
        req: &Requirement,
        visited: &mut HashSet<Requirement>,
        order: &mut Vec<Requirement>,
    ) {
        if visited.contains(req) {
            return;
        }
        visited.insert(req.clone());

        // Рекурсивно посещаем зависимости
        for dep in req.dependencies() {
            if self.nodes.contains_key(&dep) {
                self.topo_visit(&dep, visited, order);
            }
        }

        order.push(req.clone());
    }
}

/// План выполнения DAG
#[derive(Debug)]
pub struct ExecutionPlan {
    pub execution_order: Vec<Requirement>,
    pub total_cost: usize,
    pub naive_cost: usize,
    pub cse_savings: usize,
    pub plugin_count: usize,
    pub node_count: usize,
}

impl ExecutionPlan {
    /// Процент экономии CPU благодаря CSE
    pub fn savings_percent(&self) -> f64 {
        if self.naive_cost == 0 {
            0.0
        } else {
            (1.0 - self.total_cost as f64 / self.naive_cost as f64) * 100.0
        }
    }
}
