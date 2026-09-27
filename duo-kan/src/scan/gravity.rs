//! Gravity Attention (Einstein Metric / Differential Geometry)
//!
//! Инспирировано `Gravity subsystem` из Rust KAN (`metric.rs`, `schwarzschild.rs`).
//!
//! Этот модуль рассматривает кодовую базу не как абстрактный граф, 
//! а как непрерывное Риманово пространство.
//! 
//! "Масса" (Mass) модуля = его энтропия, цикломатическая сложность или LOC.
//! Чем больше модуль, тем сильнее он искривляет метрику $g_{\mu\nu}$ вокруг себя.
//! Если мелкий скрипт сильно связан ("близко расположен") с God Object, он попадает
//! в гравитационную яму и внутрь "Радиуса Шварцшильда".
//! Внутри этого радиуса риск стремится к бесконечности (горизонт событий техдолга).

use std::collections::HashMap;

/// Физические константы нашей семантической вселенной
const G_CONSTANT: f64 = 6.674e-2; // Гравитационная постоянная (адаптированная)
const C_SQUARED: f64 = 1000.0;     // Скорость рефакторинга в квадрате

#[derive(Debug, Clone)]
pub struct CodeMass {
    pub name: String,
    pub mass: f64,             // Сложность/LOC (Масса)
    pub coordinates: (f64, f64), // Абстрактные координаты в пространстве проекта
    pub inherent_risk: f64,    // Базовый риск самого компонента без учёта соседей
}

pub struct GravitySpacetime {
    objects: HashMap<String, CodeMass>,
}

impl GravitySpacetime {
    pub fn new() -> Self {
        Self {
            objects: HashMap::new(),
        }
    }

    /// Помещаем модуль в семантическое пространство
    pub fn add_mass(&mut self, name: &str, mass: f64, inherent_risk: f64, x: f64, y: f64) {
        self.objects.insert(name.to_string(), CodeMass {
            name: name.to_string(),
            mass,
            inherent_risk,
            coordinates: (x, y),
        });
    }

    /// Вычисляет радиус Шварцшильда (Горизонт Событий) для Объекта
    /// r_s = 2GM / c^2
    pub fn calculate_schwarzschild_radius(&self, name: &str) -> Option<f64> {
        let obj = self.objects.get(name)?;
        let r_s = (2.0 * G_CONSTANT * obj.mass) / C_SQUARED;
        Some(r_s)
    }

    /// Расстояние между двумя объектами в семантическом пространстве
    fn calculate_distance(p1: (f64, f64), p2: (f64, f64)) -> f64 {
        let dx = p1.0 - p2.0;
        let dy = p1.1 - p2.1;
        (dx * dx + dy * dy).sqrt()
    }

    /// Метрика Шварцшильда: вычисляет "Замедление Времени" (Искажение риска) 
    /// под действием гравитационного поля всех соседей.
    /// Чем ближе к горизонту событий тяжелого соседа — тем больше множитель риск-скора!
    pub fn evaluate_warped_risk(&self, target_name: &str) -> Option<(f64, f64)> {
        let target = self.objects.get(target_name)?;
        let mut total_warp_multiplier = 1.0;

        for (other_name, other_obj) in &self.objects {
            if other_name == target_name { continue; }

            let distance = Self::calculate_distance(target.coordinates, other_obj.coordinates);
            let r_s = self.calculate_schwarzschild_radius(other_name).unwrap_or(0.0);

            // Если компонент попал внутрь Радиуса Шварцшильда Чёрной Дыры (God Object)
            if distance <= r_s {
                // Риск стремится к бесконечности. Код невозможно безопасно модифицировать.
                return Some((f64::INFINITY, f64::INFINITY));
            }

            // Искажение метрики (Lorentz-like factor from General Relativity)
            // Искривление = 1 / sqrt(1 - r_s / r)
            let curvature = 1.0 / (1.0 - r_s / distance).sqrt();

            // Если объект близко к горизонту событий, curvature будет огромным
            if curvature > 1.0 {
                // Малые флуктуации не учитываем, берем только значимое замедление
                total_warp_multiplier *= curvature;
            }
        }

        let warped_risk = target.inherent_risk * total_warp_multiplier;
        Some((warped_risk, total_warp_multiplier))
    }
}
