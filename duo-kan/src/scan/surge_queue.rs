//! Surge Pricing для Scan Queue (Demand-Driven Prioritization)
//!
//! Инспирировано `uber.md` из Haskell KAN:
//! `surge = 1 + (waiting_passengers − idle_drivers) × 0.15`, range 1.0–3.0×
//!
//! В контексте Duo Agents:
//! - "Пассажиры" (waiting) = файлы в очереди на сканирование
//! - "Водители" (idle) = свободные Rayon-потоки
//! - Surge Multiplier повышает приоритет файлов, ожидающих дольше всех,
//!   предотвращая "голодание" (starvation) низкоприоритетных задач.


use std::cmp::Ordering;

/// Элемент очереди сканирования с динамическим приоритетом
#[derive(Debug, Clone)]
pub struct ScanTask {
    pub file_path: String,
    pub base_priority: f64,     // Статический приоритет (от Graph KAN, Gravity, etc.)
    pub wait_ticks: u32,        // Сколько "тиков" задача ждёт в очереди
    pub surge_multiplier: f64,  // Динамический множитель (Uber-style)
}

impl ScanTask {
    pub fn new(path: &str, base_priority: f64) -> Self {
        Self {
            file_path: path.to_string(),
            base_priority,
            wait_ticks: 0,
            surge_multiplier: 1.0,
        }
    }

    /// Эффективный приоритет = base × surge
    pub fn effective_priority(&self) -> f64 {
        self.base_priority * self.surge_multiplier
    }
}

// Для BinaryHeap (max-heap по effective_priority)
impl PartialEq for ScanTask {
    fn eq(&self, other: &Self) -> bool {
        self.effective_priority() == other.effective_priority()
    }
}
impl Eq for ScanTask {}

impl PartialOrd for ScanTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ScanTask {
    fn cmp(&self, other: &Self) -> Ordering {
        self.effective_priority()
            .partial_cmp(&other.effective_priority())
            .unwrap_or(Ordering::Equal)
    }
}

/// Scan Queue с Surge Pricing
pub struct SurgeScanQueue {
    queue: Vec<ScanTask>,
    pub available_threads: usize,  // Количество свободных Rayon-потоков
    pub surge_coefficient: f64,    // Коэффициент формулы surge (default: 0.15)
    pub max_surge: f64,            // Максимальный surge (default: 3.0)
}

impl SurgeScanQueue {
    pub fn new(threads: usize) -> Self {
        Self {
            queue: Vec::new(),
            available_threads: threads,
            surge_coefficient: 0.15,
            max_surge: 3.0,
        }
    }

    /// Добавить файл в очередь сканирования
    pub fn enqueue(&mut self, path: &str, base_priority: f64) {
        self.queue.push(ScanTask::new(path, base_priority));
    }

    /// Пересчитать Surge Multiplier для всех задач в очереди
    /// Формула: surge = clamp(1 + (pending - threads) × coeff, 1.0, max_surge)
    pub fn recalculate_surge(&mut self) {
        let pending = self.queue.len();
        let threads = self.available_threads;

        // Глобальный surge (как в Uber: спрос > предложения)
        let global_surge = (1.0
            + (pending as f64 - threads as f64) * self.surge_coefficient)
            .clamp(1.0, self.max_surge);

        for task in self.queue.iter_mut() {
            // Задачи, которые ждут дольше, получают ДОПОЛНИТЕЛЬНЫЙ бонус
            // Anti-starvation: wait_bonus растёт с каждым тиком ожидания
            let wait_bonus = 1.0 + task.wait_ticks as f64 * 0.1;
            task.surge_multiplier = (global_surge * wait_bonus).min(self.max_surge);
        }
    }

    /// Имитация одного "тика" пайплайна: увеличиваем счётчик ожидания
    pub fn tick(&mut self) {
        for task in self.queue.iter_mut() {
            task.wait_ticks += 1;
        }
        self.recalculate_surge();
    }

    /// Извлечь задачу с наивысшим эффективным приоритетом
    pub fn dequeue_top(&mut self) -> Option<ScanTask> {
        if self.queue.is_empty() { return None; }

        // Находим индекс задачи с максимальным effective_priority
        let mut best_idx = 0;
        let mut best_prio = f64::NEG_INFINITY;
        for (i, task) in self.queue.iter().enumerate() {
            let ep = task.effective_priority();
            if ep > best_prio {
                best_prio = ep;
                best_idx = i;
            }
        }

        Some(self.queue.remove(best_idx))
    }

    pub fn pending_count(&self) -> usize {
        self.queue.len()
    }

    /// Снимок текущего состояния очереди (отсортированный по effective_priority desc)
    pub fn snapshot(&self) -> Vec<(String, f64, f64, u32)> {
        let mut items: Vec<_> = self.queue.iter()
            .map(|t| (t.file_path.clone(), t.base_priority, t.effective_priority(), t.wait_ticks))
            .collect();
        items.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(Ordering::Equal));
        items
    }
}
