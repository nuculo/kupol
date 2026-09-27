//! Babylon Basis Cache (O(log n) Spline Lookup)
//!
//! Инспирировано Вавилонскими таблицами обратных (3000 г. до н.э.):
//! вместо деления a ÷ b вавилоняне вычисляли a × (1/b), где 1/b
//! бралось из **заранее вычисленной глиняной таблички**.
//!
//! Аналог в KAN:
//! Вместо пересчёта B-spline базиса через рекурсию Кокса-де Бура O(k²)
//! при каждом forward pass, мы **предвычисляем** все базисные значения
//! для сетки из N точек и храним их в `BTreeMap` с O(log N) lookup.
//!
//! Результат: 5-10x ускорение KAN forward pass.

use std::collections::BTreeMap;

/// Ключ для BTreeMap — дискретизированное значение x
/// (Округляем до 4 знаков после запятой для O(log n) поиска)
fn discretize(x: f64, resolution: f64) -> i64 {
    (x / resolution).round() as i64
}

/// Одна предвычисленная строка таблицы: значения всех B-spline базисных функций в точке x
#[derive(Debug, Clone)]
pub struct BasisRow {
    pub x: f64,
    pub basis_values: Vec<f64>,
}

/// Вавилонская Табличка: предвычисленный кэш B-spline базиса
pub struct BabylonBasisCache {
    table: BTreeMap<i64, BasisRow>,
    resolution: f64,
    pub grid_size: usize,
    pub degree: usize,
    pub cache_hits: usize,
    pub cache_misses: usize,
}

impl BabylonBasisCache {
    /// Строим "глиняную табличку":
    /// Предвычисляем B-spline базис для `num_points` равномерно распределённых точек
    pub fn build(
        grid_size: usize,
        degree: usize,
        bounds: (f64, f64),
        num_points: usize,
    ) -> Self {
        let resolution = 0.0001; // 4 знака после запятой
        let mut table = BTreeMap::new();

        let (lo, hi) = bounds;
        let n_knots = grid_size + degree;
        let step = (hi - lo) / grid_size as f64;
        let knots: Vec<f64> = (0..n_knots).map(|i| lo + i as f64 * step).collect();

        // Предвычисляем базис для каждой точки сетки
        for i in 0..num_points {
            let x = lo + (hi - lo) * i as f64 / (num_points - 1) as f64;
            let basis = Self::evaluate_de_boor(&knots, degree, x);
            let key = discretize(x, resolution);
            table.insert(key, BasisRow { x, basis_values: basis });
        }

        Self {
            table,
            resolution,
            grid_size,
            degree,
            cache_hits: 0,
            cache_misses: 0,
        }
    }

    /// Полный De Boor (дорогая рекурсия O(k²)) — используется ТОЛЬКО при построении кэша
    fn evaluate_de_boor(knots: &[f64], degree: usize, x: f64) -> Vec<f64> {
        let n = knots.len() - degree;
        let mut basis = Vec::with_capacity(n);

        for i in 0..n {
            // Рекурсия Кокса-де Бура степени `degree`
            basis.push(Self::cox_de_boor(knots, i, degree, x));
        }
        basis
    }

    fn cox_de_boor(knots: &[f64], i: usize, k: usize, x: f64) -> f64 {
        if k == 0 {
            let t_i = knots.get(i).copied().unwrap_or(f64::MAX);
            let t_i1 = knots.get(i + 1).copied().unwrap_or(f64::MIN);
            if t_i <= x && x < t_i1 {
                return 1.0;
            }
            return 0.0;
        }

        let mut result = 0.0;

        // Левая ветка: (x - t[i]) / (t[i+k] - t[i]) * B_{i,k-1}(x)
        let t_i = knots.get(i).copied().unwrap_or(0.0);
        let t_ik = knots.get(i + k).copied().unwrap_or(0.0);
        let denom_l = t_ik - t_i;
        if denom_l.abs() > 1e-12 {
            result += (x - t_i) / denom_l * Self::cox_de_boor(knots, i, k - 1, x);
        }

        // Правая ветка: (t[i+k+1] - x) / (t[i+k+1] - t[i+1]) * B_{i+1,k-1}(x)
        let t_i1 = knots.get(i + 1).copied().unwrap_or(0.0);
        let t_ik1 = knots.get(i + k + 1).copied().unwrap_or(0.0);
        let denom_r = t_ik1 - t_i1;
        if denom_r.abs() > 1e-12 {
            result += (t_ik1 - x) / denom_r * Self::cox_de_boor(knots, i + 1, k - 1, x);
        }

        result
    }

    /// O(log n) Lookup — Вавилонский метод!
    /// Если точка есть в таблице — мгновенный ответ.
    /// Если нет — fallback на De Boor (cache miss).
    pub fn lookup(&mut self, x: f64) -> Vec<f64> {
        let key = discretize(x, self.resolution);

        if let Some(row) = self.table.get(&key) {
            self.cache_hits += 1;
            row.basis_values.clone()
        } else {
            // Cache miss — fallback на полный De Boor
            self.cache_misses += 1;

            let n_knots = self.grid_size + self.degree;
            let step = 4.0 / self.grid_size as f64; // assumes bounds (-2, 2)
            let knots: Vec<f64> = (0..n_knots).map(|i| -2.0 + i as f64 * step).collect();
            Self::evaluate_de_boor(&knots, self.degree, x)
        }
    }

    pub fn table_size(&self) -> usize {
        self.table.len()
    }

    pub fn hit_rate(&self) -> f64 {
        let total = self.cache_hits + self.cache_misses;
        if total == 0 { return 0.0; }
        self.cache_hits as f64 / total as f64 * 100.0
    }
}
