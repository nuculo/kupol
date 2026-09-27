//! KAT DSL — Composable Rule Algebra for Security Scanning
//!
//! Вдохновлён `lib/kat/dsl.ml` из OCaml KAN.
//! Вместо императивных `if content.contains(...)` правил,
//! плагины описывают правила декларативно через алгебру:
//!
//! ```
//! let rule = and(
//!     contains("eval("),
//!     not(inside_test()),
//!     or(near("user_input", 3), near("request.body", 3))
//! );
//! ```

/// Алгебраическое правило сканирования (AST)
#[derive(Debug, Clone)]
pub enum Rule {
    /// Строка содержит паттерн
    Contains(String),
    /// Строка начинается с паттерна
    StartsWith(String),
    /// Regex-подобный match (glob)
    Glob(String),
    /// Паттерн находится рядом (в пределах radius строк)
    Near(String, usize),
    /// Строка внутри тестового файла (_test, test_, spec)
    InsideTest,
    /// Строка внутри комментария (//, /*, #)
    InsideComment,
    /// Логическое И (все должны быть true)
    And(Vec<Rule>),
    /// Логическое ИЛИ (хотя бы один true)
    Or(Vec<Rule>),
    /// Логическое НЕ
    Not(Box<Rule>),
    /// Всегда true
    Always,
    /// Всегда false
    Never,
}

// ── Builder functions (DSL API) ──────────────────────────────────────────────

/// Создать правило "содержит"
pub fn contains(s: &str) -> Rule { Rule::Contains(s.to_string()) }

/// Создать правило "начинается с"
pub fn starts_with(s: &str) -> Rule { Rule::StartsWith(s.to_string()) }

/// Создать правило "рядом" (keyword в пределах radius строк)
pub fn near(s: &str, radius: usize) -> Rule { Rule::Near(s.to_string(), radius) }

/// Логическое И
pub fn and(rules: Vec<Rule>) -> Rule { Rule::And(rules) }

/// Логическое ИЛИ
pub fn or(rules: Vec<Rule>) -> Rule { Rule::Or(rules) }

/// Логическое НЕ
pub fn not(rule: Rule) -> Rule { Rule::Not(Box::new(rule)) }

/// Внутри теста
pub fn inside_test() -> Rule { Rule::InsideTest }

/// Внутри комментария
pub fn inside_comment() -> Rule { Rule::InsideComment }

// ── Context для evaluation ───────────────────────────────────────────────────

/// Контекст оценки правила (файл + строка + окрестные строки)
pub struct EvalContext<'a> {
    pub file_path: &'a str,
    pub line: &'a str,
    pub line_number: usize,
    /// Все строки файла (для Near)
    pub all_lines: &'a [String],
}

// ── Evaluation engine ────────────────────────────────────────────────────────

impl Rule {
    /// Оценить правило в контексте
    pub fn evaluate(&self, ctx: &EvalContext) -> bool {
        match self {
            Rule::Contains(pat) => ctx.line.contains(pat.as_str()),

            Rule::StartsWith(pat) => ctx.line.trim().starts_with(pat.as_str()),

            Rule::Glob(glob) => {
                // Простой glob: проверяем все части между *
                let parts: Vec<&str> = glob.split('*').collect();
                let mut pos = 0;
                for part in &parts {
                    if part.is_empty() { continue; }
                    if let Some(found) = ctx.line[pos..].find(part) {
                        pos += found + part.len();
                    } else {
                        return false;
                    }
                }
                true
            }

            Rule::Near(keyword, radius) => {
                let start = ctx.line_number.saturating_sub(*radius);
                let end = (ctx.line_number + radius + 1).min(ctx.all_lines.len());
                ctx.all_lines[start..end].iter().any(|l| l.contains(keyword.as_str()))
            }

            Rule::InsideTest => {
                let fp = ctx.file_path.to_lowercase();
                fp.contains("test") || fp.contains("spec") || fp.contains("mock")
            }

            Rule::InsideComment => {
                let trimmed = ctx.line.trim();
                trimmed.starts_with("//")
                    || trimmed.starts_with('#')
                    || trimmed.starts_with("/*")
                    || trimmed.starts_with('*')
            }

            Rule::And(rules) => rules.iter().all(|r| r.evaluate(ctx)),
            Rule::Or(rules) => rules.iter().any(|r| r.evaluate(ctx)),
            Rule::Not(rule) => !rule.evaluate(ctx),
            Rule::Always => true,
            Rule::Never => false,
        }
    }

    /// Оптимизация правила (constant folding, short-circuit)
    pub fn optimize(self) -> Rule {
        match self {
            Rule::And(rules) => {
                let optimized: Vec<Rule> = rules.into_iter()
                    .map(|r| r.optimize())
                    .filter(|r| !matches!(r, Rule::Always)) // Always в AND — skip
                    .collect();

                if optimized.iter().any(|r| matches!(r, Rule::Never)) {
                    return Rule::Never;  // Never в AND → Never
                }
                if optimized.is_empty() { return Rule::Always; }
                if optimized.len() == 1 { return optimized.into_iter().next().unwrap(); }
                Rule::And(optimized)
            }
            Rule::Or(rules) => {
                let optimized: Vec<Rule> = rules.into_iter()
                    .map(|r| r.optimize())
                    .filter(|r| !matches!(r, Rule::Never)) // Never в OR — skip
                    .collect();

                if optimized.iter().any(|r| matches!(r, Rule::Always)) {
                    return Rule::Always;  // Always в OR → Always
                }
                if optimized.is_empty() { return Rule::Never; }
                if optimized.len() == 1 { return optimized.into_iter().next().unwrap(); }
                Rule::Or(optimized)
            }
            Rule::Not(inner) => {
                match *inner {
                    Rule::Not(double) => double.optimize(), // !!x → x
                    Rule::Always => Rule::Never,
                    Rule::Never => Rule::Always,
                    other => Rule::Not(Box::new(other.optimize())),
                }
            }
            other => other,
        }
    }

    /// Человекочитаемое представление правила
    pub fn describe(&self) -> String {
        match self {
            Rule::Contains(s) => format!("contains(\"{}\")", s),
            Rule::StartsWith(s) => format!("starts_with(\"{}\")", s),
            Rule::Glob(s) => format!("glob(\"{}\")", s),
            Rule::Near(s, r) => format!("near(\"{}\", {})", s, r),
            Rule::InsideTest => "inside_test()".into(),
            Rule::InsideComment => "inside_comment()".into(),
            Rule::And(rules) => {
                let parts: Vec<String> = rules.iter().map(|r| r.describe()).collect();
                format!("and({})", parts.join(", "))
            }
            Rule::Or(rules) => {
                let parts: Vec<String> = rules.iter().map(|r| r.describe()).collect();
                format!("or({})", parts.join(", "))
            }
            Rule::Not(r) => format!("not({})", r.describe()),
            Rule::Always => "always".into(),
            Rule::Never => "never".into(),
        }
    }

    /// Сложность правила (число узлов AST)
    pub fn complexity(&self) -> usize {
        match self {
            Rule::And(rules) | Rule::Or(rules) => {
                1 + rules.iter().map(|r| r.complexity()).sum::<usize>()
            }
            Rule::Not(r) => 1 + r.complexity(),
            _ => 1,
        }
    }
}
