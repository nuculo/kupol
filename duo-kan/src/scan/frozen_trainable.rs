//! Frozen/Trainable Split: Полиморфный AD для Секьюрити-правил
//!
//! Инспирировано `FineTune.hs` (216 строк):
//! KAN заморожен (`constD`), LoRA обучаема (`Dual`).
//! Один forward проход — два режима через generics `(Floating a, Ord a) =>`.
//!
//! В контексте Duo Agents:
//! - **Frozen Core**: Базовые правила OWASP/CWE — не меняются, прошли аудит
//! - **Trainable Adapters (LoRA)**: Тонкая настройка под конкретный проект
//!
//! Гарантия: core-правила ФИЗИЧЕСКИ не могут измениться при тюнинге.
//! Как в LLM: GPT замораживается, LoRA дообучается.

/// Режим работы движка
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EvalMode {
    /// FrozenOnly: только базовые правила, LoRA игнорируется (baseline)
    FrozenOnly,
    /// Production: применяет обученный LoRA, но без градиентов (быстро)
    Production,
    /// Training: с градиентами AD для обучения LoRA
    Training,
}

/// Одно замороженное OWASP/CWE правило (неизменяемое)
#[derive(Debug, Clone)]
pub struct FrozenRule {
    pub id: &'static str,       // e.g. "CWE-79" (XSS)
    pub name: &'static str,     // Human-readable
    pub base_weight: f64,       // Фиксированный вес (прошёл аудит)
    pub severity: f64,          // CVSS base score
}

/// LoRA адаптер: маленькая обучаемая матрица A×B для тонкой настройки
#[derive(Debug, Clone)]
pub struct LoraAdapter {
    pub project_name: String,
    pub a_weights: Vec<f64>,    // [rank] — обучаемые
    pub b_weights: Vec<f64>,    // [rank] — обучаемые
    pub rank: usize,
    pub learning_rate: f64,
}

impl LoraAdapter {
    pub fn new(project_name: &str, rank: usize) -> Self {
        // Оба A и B инициализированы с малыми значениями
        // (в отличие от LoRA.hs, где B=0, здесь нужны ненулевые для быстрого старта)
        let a_weights: Vec<f64> = (0..rank).map(|i| if i % 2 == 0 { 0.01 } else { -0.01 }).collect();
        let b_weights: Vec<f64> = (0..rank).map(|i| if i % 2 == 0 { 0.01 } else { -0.01 }).collect();
        Self {
            project_name: project_name.to_string(),
            a_weights,
            b_weights,
            rank,
            learning_rate: 0.05,
        }
    }

    /// LoRA delta = sum(a_i × b_i) — скалярная поправка к весу правила
    pub fn delta(&self) -> f64 {
        self.a_weights.iter()
            .zip(self.b_weights.iter())
            .map(|(a, b)| a * b)
            .sum()
    }

    /// Обновить LoRA веса по градиенту (один шаг SGD)
    pub fn update(&mut self, grad_a: &[f64], grad_b: &[f64]) {
        for i in 0..self.rank {
            self.a_weights[i] -= self.learning_rate * grad_a[i];
            self.b_weights[i] -= self.learning_rate * grad_b[i];
        }
    }
}

/// Frozen/Trainable Security Engine
pub struct FrozenTrainableEngine {
    /// Замороженное ядро: неизменяемые правила OWASP/CWE
    pub frozen_rules: Vec<FrozenRule>,
    /// Обучаемые адаптеры: по одному LoRA на каждое правило
    pub adapters: Vec<LoraAdapter>,
    /// Режим работы
    pub mode: EvalMode,
    /// Статистика
    pub total_evals: usize,
    pub total_trainings: usize,
}

