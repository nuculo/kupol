//! # 🧬 Actor-инфраструктура (2-фазная модель Execute/Complete)
//!
//! Ядро акторной системы платформы Duo Agent:
//!
//! - **ActorContext** — Контекст актора: каналы для оркестратора и телеметрии
//! - **TxResult** — Результат фазы Execute (передаётся в Complete)
//! - **Actor trait** — 2-фазный контракт: Execute (мутация Dirty) → Complete (Committed)
//! - **spawn_actor_2phase** — Запуск актора с heartbeat и lease-таймаутом

use async_trait::async_trait;
use tokio::sync::{mpsc, broadcast};
use tokio::time::{timeout, Duration};
use tracing::{info, warn};

use crate::models::{AstPatch, EntityGraph};
use crate::protocol::{ActorLifecycle, GraphDelta, Message};

// ─────────────────────────────────────────────────────────────────────────────
// § 1. Контекст актора
// ─────────────────────────────────────────────────────────────────────────────

/// Контекст, передаваемый в фазу Complete каждого актора.
/// Содержит канал обратной связи к оркестратору и канал телеметрии для UI.
#[derive(Clone)]
pub struct ActorContext {
    /// Канал отправки сообщений обратно в оркестратор
    pub orchestrator_tx: mpsc::Sender<Message>,
    /// Канал широковещательной телеметрии (WebSocket → React UI)
    pub telemetry_tx: broadcast::Sender<String>,
}

// ─────────────────────────────────────────────────────────────────────────────
// § 2. Результат фазы Execute
// ─────────────────────────────────────────────────────────────────────────────

/// Непрозрачный результат фазы Execute, передаваемый в Complete.
/// Каждый вариант соответствует конкретному типу обработки.
#[derive(Debug)]
pub enum TxResult {
    /// AST-анализ: построены дельты графа
    AstResult { mr_id: u64, title: String, deltas: Vec<GraphDelta> },
    /// Дрифт обнаружен: нарушения + семантический анализ
    DriftResult { mr_id: u64, violations: Vec<(String, String)>, semantic_drift: Option<String>, plantuml: String },
    /// Дрифт не обнаружен — MR прошёл проверку
    DriftPass { mr_id: u64 },
    /// Уязвимости найдены: taint-пути + патчи + PlantUML
    SecurityResult { mr_id: u64, vulns: Vec<String>, patches: Vec<String>, plantuml: String },
    /// Безопасность подтверждена
    SecurityPass { mr_id: u64 },
    /// Комментарий к MR опубликован
    ReviewPosted,
    /// Узел зарегистрирован в Node Broker
    RegisterNode { node_id: u32, address: String, version: u64 },
    /// Состояние узла обновлено
    UpdateNode { node_id: u32, state: ActorLifecycle, version: u64 },
    /// MCP JSON-RPC payload сформирован
    McpPayloadReady { payload: String },

    // --- Граф сущностей ---
    /// EntityGraph построен — готов к taint-анализу
    GraphBuilt { mr_id: u64, files: Vec<String>, graph: Box<EntityGraph> },

    // --- Swarm ---
    /// Swarm: найдены уязвимости
    SwarmSecurity { mr_id: u64, issues: Vec<String> },
    /// Swarm: AST-патч готов
    SwarmFix { mr_id: u64, patch: AstPatch },
    /// Swarm: ревью-отчёт готов
    SwarmReview { mr_id: u64, comments: Vec<String> },
    /// Swarm: все агенты завершили работу
    AggregatorReady { mr_id: u64 },

    /// Сообщение проигнорировано (не обработано данным актором)
    Ignored,
}

// ─────────────────────────────────────────────────────────────────────────────
// § 3. Actor trait — 2-фазный контракт Execute/Complete
// ─────────────────────────────────────────────────────────────────────────────

