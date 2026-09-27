//! # 🧪 Интеграционные тесты Infrastructure
//!
//! Тесты для 3 модулей инфраструктуры, портированных из Clojure KAN:
//! - **Lazy DAG** — Отложенный планировщик DAG в стиле XLA (DCE, дедупликация, fusion)
//! - **Ring Reduce** — Кольцевая агрегация находок безопасности
//! - **Streaming** — 3-стадийный асинхронный конвейер (Producer→Window→Consumer)

// ─────────────────────────────────────────────────────────────────────────────
// § 1. Lazy DAG: Запись → Оптимизация → Исполнение
// ─────────────────────────────────────────────────────────────────────────────
mod lazy_dag_tests {
    use crate::kan_intelligence::infrastructure::lazy_dag::*;

    /// record() должен возвращать монотонно возрастающие ID.
    #[test]
    fn test_dag_record_increments_ids() {
        let mut dag = LazyDag::new();
        let id0 = dag.record(DagOp::AstAnalysis, vec![], vec!["main.rs".into()]);
        let id1 = dag.record(DagOp::SecurityScan, vec![id0], vec!["main.rs".into()]);
        assert_eq!(id0, 0);
        assert_eq!(id1, 1);
    }

    /// stats() возвращает (всего узлов, ожидающих узлов).
    #[test]
    fn test_dag_stats() {
        let mut dag = LazyDag::new();
        dag.record(DagOp::AstAnalysis, vec![], vec![]);
        dag.record(DagOp::SecurityScan, vec![], vec![]);
        let (total, pending) = dag.stats();
        assert_eq!(total, 2);
        assert_eq!(pending, 2); // ни один ещё не выполнен
    }

    /// DCE: узлы, недостижимые из output_ids, должны быть удалены.
    #[test]
    fn test_dead_code_elimination() {
        let mut dag = LazyDag::new();
        let id0 = dag.record(DagOp::AstAnalysis, vec![], vec!["a.rs".into()]);
        let _dead = dag.record(DagOp::CreateJira, vec![], vec![]); // недостижимый
        let id2 = dag.record(DagOp::SecurityScan, vec![id0], vec!["a.rs".into()]);

        dag.eliminate_dead(&[id2]);
        let (total, _) = dag.stats();
        assert_eq!(total, 2, "Мёртвый узел должен быть удалён, осталось 2");
    }

    /// Дедупликация: одинаковые пары (op, files) объединяются.
    #[test]
    fn test_deduplication() {
        let mut dag = LazyDag::new();
        dag.record(DagOp::AstAnalysis, vec![], vec!["main.rs".into()]);
        dag.record(DagOp::AstAnalysis, vec![], vec!["main.rs".into()]); // дубликат
        dag.record(DagOp::SecurityScan, vec![], vec!["main.rs".into()]);

        dag.deduplicate();
        let (total, _) = dag.stats();
        assert_eq!(total, 2, "Дубликат должен быть объединён, получили {}", total);
    }

    /// Топологическая сортировка: зависимости перед зависимыми.
    /// AST(0) → Security(1) → Comment(2)
    #[test]
    fn test_execution_plan_topological_order() {
        let mut dag = LazyDag::new();
        let ast = dag.record(DagOp::AstAnalysis, vec![], vec!["a.rs".into()]);
        let sec = dag.record(DagOp::SecurityScan, vec![ast], vec!["a.rs".into()]);
        let _comment = dag.record(DagOp::PostComment, vec![sec], vec!["a.rs".into()]);

        let plan = dag.execution_plan();
        assert_eq!(plan.len(), 3);
        let ast_pos = plan.iter().position(|&id| id == ast).unwrap();
        let sec_pos = plan.iter().position(|&id| id == sec).unwrap();
        assert!(ast_pos < sec_pos, "AST должен выполняться до Security");
    }

    /// Полная оптимизация: DCE + дедуп + fusion уменьшают количество узлов.
    #[test]
    fn test_full_optimization_pipeline() {
        let mut dag = LazyDag::new();
        let ast = dag.record(DagOp::AstAnalysis, vec![], vec!["main.rs".into()]);
        let _dead1 = dag.record(DagOp::CreateJira, vec![], vec![]); // мёртвый
        let sec = dag.record(DagOp::SecurityScan, vec![ast], vec!["main.rs".into()]);
        let _dup = dag.record(DagOp::AstAnalysis, vec![], vec!["main.rs".into()]); // дубль
        let comment = dag.record(DagOp::PostComment, vec![sec], vec!["main.rs".into()]);

        let (before, _) = dag.stats();
        dag.optimize(&[comment]);
        let (after, _) = dag.stats();
        assert!(after < before, "Оптимизация должна уменьшить узлы: {} → {}", before, after);
    }