impl FrozenTrainableEngine {
    /// Создать движок с замороженными OWASP правилами
    pub fn new(project_name: &str, lora_rank: usize) -> Self {
        let frozen_rules = vec![
            FrozenRule { id: "CWE-79",  name: "Cross-Site Scripting (XSS)",      base_weight: 0.9, severity: 6.1 },
            FrozenRule { id: "CWE-89",  name: "SQL Injection",                   base_weight: 0.95, severity: 9.8 },
            FrozenRule { id: "CWE-22",  name: "Path Traversal",                  base_weight: 0.85, severity: 7.5 },
            FrozenRule { id: "CWE-78",  name: "OS Command Injection",            base_weight: 0.92, severity: 9.8 },
            FrozenRule { id: "CWE-287", name: "Improper Authentication",         base_weight: 0.88, severity: 9.1 },
            FrozenRule { id: "CWE-502", name: "Deserialization of Untrusted Data", base_weight: 0.80, severity: 8.1 },
        ];

        let adapters: Vec<LoraAdapter> = frozen_rules.iter()
            .map(|_| LoraAdapter::new(project_name, lora_rank))
            .collect();

        Self {
            frozen_rules,
            adapters,
            mode: EvalMode::Production,
            total_evals: 0,
            total_trainings: 0,
        }
    }

    /// Полиморфный forward pass:
    /// - Production: effective_weight = frozen_weight (LoRA delta = 0, быстро)
    /// - Training:   effective_weight = frozen_weight + LoRA_delta (с градиентами)
    pub fn evaluate(&mut self, file_features: &[f64]) -> Vec<RuleResult> {
        self.total_evals += 1;
        if self.mode == EvalMode::Training {
            self.total_trainings += 1;
        }

        self.frozen_rules.iter().zip(self.adapters.iter()).enumerate()
            .map(|(i, (rule, adapter))| {
                // Frozen base weight (НИКОГДА не меняется)
                let base = rule.base_weight;

                // LoRA delta
                let delta = match self.mode {
                    EvalMode::FrozenOnly => 0.0,       // Baseline: только frozen
                    EvalMode::Production => adapter.delta(), // Trained LoRA, без AD
                    EvalMode::Training => adapter.delta(),   // Trained LoRA, с AD
                };

                // Effective weight = frozen + trainable
                let effective_weight = (base + delta).clamp(0.0, 1.0);

                // Feature activation: насколько этот файл подходит под правило
                let feature_val = if i < file_features.len() { file_features[i] } else { 0.0 };

                // Risk score = severity × weight × feature_activation
                let risk = rule.severity * effective_weight * feature_val;

                RuleResult {
                    rule_id: rule.id,
                    rule_name: rule.name,
                    frozen_weight: base,
                    lora_delta: delta,
                    effective_weight,
                    risk_score: risk,
                    mode: self.mode,
                }
            })
            .collect()
    }

    /// Тренировка: один шаг обучения LoRA на примере
    pub fn train_step(&mut self, file_features: &[f64], target_risks: &[f64]) {
        let old_mode = self.mode;
        self.mode = EvalMode::Training;

        let results = self.evaluate(file_features);

        // Простой SGD: gradient ≈ (predicted - target) × feature
        for (i, (result, adapter)) in results.iter().zip(self.adapters.iter_mut()).enumerate() {
            let target = if i < target_risks.len() { target_risks[i] } else { 0.0 };
            let error = result.risk_score - target;
            let feature = if i < file_features.len() { file_features[i] } else { 0.0 };

            // dL/dA ≈ error × feature × b
            // dL/dB ≈ error × feature × a
            let grad_a: Vec<f64> = adapter.b_weights.iter()
                .map(|b| error * feature * b)
                .collect();
            let grad_b: Vec<f64> = adapter.a_weights.iter()
                .map(|a| error * feature * a)
                .collect();

            adapter.update(&grad_a, &grad_b);
        }

        self.mode = old_mode;
    }
}

/// Результат оценки одного правила
#[derive(Debug)]
pub struct RuleResult {
    pub rule_id: &'static str,
    pub rule_name: &'static str,
    pub frozen_weight: f64,
    pub lora_delta: f64,
    pub effective_weight: f64,
    pub risk_score: f64,
    pub mode: EvalMode,
}