/// Двухфазный актор, вдохновлённый YDB-таблетками.
///
/// **Фаза 1 (Execute):** Вычисление результата, мутация только Dirty-состояния.
/// **Фаза 2 (Complete):** Продвижение результата в Committed, уведомление оркестратора.
#[async_trait]
pub trait Actor: Send + Sync {
    /// Имя актора (используется в логах и heartbeat)
    fn name(&self) -> &'static str;
    /// Текущее состояние жизненного цикла
    fn lifecycle(&self) -> &ActorLifecycle;
    /// Установить новое состояние жизненного цикла
    fn set_lifecycle(&mut self, state: ActorLifecycle);

    /// Вызывается при старте актора (инициализация таблетки)
    async fn on_start(&mut self) {
        info!("▶️  [{}] Актор запущен (Инит таблетки, Состояние: {:?})", self.name(), self.lifecycle());
    }

    /// Фаза 1: Execute — вычислить результат, мутировать только локальное (Dirty) состояние.
    async fn execute(&mut self, msg: Message) -> TxResult;

    /// Фаза 2: Complete — продвинуть результат в Committed, уведомить оркестратор.
    async fn complete(&mut self, result: TxResult, ctx: &mut ActorContext);

    /// Обработка PoisonPill — переход в режим завершения (Draining)
    async fn on_poison_pill(&mut self) {
        warn!("☠️  [{}] Получен PoisonPill → Draining...", self.name());
        self.set_lifecycle(ActorLifecycle::Draining);
    }

    /// Финальная остановка актора
    async fn on_stop(&mut self) {
        self.set_lifecycle(ActorLifecycle::Removed);
        info!("⏹️  [{}] Актор остановлен (Состояние: Removed)", self.name());
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// § 4. Запуск актора с 2-фазным циклом, heartbeat и lease-таймаутом
// ─────────────────────────────────────────────────────────────────────────────

/// Запустить актора в tokio-задаче с 2-фазным циклом Execute/Complete.
///
/// - `lease_ms` — таймаут на фазу Execute (при превышении → ActorLeaseExpired)
/// - `heartbeat_ms` — интервал heartbeat (0 = отключен)
pub async fn spawn_actor_2phase<A: Actor + 'static>(
    mut actor: A, mut rx: mpsc::Receiver<Message>, mut ctx: ActorContext, lease_ms: u64, heartbeat_ms: u64
) {
    actor.on_start().await;
    let actor_name = actor.name();

    // Запуск отправки heartbeat в отдельной задаче
    let hb_tx = ctx.orchestrator_tx.clone();
    if heartbeat_ms > 0 {
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(heartbeat_ms)).await;
                if hb_tx.send(Message::Heartbeat { actor_name }).await.is_err() { break; }
            }
        });
    }

    while let Some(msg) = rx.recv().await {
        // Обработка PoisonPill — graceful shutdown
        if matches!(msg, Message::PoisonPill) {
            actor.on_poison_pill().await;
            let _ = ctx.orchestrator_tx.send(Message::PoisonTaken { actor_name }).await;
            break;
        }

        let mr_id = match &msg {
            Message::AnalyzeAST { mr_id, .. } => Some(*mr_id),
            Message::ScanSecurity { mr_id, .. } => Some(*mr_id),
            Message::CheckDrift { mr_id, .. } => Some(*mr_id),
            _ => None,
        };

        // Фаза 1: Execute (с таймаутом аренды)
        let exec_result = timeout(Duration::from_millis(lease_ms), actor.execute(msg)).await;

        match exec_result {
            Ok(tx_result) => {
                // Фаза 2: Complete (продвижение в Committed)
                actor.complete(tx_result, &mut ctx).await;
            }
            Err(_) => {
                // Истёк таймаут аренды! (паттерн TTxExtendLease из YDB)
                if let Some(id) = mr_id {
                    let _ = ctx.orchestrator_tx.send(Message::ActorLeaseExpired { actor_name, mr_id: id }).await;
                }
            }
        }
    }
    actor.on_stop().await;
}
