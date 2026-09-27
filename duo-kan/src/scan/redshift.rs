//! Gravitational Redshift (Семантический Сдвиг Частоты)
//!
//! Инспирировано `gravity/redshift.rs` из Rust KAN.
//!
//! В Общей Теории Относительности, свет, покидающий массивный объект,
//! теряет энергию (частоту) — это **красное смещение**:
//!
//!   z = sqrt(g₀₀(emitter) / g₀₀(receiver)) - 1
//!
//! В контексте кода:
//! - God Object = "массивный объект" (источник API, типов, документации)
//! - Модули-потребители находятся на разном "расстоянии" в графе зависимостей
//! - Чем дальше модуль от источника истины → тем выше z (redshift)
//! - Высокий z = вероятность Deprecated API, Misunderstanding, Drift





/// Один модуль в графе зависимостей
#[derive(Debug, Clone)]
pub struct CodeModule {
    pub name: String,
    pub mass: f64,          // "Гравитационная масса" (LOC × coupling_factor)
    pub distance: f64,      // Расстояние от God Object (глубина в графе)
    pub redshift_z: f64,    // Красное смещение z
    pub staleness_risk: &'static str, // Уровень устаревания API
}

/// Гравитационное поле кодовой базы
pub struct RedshiftField {
    pub god_object_mass: f64,     // Масса God Object (источника API)
    pub modules: Vec<CodeModule>,
}

impl RedshiftField {
    pub fn new(god_object_mass: f64) -> Self {
        Self {
            god_object_mass,
            modules: Vec::new(),
        }
    }

    /// Добавить модуль-потребитель на заданном расстоянии от God Object
    /// distance измеряется в единицах Schwarzschild-радиуса (2M):
    ///   distance=1.2 = прямо на "поверхности" God Object
    ///   distance=10  = далеко в графе
    pub fn add_consumer(&mut self, name: &str, module_mass: f64, distance_factor: f64) {
        let rs = 2.0 * self.god_object_mass; // Schwarzschild radius
        let r_emitter = rs * 1.01; // Emitter = на самой поверхности God Object
        let r_receiver = rs * distance_factor; // Receiver = на расстоянии

        // g₀₀ = 1 - rs/r  (simplified Schwarzschild)
        let g00_emitter = (1.0 - rs / r_emitter).max(0.001);
        let g00_receiver = (1.0 - rs / r_receiver).max(0.001);

        // z = sqrt(g₀₀_emitter / g₀₀_receiver) - 1
        // Если receiver дальше от массы → g₀₀_receiver ≈ 1 → z > 0
        // Свет из глубокой гравитационной ямы приходит красносмещённым
        let z = if g00_receiver > g00_emitter {
            (g00_receiver / g00_emitter).sqrt() - 1.0
        } else {
            0.0
        };

        let staleness = match z {
            z if z < 3.5  => "🟢 Fresh (API актуален)",
            z if z < 5.0  => "🟡 Drifting (возможен minor drift)",
            z if z < 7.0  => "🟠 Stale (вероятен deprecated API)",
            _             => "🔴 Fossilized (API скорее всего сломан!)",
        };

        self.modules.push(CodeModule {
            name: name.to_string(),
            mass: module_mass,
            distance: distance_factor,
            redshift_z: z,
            staleness_risk: staleness,
        });
    }

    /// Отсортировать модули по redshift (от наибольшего к наименьшему)
    pub fn ranked_by_redshift(&self) -> Vec<&CodeModule> {
        let mut sorted: Vec<&CodeModule> = self.modules.iter().collect();
        sorted.sort_by(|a, b| b.redshift_z.partial_cmp(&a.redshift_z).unwrap());
        sorted
    }

    /// Средний redshift по всей кодовой базе
    pub fn mean_redshift(&self) -> f64 {
        if self.modules.is_empty() { return 0.0; }
        let sum: f64 = self.modules.iter().map(|m| m.redshift_z).sum();
        sum / self.modules.len() as f64
    }
}
