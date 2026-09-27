//! # 👑 Flow Orchestrator (DAG-контроллер + Heartbeat Monitor + WebSocket)
//!
//! Центральный оркестратор платформы Duo Agent:
//!
//! - **FlowOrchestratorActor** — Маршрутизация сообщений между акторами,
//!   управление жизненным циклом MR (Dirty → Committed), Lazy DAG запись,
//!   Crossover фильтрация ложных срабатываний, телеметрия для React UI.
//!
//! - **WebSocket Handler** — Трансляция телеметрии акторов в React UI
//!   через broadcast channel.

use async_trait::async_trait;
use std::collections::HashMap;
use std::time::Instant;
use tokio::sync::{mpsc, broadcast};
use serde_json::json;
use tracing::{info, warn, error};
use axum::{
    extract::{ws::{Message as WsMessage, WebSocket, WebSocketUpgrade}, State},
    response::IntoResponse,
};

use crate::protocol::*;
use crate::actor::*;

// ─────────────────────────────────────────────────────────────────────────────
// § 1. Фаза MR и внутреннее состояние
// ─────────────────────────────────────────────────────────────────────────────

/// Фаза обработки MR в оркестраторе.
#[derive(Debug, PartialEq)]
pub enum MRPhase {
    /// Начальная фаза: MR принят, анализ начат
    Dirty,
    /// Все проверки завершены, результат зафиксирован
    Committed,
}

/// Состояние отдельного MR в оркестраторе.
pub struct MRState {
    pub phase: MRPhase,
}

// ─────────────────────────────────────────────────────────────────────────────
// § 2. FlowOrchestratorActor
// ─────────────────────────────────────────────────────────────────────────────

/// Центральный оркестратор: маршрутизирует сообщения между всеми акторами,
/// управляет Lazy DAG, heartbeat мониторингом и PoisonPill каскадом.
pub struct FlowOrchestratorActor {
    pub ast_tx: mpsc::Sender<Message>,
    pub sec_tx: mpsc::Sender<Message>,
    pub drift_tx: mpsc::Sender<Message>,
    pub action_tx: mpsc::Sender<Message>,
    pub broker_tx: mpsc::Sender<Message>,
    pub mcp_tx: mpsc::Sender<Message>,
    pub telemetry_tx: broadcast::Sender<String>,
    pub dirty_state: HashMap<u64, MRState>,
    /// Монитор heartbeat: имя актора → время последнего heartbeat
    pub heartbeats: HashMap<&'static str, Instant>,
    pub lifecycle: ActorLifecycle,
    pub child_actors: Vec<&'static str>,
    pub poison_acks: Vec<&'static str>,
}

#[async_trait]
impl Actor for FlowOrchestratorActor {
    fn name(&self) -> &'static str { "FlowOrchestratorActor" }
    fn lifecycle(&self) -> &ActorLifecycle { &self.lifecycle }
    fn set_lifecycle(&mut self, s: ActorLifecycle) { self.lifecycle = s; }

    async fn execute(&mut self, _msg: Message) -> TxResult { TxResult::Ignored }
    async fn complete(&mut self, _result: TxResult, _ctx: &mut ActorContext) {}
}

