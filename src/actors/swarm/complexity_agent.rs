//! # 📏 ComplexityAgent — Анализ цикломатической сложности
//!
//! Агент подсчёта цикломатической сложности функций в MR.
//! Считает управляющие конструкции (`if`, `else`, `match`, `for`,
//! `while`, `loop`, `?`, `&&`, `||`) и генерирует предупреждения
//! для функций с высокой сложностью.
//!
//! ## Пороги сложности
//!
//! | Сложность | Оценка |
//! |-----------|--------|
//! | 1–5 | ✅ Простая |
//! | 6–10 | ⚠️ Умеренная |
//! | 11–20 | 🔶 Высокая |
//! | 21+ | 🚨 Критическая |

use async_trait::async_trait;
use tracing::info;

use crate::protocol::*;
use crate::actor::*;

// ─────────────────────────────────────────────────────────────────────────────
// § ComplexityAgent — цикломатическая сложность
// ─────────────────────────────────────────────────────────────────────────────

/// Агент анализа цикломатической сложности кода в Merge Request.
/// Подсчитывает управляющие конструкции и генерирует отчёт.
pub struct ComplexityAgent {
    lifecycle: ActorLifecycle,
}

impl ComplexityAgent {
    pub fn new() -> Self { Self { lifecycle: ActorLifecycle::Active } }

    /// Подсчитать цикломатическую сложность фрагмента кода.
    /// Базовая сложность = 1, каждая ветка +1.
    fn compute_complexity(source: &str) -> Vec<(String, usize)> {
        let mut results = Vec::new();
        let mut current_fn: Option<String> = None;
        let mut complexity: usize = 1; // Базовая сложность
        let mut brace_depth: i32 = 0;
        let mut fn_start_depth: i32 = 0;

        for line in source.lines() {
            let trimmed = line.trim();

            // Обнаружение начала функции
            if trimmed.starts_with("fn ") || trimmed.starts_with("pub fn ") ||
               trimmed.starts_with("async fn ") || trimmed.starts_with("pub async fn ") {
                if let Some(name) = Self::extract_fn_name(trimmed) {
                    // Сохранить предыдущую функцию
                    if let Some(prev_fn) = current_fn.take() {
                        results.push((prev_fn, complexity));
                    }
                    current_fn = Some(name);
                    complexity = 1;
                    fn_start_depth = brace_depth;
                }
            }

            // Подсчёт управляющих конструкций
            if current_fn.is_some() {
                // Ветвления
                if trimmed.starts_with("if ") || trimmed.contains("} else if ") { complexity += 1; }
                if trimmed.starts_with("else ") || trimmed.contains("} else {") { complexity += 1; }
                // Сопоставление с образцом
                if trimmed.starts_with("match ") || trimmed.contains("=> ") { complexity += 1; }
                // Циклы
                if trimmed.starts_with("for ") { complexity += 1; }
                if trimmed.starts_with("while ") { complexity += 1; }
                if trimmed.starts_with("loop ") || trimmed == "loop {" { complexity += 1; }
                // Оператор ? (ранний выход)
                complexity += trimmed.matches('?').count();
                // Логические операторы (создают ветвления)
                complexity += trimmed.matches("&&").count();
                complexity += trimmed.matches("||").count();
            }

            // Трекинг глубины фигурных скобок
            brace_depth += trimmed.matches('{').count() as i32;
            brace_depth -= trimmed.matches('}').count() as i32;

            // Конец функции
            if current_fn.is_some() && brace_depth <= fn_start_depth {
                if let Some(fn_name) = current_fn.take() {
                    results.push((fn_name, complexity));
                    complexity = 1;
                }
            }
        }

        // Последняя функция
        if let Some(fn_name) = current_fn {
            results.push((fn_name, complexity));
        }

        results
    }

    /// Извлечь имя функции из строки определения.
    fn extract_fn_name(line: &str) -> Option<String> {
        let trimmed = line.trim_start_matches("pub ").trim_start_matches("async ").trim_start_matches("fn ");
        trimmed.split('(').next().map(|s| s.trim().to_string())
    }

    /// Определить уровень риска по сложности.
    fn risk_label(complexity: usize) -> &'static str {
        match complexity {
            0..=5 => "✅ Простая",
            6..=10 => "⚠️ Умеренная",
            11..=20 => "🔶 Высокая",
            _ => "🚨 Критическая",
        }
    }
}

#[async_trait]
impl Actor for ComplexityAgent {
    fn name(&self) -> &'static str { "ComplexityAgent" }
    fn lifecycle(&self) -> &ActorLifecycle { &self.lifecycle }
    fn set_lifecycle(&mut self, s: ActorLifecycle) { self.lifecycle = s; }

    async fn execute(&mut self, msg: Message) -> TxResult {
        if let Message::MergeRequestCreated { mr } = msg {
            info!("📏 [Complexity:Execute] Анализ цикломатической сложности для MR-{}", mr.mr_id);

            let mut comments = Vec::new();
            let mut total_warnings = 0usize;

            for file_path in &mr.changed_files {
                // Чтение содержимого файла (fallback на демо-содержимое)
                let content = std::fs::read_to_string(file_path)
                    .unwrap_or_else(|_| {
                        // Демо-заглушка для файлов, которых нет на диске
                        format!("fn main() {{\n  if true {{\n    for i in 0..10 {{\n      if i > 5 {{\n        match i {{\n          _ => {{}}\n        }}\n      }}\n    }}\n  }}\n}}")
                    });

                let fn_complexities = Self::compute_complexity(&content);

                let mut file_report = format!("### 📏 Цикломатическая сложность: `{}`\n\n", file_path);
                file_report.push_str("| Функция | Сложность | Оценка |\n|---------|-----------|--------|\n");

                for (fn_name, cx) in &fn_complexities {
                    let label = Self::risk_label(*cx);
                    file_report.push_str(&format!("| `{}` | {} | {} |\n", fn_name, cx, label));
                    if *cx > 10 {
                        total_warnings += 1;
                    }
                }

                if total_warnings > 0 {
                    file_report.push_str(&format!(
                        "\n> ⚠️ **{} функций с высокой сложностью.** Рекомендуется рефакторинг.\n", total_warnings
                    ));
                }

                comments.push(file_report);
            }

            info!("📏 [Complexity:Execute] Найдено {} предупреждений по сложности", total_warnings);
            return TxResult::SwarmReview { mr_id: mr.mr_id, comments };
        }
        TxResult::Ignored
    }

    async fn complete(&mut self, result: TxResult, ctx: &mut ActorContext) {
        if let TxResult::SwarmReview { mr_id, comments } = result {
            info!("📏 [Complexity:Complete] Отправка отчёта сложности для MR-{}", mr_id);
            let _ = ctx.orchestrator_tx.send(Message::ReviewReport { mr_id, comments }).await;
        }
    }
}
