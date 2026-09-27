//! Training Utilities — Gradient Clipping & Early Stopping
//!
//! Вдохновлён `lib/optim/utils.ml` из OCaml KAN (фаза 39c).
//! Стабилизация DPO-тренировки:
//! - GradientClipper: нормализация величины обновления весов
//! - EarlyStopper: мониторинг loss, автоматическая остановка при стагнации

/// Gradient Clipper — защита от взрывающихся градиентов
///
/// При backpropagation через глубокие KAN-сети (или DPO training на малом
/// датасете), градиенты могут расти экспоненциально. Clipper нормализует
/// вектор градиентов если его L2-норма превышает `max_norm`.
#[derive(Debug, Clone)]
pub struct GradientClipper {
    /// Максимально допустимая L2-норма градиента
    pub max_norm: f64,
    /// Статистика: сколько раз был применён clipping
    pub clip_count: usize,
    /// Статистика: общее число вызовов
    pub total_calls: usize,
}

impl GradientClipper {
    /// Создать clipper с максимальной нормой
    pub fn new(max_norm: f64) -> Self {
        Self {
            max_norm,
            clip_count: 0,
            total_calls: 0,
        }
    }

    /// Применить clipping к вектору градиентов (in-place)
    /// Если ||grad|| > max_norm → grad = grad * (max_norm / ||grad||)
    pub fn clip(&mut self, gradients: &mut [f64]) -> ClipResult {
        self.total_calls += 1;

        let grad_norm: f64 = gradients.iter().map(|g| g * g).sum::<f64>().sqrt();

        if grad_norm > self.max_norm {
            let scale = self.max_norm / grad_norm;
            for g in gradients.iter_mut() {
                *g *= scale;
            }
            self.clip_count += 1;

            ClipResult {
                original_norm: grad_norm,
                clipped_norm: self.max_norm,
                was_clipped: true,
                scale_factor: scale,
            }
        } else {
            ClipResult {
                original_norm: grad_norm,
                clipped_norm: grad_norm,
                was_clipped: false,
                scale_factor: 1.0,
            }
        }
    }

    /// Процент вызовов с clipping
    pub fn clip_ratio(&self) -> f64 {
        if self.total_calls == 0 {
            0.0
        } else {
            self.clip_count as f64 / self.total_calls as f64
        }
    }
}

/// Результат одного clip-шага
#[derive(Debug, Clone)]
pub struct ClipResult {
    pub original_norm: f64,
    pub clipped_norm: f64,
    pub was_clipped: bool,
    pub scale_factor: f64,
}

/// Early Stopper — предотвращение переобучения
///
/// Мониторит валидационный loss. Если loss не улучшается
/// в течение `patience` эпох → сигнализирует остановку.
#[derive(Debug, Clone)]
pub struct EarlyStopper {
    /// Количество эпох без улучшения до остановки
    pub patience: usize,
    /// Минимальная величина улучшения (delta)
    pub min_delta: f64,
    /// Лучший loss за всё время
    best_loss: f64,
    /// Текущий счётчик эпох без улучшения
    epochs_without_improvement: usize,
    /// Эпоха лучшего loss
    best_epoch: usize,
    /// Текущая эпоха
    current_epoch: usize,
    /// История loss
    loss_history: Vec<f64>,
}

impl EarlyStopper {
    /// Создать EarlyStopper
    pub fn new(patience: usize, min_delta: f64) -> Self {
        Self {
            patience,
            min_delta,
            best_loss: f64::INFINITY,
            epochs_without_improvement: 0,
            best_epoch: 0,
            current_epoch: 0,
            loss_history: Vec::new(),
        }
    }

    /// Подать текущий loss. Returns: нужно ли остановить тренировку
    pub fn step(&mut self, loss: f64) -> StopDecision {
        self.current_epoch += 1;
        self.loss_history.push(loss);

        if loss < self.best_loss - self.min_delta {
            // Улучшение!
            self.best_loss = loss;
            self.best_epoch = self.current_epoch;
            self.epochs_without_improvement = 0;

            StopDecision {
                should_stop: false,
                reason: "Improvement detected".to_string(),
                epochs_without_improvement: 0,
                best_loss: self.best_loss,
                best_epoch: self.best_epoch,
            }
        } else {
            // Стагнация
            self.epochs_without_improvement += 1;

            let should_stop = self.epochs_without_improvement >= self.patience;
            let reason = if should_stop {
                format!(
                    "No improvement for {} epochs (patience={}). STOPPING.",
                    self.epochs_without_improvement, self.patience
                )
            } else {
                format!(
                    "No improvement for {}/{} epochs",
                    self.epochs_without_improvement, self.patience
                )
            };

            StopDecision {
                should_stop,
                reason,
                epochs_without_improvement: self.epochs_without_improvement,
                best_loss: self.best_loss,
                best_epoch: self.best_epoch,
            }
        }
    }

    /// Вернуть историю loss
    pub fn history(&self) -> &[f64] {
        &self.loss_history
    }
}

/// Решение EarlyStopper
#[derive(Debug, Clone)]
pub struct StopDecision {
    pub should_stop: bool,
    pub reason: String,
    pub epochs_without_improvement: usize,
    pub best_loss: f64,
    pub best_epoch: usize,
}
