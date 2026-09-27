//! Forward AD Sensitivity Analysis (Dual Numbers)
//!
//! Инспирировано `AD.hs` из Haskell KAN. Реализует Дуальные числа для вычисления
//! точных производных функций безопасности (`∂CVSS / ∂threshold`) "на лету",
//! без Wengert tape (графа вычислений) или численного дифференцирования.
//!
//! Dual { v: Значение (a), d: Градиент (bε) }
//!
//! Используется для Автоматического Тюнинга правил. Если мы изменим параметр правила на +1,
//! как сильно изменится базовый CVSS-рейтинг? Dual-алгебра говорит это за 1 прямой проход.

use std::ops::{Add, Sub, Mul, Div, AddAssign};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Dual {
    pub v: f64, // Реальное значение
    pub d: f64, // Производная по целевому параметру
}

impl Dual {
    pub fn new(v: f64, d: f64) -> Self {
        Self { v, d }
    }

    /// Превращает число в константу (Градиент = 0)
    pub fn constant(v: f64) -> Self {
        Self { v, d: 0.0 }
    }

    /// Превращает переменную в целевую (Градиент = 1.0)
    /// Мы считаем производную `∂F / ∂self`
    pub fn var(v: f64) -> Self {
        Self { v, d: 1.0 }
    }

    /// SiLU (Sigmoid Linear Unit) для сплайнов
    /// SiLU(x) = x / (1 + exp(-x))
    /// d/dx SiLU = SiLU(x) + sigmoid(x) * (1 - SiLU(x))
    pub fn silu(&self) -> Self {
        let sig = 1.0 / (1.0 + (-self.v).exp());
        let silu_v = self.v * sig;
        let silu_d = self.d * (silu_v + sig * (1.0 - silu_v));
        Self::new(silu_v, silu_d)
    }

    /// Max функция: ReLU-like пороги
    pub fn relu(&self) -> Self {
        if self.v > 0.0 {
            Self::new(self.v, self.d)
        } else {
            Self::new(0.0, 0.0)
        }
    }

    /// Квадрат числа
    pub fn pow2(&self) -> Self {
        Self::new(self.v * self.v, 2.0 * self.v * self.d)
    }
}

// -------------------------------------------------------------
// Перегрузка операторов арифметики
// -------------------------------------------------------------

impl Add for Dual {
    type Output = Dual;
    fn add(self, rhs: Self) -> Self::Output {
        Dual::new(self.v + rhs.v, self.d + rhs.d)
    }
}

impl Add<f64> for Dual {
    type Output = Dual;
    fn add(self, rhs: f64) -> Self::Output {
        Dual::new(self.v + rhs, self.d)
    }
}

impl Sub for Dual {
    type Output = Dual;
    fn sub(self, rhs: Self) -> Self::Output {
        Dual::new(self.v - rhs.v, self.d - rhs.d)
    }
}

impl Sub<f64> for Dual {
    type Output = Dual;
    fn sub(self, rhs: f64) -> Self::Output {
        Dual::new(self.v - rhs, self.d)
    }
}

impl Mul for Dual {
    type Output = Dual;
    fn mul(self, rhs: Self) -> Self::Output {
        // Правило Лейбница: (fg)' = f'g + fg'
        Dual::new(self.v * rhs.v, self.d * rhs.v + self.v * rhs.d)
    }
}

impl Mul<f64> for Dual {
    type Output = Dual;
    fn mul(self, rhs: f64) -> Self::Output {
        Dual::new(self.v * rhs, self.d * rhs)
    }
}

impl Div for Dual {
    type Output = Dual;
    fn div(self, rhs: Self) -> Self::Output {
        // Правило частного: (f/g)' = (f'g - fg') / g^2
        let den = rhs.v * rhs.v;
        Dual::new(self.v / rhs.v, (self.d * rhs.v - self.v * rhs.d) / den)
    }
}

impl Div<f64> for Dual {
    type Output = Dual;
    fn div(self, rhs: f64) -> Self::Output {
        Dual::new(self.v / rhs, self.d / rhs)
    }
}

impl AddAssign for Dual {
    fn add_assign(&mut self, rhs: Self) {
        self.v += rhs.v;
        self.d += rhs.d;
    }
}

// -------------------------------------------------------------
// Пример CVSS Scoring Engine, использующий Дуальные числа
// -------------------------------------------------------------

pub struct SecurityRuleEngine;

impl SecurityRuleEngine {
    /// Вычисляет итоговый Security Score функции.
    /// `nesting_depth` (Вложенность): Константа для данного куска кода (e.g., 5 циклов).
    /// `threshold`: Конфигурируемый порог в правине (относительно которого мы берём градиент).
    /// Возвращает Dual, где `v` — это сам Score, а `d` — чувствительность (∂Score / ∂threshold).
    pub fn evaluate_cvss_sensitivity(nesting_depth: f64, threshold: Dual) -> Dual {
        // Сложная, нелинейная эвристика CVSS (аналог KAN scoring_function)
        // Score = 2.0 * SiLU( depth - threshold ) + (depth / threshold)^2

        let depth_dual = Dual::constant(nesting_depth);
        let diff = depth_dual - threshold;
        
        let term1 = diff.silu() * 2.0;
        let term2 = (depth_dual / threshold).pow2();
        
        term1 + term2
    }
}
