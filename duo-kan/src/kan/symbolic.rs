//! Hybrid Symbolic Discovery — Автоматическое Создание Правил Безопасности
//!
//! Вдохновлён `hybrid_kan.clj` + `kan_symbolic.clj` из Clojure KAN.
//! В KAN-сетях система пробует 11+ математических функций и, если R² > 0.95,
//! замораживает числовой сплайн в точную символьную формулу (FrozenPhi).
//!
//! Мы применяем аналогичный подход к правилам безопасности:
//! - Собираем корпус найденных уязвимостей и их контекст
//! - Discoverer извлекает общие подстроки/паттерны
//! - Если точность (precision × recall)^0.5 > threshold, правило замораживается
//!   как `FrozenRule` и автоматически добавляется в набор плагинов.

/// Наблюдение: одна найденная уязвимость с контекстом
#[derive(Debug, Clone)]
pub struct Observation {
    pub plugin: String,
    pub matched_line: String,
    pub context_before: Vec<String>,  // 3 строки до
    pub context_after: Vec<String>,   // 3 строки после
    pub file_path: String,
}

/// Замороженное правило — автоматически выведенное из корпуса наблюдений
#[derive(Debug, Clone)]
pub struct FrozenRule {
    pub name: String,
    pub pattern: String,        // Обобщённый паттерн (substring / keyword)
    pub source_plugin: String,  // Из какого плагина пришли наблюдения
    pub confidence: f64,        // R² (precision × recall)
    pub sample_count: usize,    // На скольких наблюдениях обучено
}

/// Кандидатный паттерн для проверки
#[derive(Debug, Clone)]
struct CandidatePattern {
    pattern: String,
    hits: usize,        // Сколько наблюдений матчит
    false_positives: usize,
}

/// Symbolic Discoverer Engine
pub struct SymbolicDiscoverer {
    /// Минимальный порог R² для заморозки правила
    pub freeze_threshold: f64,
    /// Минимальное число наблюдений для генерализации
    pub min_observations: usize,
}

impl SymbolicDiscoverer {
    pub fn new() -> Self {
        Self {
            freeze_threshold: 0.7,
            min_observations: 2,
        }
    }

    /// Извлечь общие подстроки (N-граммы) из набора совпавших строк
    fn extract_common_fragments(observations: &[Observation]) -> Vec<String> {
        if observations.is_empty() {
            return vec![];
        }

        let mut fragment_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

        for obs in observations {
            let line = obs.matched_line.trim();
            // Извлекаем все подстроки длиной 4-30 символов
            let chars: Vec<char> = line.chars().collect();
            let mut seen_in_this_obs: std::collections::HashSet<String> = std::collections::HashSet::new();
            for window_size in 4..=30.min(chars.len()) {
                for start in 0..=(chars.len() - window_size) {
                    let fragment: String = chars[start..start + window_size].iter().collect();
                    let trimmed = fragment.trim().to_string();
                    if trimmed.len() >= 4 && !seen_in_this_obs.contains(&trimmed) {
                        seen_in_this_obs.insert(trimmed.clone());
                        *fragment_counts.entry(trimmed).or_insert(0) += 1;
                    }
                }
            }
        }

        // Выбрать фрагменты, которые встречаются хотя бы в 60% наблюдений
        let threshold = (observations.len() as f64 * 0.6).ceil() as usize;
        let mut common: Vec<(String, usize)> = fragment_counts
            .into_iter()
            .filter(|(_, count)| *count >= threshold)
            .collect();

        // Сортировать по длине (длинные паттерны предпочтительнее)
        common.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        // Дедупликация: убрать подстроки, которые целиком содержатся в более длинных
        let mut result: Vec<String> = Vec::new();
        for (fragment, _) in &common {
            let is_substring = result.iter().any(|existing| existing.contains(fragment.as_str()));
            if !is_substring {
                result.push(fragment.clone());
            }
            if result.len() >= 5 {
                break; // Максимум 5 кандидатов
            }
        }

        result
    }

    /// Вычислить confidence (R²) кандидатного паттерна на корпусе
    fn evaluate_candidate(
        pattern: &str,
        positive_samples: &[Observation],   // true positives (из целевого плагина)
        negative_samples: &[&str],          // true negatives (безопасный код)
    ) -> f64 {
        let true_positives = positive_samples
            .iter()
            .filter(|obs| obs.matched_line.contains(pattern))
            .count();

        let false_positives = negative_samples
            .iter()
            .filter(|line| line.contains(pattern))
            .count();

        let precision = if true_positives + false_positives > 0 {
            true_positives as f64 / (true_positives + false_positives) as f64
        } else {
            0.0
        };

        let recall = if !positive_samples.is_empty() {
            true_positives as f64 / positive_samples.len() as f64
        } else {
            0.0
        };

        // F1 ≈ geometric mean of precision and recall (R²-подобная метрика)
        if precision + recall > 0.0 {
            (precision * recall).sqrt()
        } else {
            0.0
        }
    }

    /// Главная функция: попытаться обнаружить и заморозить новые правила
    pub fn discover(
        &self,
        observations: &[Observation],
        negative_samples: &[&str],
    ) -> Vec<FrozenRule> {
        if observations.len() < self.min_observations {
            return vec![];
        }

        // Группировка наблюдений по исходному плагину
        let mut by_plugin: std::collections::HashMap<String, Vec<Observation>> = std::collections::HashMap::new();
        for obs in observations {
            by_plugin
                .entry(obs.plugin.clone())
                .or_default()
                .push(obs.clone());
        }

        let mut frozen_rules = Vec::new();

        for (plugin_name, plugin_obs) in &by_plugin {
            if plugin_obs.len() < self.min_observations {
                continue;
            }

            // Извлечь общие фрагменты
            let candidates = Self::extract_common_fragments(plugin_obs);

            for candidate in &candidates {
                let confidence = Self::evaluate_candidate(candidate, plugin_obs, negative_samples);

                if confidence >= self.freeze_threshold {
                    // 🧊 FREEZE! Создаём новое правило
                    let rule = FrozenRule {
                        name: format!("auto-{}-{}", plugin_name, frozen_rules.len()),
                        pattern: candidate.clone(),
                        source_plugin: plugin_name.clone(),
                        confidence,
                        sample_count: plugin_obs.len(),
                    };
                    frozen_rules.push(rule);
                    break; // Один FrozenRule на плагин за итерацию
                }
            }
        }

        frozen_rules
    }
}
