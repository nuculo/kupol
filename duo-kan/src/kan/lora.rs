//! LoRA Fine-Tuning — Low-Rank Adaptation for Security Plugin Rules
//!
//! Вдохновлён `lora.clj` из Clojure KAN.
//! Вместо копирования и модификации всего плагина, клиент создаёт
//! маленький «LoRA-адаптер» — набор дельт поверх замороженных базовых правил.
//!
//! effective_rules = frozen_base_rules + lora_delta_rules
//! effective_weights = base_weights ⊕ (A × B)  // low-rank decomposition

use std::collections::HashMap;

/// Одно правило плагина с весом
#[derive(Debug, Clone)]
pub struct WeightedRule {
    pub pattern: String,
    pub weight: f64,       // 1.0 = полный вес, 0.0 = отключено
    pub source: RuleSource,
}

/// Источник правила
#[derive(Debug, Clone, PartialEq)]
pub enum RuleSource {
    /// Замороженное базовое правило (обновляется централизованно)
    Base,
    /// LoRA-дельта клиента (обучается локально)
    LoRA(String), // client_id
}

/// LoRA-адаптер клиента
#[derive(Debug, Clone)]
pub struct LoraAdapter {
    pub client_id: String,
    pub rank: usize,
    /// Дополнительные правила (добавленные клиентом)
    pub added_rules: Vec<WeightedRule>,
    /// Подавленные правила (отключённые клиентом, weight → 0)
    pub suppressed_patterns: HashMap<String, f64>, // pattern → override weight
    /// Усиленные правила (увеличенный weight)
    pub boosted_patterns: HashMap<String, f64>,    // pattern → weight multiplier
}

impl LoraAdapter {
    /// Создать пустой адаптер
    pub fn new(client_id: &str, rank: usize) -> Self {
        Self {
            client_id: client_id.to_string(),
            rank,
            added_rules: Vec::new(),
            suppressed_patterns: HashMap::new(),
            boosted_patterns: HashMap::new(),
        }
    }

    /// Добавить новое правило (LoRA extension)
    pub fn add_rule(&mut self, pattern: &str, weight: f64) {
        self.added_rules.push(WeightedRule {
            pattern: pattern.to_string(),
            weight,
            source: RuleSource::LoRA(self.client_id.clone()),
        });
    }

    /// Подавить базовое правило (weight → 0)
    pub fn suppress(&mut self, pattern: &str) {
        self.suppressed_patterns.insert(pattern.to_string(), 0.0);
    }

    /// Усилить базовое правило (multiplier)
    pub fn boost(&mut self, pattern: &str, multiplier: f64) {
        self.boosted_patterns.insert(pattern.to_string(), multiplier);
    }

    /// Число обучаемых параметров (аналог LoRA rank)
    pub fn trainable_params(&self) -> usize {
        self.added_rules.len() + self.suppressed_patterns.len() + self.boosted_patterns.len()
    }
}

/// LoRA-совместимый плагин — frozen base + client adapters
pub struct LoraPlugin {
    pub name: String,
    /// Замороженные базовые правила (общие для всех)
    base_rules: Vec<WeightedRule>,
    /// Клиентские LoRA-адаптеры (по client_id)
    adapters: HashMap<String, LoraAdapter>,
}

impl LoraPlugin {
    /// Создать плагин с базовыми правилами
    pub fn new(name: &str, base_patterns: Vec<(&str, f64)>) -> Self {
        let base_rules = base_patterns.into_iter()
            .map(|(p, w)| WeightedRule {
                pattern: p.to_string(),
                weight: w,
                source: RuleSource::Base,
            })
            .collect();

        Self {
            name: name.to_string(),
            base_rules,
            adapters: HashMap::new(),
        }
    }

    /// Подключить LoRA-адаптер клиента
    pub fn attach_adapter(&mut self, adapter: LoraAdapter) {
        self.adapters.insert(adapter.client_id.clone(), adapter);
    }

    /// Получить эффективные правила для клиента
    /// effective = base ⊕ lora_delta
    pub fn effective_rules(&self, client_id: &str) -> Vec<WeightedRule> {
        let mut rules: Vec<WeightedRule> = Vec::new();

        // 1. Базовые правила (с возможными override)
        let adapter = self.adapters.get(client_id);

        for base in &self.base_rules {
            let mut rule = base.clone();

            if let Some(adapter) = adapter {
                // Подавление?
                if let Some(&override_weight) = adapter.suppressed_patterns.get(&base.pattern) {
                    rule.weight = override_weight;
                }
                // Усиление?
                if let Some(&multiplier) = adapter.boosted_patterns.get(&base.pattern) {
                    rule.weight *= multiplier;
                }
            }

            rules.push(rule);
        }

        // 2. Добавленные LoRA-правила
        if let Some(adapter) = adapter {
            for added in &adapter.added_rules {
                rules.push(added.clone());
            }
        }

        rules
    }

    /// Сканировать строку с учётом LoRA-адаптера
    pub fn scan_line(&self, line: &str, client_id: &str) -> Vec<(String, f64)> {
        self.effective_rules(client_id)
            .into_iter()
            .filter(|r| r.weight > 0.0 && line.contains(&r.pattern))
            .map(|r| (r.pattern, r.weight))
            .collect()
    }

    /// Статистика
    pub fn stats(&self, client_id: &str) -> LoraStats {
        let effective = self.effective_rules(client_id);
        let adapter = self.adapters.get(client_id);

        LoraStats {
            base_rules: self.base_rules.len(),
            total_effective: effective.len(),
            active_rules: effective.iter().filter(|r| r.weight > 0.0).count(),
            suppressed: adapter.map(|a| a.suppressed_patterns.len()).unwrap_or(0),
            boosted: adapter.map(|a| a.boosted_patterns.len()).unwrap_or(0),
            added: adapter.map(|a| a.added_rules.len()).unwrap_or(0),
            trainable_params: adapter.map(|a| a.trainable_params()).unwrap_or(0),
            frozen_params: self.base_rules.len(),
        }
    }
}

/// Статистика LoRA
#[derive(Debug)]
pub struct LoraStats {
    pub base_rules: usize,
    pub total_effective: usize,
    pub active_rules: usize,
    pub suppressed: usize,
    pub boosted: usize,
    pub added: usize,
    pub trainable_params: usize,
    pub frozen_params: usize,
}

impl LoraStats {
    /// Процент параметров, которые обучаются (vs frozen)
    pub fn trainable_ratio(&self) -> f64 {
        let total = self.trainable_params + self.frozen_params;
        if total == 0 { 0.0 } else { self.trainable_params as f64 / total as f64 * 100.0 }
    }
}
