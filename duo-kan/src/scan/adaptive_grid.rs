//! Adaptive Grid Extension (Plateau Detect → Refine)
//!
//! Инспирировано `kan/adaptive.rs` (337 строк) из Rust KAN и Вавилонскими
//! делителями числа 60 как Grid Schedule: `[3, 5, 12, 20, 60]`.
//!
//! Сканер начинает с грубой сетки (gridSize=3) — быстро, но неточно.
//! PlateauDetector отслеживает loss/accuracy: если улучшение < threshold
//! за `window` эпох, gridSize автоматически расширяется.
//! Коэффициенты B-spline перепроецируются через Greville abscissa.
//!
//! Литературное название: **Coarse-to-Fine Scanning**.

use std::collections::VecDeque;

/// Вавилонские делители 60 как оптимальные размеры сетки
pub const BABYLON_GRID_SCHEDULE: &[usize] = &[3, 5, 12, 20, 60];

/// Детектор плато: отслеживает, остановилось ли улучшение метрики
pub struct PlateauDetector {
    history: VecDeque<f64>,
    pub window: usize,       // Окно наблюдения (сколько эпох смотреть назад)
    pub threshold: f64,      // Минимальное улучшение, ниже которого = плато
}

impl PlateauDetector {
    pub fn new(window: usize, threshold: f64) -> Self {
        Self {
            history: VecDeque::with_capacity(window + 1),
            window,
            threshold,
        }
    }

    /// Записать новое значение метрики (loss / accuracy / recall)
    pub fn record(&mut self, metric: f64) {
        if self.history.len() >= self.window + 1 {
            self.history.pop_front();
        }
        self.history.push_back(metric);
    }

    /// Определить, наступило ли плато
    /// Плато = разница между лучшим и худшим значением за window < threshold
    pub fn is_plateau(&self) -> bool {
        if self.history.len() < self.window {
            return false; // Недостаточно данных
        }

        let recent: Vec<f64> = self.history.iter()
            .rev()
            .take(self.window)
            .copied()
            .collect();

        let best = recent.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
        let worst = recent.iter().cloned().fold(f64::INFINITY, f64::min);

        (best - worst).abs() < self.threshold
    }
}

/// Адаптивный KAN Grid — автоматически расширяется при плато
pub struct AdaptiveKanGrid {
    pub current_grid_size: usize,
    schedule_index: usize,
    pub schedule: Vec<usize>,
    pub coefficients: Vec<f64>,  // Текущие B-spline коэффициенты
    pub extensions_count: usize, // Сколько раз расширяли
}

impl AdaptiveKanGrid {
    pub fn new(initial_coefficients: Vec<f64>) -> Self {
        Self {
            current_grid_size: BABYLON_GRID_SCHEDULE[0],
            schedule_index: 0,
            schedule: BABYLON_GRID_SCHEDULE.to_vec(),
            coefficients: initial_coefficients,
            extensions_count: 0,
        }
    }

    /// Попытка расширить Grid на следующий уровень Вавилонского расписания
    /// Возвращает true если расширение произошло
    pub fn try_extend(&mut self) -> bool {
        if self.schedule_index + 1 >= self.schedule.len() {
            return false; // Максимальный grid уже достигнут
        }

        let old_size = self.current_grid_size;
        self.schedule_index += 1;
        let new_size = self.schedule[self.schedule_index];

        // Перепроецирование коэффициентов через Greville abscissa
        self.coefficients = Self::greville_reproject(&self.coefficients, old_size, new_size);
        self.current_grid_size = new_size;
        self.extensions_count += 1;
        true
    }

    /// Greville Abscissa Reprojection:
    /// Интерполируем старые коэффициенты на новую, более мелкую сетку.
    /// Greville abscissa: ξ_i = (t_{i+1} + ... + t_{i+k}) / k
    fn greville_reproject(old_coeffs: &[f64], _old_size: usize, new_size: usize) -> Vec<f64> {
        let old_len = old_coeffs.len();
        let mut new_coeffs = Vec::with_capacity(new_size);

        for j in 0..new_size {
            // Позиция нового узла в нормализованном пространстве [0, 1]
            let t = j as f64 / (new_size - 1).max(1) as f64;

            // Находим позицию в старой сетке
            let pos_in_old = t * (old_len - 1).max(1) as f64;
            let idx = (pos_in_old.floor() as usize).min(old_len.saturating_sub(2));
            let frac = pos_in_old - idx as f64;

            // Линейная интерполяция между соседними старыми коэффициентами
            let c0 = old_coeffs.get(idx).copied().unwrap_or(0.0);
            let c1 = old_coeffs.get(idx + 1).copied().unwrap_or(c0);
            new_coeffs.push(c0 * (1.0 - frac) + c1 * frac);
        }

        new_coeffs
    }

    pub fn max_grid_reached(&self) -> bool {
        self.schedule_index + 1 >= self.schedule.len()
    }
}
