//! Neural ODE — Continuous-Time Risk Dynamics
//!
//! Статический CVSS score (например, 9.8) не учитывает фактор времени.
//! Уязвимость без патча в проде через неделю становится опаснее (появляются эксплойты).
//!
//! Мы моделируем Risk Score как непрерывную динамическую систему (ODE):
//! d(State)/dt = f(State, t)
//!
//! Состояние (State) — это вектор [RiskScore, ExploitProbability, PatchAvailability].
//! Мы используем решатель Runge-Kutta 4 (RK4) для симуляции того,
//! как вырастет риск через N дней, если уязвимость не будет исправлена.

/// Вектор состояния уязвимости
#[derive(Debug, Clone, Copy)]
pub struct VulnState {
    /// Общий риск (0.0 — 10.0, CVSS-like)
    pub risk_score: f64,
    /// Вероятность появления/написания эксплойта (0.0 — 1.0)
    pub exploit_prob: f64,
    /// Доступность патча от вендора (0.0 — 1.0)
    pub patch_avail: f64,
}

impl VulnState {
    // Вспомогательные векторные операции для RK4
    fn add(&self, other: &VulnState) -> VulnState {
        VulnState {
            risk_score: self.risk_score + other.risk_score,
            exploit_prob: self.exploit_prob + other.exploit_prob,
            patch_avail: self.patch_avail + other.patch_avail,
        }
    }

    fn mul(&self, scalar: f64) -> VulnState {
        VulnState {
            risk_score: self.risk_score * scalar,
            exploit_prob: self.exploit_prob * scalar,
            patch_avail: self.patch_avail * scalar,
        }
    }
}

/// Тип функции динамики: принимает время t и текущее состояние, возвращает производную d(State)/dt
pub type DynamicsFn = fn(f64, &VulnState) -> VulnState;

/// Решатель Обыкновенных Дифференциальных Уравнений (ODE)
pub struct OdeSolver;

impl OdeSolver {
    /// Один шаг метода Рунге-Кутты 4-го порядка (RK4)
    pub fn rk4_step(f: DynamicsFn, t: f64, y: &VulnState, dt: f64) -> VulnState {
        let k1 = f(t, y);
        let k2 = f(t + dt / 2.0, &y.add(&k1.mul(dt / 2.0)));
        let k3 = f(t + dt / 2.0, &y.add(&k2.mul(dt / 2.0)));
        let k4 = f(t + dt, &y.add(&k3.mul(dt)));

        // y_{n+1} = y_n + dt/6 * (k1 + 2k2 + 2k3 + k4)
        let sum_k = k1
            .add(&k2.mul(2.0))
            .add(&k3.mul(2.0))
            .add(&k4);

        y.add(&sum_k.mul(dt / 6.0))
    }

    /// Симуляция динамики на N дней вперёд
    pub fn simulate(
        f: DynamicsFn,
        mut current_state: VulnState,
        days_to_simulate: usize,
        steps_per_day: usize,
    ) -> Vec<(f64, VulnState)> {
        let mut trajectory = Vec::with_capacity(days_to_simulate + 1);
        trajectory.push((0.0, current_state));

        let dt = 1.0 / steps_per_day as f64;
        let mut t = 0.0;

        for day in 1..=days_to_simulate {
            for _ in 0..steps_per_day {
                current_state = Self::rk4_step(f, t, &current_state, dt);
                
                // Ограничиваем физический смысл переменных
                current_state.risk_score = current_state.risk_score.clamp(0.0, 10.0);
                current_state.exploit_prob = current_state.exploit_prob.clamp(0.0, 1.0);
                current_state.patch_avail = current_state.patch_avail.clamp(0.0, 1.0);
                
                t += dt;
            }
            trajectory.push((day as f64, current_state));
        }

        trajectory
    }
}

/// Библиотека пресетов динамики (Differential equations)
pub struct RiskDynamicsPresets;

impl RiskDynamicsPresets {
    /// Динамика Zero-Day уязвимости (SQL Injection, Log4j)
    /// - Вероятность эксплойта быстро растёт по логистической кривой.
    /// - Риск растёт пропорционально вероятности эксплойта и отсутствию патча.
    /// - Доступность патча растёт медленно (вендор фиксит).
    pub fn zero_day_dynamics(_t: f64, state: &VulnState) -> VulnState {
        // d(Exploit)/dt: логистический рост (вирусное распространение в dark web). rate = 0.15
        let d_exploit = 0.25 * state.exploit_prob * (1.0 - state.exploit_prob);

        // d(Patch)/dt: вендор потеет, патч появляется со скоростью 0.05 в день
        let d_patch = 0.08 * (1.0 - state.patch_avail);

        // d(Risk)/dt: риск растёт от появления эксплойта, но падает если патч применён
        // (предполагаем, что патч снижает exposure)
        let d_risk = 2.0 * d_exploit - 3.0 * state.patch_avail * state.risk_score;

        VulnState {
            risk_score: d_risk,
            exploit_prob: d_exploit,
            patch_avail: d_patch,
        }
    }
}
