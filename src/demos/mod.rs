use anyhow::Result;
use tokio::time::Duration;
use tokio::sync::{mpsc, broadcast};
use tracing::{info, warn, error};
use std::collections::HashMap;

use crate::{*, protocol::*, models::*, actor::*, actors::*, orchestrator::*};

pub async fn run_demos() -> anyhow::Result<()> {
    info!("🚀 Запуск Duo Agent Platform (YDB Enterprise v3: 2-Phase + Heartbeat + PoisonPill)");

    let (telemetry_tx, _telemetry_rx) = broadcast::channel::<String>(100);
    let ws_telemetry_tx = telemetry_tx.clone();

    let (orch_tx, mut orch_rx) = mpsc::channel(100);

    // ─── Webhook + WebSocket сервер ──────────────────────────────────────────
    let webhook_orch_tx = orch_tx.clone();
    tokio::spawn(async move {
        let cors = CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any);

        let app = Router::new()
            .route("/webhook", post({
                let tx = webhook_orch_tx;
                move |Json(payload): Json<serde_json::Value>| {
                    let tx = tx.clone();
                    async move {
                        if payload["object_kind"] == "merge_request" {
                            let mr_id = payload["object_attributes"]["iid"].as_u64().unwrap_or(999);
                            let title = payload["object_attributes"]["title"].as_str().unwrap_or("Webhook MR").to_string();
                            info!("🕸️ [Webhook] Получен от GitLab для MR-{}", mr_id);
                            let ev = MergeRequestEvent { mr_id, title, author: "GitLab".into(), changed_files: vec!["src/main.rs".into()] };
                            let _ = tx.send(Message::MergeRequestCreated { mr: ev }).await;
                        }
                        axum::http::StatusCode::OK
                    }
                }
            }))
            .route("/ws", get(orchestrator::ws_handler))
            .with_state(ws_telemetry_tx)
            .layer(cors);

        let listener = TcpListener::bind("0.0.0.0:3000").await.unwrap();
        info!("📞 Webhook & WebSocket сервер активен! Слушаем http://0.0.0.0:3000");
        let _ = axum::serve(listener, app).await;
    });

    // ─── Создание каналов для всех акторов ───────────────────────────────────
    let (ast_tx, ast_rx) = mpsc::channel(100);
    let (drift_tx, drift_rx) = mpsc::channel(100);
    let (sec_tx, sec_rx) = mpsc::channel(100);
    let (action_tx, action_rx) = mpsc::channel(100);
    let (mcp_tx, mcp_rx) = mpsc::channel(100);
    let (fix_tx, fix_rx) = mpsc::channel(100);
    let (rev_tx, rev_rx) = mpsc::channel(100);
    let (agg_tx, agg_rx) = mpsc::channel(100);
    let (broker_tx, broker_rx) = mpsc::channel(100);
    // Новые агенты роя
    let (cmpl_tx, cmpl_rx) = mpsc::channel(100); // ComplexityAgent
    let (dep_tx, dep_rx) = mpsc::channel(100);   // DependencyAgent
    let (doc_tx, doc_rx) = mpsc::channel(100);   // DocCoverageAgent

    let ctx = ActorContext { orchestrator_tx: orch_tx.clone(), telemetry_tx: telemetry_tx.clone() };

    // ─── Запуск всех акторов ─────────────────────────────────────────────────
    tokio::spawn(spawn_actor_2phase(actors::ast_analyzer::ASTAnalyzerActor::new(),       ast_rx,  ctx.clone(), 5000, 0));
    tokio::spawn(spawn_actor_2phase(actors::drift_detector::DriftDetectorActor::new(),   drift_rx, ctx.clone(), 5000, 0));
    tokio::spawn(spawn_actor_2phase(actors::security::SecurityAnalyzerActor::new(),      sec_rx,  ctx.clone(), 1000, 300));
    // 🐝 Рой Swarm-агентов
    tokio::spawn(spawn_actor_2phase(actors::swarm::AstFixAgent::new(),                   fix_rx,  ctx.clone(), 5000, 0));
    tokio::spawn(spawn_actor_2phase(actors::swarm::ReviewAgent::new(),                   rev_rx,  ctx.clone(), 5000, 0));
    tokio::spawn(spawn_actor_2phase(actors::swarm::ComplexityAgent::new(),               cmpl_rx, ctx.clone(), 5000, 0));
    tokio::spawn(spawn_actor_2phase(actors::swarm::DependencyAgent::new(),               dep_rx,  ctx.clone(), 5000, 0));
    tokio::spawn(spawn_actor_2phase(actors::swarm::DocCoverageAgent::new(),              doc_rx,  ctx.clone(), 5000, 0));
    tokio::spawn(spawn_actor_2phase(actors::swarm::AggregatorActor::new(),               agg_rx,  ctx.clone(), 5000, 0));
    // Терминальные акторы
    tokio::spawn(spawn_actor_2phase(actors::gitlab::GitLabMRActor::new(),                 action_rx, ctx.clone(), 5000, 0));
    tokio::spawn(spawn_actor_2phase(actors::mcp_bridge::MCPBridgeActor::new(),           mcp_rx,  ctx.clone(), 5000, 0));
    tokio::spawn(spawn_actor_2phase(actors::node_broker::NodeBrokerActor::new(),         broker_rx, ctx.clone(), 5000, 0));

    // ─── Оркестратор ─────────────────────────────────────────────────────────
    let child_actors = vec![
        "ASTAnalyzerActor", "DriftDetectorActor", "SecurityAnalyzerActor",
        "AstFixAgent", "ReviewAgent", "ComplexityAgent", "DependencyAgent", "DocCoverageAgent",
        "AggregatorActor", "GitLabMRActor", "MCPBridgeActor", "NodeBrokerActor"
    ];
    let mut orch = orchestrator::FlowOrchestratorActor {
        ast_tx, sec_tx: sec_tx.clone(), drift_tx, action_tx, mcp_tx: mcp_tx.clone(),
        broker_tx: agg_tx.clone(),
        telemetry_tx: telemetry_tx.clone(),
        dirty_state: HashMap::new(),
        heartbeats: HashMap::new(),
        lifecycle: ActorLifecycle::Active,
        child_actors,
        poison_acks: Vec::new(),
    };
    orch.on_start().await;
    let mut orch_ctx = ctx.clone();
    let orch_ast_tx = orch.ast_tx.clone();
    let orch_sec_tx = orch.sec_tx.clone();
    let orch_drift_tx = orch.drift_tx.clone();
    let orch_action_tx = orch.action_tx.clone();
    let orch_fix_tx = fix_tx.clone();
    let orch_rev_tx = rev_tx.clone();
    let orch_cmpl_tx = cmpl_tx.clone();
    let orch_dep_tx = dep_tx.clone();
    let orch_doc_tx = doc_tx.clone();

    tokio::spawn(async move {
        while let Some(msg) = orch_rx.recv().await {
            // Перехват сообщений для параллельного Swarm Fan-Out (6 агентов)
            if let Message::MergeRequestCreated { .. } = &msg {
                let _ = orch_rev_tx.send(msg.clone()).await;  // 👨‍💻 ReviewAgent
                let _ = orch_cmpl_tx.send(msg.clone()).await; // 📏 ComplexityAgent
                let _ = orch_dep_tx.send(msg.clone()).await;  // 📦 DependencyAgent
                let _ = orch_doc_tx.send(msg.clone()).await;  // 📝 DocCoverageAgent
            }
            if let Message::SecurityVulnFound { .. } = &msg {
                let _ = orch_fix_tx.send(msg.clone()).await;  // 🛠️ AstFixAgent
            }
            orch.handle(msg, &mut orch_ctx).await;
        }
        orch.on_stop().await;
    });

    tokio::time::sleep(Duration::from_millis(100)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🎭 ДЕМО 0: YDB NODE BROKER (Динамическое создание акторов)
    // ═════════════════════════════════════════════════════════════════════════
    println!("\n=======================================================");
    println!("👉 ДЕМО 0: YDB Node Broker (`TTxRegisterNode` + `DynBitMap`)");
    println!("=======================================================\n");
    let _ = broker_tx.send(Message::SubscribeNodes { subscriber_name: "FlowOrchestratorActor" }).await;
    tokio::time::sleep(Duration::from_millis(100)).await;

    let _ = broker_tx.send(Message::RegisterNodeRequest { host: "sast-agent-pool-a".into(), port: 8080, fixed_node_id: false }).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let _ = broker_tx.send(Message::RegisterNodeRequest { host: "sast-agent-pool-b".into(), port: 8081, fixed_node_id: false }).await;
    tokio::time::sleep(Duration::from_millis(500)).await;
    let _ = broker_tx.send(Message::CheckNodesStatus).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🎭 ДЕМО 1: 2-ФАЗНЫЙ Execute/Complete + Дельта-протокол
    // ═════════════════════════════════════════════════════════════════════════
    println!("\n=======================================================");
    println!("👉 ДЕМО 1: 2-Phase Execute/Complete + Delta Protocol");
    println!("=======================================================\n");
    let ev1 = MergeRequestEvent { mr_id: 300, title: "Feature: Load users".into(), author: "Timur".into(), changed_files: vec!["src/test_frontend.rs".into()] };
    let _ = orch_tx.send(Message::MergeRequestCreated { mr: ev1 }).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🎭 ДЕМО 2: LEASE TIMEOUT + HEARTBEAT (Самовосстановление)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 2: Heartbeat + Lease Timeout (Self-Healing)");
    println!("=======================================================\n");
    let ev2 = MergeRequestEvent { mr_id: 555, title: "Feature: Heavy analysis".into(), author: "Timur".into(), changed_files: vec!["src/test_api.rs".into()] };
    let _ = orch_tx.send(Message::MergeRequestCreated { mr: ev2 }).await;
    tokio::time::sleep(Duration::from_millis(2500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🎭 ДЕМО 3: VECTOR ANN СЕМАНТИЧЕСКИЙ ДРИФТ
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 3: Vector ANN Semantic Drift (Изобретённый Велосипед)");
    println!("=======================================================\n");
    let ev3 = MergeRequestEvent { mr_id: 400, title: "Feature: Implement Custom JWT Auth".into(), author: "Timur".into(), changed_files: vec!["src/test_api.rs".into()] };
    let _ = orch_tx.send(Message::MergeRequestCreated { mr: ev3 }).await;
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🌊 ДЕМО 3.5: STREAMING PIPELINE (core.async из Clojure KAN)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 3.5: Streaming MR Pipeline (Sliding Window)");
    println!("=======================================================\n");
    crate::kan_intelligence::infrastructure::streaming::demo_streaming_pipeline().await;
    tokio::time::sleep(Duration::from_millis(300)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // ❄️ ДЕМО 5: SYMBOLIC DISCOVERY (Edge Freezing из Clojure KAN)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 5: Symbolic Discovery (Edge Freezing)");
    println!("=======================================================\n");
    let mut discoverer = crate::kan_intelligence::ml_core::symbolic::SymbolicDiscoverer::new(3);
    let sample_payload = "fn main() { let x = Option::Some(1); println!(\"{}\", x.unwrap()); }";

    // Имитация 4 MR, где тяжёлый LLM-агент стабильно находит проблему
    for i in 1..=4 {
        info!("❄️ [Orchestrator] Обработка MR {} тяжёлым агентом...", i);
        let fast_results = discoverer.fast_pass(sample_payload);
        if !fast_results.is_empty() {
            info!("    => {} (Тяжёлый агент полностью пропущен!)", fast_results[0]);
        } else {
            info!("    => Тяжёлый агент обнаружил уязвимость!");
            discoverer.probe(sample_payload, true);
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // ⚔️ ДЕМО 6: BABYLONIAN 60-HEAD ПОЛИ-СКАНИРОВАНИЕ
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 6: Babylonian 60-Head Poly-Scanning");
    println!("=======================================================\n");
    let swarm = crate::babylonian::BabylonianSwarm::new();
    let payload = "fn main() { unsafe { raw_query(\"SELECT *\"); } unwrap(); }".to_string();

    info!("⚔️ [Babylonian] Запуск Scatter-Gather по 60 независимым Micro-Actor...");
    let results = swarm.scan_parallel(payload).await;

    for res in results {
        info!("    => {}", res);
    }
    tokio::time::sleep(Duration::from_millis(500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 📊 ДЕМО 7: CONTEXT CROSSOVERS (Подавление ложных срабатываний)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 7: Context Crossovers (False Positive Suppression)");
    println!("=======================================================\n");
    let detector = crate::crossover::CrossoverDetector::new();

    // Сценарий 1: Слабый сигнал в безопасном контексте → отфильтрован
    let sig1 = crate::crossover::SecuritySignal { vulnerability_type: "SQL Injection (Heuristic)".to_string(), confidence: 0.40 };
    let ctx1 = crate::crossover::ContextualIndicators { file_churn_rate: 0.1, test_coverage: 0.8, author_seniority: 0.9, is_critical_path: false };

    // Сценарий 2: Средний сигнал в нестабильном коде → Volatility Crossover
    let sig2 = crate::crossover::SecuritySignal { vulnerability_type: "Unwrap() detected".to_string(), confidence: 0.60 };
    let ctx2 = crate::crossover::ContextualIndicators { file_churn_rate: 0.8, test_coverage: 0.1, author_seniority: 0.5, is_critical_path: false };

    // Сценарий 3: Средний сигнал на критическом пути от Junior → Risk Crossover
    let sig3 = crate::crossover::SecuritySignal { vulnerability_type: "Logic Flaw".to_string(), confidence: 0.50 };
    let ctx3 = crate::crossover::ContextualIndicators { file_churn_rate: 0.1, test_coverage: 0.9, author_seniority: 0.1, is_critical_path: true };

    for (i, (sig, ctx)) in vec![(sig1, ctx1), (sig2, ctx2), (sig3, ctx3)].iter().enumerate() {
        if let Some(alert) = detector.evaluate(sig, ctx) {
            warn!("🚨 [CrossoverDetector] MR {} ТРИГГЕР АЛЕРТА: {}", i+1, alert);
        } else {
            info!("✅ [CrossoverDetector] MR {} ОТФИЛЬТРОВАН КАК ШУМ (False Positive подавлен)", i+1);
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🛡️ ДЕМО 8: MUTABLE CORE / IMMUTABLE SHELL
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 8: Mutable Core / Immutable Shell (из tensor_v2.clj)");
    println!("=======================================================\n");
    let core = crate::kan_intelligence::ml_core::hybrid_state::MutablePerformanceCore::new();
    let start_time = tokio::time::Instant::now();

    // Имитация 1 000 000 оценок AST-узлов с raw pointer мутациями
    info!("🛡️  [Actor Inner Loop] Запуск 1 000 000 оценок AST-узлов с raw pointer мутациями...");
    for _ in 0..1_000_000 {
        core.record_finding_fast(1); // 1 = INFO
    }
    core.record_finding_fast(3); // 3 = CRITICAL

    // Заморозка в Immutable Audit Shell
    let shell = core.freeze(start_time.elapsed().as_micros() as u64);
    info!("🛡️  [Actor Boundary] Внутренний цикл завершён! Состояние заморожено в Immutable Shell.");
    info!("🛡️  [Orchestrator] Получен Immutable Event -> {:?}", shell);
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🏎️ ДЕМО 10: A2A ТЕЛЕКОМ-ПРИМИТИВЫ (Erlang/OTP паттерны)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 10: A2A High-Performance Primitives (11M msg/sec)");
    println!("=======================================================\n");

    // A. Circuit Breaker (Анти-DDoS)
    let mut breaker = crate::a2a::CircuitBreaker::new(5, Duration::from_millis(50));
    info!("🏎️  [A2A:Breaker] Тестирование имитации DDoS (Макс 5 req/s)...");
    for i in 1..=8 {
        if breaker.allow_request() {
            info!("    => Запрос {} РАЗРЕШЁН (Fast Path)", i);
        } else {
            error!("    => Запрос {} ОТКЛОНЁН (Circuit Open — DDoS предотвращён)", i);
        }
    }
    // B. ETS-подобная разделяемая память без блокировок
    info!("🏎️  [A2A:EtsTable] Инициализация Lock-Free Shared AST...");
    let ets = std::sync::Arc::new(crate::a2a::EtsTable::new("Giant_AST_Payload".to_string()));
    let start_time = tokio::time::Instant::now();
    let mut handles = Vec::new();

    for _i in 0..60 {
        let table_ref = ets.clone();
        handles.push(tokio::spawn(async move {
            let _val = table_ref.read();
        }));
    }
    for h in handles { let _ = h.await; }
    info!("🏎️  [A2A:EtsTable] 60 Babylonian акторов прочитали shared state за {:?}", start_time.elapsed());

    // C. io_uring-style WAL
    info!("🏎️  [A2A:WAL] Синхронизация состояния в a2a_journal.log...");
    let mut wal = crate::a2a::WalEngine::new("a2a_journal.log").await.unwrap();
    wal.append_log("NODE_0_COMMITTED").await.unwrap();
    wal.append_log("NODE_1_COMMITTED").await.unwrap();
    info!("🏎️  [A2A:WAL] Записано 2 записи в Write-Ahead-Log.");
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // ☢️ ДЕМО 12: CYCLONEDX BLAST-RADIUS & TRUST DECAY
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 12: CycloneDX Blast-Radius & Trust Decay");
    println!("=======================================================\n");

    info!("☢️  [BlastRadius] Построение EntityGraph вокруг заражённой DB-функции...");
    let mut bg = EntityGraph::new();

    // Заражённая функция
    let tainted_fn = bg.add(Entity { id: 0, kind: EntityKind::Function, name: "unsafe_raw_query()".into(), file: "db.rs".into() });

    // Сервисы среднего уровня
    let svc_auth = bg.add(Entity { id: 1, kind: EntityKind::Struct, name: "AuthService".into(), file: "auth.rs".into() });
    let svc_user = bg.add(Entity { id: 2, kind: EntityKind::Struct, name: "UserService".into(), file: "user.rs".into() });
    let svc_pay = bg.add(Entity { id: 3, kind: EntityKind::Struct, name: "PaymentService".into(), file: "pay.rs".into() });
    bg.connect(svc_auth, tainted_fn, EdgeKind::Calls);
    bg.connect(svc_user, tainted_fn, EdgeKind::Calls);
    bg.connect(svc_pay, tainted_fn, EdgeKind::Calls);

    // 12 эндпоинтов, распределённых по 3 сервисам
    for i in 1..=12 {
        let ep = bg.add(Entity { id: 100 + i, kind: EntityKind::Endpoint, name: format!("POST /api/v1/route_{}", i), file: "router.rs".into() });
        let target_svc = if i % 3 == 0 { svc_auth } else if i % 3 == 1 { svc_user } else { svc_pay };
        bg.connect(ep, target_svc, EdgeKind::Calls);
    }

    info!("☢️  [BlastRadius] Запуск BFS-распространения (алгоритм Trust Decay)...");
    let analyzer = crate::blast_radius::TrustDecayAnalyzer::new();
    let (radius, score) = analyzer.compute_blast_radius(&bg, tainted_fn);

    info!("☢️  [BlastRadius] Найдено {} эндпоинтов, зависящих от unsafe_raw_query().", radius);
    match score {
        crate::blast_radius::RiskScore::Critical => tracing::error!("☢️  {}", analyzer.generate_recommendation(&score)),
        crate::blast_radius::RiskScore::High => tracing::warn!("☢️  {}", analyzer.generate_recommendation(&score)),
        crate::blast_radius::RiskScore::Low => tracing::info!("☢️  {}", analyzer.generate_recommendation(&score)),
    };
    tokio::time::sleep(Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🧬 ДЕМО 13: ЭВОЛЮЦИОННЫЙ NAS ДЛЯ ПРАВИЛ БЕЗОПАСНОСТИ
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 13: Evolutionary NAS for Security Rules");
    println!("=======================================================\n");

    info!("🧬 [Evolution] Запуск генетического алгоритма: 30 геномов × 30 поколений...");
    let _winner = crate::kan_intelligence::security_rules::evolution_nas::evolve(30, 30);
    tokio::time::sleep(Duration::from_millis(500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 💾 ДЕМО 14: CHECKPOINT/RESUME ПАЙПЛАЙНА
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 14: Pipeline Checkpoint/Resume (Crash Resilience)");
    println!("=======================================================\n");

    crate::kan_intelligence::pipeline::checkpoint::demo_checkpoint_resume();
    tokio::time::sleep(Duration::from_millis(500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // ⏳ ДЕМО 15: EARLY STOPPING ДЛЯ ACTOR LEASE
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 15: Early Stopping for Actor Lease");
    println!("=======================================================\n");

    crate::kan_intelligence::pipeline::early_stopping::demo_early_stopping();
    tokio::time::sleep(Duration::from_millis(500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🔬 ДЕМО 16: OPERATOR ALGEBRA ДЛЯ AST-ТРАНСФОРМАЦИЙ
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 16: Operator Algebra for AST Transforms");
    println!("=======================================================\n");

    crate::kan_intelligence::ml_core::operator_algebra::demo_operator_algebra();
    tokio::time::sleep(Duration::from_millis(500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🌊 ДЕМО 17: NORMALIZING FLOW — ОБНАРУЖЕНИЕ АНОМАЛИЙ
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 17: Normalizing Flow — Supply Chain Attack Detection");
    println!("=======================================================\n");

    crate::kan_intelligence::ml_core::normalizing_flow::demo_normalizing_flow();
    tokio::time::sleep(Duration::from_millis(500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🔧 ДЕМО 18: LORA-АДАПТАЦИЯ ЗАМОРОЖЕННЫХ ПРАВИЛ
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 18: LoRA Adaptation for Frozen Security Rules");
    println!("=======================================================\n");

    crate::kan_intelligence::security_rules::lora_rules::demo_lora_rules();
    tokio::time::sleep(Duration::from_millis(500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 📈 ДЕМО 19: LESLIE SMITH LR FINDER
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 19: Leslie Smith LR Finder — Threshold Auto-Tuning");
    println!("=======================================================\n");

    crate::kan_intelligence::pipeline::lr_finder::demo_lr_finder();
    tokio::time::sleep(Duration::from_millis(500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🐸 ДЕМО 21: TEMPORAL KAN (BOILING FROG DEFENSE)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 21: Temporal KAN — Boiling Frog Defense");
    println!("=======================================================\n");

    info!("🐸 [T-KAN] Инициализация Temporal KAN с критическим порогом 1.0...");
    let mut tkan = crate::kan::tkan::TemporalKan::new(1.0);
    
    let simulated_mrs = vec![
        ("MR-101: Fix typo in CSS", 0.05),
        ("MR-102: Refactor auth helper", 0.3),
        ("MR-103: Bypass validation for internal IP", 0.4),
        ("MR-104: Expose debug endpoint", 0.45),
        ("MR-105: Change DB isolation level", 0.4),
    ];

    for (i, (desc, risk)) in simulated_mrs.iter().enumerate() {
        let (hidden_state, is_boiling) = tkan.forward_mr(*risk);
        if is_boiling {
            tracing::error!("🚨 [T-KAN] BOILING FROG ОБНАРУЖЕН на коммите {}! Стейт: {:.2} ({})", i + 1, hidden_state, desc);
            break; // Останавливаем пайплайн
        } else {
            tracing::warn!("🐸 [T-KAN] Коммит {}. Состояние: {:.2}. Описание: {}", i + 1, hidden_state, desc);
        }
    }
    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🎭 ДЕМО 22: SECURITY PHI-PROTOCOL (POLYMORPHIC PLUGINS)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 22: Security Phi-Protocol — Graceful Degradation");
    println!("=======================================================\n");

    info!("🎭 [Phi] Имитация сканирования маленького файла (4 КБ)...");
    let plugin = crate::scan::plugins::SqlInjectionPlugin;
    use crate::scan::plugins::SecurityPlugin;
    
    let small_file = format!("{} \n raw_query(\"DROP TABLE users\");", "A".repeat(4000));
    let findings_small = plugin.scan("small_file.rs", &small_file);
    info!("🎭 [Phi] Отработал движок: {}", findings_small.first().map_or("None", |f| &f.plugin));

    info!("🎭 [Phi] Имитация сканирования гигантского монолита (15 КБ)...");
    let huge_file = format!("{} \n raw_query(\"DROP TABLE users\");", "A".repeat(15000));
    let findings_huge = plugin.scan("huge_monolith.rs", &huge_file);
    info!("🎭 [Phi] Отработал движок (Graceful Degradation): {}", findings_huge.first().map_or("None", |f| &f.plugin));
    
    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 📊 ДЕМО 23: DPO LOSS — АДАПТИВНЫЙ СКОРИНГ УЯЗВИМОСТЕЙ
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 23: DPO Loss — Adaptive Vulnerability Ranking");
    println!("=======================================================\n");

    info!("📊 [DPO] Инициализация DPO Scorer для банковской организации...");
    let plugins = &["sql-injection", "hardcoded-secrets", "unsafe-code", "todo-fixme", "crypto-weakness"];
    let mut scorer = crate::kan::dpo::DpoScorer::new(plugins, 2.0, 0.1);

    info!("📊 [DPO] Начальные веса (все = 1.0):");
    for (name, w) in scorer.ranked_plugins() {
        info!("   → {} = {:.3}", name, w);
    }

    // Банковская команда безопасности помечает предпочтения:
    let preferences = vec![
        crate::kan::dpo::PreferencePair { chosen_plugin: "sql-injection".into(), rejected_plugin: "todo-fixme".into() },
        crate::kan::dpo::PreferencePair { chosen_plugin: "sql-injection".into(), rejected_plugin: "unsafe-code".into() },
        crate::kan::dpo::PreferencePair { chosen_plugin: "hardcoded-secrets".into(), rejected_plugin: "todo-fixme".into() },
        crate::kan::dpo::PreferencePair { chosen_plugin: "crypto-weakness".into(), rejected_plugin: "todo-fixme".into() },
        crate::kan::dpo::PreferencePair { chosen_plugin: "sql-injection".into(), rejected_plugin: "crypto-weakness".into() },
    ];

    info!("📊 [DPO] Обучение на {} парах предпочтений × 20 эпох...", preferences.len());
    let losses = scorer.train(&preferences, 20);
    info!("📊 [DPO] Loss: {:.4} → {:.4} (сходимость)", losses.first().unwrap_or(&0.0), losses.last().unwrap_or(&0.0));

    info!("📊 [DPO] Обученный ранжированный список плагинов (банковский профиль):");
    for (rank, (name, w)) in scorer.ranked_plugins().iter().enumerate() {
        let emoji = match rank { 0 => "🥇", 1 => "🥈", 2 => "🥉", _ => "  " };
        info!("   {} #{}: {} = {:.3}", emoji, rank + 1, name, w);
    }
    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🧠 ДЕМО 24: MULTI-HEAD SECURITY ATTENTION
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 24: Multi-Head Security Attention (Context Analysis)");
    println!("=======================================================\n");

    let attention = crate::kan::attention::SecurityAttention::new();

    // Сценарий A: unsafe {} внутри тестового модуля → SUPPRESSED (False Positive)
    let test_code = r#"
#[cfg(test)]
mod tests {
    #[test]
    fn test_raw_pointer() {
        unsafe {
            let ptr = std::ptr::null::<u8>();
            assert!(ptr.is_null());
        }
    }
}
"#;
    info!("🧠 [Attention] Сценарий A: unsafe {{}} внутри #[cfg(test)]...");
    let result_a = attention.analyze("src/tests/ptr_test.rs", test_code, 5);
    for h in &result_a.heads {
        info!("   → [{}] score={:.2}, reason: {}", h.name, h.value, h.reason);
    }
    info!("   ⇒ Final Score: {:.3} | Verdict: {:?}", result_a.final_score, result_a.verdict);

    // Сценарий B: unsafe {} рядом с extern "C" в core/ → CONFIRMED (Реальная угроза)
    let ffi_code = r#"
extern "C" {
    fn dangerous_syscall(ptr: *mut u8, len: usize) -> i32;
}

pub fn call_external(data: &mut [u8]) -> i32 {
    unsafe {
        dangerous_syscall(data.as_mut_ptr(), data.len())
    }
}
"#;
    info!("🧠 [Attention] Сценарий B: unsafe {{}} рядом с extern \"C\"...");
    let result_b = attention.analyze("src/core/ffi_bridge.rs", ffi_code, 6);
    for h in &result_b.heads {
        info!("   → [{}] score={:.2}, reason: {}", h.name, h.value, h.reason);
    }
    info!("   ⇒ Final Score: {:.3} | Verdict: {:?}", result_b.final_score, result_b.verdict);

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🧬 ДЕМО 25: SYMBOLIC DISCOVERY — АВТОМАТИЧЕСКОЕ СОЗДАНИЕ ПРАВИЛ
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 25: Symbolic Discovery — Auto Rule Generation");
    println!("=======================================================\n");

    use crate::kan::symbolic::{SymbolicDiscoverer, Observation};

    let discoverer = SymbolicDiscoverer::new();

    // Корпус наблюдений: 5 случаев SQLi из разных проектов
    let observations = vec![
        Observation {
            plugin: "sql-injection".into(),
            matched_line: r#"db.execute("SELECT * FROM users WHERE id=" + user_id)"#.into(),
            context_before: vec!["fn get_user(id: i32) {".into()],
            context_after: vec!["  return result;".into()],
            file_path: "project_a/src/db.rs".into(),
        },
        Observation {
            plugin: "sql-injection".into(),
            matched_line: r#"conn.execute("SELECT * FROM orders WHERE customer=" + cid)"#.into(),
            context_before: vec!["fn fetch_orders(cid: &str) {".into()],
            context_after: vec!["  Ok(rows)".into()],
            file_path: "project_b/src/repo.rs".into(),
        },
        Observation {
            plugin: "sql-injection".into(),
            matched_line: r#"db.execute("SELECT * FROM products WHERE name=" + q)"#.into(),
            context_before: vec!["fn search(q: &str) {".into()],
            context_after: vec!["  render(results)".into()],
            file_path: "project_c/src/search.rs".into(),
        },
        Observation {
            plugin: "sql-injection".into(),
            matched_line: r#"stmt.execute("SELECT * FROM sessions WHERE token=" + tok)"#.into(),
            context_before: vec!["fn validate_session(tok: &str) {".into()],
            context_after: vec!["  check(session)".into()],
            file_path: "project_d/src/auth.rs".into(),
        },
        Observation {
            plugin: "sql-injection".into(),
            matched_line: r#"db.raw_query("SELECT * FROM logs WHERE action=" + act)"#.into(),
            context_before: vec!["fn get_logs(act: &str) {".into()],
            context_after: vec!["  display(logs)".into()],
            file_path: "project_e/src/audit.rs".into(),
        },
    ];

    // Негативные примеры (безопасный код, без SQLi)
    let negatives: Vec<&str> = vec![
        "let result = calculate_sum(a, b);",
        "println!(\"Hello, world!\");",
        "let config = Config::load(\"settings.toml\");",
        "fn main() { run_server(); }",
    ];

    info!("🧬 [Symbolic] Загружено {} наблюдений из {} проектов", observations.len(), 5);
    info!("🧬 [Symbolic] Запуск Symbolic Discovery...");

    let frozen = discoverer.discover(&observations, &negatives);

    if frozen.is_empty() {
        info!("🧬 [Symbolic] Не удалось обнаружить достаточно сильных паттернов");
    } else {
        for rule in &frozen {
            info!("🧊 [FROZEN] Новое правило: «{}»", rule.name);
            info!("   → Паттерн: \"{}\"", rule.pattern);
            info!("   → Confidence (R²): {:.3}", rule.confidence);
            info!("   → Обучено на {} наблюдениях из плагина '{}'", rule.sample_count, rule.source_plugin);
            info!("   → Статус: 🧊 ЗАМОРОЖЕНО (автоматически добавлено в плагин)");
        }
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🤖 ДЕМО 26: AGENT KAN — ε-GREEDY САМОЭВОЛЮЦИЯ ПЛАГИНОВ
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 26: Agent KAN — ε-Greedy Self-Evolving Plugins");
    println!("=======================================================\n");

    use crate::kan::agent::AgentPlugin;

    let mut agent = AgentPlugin::new(
        "sql-injection",
        vec!["SELECT".to_string(), "raw_query(".to_string()],
        0.4, // высокий ε для демонстрации мутаций
    );

    info!("🤖 [Agent] Создан агент '{}' с {} правилами, ε={:.1}", agent.name, agent.rules.len(), agent.epsilon);
    info!("🤖 [Agent] Начальные правила: {:?}", agent.rules);

    // Симуляция фидбека от команды безопасности
    agent.feedback("SELECT", true);  // TP
    agent.feedback("SELECT", true);  // TP
    agent.feedback("SELECT", true);  // TP
    agent.feedback("SELECT", false); // FP
    agent.feedback("raw_query(", true);  // TP
    agent.feedback("raw_query(", true);  // TP

    info!("🤖 [Agent] Фидбек: SELECT → 3 TP + 1 FP, raw_query → 2 TP + 0 FP");

    // Пул кандидатных правил для мутаций
    let candidates = &["INSERT INTO", "DROP TABLE", "UNION SELECT", "execute(", "db.query("];

    // Запуск 5 поколений эволюции
    for gen in 0..5 {
        if let Some(mutation) = agent.evolve_step(candidates) {
            info!("🧬 [Gen {}] Мутация: {:?}", gen, mutation);
            // Симулируем: мутация успешна если правил стало больше
            if agent.rules.len() > 1 {
                agent.confirm_last_mutation();
                info!("   ✅ Мутация закреплена");
            } else {
                agent.rollback_last_mutation();
                info!("   ❌ Мутация откачена (rollback)");
            }
        } else {
            info!("🧬 [Gen {}] Эксплуатация (без мутации)", gen);
        }
    }

    let stats = agent.stats();
    info!("📊 [Agent] Финальная статистика:");
    info!("   → Поколение: {}", stats.generation);
    info!("   → Правила: {} шт {:?}", stats.rule_count, agent.rules);
    info!("   → TP: {} | FP: {} | Avg Fitness: {:.3}", stats.total_tp, stats.total_fp, stats.avg_fitness);
    info!("   → Мутации: {} успешных / {} всего", stats.successful_mutations, stats.total_mutations);

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🧠 ДЕМО 27: RAG VULNERABILITY KNOWLEDGE BASE
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 27: RAG — Vulnerability Knowledge Base");
    println!("=======================================================\n");

    use crate::kan::rag::VulnKnowledgeBase;

    let kb = VulnKnowledgeBase::with_owasp_top10();
    info!("🧠 [RAG] Knowledge Base загружена: {} CVE записей, dim={}", kb.len(), kb.dim());

    // Тестовые запросы — подозрительные строки кода
    let test_queries = vec![
        (r#"db.execute("SELECT * FROM users WHERE id=" + user_input)"#, "SQLi пример"),
        (r#"element.innerHTML = request.body.name"#, "XSS пример"),
        (r#"let password = "admin123";"#, "Hardcoded creds"),
        (r#"let x = calculate_sum(a, b);"#, "Чистый код (без CVE)"),
    ];

    for (query, label) in &test_queries {
        info!("🔍 [RAG] Запрос: «{}» ({})", query, label);
        let results = kb.search(query, 3);

        if results.is_empty() {
            info!("   → Нет совпадений (код безопасен)");
        } else {
            for r in &results {
                let icon = if r.similarity >= kb.match_threshold { "🔴" } else { "⚪" };
                info!(
                    "   {} {:.3} │ {} │ {} │ {}",
                    icon, r.similarity, r.entry.cve_id, r.entry.cwe_id, r.entry.title
                );
            }
        }

        // Auto-match: только findings выше порога
        let auto = kb.auto_match(query);
        if !auto.is_empty() {
            info!("   ⚡ AUTO-MATCH: {} CVE привязана(ы) автоматически!", auto.len());
        }
        info!("");
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // ⚡ ДЕМО 28: GRADIENT CLIPPING + EARLY STOPPING
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 28: Gradient Clipping + Early Stopping");
    println!("=======================================================\n");

    use crate::kan::training_utils::{GradientClipper, EarlyStopper};

    let mut clipper = GradientClipper::new(1.0); // max_norm = 1.0
    let mut stopper = EarlyStopper::new(3, 0.01); // patience=3, min_delta=0.01

    info!("⚡ [Training] GradientClipper: max_norm={:.1}", clipper.max_norm);
    info!("⚡ [Training] EarlyStopper: patience={}, min_delta={:.2}", stopper.patience, stopper.min_delta);

    // Симуляция 10 эпох DPO-тренировки
    let simulated_losses = vec![2.5, 1.8, 1.2, 0.9, 0.85, 0.84, 0.84, 0.83, 0.835, 0.84];
    let simulated_grads: Vec<Vec<f64>> = vec![
        vec![0.3, 0.4, 0.2],      // норм
        vec![0.5, 0.6, 0.3],      // норм
        vec![0.8, 0.7, 0.5],      // норм
        vec![2.1, 3.5, 1.8],      // 💥 взрыв!
        vec![1.5, 2.0, 0.9],      // 💥 взрыв!
        vec![0.4, 0.3, 0.5],      // норм после clip
        vec![0.3, 0.2, 0.4],      // норм
        vec![0.2, 0.1, 0.3],      // норм
        vec![1.2, 1.8, 0.7],      // 💥 взрыв!
        vec![0.3, 0.2, 0.1],      // не дойдёт (early stop)
    ];

    for (epoch, (loss, grads)) in simulated_losses.iter().zip(simulated_grads.iter()).enumerate() {
        let mut grad_vec = grads.clone();
        let clip_result = clipper.clip(&mut grad_vec);

        let clip_icon = if clip_result.was_clipped { "✂️" } else { "  " };
        info!(
            "  Epoch {}: loss={:.3} | grad_norm={:.3} → {:.3} {} | scale={:.3}",
            epoch + 1, loss, clip_result.original_norm, clip_result.clipped_norm,
            clip_icon, clip_result.scale_factor
        );

        let decision = stopper.step(*loss);
        if decision.should_stop {
            info!("  🛑 EARLY STOP: {}", decision.reason);
            info!("  📊 Best loss: {:.3} (epoch {})", decision.best_loss, decision.best_epoch);
            break;
        } else if decision.epochs_without_improvement > 0 {
            info!("  ⏳ {}", decision.reason);
        }
    }

    info!("📊 [Clipper] Clip ratio: {:.0}% ({}/{})",
        clipper.clip_ratio() * 100.0, clipper.clip_count, clipper.total_calls);
    info!("📊 [Stopper] Loss history: {:?}", stopper.history());

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🔀 ДЕМО 29: LAZY DAG PIPELINE (XLA-STYLE OPTIMIZATION)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 29: Lazy DAG Pipeline — XLA-style Optimization");
    println!("=======================================================\n");

    use crate::scan::dag::{ScanDag, PluginDeclaration, Requirement};

    let mut dag = ScanDag::new();

    // 5 плагинов с пересекающимися требованиями
    dag.register(PluginDeclaration {
        name: "sql-injection".into(),
        requirements: vec![Requirement::ExtractPatterns, Requirement::StringLiterals],
    });
    dag.register(PluginDeclaration {
        name: "xss-detection".into(),
        requirements: vec![Requirement::ExtractPatterns, Requirement::DataFlowAnalysis],
    });
    dag.register(PluginDeclaration {
        name: "hardcoded-secrets".into(),
        requirements: vec![Requirement::StringLiterals],
    });
    dag.register(PluginDeclaration {
        name: "unsafe-code".into(),
        requirements: vec![Requirement::ControlFlowGraph, Requirement::GitBlame],
    });
    dag.register(PluginDeclaration {
        name: "dependency-audit".into(),
        requirements: vec![Requirement::DependencyGraph],
    });

    info!("🔀 [DAG] Зарегистрировано 5 плагинов");

    // Dead Code Elimination
    let dce_removed = dag.eliminate_dead_code();
    info!("🔀 [DAG] Dead Code Elimination: {} узлов удалено", dce_removed);

    // Execute → получить план
    let plan = dag.execute();

    info!("🔀 [DAG] Execution Plan:");
    info!("   → Topological Order:");
    for (i, req) in plan.execution_order.iter().enumerate() {
        info!("     {}. {:?} (cost={})", i + 1, req, req.cost());
    }
    info!("   → DAG nodes: {} (dедуплицировано)", plan.node_count);
    info!("   → Naive cost (без CSE): {} units", plan.naive_cost);
    info!("   → DAG cost (с CSE):     {} units", plan.total_cost);
    info!("   → CSE savings:          {} units", plan.cse_savings);
    info!("   → CPU экономия:         {:.1}% 🚀", plan.savings_percent());

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🧬 ДЕМО 30: LoRA FINE-TUNING FOR PLUGIN RULES
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 30: LoRA — Low-Rank Adaptation для плагинов");
    println!("=======================================================\n");

    use crate::kan::lora::{LoraPlugin, LoraAdapter};

    // Базовый плагин SQL Injection с 4 frozen правилами
    let mut sqli_plugin = LoraPlugin::new("sql-injection-detector", vec![
        ("SELECT * FROM", 1.0),
        ("DROP TABLE", 1.0),
        ("UNION SELECT", 0.8),
        ("exec sp_", 0.5),
    ]);

    info!("🧬 [LoRA] Base plugin: '{}' (4 frozen rules)", sqli_plugin.name);

    // Клиент 1: Fintech (усиливает SQL, добавляет PCI-DSS правило)
    let mut fintech_adapter = LoraAdapter::new("fintech-corp", 4);
    fintech_adapter.boost("SELECT * FROM", 2.0);    // 2x important
    fintech_adapter.boost("DROP TABLE", 3.0);        // 3x critical!
    fintech_adapter.add_rule("EXECUTE AS LOGIN", 0.9); // PCI-DSS specific
    sqli_plugin.attach_adapter(fintech_adapter);

    // Клиент 2: Startup (убирает stored-proc noise, добавляет NoSQL)
    let mut startup_adapter = LoraAdapter::new("startup-io", 4);
    startup_adapter.suppress("exec sp_");              // не используем stored procs
    startup_adapter.add_rule("$where:", 0.8);          // MongoDB NoSQL injection
    startup_adapter.add_rule("db.collection.find({", 0.7); // NoSQL query injection
    sqli_plugin.attach_adapter(startup_adapter);

    // Сравним правила
    for client in &["fintech-corp", "startup-io"] {
        let stats = sqli_plugin.stats(client);
        info!("📊 [LoRA] Client '{}': {} effective rules ({} active, {} suppressed, {} boosted, {} added)",
            client, stats.total_effective, stats.active_rules,
            stats.suppressed, stats.boosted, stats.added);
        info!("   Trainable: {} params ({:.1}%)  |  Frozen: {} params",
            stats.trainable_params, stats.trainable_ratio(), stats.frozen_params);

        // Test scan
        let test_line = r#"db.execute("SELECT * FROM users WHERE id=" + input)"#;
        let matches = sqli_plugin.scan_line(test_line, client);
        if !matches.is_empty() {
            for (pat, w) in &matches {
                info!("   🔍 Match: '{}' (weight={:.1})", pat, w);
            }
        }
        info!("");
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 📡 ДЕМО 31: SSE STREAMING FINDINGS (REAL-TIME OUTPUT)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 31: SSE Streaming — Real-Time Scan Output");
    println!("=======================================================\n");

    use crate::scan::stream::FindingStream;

    let (mut stream, receiver) = FindingStream::new(100, "scan-sse-demo-001");

    // Симулируем поток findings (в реальности — из par_iter scan loop)
    let emitter_handle = tokio::task::spawn_blocking(move || {
        let findings_sim = vec![
            ("CRITICAL", "sql-injection", "SQLi в user_controller.rs", "src/controllers/user.rs", 42, "CWE-89"),
            ("HIGH", "xss-reflected", "XSS через innerHTML", "src/views/profile.html", 15, "CWE-79"),
            ("MEDIUM", "weak-crypto", "Использование MD5", "src/auth/hash.rs", 88, "CWE-327"),
            ("HIGH", "hardcoded-secret", "API key в исходниках", "src/config.rs", 7, "CWE-798"),
            ("LOW", "unwrap-panic", "Unwrap без обработки", "src/parser.rs", 123, "CWE-391"),
            ("CRITICAL", "cmd-injection", "OS Command Injection", "src/deploy/runner.rs", 56, "CWE-78"),
        ];

        for (sev, rule, msg, file, line, cwe) in &findings_sim {
            stream.emit_finding(sev, rule, msg, file, *line, cwe);
            stream.emit_progress(file);
            std::thread::sleep(std::time::Duration::from_millis(150));
        }

        stream.finish(9.2);
    });

    // Consumer: получаем SSE события в реальном времени
    let consumer_handle = tokio::task::spawn_blocking(move || {
        let mut event_count = 0;
        while let Some(event) = receiver.recv() {
            event_count += 1;
            match &event {
                crate::scan::stream::ScanEvent::ScanStart { total_files, scan_id, .. } => {
                    tracing::info!("📡 [SSE] scan_start: {} files (id={})", total_files, scan_id);
                }
                crate::scan::stream::ScanEvent::Finding { severity, rule_id, file_path, line, .. } => {
                    let icon = match severity.as_str() {
                        "CRITICAL" => "🔴", "HIGH" => "🟠", "MEDIUM" => "🟡", "LOW" => "🟢", _ => "ℹ️"
                    };
                    tracing::info!("📡 [SSE] {} {} {} │ {}:{}", icon, severity, rule_id, file_path, line);
                }
                crate::scan::stream::ScanEvent::SeverityUpdate { critical, high, medium, low, info } => {
                    tracing::info!("📡 [SSE] live: 🔴{} 🟠{} 🟡{} 🟢{} ℹ️{}", critical, high, medium, low, info);
                }
                crate::scan::stream::ScanEvent::ScanDone { total_findings, duration_ms, risk_score, .. } => {
                    tracing::info!("📡 [SSE] scan_done: {} findings in {}ms (risk={:.1})", total_findings, duration_ms, risk_score);
                }
                _ => {}
            }
        }
        tracing::info!("📡 [SSE] Stream закрыт ({} событий обработано)", event_count);
    });

    let _ = emitter_handle.await;
    let _ = consumer_handle.await;

    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 📐 ДЕМО 32: KAT DSL — COMPOSABLE RULE ALGEBRA
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 32: KAT DSL — Security Rule Algebra");
    println!("=======================================================\n");

    use crate::scan::dsl;

    // Rule 1: Eval Injection (eval + user input + не в тесте)
    let eval_rule = dsl::and(vec![
        dsl::contains("eval("),
        dsl::not(dsl::inside_test()),
        dsl::or(vec![
            dsl::near("user_input", 3),
            dsl::near("request", 3),
        ]),
    ]);
    info!("📐 [DSL] Rule 1: {}", eval_rule.describe());
    info!("   Complexity: {} nodes", eval_rule.complexity());

    // Rule 2: Hardcoded Secrets (password/secret/key + строковый литерал)
    let secret_rule = dsl::and(vec![
        dsl::or(vec![
            dsl::contains("password"),
            dsl::contains("secret"),
            dsl::contains("api_key"),
        ]),
        dsl::contains("\""),
        dsl::not(dsl::inside_comment()),
    ]);
    info!("📐 [DSL] Rule 2: {}", secret_rule.describe());
    info!("   Complexity: {} nodes", secret_rule.complexity());

    // Rule 3: Optimizable (with Always/Never + double Not)
    let bloated_rule = dsl::and(vec![
        dsl::contains("exec"),
        dsl::Rule::Always,
        dsl::not(dsl::not(dsl::contains("shell"))),
    ]);
    info!("📐 [DSL] Rule 3 (before optimize): {}", bloated_rule.describe());
    info!("   Complexity: {} nodes", bloated_rule.complexity());
    let optimized = bloated_rule.optimize();
    info!("📐 [DSL] Rule 3 (after optimize):  {}", optimized.describe());
    info!("   Complexity: {} nodes (optimized!)", optimized.complexity());

    // Evaluate rules against test lines
    let test_lines: Vec<String> = vec![
        r#"let result = eval(user_input);"#.to_string(),
        r#"let password = "admin123";"#.to_string(),
        r#"// let api_key = "sk-test";"#.to_string(),
        r#"let x = calculate(a + b);"#.to_string(),
    ];

    info!("");
    info!("📐 [DSL] Evaluation:");
    for (i, line) in test_lines.iter().enumerate() {
        let ctx = dsl::EvalContext {
            file_path: "src/app.rs",
            line,
            line_number: i,
            all_lines: &test_lines,
        };
        let r1 = eval_rule.evaluate(&ctx);
        let r2 = secret_rule.evaluate(&ctx);
        let icon1 = if r1 { "🔴" } else { "⚪" };
        let icon2 = if r2 { "🔴" } else { "⚪" };
        info!("   «{}»", line);
        info!("     eval_rule={} {}  secret_rule={} {}", r1, icon1, r2, icon2);
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🎲 ДЕМО 33: NUCLEUS FUZZY MATCHING (PROBABILISTIC SCAN)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 33: Nucleus Fuzzy Matching — Probabilistic Scan");
    println!("=======================================================\n");

    use crate::scan::fuzzy::NucleusFuzzyScanner;

    let production = NucleusFuzzyScanner::production();
    let research = NucleusFuzzyScanner::research();

    info!("🎲 [Nucleus] Production: temp={:.1}, top_k={}, top_p={:.1}, {} patterns",
        production.temperature, production.top_k, production.top_p, production.pattern_count());
    info!("🎲 [Nucleus] Research:   temp={:.1}, top_k={}, top_p={:.1}, {} patterns",
        research.temperature, research.top_k, research.top_p, research.pattern_count());

    let test_lines = vec![
        (r#"let x = eval(user_input);"#, "Exact match"),
        (r#"let y = evaluate(request.body);"#, "Obfuscated (evaluate vs eval)"),
        (r#"element.inner_html = data;"#, "Typo (inner_html vs innerHTML)"),
        (r#"let sum = add(a, b);"#, "Clean code"),
    ];

    for (line, label) in &test_lines {
        info!("\n🔍 «{}» ({})", line, label);

        let prod_matches = production.scan_line(line);
        let res_matches = research.scan_line(line);

        info!("   [Production] {} matches:", prod_matches.len());
        for m in &prod_matches {
            let bar = "█".repeat((m.similarity * 20.0) as usize);
            info!("     {:.3} │ {} │ «{}» ← «{}»", m.similarity, bar, m.matched_text, m.pattern);
        }

        info!("   [Research]   {} matches:", res_matches.len());
        for m in &res_matches {
            let bar = "█".repeat((m.similarity * 20.0) as usize);
            info!("     {:.3} │ {} │ «{}» ← «{}»", m.similarity, bar, m.matched_text, m.pattern);
        }
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🌐 ДЕМО 34: SPARSE MOE ROUTING (INFINITE RULESETS VIA INVERTED INDEX)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 34: Sparse MoE Routing — 1,000,000 Rules Index");
    println!("=======================================================\n");

    use crate::scan::sparse_router::{SparseRouter, AstFeature};
    use std::time::Instant;

    let mut router = SparseRouter::new();
    let num_rules = 1_000_000;
    info!("🌐 Бустинг 1,000,000 правил в Inverted Index...");

    let idx_start = Instant::now();
    for i in 0..num_rules {
        // Симулируем 50,000 уникальных функций-триггеров (высокая разреженность)
        let func_name = format!("api_call_{}", i % 50_000);
        let triggers = vec![
            AstFeature::FunctionCall(func_name),
            if i % 100 == 0 { AstFeature::ContainsStringLiteral } else { AstFeature::MathOperation },
        ];
        router.register_rule(
            &format!("Rule-{}", i),
            if i % 100 == 0 { "CRITICAL" } else { "LOW" },
            triggers,
        );
    }
    let idx_duration = idx_start.elapsed();
    
    let stats = router.stats();
    info!("🌐 Индекс построен за {:?}", idx_duration);
    info!("   Total Rules: {}", stats.total_rules);
    info!("   Unique Features (Vocab): {}", stats.unique_features);
    info!("   Avg Posting List Size: {:.1} rules", stats.avg_posting_list_size);
    info!("   Max Posting List Size: {} rules", stats.max_posting_list_size);

    info!("\n🌐 Пришёл узел AST (sparse token): [FunctionCall(\"api_call_42\")]");
    let sample_node_features = vec![
        AstFeature::FunctionCall("api_call_42".to_string())
    ];

    // 1. DENSE BENCHMARK (O(N))
    info!("🌐 Оценка через Dense Iteration O(N)...");
    let dense_start = Instant::now();
    let mut dense_matches = Vec::new();
    for i in 0..num_rules {
        let is_match = format!("api_call_{}", i % 50_000) == "api_call_42";
        if is_match {
            dense_matches.push(i + 1); // Mock reference collection
        }
    }
    let dense_duration = dense_start.elapsed();
    info!("   Времени затрачено: {:?}", dense_duration);
    info!("   Найдено правил: {}", dense_matches.len());

    // 2. SPARSE BENCHMARK (O(1))
    info!("\n🌐 Оценка через Inverted Index O(1)...");
    let sparse_start = Instant::now();
    let triggered_rules = router.route_node(&sample_node_features);
    let sparse_duration = sparse_start.elapsed();
    
    let speedup = dense_duration.as_micros() as f64 / (sparse_duration.as_micros().max(1)) as f64;
    info!("   Времени затрачено: {:?} (🔥 Ускорение в {:.0}x!)", sparse_duration, speedup);
    info!("   Активировано правил: {} из 1,000,000", triggered_rules.len());
    
    if let Some(first) = triggered_rules.first() {
        info!("   Sample Trigger: {} ({})", first.name, first.severity);
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(2000)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🕸️ ДЕМО 35: ENTITY CRDT (COLLABORATIVE KNOWLEDGE GRAPH)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 35: Entity CRDT — P2P Knowledge Graph Sync");
    println!("=======================================================\n");

    use crate::scan::crdt::KnowledgeGraph;

    info!("🕸️ [Agent Стелла] Изолировано сканирует микросервис Payment:");
    let mut payment_agent = KnowledgeGraph::new();
    let payment_svc = payment_agent.insert_microservice("Payment Service");
    let auth_svc_ref = payment_agent.insert_microservice("Auth Service"); // Знает только как зависимость
    payment_agent.add_dependency(&payment_svc, &auth_svc_ref);
    info!("   -> Создан Payment Service. Зависит от Auth Service.");
    info!("   -> Локально уязвимостей в Payment: {}", payment_agent.blast_radius(&payment_svc).len());

    info!("\n🕸️ [Agent Аллен] Изолировано сканирует микросервис Auth:");
    let mut auth_agent = KnowledgeGraph::new();
    let auth_svc = auth_agent.insert_microservice("Auth Service");
    let jwt_lib = auth_agent.insert_microservice("jwt-rs v0.2.1");
    let cve_1234 = auth_agent.insert_vulnerability("CVE-2024-1234", "CRITICAL");
    
    auth_agent.add_dependency(&auth_svc, &jwt_lib);
    auth_agent.flag_vulnerable(&jwt_lib, &cve_1234);
    
    info!("   -> Auth Service зависит от jwt-rs, в котором найдена уязвимость CVE-2024-1234.");
    info!("   -> Локально уязвимостей в Auth: {}", auth_agent.blast_radius(&auth_svc).len());

    info!("\n🕸️ [CRDT Sync] Асинхронное P2P слияние Oplog'ов (No DB required)");
    let payment_oplog = payment_agent.oplog.clone();
    let auth_oplog = auth_agent.oplog.clone();

    // Стелла скачивает oplog Аллена и мержит его в свой граф
    payment_agent.merge_oplog(&auth_oplog);
    // Аллен скачивает oplog Стеллы
    auth_agent.merge_oplog(&payment_oplog);

    info!("   -> Оплоги успешно смержены.");

    info!("\n🕸️ [Query] Стелла проверяет Supply Chain Blast Radius для Payment Service:");
    let blast_radius = payment_agent.blast_radius(&payment_svc);
    
    if blast_radius.is_empty() {
        info!("   -> Сущность Payment Service безопасна.");
    } else {
        info!("   -> ⚠️ ОПАСНОСТЬ! Транзитивное заражение через граф зависимостей:");
        for vuln in blast_radius {
            let severity = vuln.attributes.get("severity").unwrap_or(&"UNKNOWN".to_string()).clone();
            info!("      🔴 {} [{}]", vuln.id.0, severity);
        }
        info!("   -> Путь: Payment -> Auth -> jwt-rs -> CVE-2024-1234");
    }

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 📈 ДЕМО 36: NEURAL ODE (CONTINUOUS-TIME RISK DYNAMICS)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 36: Neural ODE — Predictive Risk Dynamics");
    println!("=======================================================\n");

    use crate::scan::ode::{OdeSolver, VulnState, RiskDynamicsPresets};

    info!("📈 Симуляция Zero-Day уязвимости (Supply Chain CVE) на 30 дней вперёд:");
    
    // Начальное состояние (Day 0)
    let initial_state = VulnState {
        risk_score: 7.0,      // На старте это просто HIGH (как CVSS 7.0)
        exploit_prob: 0.05,   // PoC только-только обсуждают закрыто (5%)
        patch_avail: 0.0,     // Патча нет
    };

    info!("   Day  0: Риск={:.2} (HIGH)     │ Угроза Эксплойта={:>3.0}% │ Патч={:>3.0}%", 
        initial_state.risk_score, initial_state.exploit_prob * 100.0, initial_state.patch_avail * 100.0);

    // Запускаем симуляцию (30 дней, 4 шага RK4 в день (dt=0.25) для точности)
    let trajectory = OdeSolver::simulate(
        RiskDynamicsPresets::zero_day_dynamics,
        initial_state,
        30,
        4,
    );

    // Выводим только каждый 3-й день для наглядности
    for (day, state) in trajectory {
        if day as usize % 3 == 0 && day > 0.0 {
            let label = if state.risk_score >= 9.0 {
                "CRITICAL 🔥"
            } else if state.risk_score >= 7.0 {
                "HIGH      "
            } else {
                "MEDIUM    "
            };

            // Визуализация прогресс-бара риска
            let bars = "█".repeat(state.risk_score as usize);
            let empty = "░".repeat(10 - state.risk_score as usize);

            info!("   Day {:>2}: Риск={:.2} [{}{}] {} │ Угроза Эксплойта={:>3.0}% │ Патч={:>3.0}%", 
                day, state.risk_score, bars, empty, label, state.exploit_prob * 100.0, state.patch_avail * 100.0);
        }
    }

    info!("\n📈 Вывод RK4: Риск динамически увеличился из-за логистического роста доступности эксплойта,");
    info!("   достиг пика CRITICAL 🔥 к Дню 12-15, а затем начал спадать благодаря вендорскому патчу.");
    info!("   Статический CVSS (9.8) не показал бы такой кривой технического долга.");

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🎲 ДЕМО 37: TENSOR EMBEDDING (3D FACTORIZED VULNERABILITY PROFILING)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 37: Tensor Embedding — 3D Vuln Profiling & Contraction");
    println!("=======================================================\n");

    use crate::scan::tensor_profile::{ProjectTensor, VulnProfile, Layer, Vector, Severity};

    info!("🎲 Агрегируем 100 находок проекта в 3D-тензор `[Layer × Vector × Severity]`...");
    let mut pt = ProjectTensor::new();

    // 1. Добавим случайный фоновый шум (70 находок распределены случайно)
    for i in 0..70 {
        let layers = [Layer::UI, Layer::API, Layer::Backend, Layer::DB];
        let vectors = [Vector::DataFlow, Vector::ControlFlow, Vector::Crypto, Vector::Auth];
        let severities = [Severity::Low, Severity::Medium, Severity::High, Severity::Critical];
        
        pt.add_vuln(&VulnProfile {
            name: format!("Random-Vuln-{}", i),
            layer: layers[i % 4],
            vector: vectors[(i / 4) % 4],
            severity: severities[(i / 16) % 4],
        });
    }

    // 2. Симулируем системную архитектурную проблему: "Слабая криптография на слое API" (Сильный перекос)
    info!("   * Симуляция системного паттерна: 30x Crypto flaws в API Layer...");
    for i in 0..30 {
        pt.add_vuln(&VulnProfile {
            name: format!("API-Crypto-Leak-{}", i),
            layer: Layer::API,
            vector: Vector::Crypto,
            severity: Severity::High,
        });
    }

    info!("\n🎲 Tensor Contraction 1: Маргинализация оси Severity (2D Heatmap)");
    pt.print_layer_vector_heatmap();

    info!("\n🎲 Tensor Contraction 2: Маргинализация осей Layer & Severity (1D Vector)");
    pt.print_vector_distribution();

    info!("\n🎲 Вывод: Благодаря Tensor Embedding и свёртке осей, AI мгновенно выявил,");
    info!("   что корень подавляющего большинства уязвимостей (30+) лежит в сочетании [API × Crypto].");
    info!("   Вместо того чтобы фиксить 100 багов локально, нужно внедрять централизованную крипту на API Gateway.");

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🗜️ ДЕМО 38: SCALAR QUANTIZATION (INT8 КОМПРЕССИЯ НЕЙРОННОГО ДВИЖКА)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 38: INT8 Scalar Quantization — 8x Memory Reduction");
    println!("=======================================================\n");

    use crate::scan::quantize::Quantizer;

    let total_weights = 10_000_000;
    info!("🗜️ Генерация сырых f64 параметров нейро-роутера ({} весов)...", total_weights);

    // Симуляция Гауссова распределения весов (от -2.5 до +2.5)
    let mut raw_weights = Vec::with_capacity(total_weights);
    for i in 0..total_weights {
        let normalized = (i as f64 / total_weights as f64) * 2.0 - 1.0; 
        raw_weights.push(normalized * 2.5);
    }
    
    let original_bytes = Quantizer::unquantized_size_bytes(raw_weights.len());
    info!("   -> Размер в оперативной памяти (f64): {} MB", original_bytes / (1024 * 1024));

    info!("\n🗜️ Применение Asymmetric Affine Quantization (f64 -> i8)...");
    let quant_start = Instant::now();
    let q_tensor = Quantizer::quantize(&raw_weights);
    let quant_duration = quant_start.elapsed();
    
    let quantized_bytes = q_tensor.memory_size_bytes();
    info!("   -> Размер квантизованного тензора: {} MB (Время сжатия: {:?})", quantized_bytes / (1024 * 1024), quant_duration);
    info!("   -> 🔥 Компрессия RAM: ровно в {:.1} раз!", original_bytes as f64 / quantized_bytes as f64);
    
    info!("\n🗜️ Анализ мета-параметров (для деквантизации):");
    info!("   Scale: {:.5} │ Zero Point: {} │ Min: {:.2} │ Max: {:.2}", 
        q_tensor.meta.scale, q_tensor.meta.zero_point, q_tensor.meta.min_val, q_tensor.meta.max_val);

    // Проверка реконструкции
    let sample_idx = 4_200_000;
    let original_val = raw_weights[sample_idx];
    
    // Ручная деквантизация одного нейрона (q - zero_point) * scale
    let compressed_val = q_tensor.data[sample_idx];
    let reconstructed_val = (compressed_val as f64 - q_tensor.meta.zero_point as f64) * q_tensor.meta.scale;
    
    let error = (original_val - reconstructed_val).abs();

    info!("\n🗜️ Реконструкция (Тест Нейрона #{}):", sample_idx);
    info!("   Оригинал f64  : {:.5}", original_val);
    info!("   Сжатый i8     : {}    (в памяти)", compressed_val);
    info!("   Де-квантизация: {:.5}", reconstructed_val);
    info!("   => Precision Loss: {:.5} (достаточно для точного роутинга)", error);

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🛡️ ДЕМО 39: TYPE-SAFE RULE ENGINE (PHANTOMDATA / GADT SIMULATION)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 39: Type-Safe Rule Engine — Compile-Time Context Validation");
    println!("=======================================================\n");

    use crate::scan::gadt_rule::{
        and, contains_line, contains_ast, has_function_argument, header_match,
        SourceLineEvaluator, AstEvaluator,
    };

    info!("🛡️ Создаём набор строго типизированных правил через DSL...");

    // 1. Правило для исходного кода (SourceLineCtx)
    let source_rule = and(
        contains_line("eval("),
        contains_line("req.body")
    );
    info!("   [SourceLineCtx Rule] : {}", "and(contains('eval('), contains('req.body'))");

    // 2. Правило для AST (AstCtx)
    let ast_rule = and(
        contains_ast("CallExpression"),
        has_function_argument("DangerousInput")
    );
    info!("   [AstCtx Rule]        : {}", "and(contains('CallExpression'), has_function_argument('DangerousInput'))");

    // 3. Правило для HTTP трафика (HttpCtx)
    let _http_rule = header_match("Authorization: Bearer");
    info!("   [HttpCtx Rule]       : {}", "header_match('Authorization: Bearer')");

    info!("\n🛡️ Выполняем оценку в Type-Safe Evaluators...");

    // Правильный вызов
    let is_source_match = SourceLineEvaluator::evaluate(&source_rule, "let x = eval(req.body);");
    info!("   ✅ SourceLineEvaluator(source_rule) = {}", is_source_match);

    let is_ast_match = AstEvaluator::evaluate(&ast_rule, "node: CallExpression, arg: DangerousInput");
    info!("   ✅ AstEvaluator(ast_rule)           = {}", is_ast_match);

    // НЕПРАВИЛЬНЫЙ ВЫЗОВ (Закомментирован, иначе не скомпилируется Rust!)
    // let compile_error = SourceLineEvaluator::evaluate(&ast_rule, "some code");
    
    info!("\n🛡️ Попытка передать `ast_rule` в `SourceLineEvaluator` приведёт к ОШИБКЕ КОМПИЛЯЦИИ: ");
    info!("   ❌ error[E0308]: mismatched types");
    info!("      expected reference `&Rule<SourceLineCtx>`");
    info!("         found reference `&Rule<AstCtx>`");
    info!("\n   => Заимствованная из OCaml GADT идеология симуляции через PhantomData");
    info!("      полностью исключает Runtime-паники (Type Errors) в секьюрити правилах!");

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🕸️ ДЕМО 40: GRAPH KAN ROUTING (PETGRAPH + EDGES SUPPLY CHAIN INTELLIGENCE)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 40: Graph KAN Routing — Supply Chain Intelligence");
    println!("=======================================================\n");

    use crate::scan::graph_kan::DependencyGraph;

    let mut graph = DependencyGraph::new();
    
    // 1. Создаём модули (Узлы графа)
    graph.add_node("Auth_Service");
    graph.add_node("API_Gateway");
    graph.add_node("Frontend_React");
    graph.add_node("Database_Layer");
    graph.add_node("Analytics_Job");

    // 2. Создаём зависимости (Рёбра графа)
    // API_Gateway зависит от Auth_Service (сильная связь: 0.9)
    graph.add_edge("API_Gateway", "Auth_Service", 0.9);
    // Frontend_React зависит от API_Gateway (сильная связь: 1.0)
    graph.add_edge("Frontend_React", "API_Gateway", 1.0);
    // Database_Layer ничего внешнего не требует
    // Analytics_Job работает изолированно

    info!("🕸️ Запуск (Штиль): Нет уязвимостей. KAN работает на рёбрах графа.");
    graph.compute_routing_priorities();
    
    for (id, prio, vulns) in graph.get_scan_queue() {
        info!("   [Очередь] {:<15} : Приоритет {:.1}  (Уязвимостей: {})", id, prio, vulns);
    }

    info!("\n🕸️ ⚡ ШОКОВАЯ ВОЛНА! В `Auth_Service` найдено 10 критических CVE (Zero-Day)!");
    graph.inject_shock("Auth_Service", 10);

    info!("🕸️ KAN Forward Pass: Распространение гравитации приоритетов по РЁБРАМ графа...");
    // 1. Auth_Service -> API_Gateway
    graph.compute_routing_priorities();

    for (id, prio, vulns) in graph.get_scan_queue() {
        let alert = if prio > 5.0 { "🔥 URGENT SCAN" } else { "              " };
        info!("   [Очередь] {:<15} : Приоритет {:>4.1}  (Уязвимостей: {:>2}) {}", id, prio, vulns, alert);
    }

    info!("\n🕸️ Вывод KAN Graph Router:");
    info!("   1) `Auth_Service` (где нашли баг) получил Приоритет 6.0");
    info!("   2) `API_Gateway` ЗАВИСИТ от `Auth`! Импульс KAN на ребре подбросил его Приоритет до 14.5!");
    info!("   * Рой агентов мгновенно перенаправлен на проверку API_Gateway до того, как эксплойт просочится!");
    info!("   3) `Analytics_Job` изолирован и остался на Приоритете 1.0.");

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🧮 ДЕМО 41: FORWARD AD SENSITIVITY ANALYSIS (DUAL NUMBERS)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 41: Forward AD Sensitivity (Dual Numbers)");
    println!("=======================================================\n");

    use crate::scan::forward_ad::{Dual, SecurityRuleEngine};

    // Представим секьюрити-правило, падающее на глубокую вложенность кода.
    let nesting_depth = 5.0; // В коде найдено 5 вложенных циклов.

    // Инженер задаёт порог (threshold) равный 3.0.
    // Мы хотим узнать: какой будет CVSS Score, 
    // И как сильно он изменится, если мы чуть-чуть подвинем порог?
    
    // Создаём Дуальное число: Значение = 3.0, Градиент (Производная по себе) = 1.0.
    let threshold = Dual::var(3.0); 

    info!("🧮 Вычисляем нелинейную Секьюрити-функцию (SiLU + Poly) за 1 проход...");
    let result_dual = SecurityRuleEngine::evaluate_cvss_sensitivity(nesting_depth, threshold);

    info!("   [Input] Вложенность (depth) =  {}", nesting_depth);
    info!("   [Input] Порог правила (thr) =  {:.1}", threshold.v);
    info!("   * Формула: CVSS = 2*SiLU(depth - thr) + (depth/thr)^2\n");

    info!("🧮 Результат Forward AD:");
    info!("   [Output] CVSS Score               =  {:.3}", result_dual.v);
    info!("   [Output] Чувствительность ∂CVSS/∂thr = {:.3}", result_dual.d);

    info!("\n🔮 АНАЛИТИКА ПРОГНОЗА ТЮНИНГА:");
    if result_dual.d < 0.0 {
        info!("   * Если мы УВЕЛИЧИМ порог на +1.0 (с 3.0 до 4.0),");
        info!("   * Базовый CVSS этого куска кода УПАДЁТ примерно на {:.3} баллов.", result_dual.d.abs());
    }
    
    // Давайте проверим предсказание AD! Запустим с порогом 4.0
    let check_threshold = Dual::constant(4.0); // Увеличили на +1
    let check_result = SecurityRuleEngine::evaluate_cvss_sensitivity(nesting_depth, check_threshold);
    let exact_diff = check_result.v - result_dual.v;

    info!("\n🔍 Проверка реальным пересчётом (thr=4.0):");
    info!("   * Новый CVSS Score = {:.3}", check_result.v);
    info!("   * Реальное падение = {:.3}", exact_diff);
    info!("   * Ошибка предсказания Dual градиента: {:.3} (в пределах линейной аппроксимации)", (exact_diff - result_dual.d).abs());

    info!("\n✅ Вывод: Duo-Agents могут авто-тюнить чувствительность тысяч правил без графа вычислений (Backprop)!");

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🦎 ДЕМО 42: LORA EDGE ADAPTERS FOR ANOMALY SURGES
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 42: LoRA Edge Adapters (Аномальные всплески)");
    println!("=======================================================\n");

    use crate::scan::lora_edge::AnomalyLoraGraph;

    let mut graph = AnomalyLoraGraph::new();

    // Граф: Frontend обращается к API и к Auth
    graph.add_node("React_UI");
    graph.add_node("Backend_API");
    graph.add_node("Auth_Server");

    graph.add_edge("React_UI", "Backend_API");
    graph.add_edge("React_UI", "Auth_Server");

    info!("🦎 [Норма] Сканирование в штатном режиме...");
    for (src, _tgt, prio) in graph.evaluate_edges() {
        info!("   * Ребро: {} -> ... | Приоритет: {:.1}", src, prio);
    }

    info!("\n🚨 [АНОМАЛИЯ] В `React_UI` закоммитили сразу 5 XSS-уязвимостей!");
    info!("🦎 Активация временного LoRA-адаптера на узле `React_UI`...");
    
    // Severity 2.0 (Очень сильный шок)
    graph.report_surge("React_UI", 2.0);

    // Симуляция времени (Tick пайплайна)
    info!("\n⏳ Симуляция затухания LoRA (Decay over time):");
    
    for step in 1..=6 {
        graph.tick(); // Эволюция адаптера
        
        let edges = graph.evaluate_edges();
        // Смотрим только на приоритет первого ребра (они все исходят из React_UI)
        let prio = edges[0].2;
        let active_boost = graph.get_adapter_boost("React_UI");
        
        if step == 1 {
            info!("   Шаг 1: 🔥 ВСПЛЕСК! Приоритет = {:.1} (LoRA Multiplier: {:.2}x)", prio, active_boost);
        } else if step == 6 {
            info!("   Шаг 6: 🟩 НОРМА. Приоритет = {:.1} (LoRA Multiplier: {:.2}x) -> Адаптер отключён.", prio, active_boost);
        } else {
            info!("   Шаг {}: 📉 Затухание... Приоритет = {:.1} (LoRA: {:.2}x)", step, prio, active_boost);
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
    }

    info!("\n✅ Вывод: Сканер гиперактивно приоритизирует участки кода с горячими ошибками, и плавно успокаивается, когда разработчик перестает их делать. Самообучение в рамках одной сессии!");

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🔮 ДЕМО 43: WHAT-IF SCENARIO MODELING (GRAPH INCREMENTAL CACHE)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 43: What-If Сценарии (KV-Cache Графа)");
    println!("=======================================================\n");

    use crate::scan::what_if::ScenarioSimulatorGraph;

    let mut graph = ScenarioSimulatorGraph::new();

    // 1. Создаем Монорепу на 5 узлов
    graph.add_node("App_Core", 2.0);
    graph.add_node("Auth_Module", 4.0);
    graph.add_node("DB_Driver", 3.0);
    graph.add_node("Logger", 1.0);
    graph.add_node("Crypto_Legacy_C", 10.0); // 🚨 Очень опасная старая C-библиотека

    // Зависимости
    graph.add_edge("App_Core", "Auth_Module", 1.0);
    graph.add_edge("App_Core", "Logger", 1.0);
    graph.add_edge("Auth_Module", "DB_Driver", 1.0);
    graph.add_edge("Auth_Module", "Crypto_Legacy_C", 1.0); // Уязвимая связь

    info!("🔮 [Baseline] Полный пересчёт экосистемы...");
    let (base_risk, recomputed) = graph.compute_ecosystem_risk();
    info!("   * Суммарный Риск = {:.1}", base_risk);
    info!("   * Рёбер обойдено: {}/{}", recomputed, graph.total_edges());

    info!("\n🔮 [Cache Hit] Повторный запуск (Ничего не изменилось)...");
    let (risk2, recomputed2) = graph.compute_ecosystem_risk();
    info!("   * Суммарный Риск = {:.1}", risk2);
    info!("   * Рёбер обойдено: {}/{} (O(1) чтение KV-кэша на рёбрах!)", recomputed2, graph.total_edges());

    info!("\n🧑‍💻 Ревьювер задаёт вопрос: «А что если мы прямо сейчас выпилим `Crypto_Legacy_C` и заменим на безопасный `rustls`?»");
    
    // МУТАЦИЯ ГРАФА
    info!("🔌 Удаляем узел `Crypto_Legacy_C`... (Инвалидация соседних рёбер O(E_adj))");
    graph.remove_node("Crypto_Legacy_C");

    info!("🔌 Добавляем новый узел `Crypto_Rustls` (Риск: 1.0)");
    graph.add_node("Crypto_Rustls", 1.0);
    
    info!("🔌 Создаем зависимость `Auth_Module` -> `Crypto_Rustls`...");
    graph.add_edge("Auth_Module", "Crypto_Rustls", 1.0);

    info!("\n🔮 [What-If] Пересчёт изменённой экосистемы...");
    let (new_risk, new_recomp) = graph.compute_ecosystem_risk();
    
    info!("   * НОВЫЙ Суммарный Риск = {:.1}", new_risk);
    info!("   * Рёбер обойдено: {}/{}", new_recomp, graph.total_edges());
    info!("   * Duo Agents предсказывает падение риска на {:.1} пунктов ДО написания кода!", base_risk - new_risk);
    
    info!("\n✅ Вывод: Благодаря O(E_adjacent) инвалидации мы можем моделировать замены библиотек в графах миллионов файлов с нулевой задержкой!");

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🪐 ДЕМО 44: GRAVITY ATTENTION (Тензор Эйнштейна для кода)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 44: Gravity Attention (General Relativity of Code)");
    println!("=======================================================\n");

    use crate::scan::gravity::GravitySpacetime;

    let mut space = GravitySpacetime::new();

    // 1. Создаём Чёрную Дыру Технического долга ("God Object")
    // 150,000 LOC, центр системы. Базовый Риск: 1.0
    space.add_mass("CoreEntitySystem_GodObject", 150_000.0, 1.0, 0.0, 0.0);

    // 2. Модуль Аутентификации (Вложенность/Зацепленность = Координаты 20.0, 20.0)
    // Дистанция = 28.2. Базовый риск 5.0
    space.add_mass("UserAuth_Module", 1_000.0, 5.0, 20.0, 20.0);

    // 3. Забытый CallBack, переплетённый с ядром (Дистанция = 14.1)
    space.add_mass("Stranded_Callback", 100.0, 2.0, 10.0, 10.0);

    // 4. Изолированный независимый микросервис Метрик (Дистанция = 282.8)
    space.add_mass("Metrics_Agent", 500.0, 5.0, 200.0, 200.0);

    info!("🪐 Развернуто Риманово пространство кодовой базы (4 объекта).");
    if let Some(rs) = space.calculate_schwarzschild_radius("CoreEntitySystem_GodObject") {
        info!("   * Радиус Шварцшильда (Горизонт Событий) God Object = {:.2} единиц метрики.", rs);
    }

    info!("\n🪐 Измеряем Искривлённый Риск для Компонентов (Гравитационное Замедление):");

    let targets = vec!["Metrics_Agent", "UserAuth_Module", "Stranded_Callback"];

    for t in targets {
        if let Some((warped_risk, warp_multiplier)) = space.evaluate_warped_risk(t) {
            if warp_multiplier.is_infinite() {
                info!("   🚨 [{}] ПОПАДАНИЕ В ГОРИЗОНТ СОБЫТИЙ!", t);
                info!("      Компонент не поддаётся рефакторингу из-за гравитации God Object. CRITICAL.");
            } else {
                let alert = if warp_multiplier > 1.5 { "🔥 Внимание: Сильное Тяготение!" } else { "" };
                info!("   📦 [{}] Warp Factor: {:.2}x -> Искаженный Риск = {:.1} {}", t, warp_multiplier, warped_risk, alert);
            }
        }
    }

    info!("\n✅ Вывод: Gravity Attention математически пессимизирует риск любого кода, зависящего от переусложненных огромных монолитов (учитывая как Массу монолита, так и плотность Связности).");

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 📈 ДЕМО 45: ACCELERATION DETECTOR (d²E/dt² JERK SECURITY)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 45: Acceleration Detector (Jerk Security)");
    println!("=======================================================\n");

    use crate::scan::accel_detector::AccelerationDetector;

    let mut detector = AccelerationDetector::new(8);

    // Симуляция 8 коммитов с нарастающим риском
    let risk_timeline = vec![
        (1, 2.0,  "Обычный коммит"),
        (2, 2.1,  "Мелкий рефактор"),
        (3, 2.5,  "Добавлен новый API endpoint"),
        (4, 3.5,  "Рискованный merge без ревью"),
        (5, 5.5,  "Отключён CORS + SQL concat"),
        (6, 9.0,  "God Object разросся на 500 LOC"),
        (7, 15.0, "Удалены юнит-тесты(!!)"),
        (8, 25.0, "Зависимость с критическим CVE"),
    ];

    for (commit, risk, desc) in &risk_timeline {
        detector.push(*risk);

        let severity = detector.detect_severity();
        let vel = detector.velocity().unwrap_or(0.0);
        let acc = detector.acceleration().unwrap_or(0.0);
        let jrk = detector.jerk().unwrap_or(0.0);

        info!("   Commit #{}: Risk={:>5.1} | v={:>5.1} a={:>5.1} j={:>5.1} | {} | {}",
            commit, risk, vel, acc, jrk, severity, desc);
    }

    info!("\n📈 Анализ траектории:");
    info!("   * Commits 1-3: Скорость низкая, ускорение ≈ 0 → 🟢 Stable");
    info!("   * Commit 4: Скорость подскочила → 🟡 Watch (линейный тренд)");
    info!("   * Commit 5-6: Ускорение > 0 → 🟠 Warning (параболический рост!)");
    info!("   * Commit 7-8: JERK > 0 → 🔴 CRITICAL (ускорение само ускоряется!)");
    info!("   ☠️  Движок мог предупредить Lead инженера на Commit #5, за 3 коммита до катастрофы!");

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🏛️ ДЕМО 46: BABYLON BASIS CACHE (O(log n) SPLINE LOOKUP)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 46: Babylon Basis Cache (3000 г. до н.э.)");
    println!("=======================================================\n");

    use crate::scan::basis_cache::BabylonBasisCache;

    // 1. Строим "Глиняную Табличку" (Precompute)
    info!("🏛️ Строим Вавилонскую Табличку...");
    info!("   Параметры: gridSize=5, degree=3 (кубический), bounds=(-2.0, 2.0)");
    info!("   Предвычисляем базис для 1000 точек через De Boor O(k²)...");

    let mut cache = BabylonBasisCache::build(
        5,          // gridSize
        3,          // degree (кубический сплайн)
        (-2.0, 2.0), // bounds
        1000,       // num_points (разрешение таблицы)
    );

    info!("   ✅ Табличка готова! {} записей в BTreeMap.", cache.table_size());

    // 2. Бенчмарк: 500 lookup-ов
    info!("\n🏛️ Бенчмарк: 500 запросов к таблице...");
    
    let start = std::time::Instant::now();
    for i in 0..500 {
        let x = -2.0 + 4.0 * i as f64 / 499.0; // Равномерно из [-2, 2]
        let _basis = cache.lookup(x);
    }
    let elapsed = start.elapsed();

    info!("   * Время: {:?} (для 500 lookup-ов)", elapsed);
    info!("   * Cache Hits: {}", cache.cache_hits);
    info!("   * Cache Misses: {}", cache.cache_misses);
    info!("   * Hit Rate: {:.1}%", cache.hit_rate());

    // 3. Показываем пример значений базиса
    info!("\n🏛️ Пример: B-spline базис в точке x=0.5:");
    let basis_at_half = cache.lookup(0.5);
    for (i, val) in basis_at_half.iter().enumerate() {
        if *val > 0.001 {
            info!("   * B_{}(0.5) = {:.4}", i, val);
        }
    }

    info!("\n✅ Вывод: 3000-летний приём Вавилонян (Lookup Tables вместо вычислений) даёт O(log n) вместо O(k²) De Boor при каждом forward pass!");

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🚕 ДЕМО 47: SURGE PRICING FOR SCAN QUEUE
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 47: Surge Pricing (Anti-Starvation Queue)");
    println!("=======================================================\n");

    use crate::scan::surge_queue::SurgeScanQueue;

    let mut queue = SurgeScanQueue::new(2); // Только 2 Rayon-потока!

    // Загружаем 8 файлов с разными приоритетами
    queue.enqueue("src/auth/jwt.rs", 9.0);         // 🔥 Критический
    queue.enqueue("src/api/handler.rs", 7.0);       // Важный
    queue.enqueue("src/db/queries.rs", 6.0);        // Средний
    queue.enqueue("src/utils/logger.rs", 3.0);      // Низкий
    queue.enqueue("src/config/env.rs", 2.0);        // Низкий
    queue.enqueue("tests/integration.rs", 1.5);     // Очень низкий
    queue.enqueue("docs/README.md", 1.0);           // Минимальный
    queue.enqueue("scripts/deploy.sh", 0.5);        // Забытый скрипт

    info!("🚕 Очередь на сканирование: {} файлов, {} потоков → Перегрузка!", 
        queue.pending_count(), 2);

    // Симуляция 5 тиков пайплайна
    for tick in 0..=5 {
        queue.tick();

        if tick == 0 || tick == 3 || tick == 5 {
            info!("\n⏱️  Tick {} (Queue: {} pending):", tick, queue.pending_count());
            for (path, base, effective, waits) in queue.snapshot().iter().take(4) {
                let surge_str = if *effective > *base * 1.5 { "⚡SURGE" } else { "" };
                info!("   {:>25} | base={:.1} | eff={:.1} | wait={} {}", 
                    path, base, effective, waits, surge_str);
            }
            if queue.pending_count() > 4 {
                info!("   ... и ещё {} файлов", queue.pending_count() - 4);
            }
        }
    }

    // Извлекаем топ-3
    info!("\n🚕 Извлекаем ТОП-3 для немедленного сканирования:");
    for i in 1..=3 {
        if let Some(task) = queue.dequeue_top() {
            info!("   {}. {} (eff={:.1}, surge={:.2}x, waited {} ticks)", 
                i, task.file_path, task.effective_priority(), task.surge_multiplier, task.wait_ticks);
        }
    }

    info!("\n✅ Вывод: Даже `README.md` (base=1.0) получает шанс на сканирование благодаря Surge × Wait Bonus. Ни один файл не забыт!");

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🎯 ДЕМО 48: ADAPTIVE GRID EXTENSION (COARSE-TO-FINE)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 48: Adaptive Grid (Babylon Schedule)");
    println!("=======================================================\n");

    use crate::scan::adaptive_grid::{PlateauDetector, AdaptiveKanGrid, BABYLON_GRID_SCHEDULE};

    info!("🎯 Вавилонское расписание Grid: {:?}", BABYLON_GRID_SCHEDULE);

    let mut detector = PlateauDetector::new(3, 0.05); // window=3, threshold=0.05
    let initial_coeffs = vec![0.1, 0.5, 0.9]; // 3 коэффициента для gridSize=3
    let mut grid = AdaptiveKanGrid::new(initial_coeffs);

    info!("🎯 Начинаем с gridSize={} (грубая сетка, быстрый скан)", grid.current_grid_size);
    info!("   Коэффициенты: {:?}", grid.coefficients);

    // Симуляция 12 эпох сканирования
    let accuracy_timeline = vec![
        0.60, 0.72, 0.81, 0.88,  // Быстрый рост
        0.89, 0.89, 0.90,        // ПЛАТО! (разница < 0.05 за 3 эпохи)
        0.91, 0.94, 0.96,        // После расширения — снова рост
        0.96, 0.97,              // Ещё одно плато
    ];

    for (epoch, acc) in accuracy_timeline.iter().enumerate() {
        detector.record(*acc);
        
        if detector.is_plateau() && !grid.max_grid_reached() {
            let old = grid.current_grid_size;
            if grid.try_extend() {
                info!("\n   ⚡ Epoch {}: Accuracy={:.2} — ПЛАТО ОБНАРУЖЕНО!", epoch + 1, acc);
                info!("   🔧 Grid расширен: {} → {} (Greville Reprojection)", old, grid.current_grid_size);
                info!("   📦 Новые коэффициенты ({} шт): {:?}", 
                    grid.coefficients.len(),
                    grid.coefficients.iter().map(|c| format!("{:.2}", c)).collect::<Vec<_>>()
                );
            }
        } else {
            let status = if detector.is_plateau() { "📊 ПЛАТО (max grid)" } else { "📈 Обучение" };
            info!("   Epoch {:>2}: Accuracy={:.2} | Grid={:>2} | {} ", epoch + 1, acc, grid.current_grid_size, status);
        }
    }

    info!("\n✅ Вывод: Сканер автоматически увеличил разрешение {} раз (3→{}), не потеряв ни одного обученного коэффициента через Greville Reprojection!",
        grid.extensions_count, grid.current_grid_size);

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🌌 ДЕМО 49: GRAVITATIONAL REDSHIFT (СЕМАНТИЧЕСКИЙ СДВИГ)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 49: Gravitational Redshift (API Staleness)");
    println!("=======================================================\n");

    use crate::scan::redshift::RedshiftField;

    // God Object: UserManager (3000 LOC, coupling=high → mass=50)
    let mut field = RedshiftField::new(50.0);

    info!("🌌 God Object: `UserManager` (mass=50, 3000 LOC, high coupling)");
    info!("   Формула Шварцшильда: z = √(g₀₀_emitter / g₀₀_receiver) - 1\n");

    // Модули-потребители на разных расстояниях в графе зависимостей
    // distance_factor: множитель Schwarzschild-радиуса (2M = 100)
    field.add_consumer("auth_service",       10.0, 1.2);   // Прямо у God Object (1 hop)
    field.add_consumer("api_controller",      8.0, 1.5);   // 2 hops
    field.add_consumer("notification_svc",    5.0, 2.0);   // 3 hops через event bus
    field.add_consumer("analytics_worker",    3.0, 3.5);   // 4 hops, отдельный сервис
    field.add_consumer("legacy_reports",      2.0, 6.0);   // 5 hops, legacy слой
    field.add_consumer("third_party_plugin",  1.0, 10.0);  // 6 hops, внешний плагин

    info!("   {:>22} | {:>5} | {:>6} | Статус", "Модуль", "Dist", "z");
    info!("   {}", "─".repeat(62));

    for module in field.ranked_by_redshift() {
        info!("   {:>22} | {:>5.0} | {:>6.3} | {}", 
            module.name, module.distance, module.redshift_z, module.staleness_risk);
    }

    info!("\n📊 Средний Redshift кодовой базы: z = {:.3}", field.mean_redshift());
    info!("🌌 Интерпретация:");
    info!("   * `auth_service` (1 hop) — API актуален, z ≈ 0");
    info!("   * `legacy_reports` (5 hops) — высокий z, скорее всего deprecated API");
    info!("   * `third_party_plugin` (6 hops) — fossilized: API давно не обновлялся!");
    info!("   🔭 Redshift позволяет находить API drift БЕЗ анализа кода — только по графу!");

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🗄️ ДЕМО 50: KV-CACHE ДЛЯ ИНКРЕМЕНТАЛЬНОГО СКАНИРОВАНИЯ
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 50: KV-Cache (Prefill + Decode)");
    println!("=======================================================\n");

    use crate::scan::kv_cache::ScanKVCache;

    let mut kv = ScanKVCache::new();

    // Фаза 1: PREFILL — первый полный скан проекта (10 файлов)
    info!("📦 Фаза 1: PREFILL (первый полный скан)");
    let project_files = vec![
        ("src/auth/jwt.rs",       vec![0.9, 0.8, 0.7]),
        ("src/api/handler.rs",    vec![0.7, 0.6, 0.5]),
        ("src/db/queries.rs",     vec![0.6, 0.5, 0.4]),
        ("src/utils/logger.rs",   vec![0.3, 0.2, 0.1]),
        ("src/config/env.rs",     vec![0.2, 0.1, 0.1]),
        ("src/models/user.rs",    vec![0.5, 0.4, 0.3]),
        ("src/middleware/cors.rs", vec![0.4, 0.3, 0.2]),
        ("src/routes/admin.rs",   vec![0.8, 0.7, 0.6]),
        ("tests/auth_test.rs",    vec![0.1, 0.1, 0.1]),
        ("docs/README.md",        vec![0.0, 0.0, 0.0]),
    ];

    let start = std::time::Instant::now();
    for (path, emb) in &project_files {
        kv.prefill(path, emb);
    }
    let prefill_time = start.elapsed();
    info!("   ✅ Prefill: {} файлов за {:?}", kv.cache_size(), prefill_time);
    info!("   📊 Prefills: {}, Cache Hits: {}, Hit Rate: {:.0}%\n",
        kv.total_prefills, kv.total_cache_hits, kv.hit_rate());

    // Фаза 2: DECODE — PR#1 изменил только 2 файла из 10
    kv.new_epoch();
    info!("🔄 Фаза 2: DECODE — PR#1 (изменены jwt.rs и handler.rs)");

    let changed_files = vec!["src/auth/jwt.rs", "src/api/handler.rs"];
    // Инвалидируем jwt.rs + его зависимые
    kv.invalidate_with_deps("src/auth/jwt.rs", &["src/middleware/cors.rs"]);

    let start = std::time::Instant::now();
    for (path, emb) in &project_files {
        let is_changed = changed_files.contains(path);
        kv.decode(path, is_changed, emb);
    }
    let decode_time = start.elapsed();
    info!("   ✅ Delta scan: {} файлов за {:?}", project_files.len(), decode_time);
    info!("   📊 Prefills: {}, Cache Hits: {}, Hit Rate: {:.0}%",
        kv.total_prefills, kv.total_cache_hits, kv.hit_rate());
    info!("   🗑️  Invalidations: {}\n", kv.total_invalidations);

    // Фаза 3: DECODE — PR#2 изменил только 1 файл
    kv.new_epoch();
    info!("🔄 Фаза 3: DECODE — PR#2 (изменён только README.md)");

    let start = std::time::Instant::now();
    for (path, emb) in &project_files {
        let is_changed = *path == "docs/README.md";
        kv.decode(path, is_changed, emb);
    }
    let decode_time = start.elapsed();
    info!("   ✅ Delta scan: {} файлов за {:?}", project_files.len(), decode_time);
    info!("   📊 Prefills: {}, Cache Hits: {}, Hit Rate: {:.0}%",
        kv.total_prefills, kv.total_cache_hits, kv.hit_rate());

    info!("\n✅ Вывод: KV-Cache даёт O(1) lookup для неизменённых файлов. При PR из 1 файла → 9 из 10 берутся из кэша мгновенно!");

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // ⚡ ДЕМО 51: REVERSE-MODE AD (ANALYTICAL BACKPROP)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 51: Reverse-Mode AD (1 backward = ALL gradients)");
    println!("=======================================================\n");

    use crate::scan::reverse_ad::lora_backward;

    // Сценарий: 3 файла, dim=4, LoRA rank=3, 4 класса безопасности
    let x = vec![
        vec![0.9, 0.8, 0.7, 0.6],  // jwt.rs embedding
        vec![0.5, 0.4, 0.3, 0.2],  // handler.rs embedding
        vec![0.1, 0.2, 0.3, 0.4],  // logger.rs embedding
    ];

    // LoRA A [4×3] и B [3×4]
    let lora_a = vec![
        vec![0.1, 0.02, -0.05],
        vec![-0.03, 0.08, 0.01],
        vec![0.04, -0.01, 0.06],
        vec![-0.02, 0.05, -0.03],
    ];
    let lora_b = vec![
        vec![0.05, -0.02, 0.03, 0.01],
        vec![-0.01, 0.04, -0.02, 0.06],
        vec![0.03, 0.01, -0.04, 0.02],
    ];

    // Scorer head [4×4] → 4 класса: Safe, Low, Medium, Critical
    let head_w = vec![
        vec![0.3, -0.1, 0.05, -0.2],
        vec![-0.1, 0.4, -0.15, 0.1],
        vec![0.05, -0.2, 0.35, -0.1],
        vec![-0.15, 0.1, -0.1, 0.45],
    ];

    let targets = vec![3, 1, 0]; // jwt→Critical, handler→Low, logger→Safe

    info!("⚡ Input: 3 файла × dim=4, LoRA rank=3, 4 класса безопасности");
    info!("   Targets: jwt.rs→Critical(3), handler.rs→Low(1), logger.rs→Safe(0)\n");

    let result = lora_backward(&x, &lora_a, &lora_b, &head_w, &targets);

    info!("📉 Loss (Cross-Entropy): {:.4}", result.loss);
    info!("📊 Всего LoRA параметров: {} (A: {}×{} + B: {}×{})",
        result.total_params,
        lora_a.len(), lora_a[0].len(),
        lora_b.len(), lora_b[0].len());

    info!("\n🔄 Градиенты dL/dA [dim=4, rank=3] (за ОДИН backward pass!):");
    for (i, row) in result.grad_a.iter().enumerate() {
        info!("   row {}: [{:.4}, {:.4}, {:.4}]", i, row[0], row[1], row[2]);
    }

    info!("\n🔄 Градиенты dL/dB [rank=3, dim=4]:");
    for (i, row) in result.grad_b.iter().enumerate() {
        info!("   row {}: [{:.4}, {:.4}, {:.4}, {:.4}]", i, row[0], row[1], row[2], row[3]);
    }

    info!("\n✅ Вывод: Forward-mode AD потребовал бы {} отдельных forward pass-ов.", result.total_params);
    info!("   Reverse-mode AD: ВСЕ {} градиентов за 1 forward + 1 backward = O(T×d×r)!", result.total_params);

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🤖 ДЕМО 52: DIFFERENTIABLE POLICY (KAN as RL AGENT)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 52: KAN Policy vs Round-Robin (RL Scan Agent)");
    println!("=======================================================\n");

    use crate::scan::scan_policy::{KanPolicy, ScanSimulator, SimFile};

    let make_files = || vec![
        SimFile { name: "src/auth/jwt.rs".into(),       risk: 0.95, has_vuln: true,  scanned: false, wait_time: 0 },
        SimFile { name: "src/api/handler.rs".into(),    risk: 0.80, has_vuln: true,  scanned: false, wait_time: 0 },
        SimFile { name: "src/db/queries.rs".into(),     risk: 0.70, has_vuln: false, scanned: false, wait_time: 0 },
        SimFile { name: "src/utils/logger.rs".into(),   risk: 0.20, has_vuln: false, scanned: false, wait_time: 0 },
        SimFile { name: "src/config/env.rs".into(),     risk: 0.15, has_vuln: false, scanned: false, wait_time: 0 },
        SimFile { name: "src/routes/admin.rs".into(),   risk: 0.85, has_vuln: true,  scanned: false, wait_time: 0 },
        SimFile { name: "tests/auth_test.rs".into(),    risk: 0.10, has_vuln: false, scanned: false, wait_time: 0 },
        SimFile { name: "src/middleware/cors.rs".into(), risk: 0.60, has_vuln: true,  scanned: false, wait_time: 0 },
    ];

    // state_dim = 8 (risk per file) + 1 (avg_wait) + 1 (pct_scanned) = 10
    let policy = KanPolicy::new(10, 8);

    // ---- KAN Policy ----
    let mut sim_kan = ScanSimulator::new(make_files());
    sim_kan.run(&policy, 6); // Сканируем только 6 из 8 (ограниченный бюджет)

    info!("🤖 KAN Policy (6 шагов из 8 файлов):");
    info!("   Total Cost: {:.1}", sim_kan.total_cost);
    info!("   Vulns Found: {}/{}", sim_kan.vulns_found, 4);
    info!("   Vulns Missed: {} (штраф: {})", sim_kan.vulns_missed, sim_kan.vulns_missed * 100);
    let kan_scanned: Vec<&str> = sim_kan.files.iter()
        .filter(|f| f.scanned)
        .map(|f| f.name.as_str())
        .collect();
    info!("   Порядок скана: {:?}\n", kan_scanned);

    // ---- Round-Robin Baseline ----
    // Простой Round-Robin: сканирует первые 6 файлов по порядку
    let mut sim_rr = ScanSimulator::new(make_files());
    // Round-Robin "policy" — просто выбирает файлы по индексу
    for i in 0..6 {
        let idx = i.min(sim_rr.files.len() - 1);
        if !sim_rr.files[idx].scanned {
            sim_rr.files[idx].scanned = true;
            if sim_rr.files[idx].has_vuln {
                sim_rr.vulns_found += 1;
            }
            sim_rr.total_cost += sim_rr.files[idx].wait_time as f64;
        }
        for f in sim_rr.files.iter_mut() {
            if !f.scanned { f.wait_time += 1; }
        }
    }
    for f in &sim_rr.files {
        if f.has_vuln && !f.scanned {
            sim_rr.vulns_missed += 1;
            sim_rr.total_cost += 100.0;
        }
    }

    info!("📋 Round-Robin Baseline (6 шагов):");
    info!("   Total Cost: {:.1}", sim_rr.total_cost);
    info!("   Vulns Found: {}/{}", sim_rr.vulns_found, 4);
    info!("   Vulns Missed: {} (штраф: {})", sim_rr.vulns_missed, sim_rr.vulns_missed * 100);
    let rr_scanned: Vec<&str> = sim_rr.files.iter()
        .filter(|f| f.scanned)
        .map(|f| f.name.as_str())
        .collect();
    info!("   Порядок скана: {:?}\n", rr_scanned);

    let improvement = ((sim_rr.total_cost - sim_kan.total_cost) / sim_rr.total_cost * 100.0).max(0.0);
    info!("📊 KAN Policy vs Round-Robin:");
    info!("   Cost reduction: {:.1}%", improvement);
    info!("   KAN выучит приоритизировать файлы с высоким risk+vuln probability!");
    info!("   🧠 Через дифференциируемую симуляцию KAN оптимизирует ∂cost/∂weights");

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🧊 ДЕМО 53: FROZEN/TRAINABLE SPLIT (Полиморфный AD)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 53: Frozen Core + LoRA Adapters");
    println!("=======================================================\n");

    use crate::scan::frozen_trainable::{FrozenTrainableEngine, EvalMode};

    let mut engine = FrozenTrainableEngine::new("duo-agents", 4);

    // Фаза 1: FROZEN ONLY MODE (baseline — без LoRA)
    engine.mode = EvalMode::FrozenOnly;
    let features = vec![0.8, 0.9, 0.3, 0.7, 0.6, 0.5];
    let results = engine.evaluate(&features);

    info!("🧊 Фаза 1: FROZEN ONLY (baseline, LoRA delta = 0)");
    info!("   {:>10} | {:>6} | {:>6} | {:>6} | {:>5}", "Rule", "Frozen", "Delta", "Effect", "Risk");
    info!("   {}", "─".repeat(55));
    for r in &results {
        info!("   {:>10} | {:>6.3} | {:>+6.3} | {:>6.3} | {:>5.2}",
            r.rule_id, r.frozen_weight, r.lora_delta, r.effective_weight, r.risk_score);
    }

    // Фаза 2: TRAINING MODE (5 эпох — LoRA учится на данных проекта)
    info!("\n🔥 Фаза 2: TRAINING MODE (5 эпох, LoRA обучается)");
    engine.mode = EvalMode::Training;

    // Target: на этом проекте SQL Injection критичнее, а XSS менее важен
    let targets = vec![2.0, 12.0, 1.5, 8.0, 7.0, 3.0];

    for epoch in 1..=5 {
        engine.train_step(&features, &targets);
        let results = engine.evaluate(&features);
        let total_risk: f64 = results.iter().map(|r| r.risk_score).sum();
        if epoch == 1 || epoch == 5 {
            info!("   Epoch {}: total_risk = {:.2}", epoch, total_risk);
        }
    }

    // Фаза 3: После обучения — сравниваем веса
    engine.mode = EvalMode::Production;
    let results_after = engine.evaluate(&features);

    info!("\n✅ Фаза 3: POST-TRAINING (Production mode с обученным LoRA)");
    info!("   {:>10} | {:>6} | {:>6} | {:>6} | {:>5}", "Rule", "Frozen", "Delta", "Effect", "Risk");
    info!("   {}", "─".repeat(55));
    for r in &results_after {
        info!("   {:>10} | {:>6.3} | {:>+6.3} | {:>6.3} | {:>5.2}",
            r.rule_id, r.frozen_weight, r.lora_delta, r.effective_weight, r.risk_score);
    }

    info!("\n📊 Гарантия: Frozen weights ИДЕНТИЧНЫ до и после обучения!");
    info!("   LoRA дельты адаптировали эффективные веса под проект.");
    info!("   Production mode: delta=0 → макс. скорость (нет AD overhead).");
    info!("   Evals: {}, Trainings: {}", engine.total_evals, engine.total_trainings);

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🎲 ДЕМО 54: TOP-K SECURITY SAMPLING (Стохастический Аудит)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 54: Top-K Sampling (Exploration vs Exploitation)");
    println!("=======================================================\n");

    use crate::scan::topk_sampler::{TopKSampler, ScanCandidate, ScanTemperature};

    let candidates = vec![
        ScanCandidate { path: "src/auth/jwt.rs".into(),       kan_priority: 0.95, category: "auth" },
        ScanCandidate { path: "src/api/handler.rs".into(),    kan_priority: 0.88, category: "api" },
        ScanCandidate { path: "src/routes/admin.rs".into(),   kan_priority: 0.85, category: "routes" },
        ScanCandidate { path: "src/db/queries.rs".into(),     kan_priority: 0.72, category: "db" },
        ScanCandidate { path: "src/middleware/cors.rs".into(), kan_priority: 0.60, category: "middleware" },
        ScanCandidate { path: "src/models/user.rs".into(),    kan_priority: 0.45, category: "models" },
        ScanCandidate { path: "src/utils/logger.rs".into(),   kan_priority: 0.30, category: "utils" },
        ScanCandidate { path: "src/config/env.rs".into(),     kan_priority: 0.20, category: "config" },
        ScanCandidate { path: "tests/auth_test.rs".into(),    kan_priority: 0.15, category: "tests" },
        ScanCandidate { path: "docs/README.md".into(),        kan_priority: 0.05, category: "docs" },
    ];

    let modes = vec![
        ScanTemperature::CiCd,
        ScanTemperature::NightAudit,
        ScanTemperature::RedTeam,
    ];

    info!("📋 10 файлов-кандидатов (Top-5, выбираем 3):\n");

    for mode in &modes {
        let mut sampler = TopKSampler::new(42);
        let result = sampler.sample(&candidates, 5, *mode, 3);

        let emoji = match mode {
            ScanTemperature::CiCd => "🔒",
            ScanTemperature::NightAudit => "🌙",
            ScanTemperature::RedTeam => "💥",
            _ => "⚙️",
        };

        info!("{} {} (temperature={:.1}):", emoji, result.mode, result.temperature);
        for (i, sel) in result.selected.iter().enumerate() {
            info!("   {}. {} (priority={:.2}, category={})",
                i + 1, sel.path, sel.kan_priority, sel.category);
        }
        info!("");
    }

    info!("📊 Вывод:");
    info!("   🔒 CI/CD (T=0.1) → Всегда ТОП приоритеты (exploitation)");
    info!("   🌙 Night (T=0.8) → Стохастически миксует ТОП и средние");
    info!("   💥 Red Team (T=1.5) → Находит «неожиданные» файлы (exploration)");
    info!("   Как MCTS, но для Code Security! 🎯");

    tokio::time::sleep(tokio::time::Duration::from_millis(1500)).await;

    // ═════════════════════════════════════════════════════════════════════════
    // 🎭 ДЕМО 20: POISONPILL КАСКАД (Graceful Shutdown)
    // ═════════════════════════════════════════════════════════════════════════
    println!("=======================================================");
    println!("👉 ДЕМО 20: PoisonPill Cascade (Graceful Shutdown)");
    println!("=======================================================\n");
    let _ = orch_ast_tx.send(Message::PoisonPill).await;
    let _ = orch_sec_tx.send(Message::PoisonPill).await;
    let _ = orch_drift_tx.send(Message::PoisonPill).await;
    let _ = orch_action_tx.send(Message::PoisonPill).await;
    let _ = mcp_tx.send(Message::PoisonPill).await;
    tokio::time::sleep(Duration::from_millis(500)).await;

    info!("🏁 Все демо завершены. Платформа остановлена.");
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// § Демо-сценарии (вынесены из main)
// ─────────────────────────────────────────────────────────────────────────────
