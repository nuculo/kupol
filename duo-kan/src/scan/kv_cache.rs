//! KV-Cache для Инкрементального Сканирования (Prefill + Decode)
//!
//! Инспирировано `KATCharLM.hs` (строки 226-338):
//! - `KVCache` = `[(K_cached, V_cached)]` per layer
//! - `prefill` = первый полный проход, заполняет кэш для всех файлов
//! - `decode`  = при новом PR обновляет только затронутые файлы
//!
//! Результат: O(T) за delta-скан вместо O(T²) полного пересканирования.
//! Инвалидация кэша = по графу зависимостей (только смежные файлы).

use std::collections::HashMap;



/// Кэшированная запись для одного файла: сохранённые Key/Value векторы
#[derive(Debug, Clone)]
pub struct KVEntry {
    pub key_vector: Vec<f64>,     // K = projection(file_embedding)
    pub value_vector: Vec<f64>,   // V = projection(file_embedding)
    pub risk_score: f64,          // Последний вычисленный CVSS score
    pub scan_epoch: u64,          // Эпоха последнего обновления
}

/// KV-Cache: HashMap по пути файла → KVEntry
pub struct ScanKVCache {
    cache: HashMap<String, KVEntry>,
    pub current_epoch: u64,
    pub total_prefills: usize,     // Сколько файлов прошли полный KAN-проход
    pub total_cache_hits: usize,   // Сколько файлов взяты из кэша
    pub total_invalidations: usize, // Сколько записей было инвалидировано
}

impl ScanKVCache {
    pub fn new() -> Self {
        Self {
            cache: HashMap::new(),
            current_epoch: 0,
            total_prefills: 0,
            total_cache_hits: 0,
            total_invalidations: 0,
        }
    }

    /// PREFILL: Полный KAN-проход для файла. Дорогая операция O(k²).
    /// Сохраняет K/V в кэш для будущих delta-сканов.
    pub fn prefill(&mut self, file_path: &str, embedding: &[f64]) -> f64 {
        // Симуляция дорогого KAN forward pass
        let key_vec = embedding.iter().map(|x| x * 0.7 + 0.1).collect::<Vec<_>>();
        let value_vec = embedding.iter().map(|x| x * 0.3 + 0.05).collect::<Vec<_>>();
        let risk_score = embedding.iter().sum::<f64>() / embedding.len() as f64;

        self.cache.insert(file_path.to_string(), KVEntry {
            key_vector: key_vec,
            value_vector: value_vec,
            risk_score,
            scan_epoch: self.current_epoch,
        });

        self.total_prefills += 1;
        risk_score
    }

    /// DECODE: Инкрементальный проход. Если файл в кэше и не изменён → O(1) lookup.
    /// Если файл изменён → prefill заново O(k²).
    pub fn decode(&mut self, file_path: &str, changed: bool, embedding: &[f64]) -> f64 {
        if changed {
            // Файл изменён → полный пересчёт (cache miss)
            self.prefill(file_path, embedding)
        } else if let Some(entry) = self.cache.get(file_path) {
            // Файл не изменён → берём из кэша O(1)!
            self.total_cache_hits += 1;
            entry.risk_score
        } else {
            // Новый файл — prefill
            self.prefill(file_path, embedding)
        }
    }

    /// Инвалидация: при изменении файла A инвалидируем его зависимости
    pub fn invalidate(&mut self, file_path: &str) {
        if self.cache.remove(file_path).is_some() {
            self.total_invalidations += 1;
        }
    }

    /// Инвалидация по графу зависимостей: инвалидируем файл + все его зависимые
    pub fn invalidate_with_deps(&mut self, changed_file: &str, dependents: &[&str]) {
        self.invalidate(changed_file);
        for dep in dependents {
            self.invalidate(dep);
        }
    }

    /// Начать новую эпоху (новый PR / коммит)
    pub fn new_epoch(&mut self) {
        self.current_epoch += 1;
    }

    pub fn cache_size(&self) -> usize {
        self.cache.len()
    }

    pub fn hit_rate(&self) -> f64 {
        let total = self.total_prefills + self.total_cache_hits;
        if total == 0 { return 0.0; }
        self.total_cache_hits as f64 / total as f64 * 100.0
    }
}
