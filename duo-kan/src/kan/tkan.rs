use crate::kan::bspline::BSpline;
use serde::{Deserialize, Serialize};

/// Temporal Kolmogorov-Arnold Network (T-KAN) Layer
/// Создан для перехвата тактики "Boiling Frog" 🐸 (медленной архитектурной деградации).
/// В отличие от статических сканеров, T-KAN помнит предыдущие Merge Requests через RNN-подобное скрытое состояние (`hidden_state`),
/// но вместо умножения матриц использует обучаемые B-сплайны на гранях графа времени.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalKan {
    /// Скрытое состояние: Накопленный Индекс Деградации Архитектуры (0.0 - Идеально, >1.0 - Катастрофа)
    pub hidden_state: f64,
    /// Сплайн влияния: кривая нелинейно оценивает вклад нового MR
    pub update_spline: BSpline,
    /// Сплайн забывания: контролирует, как долго архитектурный долг помнится
    pub forget_spline: BSpline,
    /// Порог выброса критического алерта
    pub critical_threshold: f64,
}

impl TemporalKan {
    /// Инициализация сети T-KAN с порогом для срабатывания сирены
    pub fn new(threshold: f64) -> Self {
        Self {
            hidden_state: 0.0,
            // Сплайн обновления: квадратичная кривая. Риск < 0.2 игнорируется, Риск 0.5 наносит огромный урон (1.2)
            update_spline: BSpline::new(2, vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], vec![0.0, 0.5, 1.5]),
            // Сплайн забывания: удерживает 80% предыдущего архитектурного "долга" на каждый шаг
            forget_spline: BSpline::new(1, vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.8]),
            critical_threshold: threshold,
        }
    }

    /// Пропустить один Merge Request (в виде нормированного CVSS-риска [0.0 - 1.0]) через T-KAN.
    /// Возвращает кортеж: `(Новый индекс деградации, Флаг тревоги Boiling Frog)`.
    pub fn forward_mr(&mut self, current_mr_risk_normalized: f64) -> (f64, bool) {
        // Формула T-KAN ячейки: h_t = ForgetSpline(h_{t-1}) + UpdateSpline(x_t)
        
        let retention = self.forget_spline.eval(self.hidden_state);
        let impact = self.update_spline.eval(current_mr_risk_normalized);
        
        // Аккумулируем деградацию
        self.hidden_state = retention + impact;
        
        // Если индекс пробил критический потолок — хакер варит лягушку!
        let is_boiling_frog = self.hidden_state >= self.critical_threshold;
        
        (self.hidden_state, is_boiling_frog)
    }

    /// Вспомогательная функция для прогона симуляции
    pub fn reset(&mut self) {
        self.hidden_state = 0.0;
    }
}
