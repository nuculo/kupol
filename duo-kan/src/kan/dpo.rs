//! DPO Loss — Direct Preference Optimization for Security Risk Scoring
//!
//! Вдохновлён `lib/optim/dpo_loss.ml` из OCaml KAN.
//! Вместо жёсткого CVSS скоринга, мы обучаем scoring function через
//! предпочтения команды безопасности:
//!   "SQLi в db.rs КРИТИЧНЕЕ, чем TODO в readme.md"
//!
//! Формула DPO Loss:
//!   L = -log(sigmoid(β × (Δ_chosen - Δ_rejected)))
//!
//! Результат: Risk Score адаптируется под организацию.

use std::collections::HashMap;

/// Пара предпочтений: "chosen_plugin более критичен, чем rejected_plugin"
#[derive(Debug, Clone)]
pub struct PreferencePair {
    pub chosen_plugin: String,
    pub rejected_plugin: String,
}

/// Адаптивная модель организационного скоринга
pub struct DpoScorer {
    /// Веса (логиты) для каждого плагина: plugin_name → score_weight
    pub weights: HashMap<String, f64>,
    /// Гиперпараметр DPO (температура): больше β → резче разделение
    pub beta: f64,
    /// Learning rate для обновления весов
    pub lr: f64,
}

impl DpoScorer {
    /// Создать DPO скорер с инициализованными весами (все равны 1.0)
    pub fn new(plugin_names: &[&str], beta: f64, lr: f64) -> Self {
        let mut weights = HashMap::new();
        for &name in plugin_names {
            weights.insert(name.to_string(), 1.0);
        }
        Self { weights, beta, lr }
    }

    /// Получить текущий вес плагина
    pub fn score(&self, plugin_name: &str) -> f64 {
        *self.weights.get(plugin_name).unwrap_or(&1.0)
    }

    /// Sigmoid function: σ(x) = 1 / (1 + e^(-x))
    fn sigmoid(x: f64) -> f64 {
        1.0 / (1.0 + (-x).exp())
    }

    /// Вычислить DPO Loss по одной паре предпочтений
    /// L = -log(σ(β × (w_chosen - w_rejected)))
    pub fn compute_loss(&self, pair: &PreferencePair) -> f64 {
        let w_chosen = self.score(&pair.chosen_plugin);
        let w_rejected = self.score(&pair.rejected_plugin);

        let margin = self.beta * (w_chosen - w_rejected);
        let sigma = Self::sigmoid(margin);

        // L = -log(σ(margin))
        -sigma.ln()
    }

    /// Обучение: один шаг SGD по паре предпочтений
    /// Gradient: dL/dw_chosen = -β × (1 - σ(margin))
    ///           dL/dw_rejected = β × (1 - σ(margin))
    pub fn train_step(&mut self, pair: &PreferencePair) -> f64 {
        let w_chosen = self.score(&pair.chosen_plugin);
        let w_rejected = self.score(&pair.rejected_plugin);

        let margin = self.beta * (w_chosen - w_rejected);
        let sigma = Self::sigmoid(margin);
        let loss = -sigma.ln();

        // Gradient descent
        let grad = 1.0 - sigma;
        let delta_chosen = self.lr * self.beta * grad;
        let delta_rejected = -self.lr * self.beta * grad;

        // Update weights
        if let Some(w) = self.weights.get_mut(&pair.chosen_plugin) {
            *w += delta_chosen;
        }
        if let Some(w) = self.weights.get_mut(&pair.rejected_plugin) {
            *w += delta_rejected;
        }

        loss
    }

    /// Обучение на батче из пар предпочтений (несколько эпох)
    pub fn train(&mut self, pairs: &[PreferencePair], epochs: usize) -> Vec<f64> {
        let mut losses = Vec::new();
        for _epoch in 0..epochs {
            let mut epoch_loss = 0.0;
            for pair in pairs {
                epoch_loss += self.train_step(pair);
            }
            losses.push(epoch_loss / pairs.len() as f64);
        }
        losses
    }

    /// Вернуть ранжированный список плагинов (от самого критичного к наименее критичному)
    pub fn ranked_plugins(&self) -> Vec<(String, f64)> {
        let mut ranked: Vec<(String, f64)> = self.weights.iter()
            .map(|(k, v)| (k.clone(), *v))
            .collect();
        ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        ranked
    }
}
