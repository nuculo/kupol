//! Sparse MoE Routing — Inverted Index for Security Rules
//!
//! Вдохновлён паттерном Qdrant (sparse vectors) и Sparse MoE.
//! Вместо перебора всех `N` правил для каждого узла AST (Dense Routing O(NxM)),
//! мы используем инвертированный индекс фичей (AST Tokens / Keywords).
//!
//! Routing сводится к O(1) извлечению Posting List для активированных фичей,
//! позволяя поддерживать миллионы (Infinite) правил без деградации производительности.

use std::collections::{HashMap, HashSet};

/// Фичи (токенизированные свойства узла AST или строки кода)
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum AstFeature {
    /// Вызов конкретной функции (e.g., "eval", "exec", "query")
    FunctionCall(String),
    /// Обращение к свойству (e.g., "innerHTML", "location")
    PropertyAccess(String),
    /// Наличие строкового литерала
    ContainsStringLiteral,
    /// Математическая операция
    MathOperation,
    /// Специфичное ключевое слово (e.g., "password", "secret")
    Keyword(String),
}

/// Идентификатор правила
pub type RuleId = u32;

/// Метаданные правила (спрятаны за индексом)
#[derive(Debug, Clone)]
pub struct RuleMeta {
    pub id: RuleId,
    pub name: String,
    pub severity: String,
}

/// Sparse Router (Инвертированный Индекс)
pub struct SparseRouter {
    /// Inverted Index: Feature -> Posting List (Список Rule ID)
    inverted_index: HashMap<AstFeature, Vec<RuleId>>,
    /// Meta storage: RuleId -> RuleMeta
    rules: HashMap<RuleId, RuleMeta>,
    next_id: RuleId,
}

impl SparseRouter {
    pub fn new() -> Self {
        Self {
            inverted_index: HashMap::new(),
            rules: HashMap::new(),
            next_id: 1,
        }
    }

    /// Зарегистрировать правило, подписанное на конкретные фичи
    pub fn register_rule(&mut self, name: &str, severity: &str, triggers: Vec<AstFeature>) -> RuleId {
        let id = self.next_id;
        self.next_id += 1;

        self.rules.insert(id, RuleMeta {
            id,
            name: name.to_string(),
            severity: severity.to_string(),
        });

        // Добавляем правило в Posting List каждой её фичи-триггера
        for feature in triggers {
            self.inverted_index
                .entry(feature)
                .or_insert_with(Vec::new)
                .push(id);
        }

        id
    }

    /// Быстрый роутинг (Forward Pass):
    /// На вход поступают фичи анализируемого узла AST (sparse token).
    /// Возвращаем список уникальных правил, которые нужно для него запустить.
    pub fn route_node(&self, node_features: &[AstFeature]) -> Vec<&RuleMeta> {
        let mut triggered_rule_ids = HashSet::new();

        // Sparse WAND/Union Routing: проходим только по активированным Posting Lists
        for feature in node_features {
            if let Some(posting_list) = self.inverted_index.get(feature) {
                for &rule_id in posting_list {
                    triggered_rule_ids.insert(rule_id);
                }
            }
        }

        // Resolving IDs to Meta
        triggered_rule_ids
            .into_iter()
            .filter_map(|id| self.rules.get(&id))
            .collect()
    }

    /// Статистика индекса
    pub fn stats(&self) -> RouterStats {
        let mut max_posting_list_len = 0;
        let mut sum_posting_list_len = 0;

        for list in self.inverted_index.values() {
            let len = list.len();
            if len > max_posting_list_len { max_posting_list_len = len; }
            sum_posting_list_len += len;
        }

        let avg_posting_list_len = if self.inverted_index.is_empty() {
            0.0
        } else {
            sum_posting_list_len as f64 / self.inverted_index.len() as f64
        };

        RouterStats {
            total_rules: self.rules.len(),
            unique_features: self.inverted_index.len(),
            max_posting_list_size: max_posting_list_len,
            avg_posting_list_size: avg_posting_list_len,
        }
    }
}

#[derive(Debug)]
pub struct RouterStats {
    pub total_rules: usize,
    pub unique_features: usize,
    pub max_posting_list_size: usize,
    pub avg_posting_list_size: f64,
}
