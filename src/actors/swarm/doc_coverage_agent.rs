//! # 📝 DocCoverageAgent — Покрытие документацией
//!
//! Агент анализа покрытия публичных элементов кода doc-комментариями.
//! Сканирует `pub fn`, `pub struct`, `pub enum`, `pub trait` и проверяет
//! наличие `///` или `//!` doc-комментария над ними.
//!
//! ## Результат
//!
//! Генерирует Markdown-таблицу с процентом покрытия
//! и списком недокументированных элементов.

use async_trait::async_trait;
use tracing::info;

use crate::protocol::*;
use crate::actor::*;

// ─────────────────────────────────────────────────────────────────────────────
// § DocCoverageAgent — покрытие документацией
// ─────────────────────────────────────────────────────────────────────────────

/// Запись о публичном элементе кода.
struct PubItem {
    /// Имя элемента (например, `fn main()`)
    name: String,
    /// Номер строки
    line_no: usize,
    /// Есть ли doc-комментарий
    has_doc: bool,
}

/// Агент анализа покрытия документацией: проверяет наличие doc-комментариев
/// над публичными функциями, структурами, enum и trait.
pub struct DocCoverageAgent {
    lifecycle: ActorLifecycle,
}

impl DocCoverageAgent {
    pub fn new() -> Self { Self { lifecycle: ActorLifecycle::Active } }

    /// Проанализировать исходный код и вернуть список публичных элементов.
    fn analyze_doc_coverage(source: &str) -> Vec<PubItem> {
        let lines: Vec<&str> = source.lines().collect();
        let mut items = Vec::new();

        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();

            // Обнаружение публичных элементов
            let is_pub_fn = trimmed.starts_with("pub fn ") || trimmed.starts_with("pub async fn ");
            let is_pub_struct = trimmed.starts_with("pub struct ");
            let is_pub_enum = trimmed.starts_with("pub enum ");
            let is_pub_trait = trimmed.starts_with("pub trait ");

            if is_pub_fn || is_pub_struct || is_pub_enum || is_pub_trait {
                // Извлечение имени элемента
                let name = trimmed.split('{').next().unwrap_or(trimmed)
                    .split('(').next().unwrap_or(trimmed)
                    .split('<').next().unwrap_or(trimmed)
                    .trim().to_string();

                // Проверка наличия doc-комментария на предыдущих строках
                let mut has_doc = false;
                let mut check_line = i;
                while check_line > 0 {
                    check_line -= 1;
                    let prev = lines[check_line].trim();
                    if prev.starts_with("///") || prev.starts_with("//!") {
                        has_doc = true;
                        break;
                    }
                    if prev.starts_with("#[") || prev.is_empty() {
                        continue; // Пропускаем атрибуты и пустые строки
                    }
                    break; // Любая другая строка — doc-комментария нет
                }

                items.push(PubItem { name, line_no: i + 1, has_doc });
            }
        }

        items
    }
}

#[async_trait]
impl Actor for DocCoverageAgent {
    fn name(&self) -> &'static str { "DocCoverageAgent" }
    fn lifecycle(&self) -> &ActorLifecycle { &self.lifecycle }
    fn set_lifecycle(&mut self, s: ActorLifecycle) { self.lifecycle = s; }

    async fn execute(&mut self, msg: Message) -> TxResult {
        if let Message::MergeRequestCreated { mr } = msg {
            info!("📝 [DocCoverage:Execute] Анализ покрытия документацией для MR-{}", mr.mr_id);

            let mut comments = Vec::new();

            for file_path in &mr.changed_files {
                let content = std::fs::read_to_string(file_path).unwrap_or_else(|_| {
                    // Демо-заглушка
                    r#"
/// Документированная функция
pub fn documented_fn() {}

pub fn undocumented_fn() {}

/// Документированная структура
pub struct DocStruct {}

pub struct UndocStruct {}

pub enum UndocEnum { A, B }

/// Документированный trait
pub trait DocTrait {}

pub trait UndocTrait {}
"#.to_string()
                });

                let items = Self::analyze_doc_coverage(&content);

                if items.is_empty() {
                    continue;
                }

                let total = items.len();
                let documented = items.iter().filter(|i| i.has_doc).count();
                let coverage_pct = if total > 0 { (documented as f64 / total as f64 * 100.0) as usize } else { 100 };

                let mut report = format!("### 📝 Покрытие документацией: `{}`\n\n", file_path);

                // Индикатор прогресса
                let bar_filled = coverage_pct / 5;
                let bar_empty = 20 - bar_filled;
                let bar = format!("{}{}", "█".repeat(bar_filled), "░".repeat(bar_empty));
                let emoji = if coverage_pct >= 80 { "✅" } else if coverage_pct >= 50 { "⚠️" } else { "🚨" };
                report.push_str(&format!("**{}** {}/{} элементов ({} {}%)\n\n", emoji, documented, total, bar, coverage_pct));

                // Таблица недокументированных элементов
                let undoc: Vec<&PubItem> = items.iter().filter(|i| !i.has_doc).collect();
                if !undoc.is_empty() {
                    report.push_str("| Строка | Элемент | Статус |\n|--------|---------|--------|\n");
                    for item in &undoc {
                        report.push_str(&format!("| L{} | `{}` | ❌ Нет doc-комментария |\n", item.line_no, item.name));
                    }
                    report.push_str(&format!(
                        "\n> 📝 **{} публичных элементов без документации.** Добавьте `///` комментарии.\n",
                        undoc.len()
                    ));
                }

                comments.push(report);
            }

            return TxResult::SwarmReview { mr_id: mr.mr_id, comments };
        }
        TxResult::Ignored
    }

    async fn complete(&mut self, result: TxResult, ctx: &mut ActorContext) {
        if let TxResult::SwarmReview { mr_id, comments } = result {
            info!("📝 [DocCoverage:Complete] Отправка отчёта покрытия для MR-{}", mr_id);
            let _ = ctx.orchestrator_tx.send(Message::ReviewReport { mr_id, comments }).await;
        }
    }
}
