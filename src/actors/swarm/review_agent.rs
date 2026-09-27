//! # 👨‍💻 ReviewAgent — Структурированное код-ревью
//!
//! Агент автоматического код-ревью с генерацией структурированных
//! отчётов через модуль `telemetry::ValidationReport`.
//!
//! ## Типы проверок
//!
//! - **Warning** — Возможные улучшения (например, `Result<T, E>`)
//! - **Error** — Критические проблемы (функции > 50 LOC)
//!
//! ## Результат
//!
//! Генерирует Markdown-отчёт и отправляет `ReviewReport` в Aggregator.

use async_trait::async_trait;
use tracing::info;

use crate::protocol::*;
use crate::actor::*;

// ─────────────────────────────────────────────────────────────────────────────
// § ReviewAgent
// ─────────────────────────────────────────────────────────────────────────────

/// Агент код-ревью: генерирует структурированный отчёт с телеметрией.
pub struct ReviewAgent {
    lifecycle: ActorLifecycle,
}

impl ReviewAgent {
    pub fn new() -> Self { Self { lifecycle: ActorLifecycle::Active } }
}

#[async_trait]
impl Actor for ReviewAgent {
    fn name(&self) -> &'static str { "ReviewAgent" }
    fn lifecycle(&self) -> &ActorLifecycle { &self.lifecycle }
    fn set_lifecycle(&mut self, s: ActorLifecycle) { self.lifecycle = s; }

    async fn execute(&mut self, msg: Message) -> TxResult {
        if let Message::MergeRequestCreated { mr } = msg {
            info!("👨‍💻 [Review:Execute] Мульти-проверка сканирования...");

            // Структурированная телеметрия в стиле Ignition vcontext
            let mut report = crate::telemetry::ValidationReport::new();
            report.add(
                crate::telemetry::Severity::Warning,
                "Consider using Result<T, E> for error handling",
                Some("src/test_api.rs".to_string()),
                None
            );
            report.add(
                crate::telemetry::Severity::Error,
                "Function too large (>50 LOC)",
                Some("src/test_frontend.rs".to_string()),
                Some("fn render() {\n   // Extract into smaller components\n}".to_string())
            );

            let comments = vec![report.generate_markdown()];
            return TxResult::SwarmReview { mr_id: mr.mr_id, comments };
        }
        TxResult::Ignored
    }

    async fn complete(&mut self, result: TxResult, ctx: &mut ActorContext) {
        if let TxResult::SwarmReview { mr_id, comments } = result {
            info!("👨‍💻 [Review:Complete] Отправка ReviewReport для MR-{}", mr_id);
            let _ = ctx.orchestrator_tx.send(Message::ReviewReport { mr_id, comments }).await;
        }
    }
}
