//! Agent KAN — ε-Greedy Self-Evolving Security Plugins
//!
//! Вдохновлён `agent_kan.clj` из Clojure KAN.
//! Каждое ребро KAN действует как автономный агент с памятью и стратегией.
//!
//! В Duo Agents каждый OWASP плагин становится агентом:
//! - Хранит память: какие паттерны → True Positive, какие → False Positive
//! - С вероятностью ε мутирует свой набор правил (добавляет/удаляет regex)
//! - Успешные мутации закрепляются, неуспешные — откатываются
//! - Результат: самоэволюционирующая система обнаружения

use rand::Rng;

/// Запись в памяти агента
#[derive(Debug, Clone)]
pub struct MemoryEntry {
    pub pattern: String,
    pub true_positives: usize,
    pub false_positives: usize,
}

impl MemoryEntry {
    /// Fitness: TP / (TP + FP) — precision этого правила
    pub fn fitness(&self) -> f64 {
        let total = self.true_positives + self.false_positives;
        if total == 0 { return 0.5; } // no data = neutral
        self.true_positives as f64 / total as f64
    }
}

/// Тип мутации
#[derive(Debug, Clone)]
pub enum Mutation {
    /// Добавление нового правила
    AddRule(String),
    /// Удаление слабого правила
    RemoveRule(String),
    /// Модификация существующего правила
    ModifyRule { old: String, new: String },
}

/// Агент-плагин с памятью и стратегией ε-greedy
pub struct AgentPlugin {
    pub name: String,
    /// Текущий набор правил (паттернов)
    pub rules: Vec<String>,
    /// Память: история каждого паттерна
    pub memory: Vec<MemoryEntry>,
    /// ε — вероятность мутации (exploration rate)
    pub epsilon: f64,
    /// История мутаций (для возможного отката)
    pub mutation_log: Vec<(Mutation, bool)>,  // (mutation, was_successful)
    /// Поколение (generation count)
    pub generation: usize,
}

impl AgentPlugin {
    /// Создать нового агента-плагина
    pub fn new(name: &str, initial_rules: Vec<String>, epsilon: f64) -> Self {
        let memory: Vec<MemoryEntry> = initial_rules
            .iter()
            .map(|r| MemoryEntry {
                pattern: r.clone(),
                true_positives: 0,
                false_positives: 0,
            })
            .collect();

        Self {
            name: name.to_string(),
            rules: initial_rules,
            memory,
            epsilon,
            mutation_log: Vec::new(),
            generation: 0,
        }
    }

    /// Сканировать строку — есть ли совпадение с любым правилом?
    pub fn scan_line(&self, line: &str) -> Vec<String> {
        self.rules
            .iter()
            .filter(|rule| line.contains(rule.as_str()))
            .cloned()
            .collect()
    }

    /// Зарегистрировать feedback (True/False positive) для паттерна
    pub fn feedback(&mut self, pattern: &str, is_true_positive: bool) {
        if let Some(entry) = self.memory.iter_mut().find(|m| m.pattern == pattern) {
            if is_true_positive {
                entry.true_positives += 1;
            } else {
                entry.false_positives += 1;
            }
        }
    }

    /// ε-Greedy шаг: мутировать правила или эксплуатировать текущие
    pub fn evolve_step(&mut self, candidate_rules: &[&str]) -> Option<Mutation> {
        let mut rng = rand::rng();

        // С вероятностью ε — explore (мутация)
        if rng.random::<f64>() < self.epsilon {
            let mutation = self.generate_mutation(candidate_rules);
            if let Some(ref m) = mutation {
                self.apply_mutation(m);
            }
            self.generation += 1;
            mutation
        } else {
            // Exploit: прунить слабые правила (fitness < 0.3)
            let weak: Vec<String> = self.memory
                .iter()
                .filter(|m| m.fitness() < 0.3 && (m.true_positives + m.false_positives) >= 5)
                .map(|m| m.pattern.clone())
                .collect();

            if let Some(bad_rule) = weak.first() {
                let mutation = Mutation::RemoveRule(bad_rule.clone());
                self.apply_mutation(&mutation);
                self.generation += 1;
                Some(mutation)
            } else {
                None
            }
        }
    }