    /// Параллельные корни (без взаимных зависимостей) оба до их потребителя.
    #[test]
    fn test_parallel_roots() {
        let mut dag = LazyDag::new();
        let a = dag.record(DagOp::AstAnalysis, vec![], vec!["a.rs".into()]);
        let b = dag.record(DagOp::SecurityScan, vec![], vec!["b.rs".into()]);
        let c = dag.record(DagOp::PostComment, vec![a, b], vec![]);

        let plan = dag.execution_plan();
        let c_pos = plan.iter().position(|&id| id == c).unwrap();
        let a_pos = plan.iter().position(|&id| id == a).unwrap();
        let b_pos = plan.iter().position(|&id| id == b).unwrap();
        assert!(a_pos < c_pos, "a должен быть до c");
        assert!(b_pos < c_pos, "b должен быть до c");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// § 2. Ring Reduce: Кольцевая топология сканеров
// ─────────────────────────────────────────────────────────────────────────────
mod ring_reduce_tests {
    use crate::kan_intelligence::infrastructure::ring_reduce::*;

    /// Пустое кольцо (0 сканеров) → пустой результат.
    #[test]
    fn test_empty_ring_returns_empty() {
        let ring = FindingsRing::new(0);
        assert!(ring.all_reduce().is_empty());
    }

    /// Находки единственного сканера переживают кольцевой проход.
    #[test]
    fn test_single_scanner() {
        let mut ring = FindingsRing::new(1);
        ring.nodes[0].add_finding("SQL", "injection detected", 8);
        let merged = ring.all_reduce();
        assert_eq!(merged.len(), 1);
        assert_eq!(merged[0].category, "SQL");
        assert_eq!(merged[0].severity, 8);
    }

    /// Находки N разных сканеров все появляются в итоговом результате.
    #[test]
    fn test_multi_scanner_aggregation() {
        let mut ring = FindingsRing::new(3);
        ring.nodes[0].add_finding("XSS", "reflected XSS", 6);
        ring.nodes[1].add_finding("SQL", "injection", 8);
        ring.nodes[2].add_finding("CSRF", "no token", 5);
        let merged = ring.all_reduce();
        assert_eq!(merged.len(), 3, "Все 3 уникальные находки должны присутствовать");
    }

    /// Дедупликация: при одинаковой находке от 2 сканеров сохраняется высший severity.
    #[test]
    fn test_deduplication_keeps_highest_severity() {
        let mut ring = FindingsRing::new(2);
        ring.nodes[0].add_finding("SQL", "injection", 5);
        ring.nodes[1].add_finding("SQL", "injection", 9);
        let merged = ring.all_reduce();
        assert_eq!(merged.len(), 1, "Дубликат должен быть объединён");
        assert_eq!(merged[0].severity, 9, "Должен сохраниться высший severity");
    }

    /// Результаты отсортированы по severity по убыванию.
    #[test]
    fn test_results_sorted_by_severity_desc() {
        let mut ring = FindingsRing::new(1);
        ring.nodes[0].add_finding("low", "minor issue", 2);
        ring.nodes[0].add_finding("high", "critical bug", 9);
        ring.nodes[0].add_finding("mid", "medium issue", 5);
        let merged = ring.all_reduce();
        assert_eq!(merged[0].severity, 9);
        assert_eq!(merged[1].severity, 5);
        assert_eq!(merged[2].severity, 2);
    }

    /// Smoke-тест: 5 сканеров, у каждого 1 уникальная находка → все 5 выживают.
    #[test]
    fn test_large_ring_all_findings_survive() {
        let mut ring = FindingsRing::new(5);
        for i in 0..5 {
            ring.nodes[i].add_finding(
                &format!("cat_{}", i), &format!("msg_{}", i), (i + 1) as u8,
            );
        }
        assert_eq!(ring.all_reduce().len(), 5);
    }

    /// Конструктор ScannerNode задаёт id и пустой список находок.
    #[test]
    fn test_scanner_node_creation() {
        let node = ScannerNode::new(42);
        assert_eq!(node.id, 42);
        assert!(node.local_findings.is_empty());
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// § 3. Streaming: Асинхронный 3-стадийный конвейер (Producer→Window→Consumer)
// ─────────────────────────────────────────────────────────────────────────────
mod streaming_tests {
    use crate::kan_intelligence::infrastructure::streaming::*;
    use tokio::sync::mpsc;

    /// 4 тика с window_size=3 → ровно 2 скользящих окна:
    /// [tick0,tick1,tick2] и [tick1,tick2,tick3].
    #[tokio::test]
    async fn test_sliding_window_buffer_basic() {
        let (tick_tx, tick_rx) = mpsc::channel::<MRTick>(10);
        let (window_tx, mut window_rx) = mpsc::channel::<Vec<MRTick>>(10);
        tokio::spawn(sliding_window_buffer(3, tick_rx, window_tx));
        for i in 0..4u64 {
            let _ = tick_tx.send(MRTick {
                mr_id: i, author: "alice".into(),
                changed_files: (i + 1) as usize, timestamp_ms: i * 1000,
            }).await;
        }
        drop(tick_tx);
        let mut windows = Vec::new();
        while let Some(w) = window_rx.recv().await { windows.push(w); }
        assert_eq!(windows.len(), 2, "4 тика / окно=3 → 2 окна");
        assert_eq!(windows[0].len(), 3);
        assert_eq!(windows[1].len(), 3);
    }

    /// Окно, где все 3 MR от одного автора → AnomalyDetected.
    #[tokio::test]
    async fn test_analytics_detects_anomaly() {
        let (window_tx, window_rx) = mpsc::channel::<Vec<MRTick>>(10);
        let (signal_tx, mut signal_rx) = mpsc::channel::<AnalyticsSignal>(10);
        tokio::spawn(analytics_consumer(window_rx, signal_tx));
        let burst = vec![
            MRTick { mr_id: 1, author: "hacker".into(), changed_files: 50, timestamp_ms: 0 },
            MRTick { mr_id: 2, author: "hacker".into(), changed_files: 60, timestamp_ms: 0 },
            MRTick { mr_id: 3, author: "hacker".into(), changed_files: 70, timestamp_ms: 0 },
        ];
        let _ = window_tx.send(burst).await;
        drop(window_tx);
        let signal = signal_rx.recv().await.unwrap();
        assert!(
            matches!(signal, AnalyticsSignal::AnomalyDetected { .. }),
            "Пакет от одного автора должен вызвать аномалию"
        );
    }

    /// Окно с разными авторами → Normal сигнал.
    #[tokio::test]
    async fn test_analytics_normal_window() {
        let (window_tx, window_rx) = mpsc::channel::<Vec<MRTick>>(10);
        let (signal_tx, mut signal_rx) = mpsc::channel::<AnalyticsSignal>(10);
        tokio::spawn(analytics_consumer(window_rx, signal_tx));
        let normal = vec![
            MRTick { mr_id: 1, author: "alice".into(), changed_files: 3, timestamp_ms: 0 },
            MRTick { mr_id: 2, author: "bob".into(), changed_files: 5, timestamp_ms: 0 },
            MRTick { mr_id: 3, author: "carol".into(), changed_files: 2, timestamp_ms: 0 },
        ];
        let _ = window_tx.send(normal).await;
        drop(window_tx);
        let signal = signal_rx.recv().await.unwrap();
        assert!(matches!(signal, AnalyticsSignal::Normal { .. }), "Разные авторы → Normal");
    }

    /// Полный E2E: Producer→Window→Consumer → минимум один сигнал.
    #[tokio::test]
    async fn test_full_pipeline_end_to_end() {
        let (tick_tx, tick_rx) = mpsc::channel::<MRTick>(10);
        let (window_tx, window_rx) = mpsc::channel::<Vec<MRTick>>(10);
        let (signal_tx, mut signal_rx) = mpsc::channel::<AnalyticsSignal>(10);
        // Запуск всех 3 стадий конвейера
        tokio::spawn(start_mr_feed(vec![
            MRTick { mr_id: 1, author: "alice".into(), changed_files: 3, timestamp_ms: 0 },
            MRTick { mr_id: 2, author: "alice".into(), changed_files: 7, timestamp_ms: 0 },
            MRTick { mr_id: 3, author: "alice".into(), changed_files: 2, timestamp_ms: 0 },
            MRTick { mr_id: 4, author: "bob".into(), changed_files: 15, timestamp_ms: 0 },
        ], tick_tx, 10));
        tokio::spawn(sliding_window_buffer(3, tick_rx, window_tx));
        tokio::spawn(analytics_consumer(window_rx, signal_tx));
        let mut signals = Vec::new();
        while let Some(s) = signal_rx.recv().await { signals.push(s); }
        assert!(!signals.is_empty(), "Конвейер должен произвести минимум один сигнал");
    }

    /// MRTick должен поддерживать Clone (необходимо для буферизации окон).
    #[test]
    fn test_mr_tick_clone() {
        let tick = MRTick { mr_id: 42, author: "test".into(), changed_files: 5, timestamp_ms: 1000 };
        let cloned = tick.clone();
        assert_eq!(cloned.mr_id, 42);
        assert_eq!(cloned.author, "test");
    }
}
