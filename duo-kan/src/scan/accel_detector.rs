//! Acceleration Detector (d²E/dt² Jerk Security)
//!
//! Инспирировано `psi/acceleration_detector.rs` из Rust KAN (369 строк).
//!
//! Классический Temporal KAN (Boiling Frog) отслеживает скорость (velocity)
//! деградации кода. Но скорость — это лишь первая производная.
//!
//! Этот модуль добавляет:
//! - **Ускорение** (Acceleration = d²E/dt²): "Риск растёт ВСЁ БЫСТРЕЕ"
//! - **Рывок** (Jerk = d³E/dt³): "Ускорение УСКОРЯЕТСЯ — неконтролируемый коллапс!"
//!
//! 4 уровня Severity: Stable → Watch → Warning → Critical

use std::collections::VecDeque;
use std::fmt;

/// Уровень угрозы, определяемый производными риска
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Severity {
    Stable,   // Всё спокойно: velocity ≈ 0, acceleration ≈ 0
    Watch,    // Скорость > 0, но ускорение ≈ 0 (линейный рост)
    Warning,  // Ускорение > 0 (параболический рост — экспоненциально опаснее!)
    Critical, // Jerk > 0 (ускорение само ускоряется — неуправляемая спираль)
}

impl fmt::Display for Severity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Severity::Stable   => write!(f, "🟢 Stable"),
            Severity::Watch    => write!(f, "🟡 Watch"),
            Severity::Warning  => write!(f, "🟠 Warning"),
            Severity::Critical => write!(f, "🔴 CRITICAL"),
        }
    }
}

/// Ring Buffer риск-метрик по времени (каждый "тик" — один коммит / PR / scan)
pub struct AccelerationDetector {
    history: VecDeque<f64>, // Последние N замеров Risk Score
    max_window: usize,     // Максимальный размер окна
}

impl AccelerationDetector {
    pub fn new(window: usize) -> Self {
        Self {
            history: VecDeque::with_capacity(window),
            max_window: window,
        }
    }

    /// Записываем новый замер Risk Score (после каждого скана/коммита)
    pub fn push(&mut self, risk_score: f64) {
        if self.history.len() >= self.max_window {
            self.history.pop_front(); // Удаляем самый старый
        }
        self.history.push_back(risk_score);
    }

    /// Velocity (Скорость): dE/dt = E(t) - E(t-1)
    pub fn velocity(&self) -> Option<f64> {
        if self.history.len() < 2 { return None; }
        let n = self.history.len();
        Some(self.history[n - 1] - self.history[n - 2])
    }

    /// Acceleration (Ускорение): d²E/dt² = E(t) - 2·E(t-1) + E(t-2)
    pub fn acceleration(&self) -> Option<f64> {
        if self.history.len() < 3 { return None; }
        let n = self.history.len();
        Some(self.history[n - 1] - 2.0 * self.history[n - 2] + self.history[n - 3])
    }

    /// Jerk (Рывок): d³E/dt³ = E(t) - 3·E(t-1) + 3·E(t-2) - E(t-3)
    pub fn jerk(&self) -> Option<f64> {
        if self.history.len() < 4 { return None; }
        let n = self.history.len();
        Some(
            self.history[n - 1]
            - 3.0 * self.history[n - 2]
            + 3.0 * self.history[n - 3]
            - self.history[n - 4]
        )
    }

    /// Классификация текущей ситуации по 4 уровням Severity
    pub fn detect_severity(&self) -> Severity {
        let vel = self.velocity().unwrap_or(0.0);
        let acc = self.acceleration().unwrap_or(0.0);
        let jrk = self.jerk().unwrap_or(0.0);

        if jrk > 0.5 {
            Severity::Critical  // Рывок положителен — всё летит в пропасть
        } else if acc > 0.3 {
            Severity::Warning   // Ускорение положительно — параболический рост
        } else if vel > 0.1 {
            Severity::Watch     // Скорость положительна — линейный тренд
        } else {
            Severity::Stable    // Тишина
        }
    }
}
