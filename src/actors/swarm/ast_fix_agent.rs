//! # 🛠️ AstFixAgent — Семантические AST-патчи
//!
//! Агент генерации автоматических фиксов на основе EntityGraph.
//! Получает `SecurityVulnFound` → создаёт семантические AST-патчи
//! через ContextEngine (абстрактные провайдеры: GitLab / LocalFs).
//!
//! ## Алгоритм работы
//!
//! 1. Получить список уязвимостей из `SecurityAnalyzerActor`
//! 2. Загрузить исходники через абстрактный ContextEngine
//! 3. Построить Workspace из полученных файлов
//! 4. Для каждого файла — найти и заменить `raw_query` → `prepare_query`
//! 5. Отправить `FixPatch` через Aggregator → GitLab (создание ветки + MR)

use async_trait::async_trait;
use tracing::info;

use crate::models::*;
use crate::protocol::*;
use crate::actor::*;

// ─────────────────────────────────────────────────────────────────────────────
// § AstFixAgent
// ─────────────────────────────────────────────────────────────────────────────

/// Агент генерации AST-патчей: получает уязвимости, создаёт семантические фиксы
/// через ContextEngine (абстрактный провайдер: GitLab / LocalFs).
pub struct AstFixAgent {
    lifecycle: ActorLifecycle,
}

impl AstFixAgent {
    pub fn new() -> Self { Self { lifecycle: ActorLifecycle::Active } }
}

#[async_trait]
impl Actor for AstFixAgent {
    fn name(&self) -> &'static str { "AstFixAgent" }
    fn lifecycle(&self) -> &ActorLifecycle { &self.lifecycle }
    fn set_lifecycle(&mut self, s: ActorLifecycle) { self.lifecycle = s; }

    async fn execute(&mut self, msg: Message) -> TxResult {
        if let Message::SecurityVulnFound { mr_id, vulns, patches: _, plantuml: _ } = msg {
            info!("🛠️ [AstFix:Execute] Граф данных показал {} уязвимостей! Маппинг сущностей в семантические патчи...", vulns.len());

            // Абстрактный провайдер контекста (паттерн Ignition)
            let mut engine = crate::fetcher::ContextEngine::new();
            engine.register(Box::new(crate::fetcher::GitLabProvider { token: "glpat-demo".into() }));
            engine.register(Box::new(crate::fetcher::LocalFsProvider));

            // Получение исходного кода абстрактно
            let target_uri = "gitlab://merge_requests/400";
            let list = engine.fetch(target_uri).await.unwrap_or_default();
            let mut workspace = crate::semantic_engine::Workspace::new(list);

            let mut multi_patch = crate::semantic_engine::MultiFilePatch::new();
            for (file_id, _, content) in &workspace.files {
                if let Some(edit) = crate::semantic_engine::generate_semantic_patch(content, "raw_query") {
                    multi_patch.edits.insert(*file_id, edit);
                }
            }

            // Генерация полных исправленных файлов
            let patched_files = multi_patch.apply(&mut workspace);
            return TxResult::SwarmFix { mr_id, patch: AstPatch { files: patched_files } };
        }
        TxResult::Ignored
    }

    async fn complete(&mut self, result: TxResult, ctx: &mut ActorContext) {
        if let TxResult::SwarmFix { mr_id, patch } = result {
            info!("🛠️ [AstFix:Complete] Отправка FixPatch для MR-{}", mr_id);
            let _ = ctx.orchestrator_tx.send(Message::FixPatch { mr_id, patch }).await;
        }
    }
}