impl FlowOrchestratorActor {
    /// Основной обработчик сообщений оркестратора.
    pub async fn handle(&mut self, msg: Message, _ctx: &mut ActorContext) {
        match msg {
            Message::MergeRequestCreated { mr } => {
                info!("🔱 [Orch] Запущен DAG для MR-{} (Фаза: Dirty)", mr.mr_id);
                self.dirty_state.insert(mr.mr_id, MRState { phase: MRPhase::Dirty });

                // 🦥 LAZY DAG: Запись операций без немедленного исполнения
                let mut dag = crate::kan_intelligence::infrastructure::lazy_dag::LazyDag::new();
                let files = mr.changed_files.clone();
                let ast_id = dag.record(crate::kan_intelligence::infrastructure::lazy_dag::DagOp::AstAnalysis, vec![], files.clone());
                let sec_id = dag.record(crate::kan_intelligence::infrastructure::lazy_dag::DagOp::SecurityScan, vec![ast_id], files.clone());
                let drift_id = dag.record(crate::kan_intelligence::infrastructure::lazy_dag::DagOp::DriftDetection, vec![ast_id], files.clone());
                let review_id = dag.record(crate::kan_intelligence::infrastructure::lazy_dag::DagOp::CodeReview, vec![], files.clone());
                let fix_id = dag.record(crate::kan_intelligence::infrastructure::lazy_dag::DagOp::AstFix, vec![sec_id], files.clone());
                let comment_id = dag.record(crate::kan_intelligence::infrastructure::lazy_dag::DagOp::PostComment, vec![fix_id, drift_id, review_id], vec![]);
                // Мёртвый узел: дубликат SecurityScan (будет удалён)
                let _dead = dag.record(crate::kan_intelligence::infrastructure::lazy_dag::DagOp::SecurityScan, vec![ast_id], files.clone());

                // Оптимизация: DCE + дедупликация + fusion
                dag.optimize(&[comment_id]);
                dag.print_plan();

                // Исполнение: отправка реальных сообщений
                let _ = self.ast_tx.send(Message::AnalyzeAST { mr_id: mr.mr_id, files: mr.changed_files.clone() }).await;
                let _ = self.sec_tx.send(Message::ScanSecurity { mr_id: mr.mr_id, files: mr.changed_files.clone() }).await;
            }
            Message::GraphReady { mr_id: _, files: _, graph: _ } => {
                info!("🔱 [Orch] EntityGraph извлечён! Рассылка SecurityAnalyzerActor...");
                let _ = self.sec_tx.send(msg).await;
            }
            Message::AstDelta { mr_id, title, deltas } => {
                let _ = self.drift_tx.send(Message::CheckDrift { mr_id, title, deltas }).await;
            }
            Message::DriftDetected { mr_id, violations, semantic_drift, plantuml } => {
                warn!("🔱 [Orch] Обнаружен дрифт. Формирование ревью-комментария...");

                // 🌌 ТЕЛЕМЕТРИЯ: Рассылка дрифта в React UI
                let _ = self.telemetry_tx.send(json!({
                    "type": "NODE_UPDATE", "id": "frontend",
                    "style": { "background": "#ff073a", "color": "#fff", "border": "2px solid red" },
                    "label": "🚨 DRIFT: Frontend"
                }).to_string());
                let _ = self.telemetry_tx.send(json!({
                    "type": "NODE_UPDATE", "id": "database",
                    "style": { "background": "#ff073a", "color": "#fff", "border": "2px solid red" },
                    "label": "🚨 DRIFT: PostgreSQL"
                }).to_string());

                let mut payload = String::new();
                if let Some(s) = semantic_drift {
                    payload.push_str(&format!("🧠 **Семантический дрифт**\n- 🚲 Велосипед: `{}`\n\n", s));
                }
                if !violations.is_empty() {
                    payload.push_str(&format!("🚨 **Структурный дрифт ({})**\n", violations.len()));
                    for (s, t) in &violations { payload.push_str(&format!("- `{}`→`{}`\n", s, t)); }
                    payload.push_str(&format!("```plantuml\n{}\n```\n", plantuml));
                }

                let jira_args = serde_json::json!({
                    "projectKey": "ARCH",
                    "summary": format!("Structural Drift Detected (MR-{})", mr_id),
                    "description": payload.clone()
                });
                let _ = self.mcp_tx.send(Message::ExecuteMCPTool { tool_name: "create_jira_ticket".into(), args: jira_args }).await;

                let _ = self.action_tx.send(Message::PostReviewComment { mr_id, payload }).await;
                if let Some(st) = self.dirty_state.get_mut(&mr_id) { st.phase = MRPhase::Committed; }
            }
            Message::SecurityVulnFound { mr_id, vulns, patches: _, plantuml } => {
                warn!("🔱 [Orch] Найдены уязвимости! Пропускаем через Context Crossover фильтр...");

                // Контекст для MR (в реальной системе — из GitLab API + Git History)
                let context = if mr_id == 400 {
                    crate::crossover::ContextualIndicators {
                        file_churn_rate: 0.85, test_coverage: 0.15,
                        author_seniority: 0.2, is_critical_path: true,
                    }
                } else {
                    crate::crossover::ContextualIndicators {
                        file_churn_rate: 0.1, test_coverage: 0.95,
                        author_seniority: 0.9, is_critical_path: false,
                    }
                };

                let detector = crate::crossover::CrossoverDetector::new();
                let mut valid_issues = Vec::new();

                for vuln in &vulns {
                    let sig = crate::crossover::SecuritySignal {
                        vulnerability_type: vuln.clone(),
                        confidence: 0.60,
                    };

                    if let Some(alert) = detector.evaluate(&sig, &context) {
                        valid_issues.push(alert);
                    } else {
                        info!("✅ [Orch] Подавлен False Positive через Crossover: {}", vuln);
                    }
                }

                if !valid_issues.is_empty() {
                    warn!("🔱 [Orch] {} уязвимостей прошли Crossover фильтр. Fan-out в Aggregator...", valid_issues.len());

                    let _ = self.telemetry_tx.send(json!({
                        "type": "NODE_UPDATE", "id": "api",
                        "style": { "background": "#8b0000", "color": "#fff", "border": "3px solid #ff073a" },
                        "label": "🔥 CROSSOVER RISK: API"
                    }).to_string());

                    let _ = self.broker_tx.send(Message::SecurityReport { mr_id, issues: valid_issues, plantuml }).await;
                } else {
                    info!("🔱 [Orch] Все уязвимости — False Positive! Безопасность MR-{} подтверждена.", mr_id);
                    let _ = self.telemetry_tx.send(json!({
                        "type": "NODE_UPDATE", "id": "api",
                        "style": { "background": "#113311", "color": "#88ff88", "border": "2px solid #39ff14" },
                        "label": "✅ SUPPRESSED (Stable)"
                    }).to_string());
                    let _ = self.drift_tx.send(Message::SecurityScanPassed { mr_id }).await;
                }
            }
            Message::SecurityReport { mr_id, issues, plantuml } => { let _ = self.broker_tx.send(Message::SecurityReport { mr_id, issues, plantuml }).await; }
            Message::FixPatch { mr_id, patch } => { let _ = self.broker_tx.send(Message::FixPatch { mr_id, patch }).await; }
            Message::ReviewReport { mr_id, comments } => { let _ = self.broker_tx.send(Message::ReviewReport { mr_id, comments }).await; }
            Message::AggregatedResult { mr_id, patch, issues, comments, failed_entities, plantuml } => {
                let _ = self.action_tx.send(Message::AggregatedResult { mr_id, patch, issues, comments, failed_entities, plantuml }).await;
            }
            Message::SecurityScanPassed { mr_id } => {
                info!("🔱 [Orch] MR-{} прошёл проверку безопасности.", mr_id);
            }
            Message::DriftPassed { mr_id } => {
                info!("🔱 [Orch] MR-{} прошёл проверку архитектуры.", mr_id);
                if let Some(st) = self.dirty_state.get_mut(&mr_id) { st.phase = MRPhase::Committed; }
            }
            Message::Heartbeat { actor_name } => {
                self.heartbeats.insert(actor_name, Instant::now());
                info!("💓 [Orch] Heartbeat получен от '{}'", actor_name);
            }
            Message::ActorLeaseExpired { actor_name, mr_id } => {
                error!("💀 [Orch] LEASE ИСТЁК для '{}' на MR-{}! Самовосстановление...", actor_name, mr_id);
                info!("   🔄 Переотправка задачи в пул свежих узлов...");
                let _ = self.broker_tx.send(Message::UpdateNodeState { node_id: 1, state: ActorLifecycle::Expired }).await;
            }
            Message::PoisonTaken { actor_name } => {
                info!("☠️  [Orch] PoisonTaken ACK от '{}'", actor_name);
                self.poison_acks.push(actor_name);
                if self.poison_acks.len() == self.child_actors.len() {
                    info!("☠️  [Orch] Все дочерние акторы подтвердили PoisonTaken. Оркестратор завершается.");
                }
            }
            _ => {}
        }
    }