    /// Генерировать случайную мутацию
    fn generate_mutation(&self, candidate_rules: &[&str]) -> Option<Mutation> {
        let mut rng = rand::rng();

        if candidate_rules.is_empty() && self.rules.is_empty() {
            return None;
        }

        let action: u8 = rng.random_range(0..3);

        match action {
            0 if !candidate_rules.is_empty() => {
                // ADD: добавить случайное правило из пула кандидатов
                let idx = rng.random_range(0..candidate_rules.len());
                let new_rule = candidate_rules[idx].to_string();
                if !self.rules.contains(&new_rule) {
                    Some(Mutation::AddRule(new_rule))
                } else {
                    None
                }
            }
            1 if !self.rules.is_empty() => {
                // REMOVE: удалить случайное правило
                let idx = rng.random_range(0..self.rules.len());
                Some(Mutation::RemoveRule(self.rules[idx].clone()))
            }
            2 if !self.rules.is_empty() && !candidate_rules.is_empty() => {
                // MODIFY: заменить одно правило на другое
                let old_idx = rng.random_range(0..self.rules.len());
                let new_idx = rng.random_range(0..candidate_rules.len());
                let old = self.rules[old_idx].clone();
                let new = candidate_rules[new_idx].to_string();
                if old != new {
                    Some(Mutation::ModifyRule { old, new })
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Применить мутацию к набору правил
    fn apply_mutation(&mut self, mutation: &Mutation) {
        match mutation {
            Mutation::AddRule(rule) => {
                self.rules.push(rule.clone());
                self.memory.push(MemoryEntry {
                    pattern: rule.clone(),
                    true_positives: 0,
                    false_positives: 0,
                });
            }
            Mutation::RemoveRule(rule) => {
                self.rules.retain(|r| r != rule);
                self.memory.retain(|m| m.pattern != *rule);
            }
            Mutation::ModifyRule { old, new } => {
                if let Some(pos) = self.rules.iter().position(|r| r == old) {
                    self.rules[pos] = new.clone();
                }
                if let Some(entry) = self.memory.iter_mut().find(|m| m.pattern == *old) {
                    entry.pattern = new.clone();
                    entry.true_positives = 0;
                    entry.false_positives = 0;
                }
            }
        }
        self.mutation_log.push((mutation.clone(), false)); // success TBD
    }

    /// Отметить последнюю мутацию как успешную
    pub fn confirm_last_mutation(&mut self) {
        if let Some(last) = self.mutation_log.last_mut() {
            last.1 = true;
        }
    }

    /// Откатить последнюю неуспешную мутацию
    pub fn rollback_last_mutation(&mut self) {
        if let Some((mutation, _)) = self.mutation_log.pop() {
            // Обратная мутация
            match mutation {
                Mutation::AddRule(rule) => {
                    self.rules.retain(|r| *r != rule);
                    self.memory.retain(|m| m.pattern != rule);
                }
                Mutation::RemoveRule(rule) => {
                    self.rules.push(rule.clone());
                    self.memory.push(MemoryEntry {
                        pattern: rule,
                        true_positives: 0,
                        false_positives: 0,
                    });
                }
                Mutation::ModifyRule { old, new } => {
                    if let Some(pos) = self.rules.iter().position(|r| *r == new) {
                        self.rules[pos] = old.clone();
                    }
                    if let Some(entry) = self.memory.iter_mut().find(|m| m.pattern == new) {
                        entry.pattern = old;
                        entry.true_positives = 0;
                        entry.false_positives = 0;
                    }
                }
            }
        }
    }

    /// Статистика агента
    pub fn stats(&self) -> AgentStats {
        let total_tp: usize = self.memory.iter().map(|m| m.true_positives).sum();
        let total_fp: usize = self.memory.iter().map(|m| m.false_positives).sum();
        let avg_fitness = if self.memory.is_empty() {
            0.0
        } else {
            self.memory.iter().map(|m| m.fitness()).sum::<f64>() / self.memory.len() as f64
        };
        let successful_mutations = self.mutation_log.iter().filter(|(_, s)| *s).count();

        AgentStats {
            name: self.name.clone(),
            generation: self.generation,
            rule_count: self.rules.len(),
            total_tp,
            total_fp,
            avg_fitness,
            successful_mutations,
            total_mutations: self.mutation_log.len(),
        }
    }
}

/// Статистика агента-плагина
#[derive(Debug)]
pub struct AgentStats {
    pub name: String,
    pub generation: usize,
    pub rule_count: usize,
    pub total_tp: usize,
    pub total_fp: usize,
    pub avg_fitness: f64,
    pub successful_mutations: usize,
    pub total_mutations: usize,
}
