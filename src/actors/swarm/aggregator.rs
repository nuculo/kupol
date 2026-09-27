//! # 🐝 AggregatorActor — Fan-in агрегация результатов Swarm
//!
//! Центральный сборщик результатов от всех агентов роя.
//! Собирает `SecurityReport`, `FixPatch` и `ReviewReport` (от всех агентов).
//! Когда все агенты завершили работу — формирует `AggregatedResult`
//! и отправляет в `GitLabMRActor` для публикации.
//!
//! ## Поддерживаемые агенты-отправители
//!
//! - `AstFixAgent` → `FixPatch`
//! - `ReviewAgent` → `ReviewReport`
//! - `ComplexityAgent` → `ReviewReport`
//! - `DependencyAgent` → `ReviewReport`
//! - `DocCoverageAgent` → `ReviewReport`

use async_trait::async_trait;
use std::collections::HashMap;
use tracing::info;

use crate::models::*;
use crate::protocol::*;
use crate::actor::*;

// ─────────────────────────────────────────────────────────────────────────────
// § AggregatorState — внутреннее состояние агрегации
// ─────────────────────────────────────────────────────────────────────────────

/// Внутреннее состояние агрегации для одного MR.
pub struct AggregatorState {
    /// Список найденных проблем безопасности
    issues: Vec<String>,
    /// AST-патч (если был сгенерирован AstFixAgent)
    patch: Option<AstPatch>,
    /// Комментарии от всех ревью-агентов
    comments: Vec<String>,
    /// Получен ли SecurityReport
    sec_done: bool,
    /// Получен ли FixPatch
    fix_done: bool,
    /// Счётчик полученных ReviewReport
    review_count: usize,
    /// PlantUML-диаграмма из SecurityReport
    plantuml: String,
}

/// Ожидаемое количество ревью-агентов (ReviewAgent + ComplexityAgent + DependencyAgent + DocCoverageAgent)
const EXPECTED_REVIEW_AGENTS: usize = 4;

// ─────────────────────────────────────────────────────────────────────────────
// § AggregatorActor — Fan-in сборка
// ─────────────────────────────────────────────────────────────────────────────

/// Агрегатор: собирает результаты от всех агентов Swarm.
/// Когда все агенты завершили работу — отправляет `AggregatedResult` в оркестратор.
pub struct AggregatorActor {
    lifecycle: ActorLifecycle,
    /// Состояние агрегации для каждого MR (MR ID → AggregatorState)
    mr_states: HashMap<u64, AggregatorState>,
}

impl AggregatorActor {
    pub fn new() -> Self { Self { lifecycle: ActorLifecycle::Active, mr_states: HashMap::new() } }

    /// Гарантировать наличие состояния для MR.
    fn ensure_state(&mut self, mr_id: u64) {
        self.mr_states.entry(mr_id).or_insert(AggregatorState {
            issues: vec![], patch: None, comments: vec![],
            sec_done: false, fix_done: false, review_count: 0, plantuml: String::new()
        });
    }

    /// Проверить: все ли агенты завершили работу для данного MR.
    fn is_complete(&self, mr_id: u64) -> bool {
        if let Some(st) = self.mr_states.get(&mr_id) {
            st.sec_done && st.fix_done && st.review_count >= EXPECTED_REVIEW_AGENTS
        } else {
            false
        }
    }
}

#[async_trait]
impl Actor for AggregatorActor {
    fn name(&self) -> &'static str { "AggregatorActor" }
    fn lifecycle(&self) -> &ActorLifecycle { &self.lifecycle }
    fn set_lifecycle(&mut self, s: ActorLifecycle) { self.lifecycle = s; }

    async fn execute(&mut self, msg: Message) -> TxResult {
        match msg {
            Message::SecurityReport { mr_id, issues, plantuml } => {
                self.ensure_state(mr_id);
                let st = self.mr_states.get_mut(&mr_id).unwrap();
                st.issues.extend(issues); st.sec_done = true;
                st.plantuml = plantuml;
                info!("🐝 [Aggregator:Execute] SecurityReport получен для MR-{} (rev: {}/{})", mr_id, st.review_count, EXPECTED_REVIEW_AGENTS);
                if self.is_complete(mr_id) { return TxResult::AggregatorReady { mr_id }; }
            }
            Message::FixPatch { mr_id, patch } => {
                self.ensure_state(mr_id);
                let st = self.mr_states.get_mut(&mr_id).unwrap();
                st.patch = Some(patch); st.fix_done = true;
                info!("🐝 [Aggregator:Execute] FixPatch получен для MR-{} (rev: {}/{})", mr_id, st.review_count, EXPECTED_REVIEW_AGENTS);
                if self.is_complete(mr_id) { return TxResult::AggregatorReady { mr_id }; }
            }
            Message::ReviewReport { mr_id, comments } => {
                self.ensure_state(mr_id);
                let st = self.mr_states.get_mut(&mr_id).unwrap();
                st.comments.extend(comments);
                st.review_count += 1;
                info!("🐝 [Aggregator:Execute] ReviewReport #{} получен для MR-{} ({}/{})", st.review_count, mr_id, st.review_count, EXPECTED_REVIEW_AGENTS);
                if self.is_complete(mr_id) { return TxResult::AggregatorReady { mr_id }; }
            }
            _ => {}
        }
        TxResult::Ignored
    }

    async fn complete(&mut self, result: TxResult, ctx: &mut ActorContext) {
        if let TxResult::AggregatorReady { mr_id } = result {
            if let Some(st) = self.mr_states.remove(&mr_id) {
                // Декодирование Entity из JSON (упакованные в issues из SecurityReport)
                let mut failed_entities = Vec::new();
                for ent_json in &st.issues {
                    if let Ok(ent) = serde_json::from_str::<Entity>(ent_json) {
                        failed_entities.push(ent);
                    }
                }

                info!("🐝 [Aggregator:Complete] FAN-IN завершён для MR-{} ({} комментариев от {} агентов) → GitLabMRActor",
                      mr_id, st.comments.len(), st.review_count);
                let _ = ctx.orchestrator_tx.send(Message::AggregatedResult {
                    mr_id, patch: st.patch, issues: st.issues, comments: st.comments, failed_entities, plantuml: st.plantuml
                }).await;
            }
        }
    }
}
