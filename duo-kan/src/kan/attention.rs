//! Multi-Head Security Attention — Контекстное Сканирование Уязвимостей
//!
//! Вдохновлён `lib/kat/attention.ml` из OCaml KAN.
//! Вместо построчного сканирования, мы анализируем **контекст** вокруг подозрительной строки
//! через 3 независимые "головы внимания":
//!
//! - Head 1 (Control Flow): if/else, loops, cfg(test) вокруг строки
//! - Head 2 (Data Flow): откуда пришли данные (taint: user_input, extern, const)
//! - Head 3 (Historical): возраст строки, автор (junior/senior), частота изменений
//!
//! Финальный Score = concat(H1, H2, H3) → OutProjection (взвешенная сумма)

/// Контекстный вектор одной "головы внимания"
#[derive(Debug, Clone)]
pub struct HeadScore {
    pub name: &'static str,
    pub value: f64,       // 0.0 = безопасно, 1.0 = максимальный риск
    pub reason: String,
}

/// Результат Multi-Head Attention анализа
#[derive(Debug, Clone)]
pub struct AttentionResult {
    pub heads: Vec<HeadScore>,
    pub final_score: f64,
    pub verdict: AttentionVerdict,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AttentionVerdict {
    /// Контекст подтверждает угрозу (все головы согласны)
    Confirmed,
    /// Контекст понижает серьёзность (одна или более голов видят safe context)
    Downgraded,
    /// Контекст полностью нейтрализует находку (false positive)
    Suppressed,
}

/// Multi-Head Security Attention Engine
pub struct SecurityAttention {
    /// Веса проекции для каждой головы [control_flow, data_flow, historical]
    pub head_weights: [f64; 3],
    /// Порог подавления: если final_score < threshold → Suppressed
    pub suppress_threshold: f64,
    /// Порог понижения: если final_score < downgrade_threshold → Downgraded
    pub downgrade_threshold: f64,
}

impl SecurityAttention {
    pub fn new() -> Self {
        Self {
            head_weights: [0.4, 0.35, 0.25],  // Control Flow самый важный
            suppress_threshold: 0.2,
            downgrade_threshold: 0.5,
        }
    }

    /// Head 1: Control Flow Attention
    /// Анализирует 5 строк до и после подозрительной строки
    fn head_control_flow(&self, context_lines: &[&str], target_line_idx: usize) -> HeadScore {
        let window_start = target_line_idx.saturating_sub(5);
        let window_end = (target_line_idx + 5).min(context_lines.len());
        let window: Vec<&str> = context_lines[window_start..window_end].to_vec();

        let mut risk: f64 = 0.5; // neutral baseline
        let mut reason = String::from("Стандартный контекст");

        // Понижающие факторы (safe context)
        for line in &window {
            let trimmed = line.trim();
            if trimmed.contains("#[cfg(test)]") || trimmed.contains("#[test]") {
                risk -= 0.4;
                reason = "Внутри тестового модуля (#[cfg(test)])".into();
            }
            if trimmed.contains("// SAFETY:") || trimmed.contains("// SAFE:") {
                risk -= 0.2;
                reason = "Есть комментарий SAFETY".into();
            }
            if trimmed.contains("assert!") || trimmed.contains("debug_assert!") {
                risk -= 0.1;
                reason = format!("{} + assert guard", reason);
            }
        }

        // Повышающие факторы (danger context)
        for line in &window {
            let trimmed = line.trim();
            if trimmed.contains("extern \"C\"") || trimmed.contains("extern \"system\"") {
                risk += 0.3;
                reason = "FFI контекст (extern \"C\") — высокий риск".into();
            }
            if trimmed.contains("pub fn") && !trimmed.contains("pub(crate)") {
                risk += 0.1;
                reason = format!("{} + публичная функция", reason);
            }
        }

        HeadScore {
            name: "ControlFlow",
            value: risk.clamp(0.0, 1.0),
            reason,
        }
    }

    /// Head 2: Data Flow Attention
    /// Отслеживает источники данных, входящих в подозрительную строку
    fn head_data_flow(&self, context_lines: &[&str], target_line_idx: usize) -> HeadScore {
        let window_start = target_line_idx.saturating_sub(10);
        let window_end = (target_line_idx + 3).min(context_lines.len());
        let window: Vec<&str> = context_lines[window_start..window_end].to_vec();

        let mut risk: f64 = 0.3; // baseline
        let mut reason = String::from("Нет явных источников данных");

        for line in &window {
            let trimmed = line.trim();
            // Tainted sources (user input)
            if trimmed.contains("req.body") || trimmed.contains("req.query") 
                || trimmed.contains("stdin") || trimmed.contains("user_input")
                || trimmed.contains("from_request") || trimmed.contains("Form<") {
                risk += 0.4;
                reason = "Данные из пользовательского ввода (tainted source)".into();
            }
            // Sanitized data
            if trimmed.contains("sanitize") || trimmed.contains("validate") || trimmed.contains("escape") {
                risk -= 0.3;
                reason = "Данные прошли санитацию/валидацию".into();
            }
            // Constant data (safe)
            if trimmed.contains("const ") || trimmed.contains("static ") {
                risk -= 0.2;
                reason = "Константные данные (не от пользователя)".into();
            }
        }

        HeadScore {
            name: "DataFlow",
            value: risk.clamp(0.0, 1.0),
            reason,
        }
    }

    /// Head 3: Historical Attention
    /// Оценивает "возраст" и "авторство" строки (симуляция git blame)
    fn head_historical(&self, file_path: &str, target_line_idx: usize) -> HeadScore {
        // В реальной системе здесь был бы вызов `git blame`.
        // Для демо мы используем эвристики по имени файла и позиции строки.
        let mut risk: f64 = 0.5;
        let mut reason = String::from("Стандартная история");

        // Файлы в core/ или lib/ — старый, стабильный код
        if file_path.contains("/core/") || file_path.contains("/lib/") {
            risk -= 0.2;
            reason = "Стабильный core/lib код (низкий churn)".into();
        }

        // Файлы в src/main.rs или новые модули — высокий churn
        if file_path.contains("main.rs") || file_path.contains("/new/") {
            risk += 0.15;
            reason = "Часто изменяемый файл (высокий churn)".into();
        }

        // Строки в конце файла — часто свежедобавленный код
        if target_line_idx > 200 {
            risk += 0.1;
            reason = format!("{} + глубокая позиция (строка {})", reason, target_line_idx);
        }

        HeadScore {
            name: "Historical",
            value: risk.clamp(0.0, 1.0),
            reason,
        }
    }

    /// Главная функция: Multi-Head Attention Forward Pass
    /// Анализирует контекст вокруг подозрительной строки и выдаёт вердикт
    pub fn analyze(
        &self,
        file_path: &str,
        content: &str,
        target_line: usize,  // 0-indexed
    ) -> AttentionResult {
        let lines: Vec<&str> = content.lines().collect();

        let h1 = self.head_control_flow(&lines, target_line);
        let h2 = self.head_data_flow(&lines, target_line);
        let h3 = self.head_historical(file_path, target_line);

        // OutProjection: взвешенная сумма голов
        let final_score = h1.value * self.head_weights[0]
            + h2.value * self.head_weights[1]
            + h3.value * self.head_weights[2];

        let verdict = if final_score < self.suppress_threshold {
            AttentionVerdict::Suppressed
        } else if final_score < self.downgrade_threshold {
            AttentionVerdict::Downgraded
        } else {
            AttentionVerdict::Confirmed
        };

        AttentionResult {
            heads: vec![h1, h2, h3],
            final_score,
            verdict,
        }
    }
}