    /// Разослать PoisonPill всем дочерним акторам.
    pub async fn send_poison_to_all(&self) {
        warn!("☠️  [Orch] Рассылка PoisonPill всем {} дочерним акторам...", self.child_actors.len());
        let _ = self.telemetry_tx.send(json!({ "type": "CLEAR" }).to_string());
        let _ = self.ast_tx.send(Message::PoisonPill).await;
        let _ = self.sec_tx.send(Message::PoisonPill).await;
        let _ = self.drift_tx.send(Message::PoisonPill).await;
        let _ = self.action_tx.send(Message::PoisonPill).await;
        let _ = self.mcp_tx.send(Message::PoisonPill).await;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// § 3. WebSocket handler для трансляции телеметрии в React UI
// ─────────────────────────────────────────────────────────────────────────────

/// Axum WebSocket upgrader: подключает клиента к телеметрии.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    State(telemetry_tx): State<broadcast::Sender<String>>,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| handle_socket(socket, telemetry_tx))
}

/// Обработка WebSocket-соединения: отправка начального payload и трансляция.
async fn handle_socket(mut socket: WebSocket, telemetry_tx: broadcast::Sender<String>) {
    let mut rx = telemetry_tx.subscribe();

    // Начальный payload для React UI
    let _ = socket.send(WsMessage::Text("{\"type\":\"CONNECTION_ESTABLISHED\"}".into())).await;

    while let Ok(msg) = rx.recv().await {
        if socket.send(WsMessage::Text(msg.into())).await.is_err() {
            break; // Клиент отключился
        }
    }
}
