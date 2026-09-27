//! # 🧪 Интеграционные тесты Pipeline
//!
//! Тесты для 3 модулей конвейера, портированных из Clojure KAN:
//! - **Early Stopping** — Адаптивный монитор аренды (Active→Throttled→Draining→Stopped)
//! - **LR Finder** — Алгоритм поиска порога Лесли Смита
//! - **Checkpoint** — Атомарное сохранение/восстановление состояния в JSON

use std::collections::HashMap;

// ─────────────────────────────────────────────────────────────────────────────
// § 1. Early Stopping: Машина состояний адаптивной аренды
// ─────────────────────────────────────────────────────────────────────────────
mod early_stopping_tests {
    use crate::kan_intelligence::pipeline::early_stopping::*;
    use std::time::Duration;

    /// Новый монитор стартует в Active.
    #[test]
    fn test_initial_state_is_active() {
        let monitor = AdaptiveLeaseMonitor::new("test", 3, Duration::from_secs(30));
        assert_eq!(monitor.state, LeaseState::Active);
    }

    /// Committed сбрасывает счётчик в ноль.
    #[test]
    fn test_committed_resets_counter() {
        let mut monitor = AdaptiveLeaseMonitor::new("test", 3, Duration::from_secs(30));
        monitor.record_result(&TxResult::Ignored);
        monitor.record_result(&TxResult::Ignored);
        assert_eq!(monitor.no_progress_counter, 2);
        monitor.record_result(&TxResult::Committed("found".into()));
        assert_eq!(monitor.no_progress_counter, 0);
    }

    /// patience раз Ignored подряд → Throttled.
    #[test]
    fn test_patience_exhausted_throttles() {
        let mut monitor = AdaptiveLeaseMonitor::new("test", 3, Duration::from_secs(30));
        for _ in 0..3 { monitor.record_result(&TxResult::Ignored); }
        assert_eq!(monitor.state, LeaseState::Throttled);
    }

    /// 2×patience подряд Ignored → Draining.
    #[test]
    fn test_double_patience_drains() {
        let mut monitor = AdaptiveLeaseMonitor::new("test", 3, Duration::from_secs(30));
        for _ in 0..3 { monitor.record_result(&TxResult::Ignored); }
        assert_eq!(monitor.state, LeaseState::Throttled);
        for _ in 0..3 { monitor.record_result(&TxResult::Ignored); }
        assert_eq!(monitor.state, LeaseState::Draining);
    }

    /// 3×patience подряд Ignored → Stopped (терминальное).
    #[test]
    fn test_triple_patience_stops() {
        let mut monitor = AdaptiveLeaseMonitor::new("test", 3, Duration::from_secs(30));
        for _ in 0..9 { monitor.record_result(&TxResult::Ignored); }
        assert_eq!(monitor.state, LeaseState::Stopped);
    }

    /// Critical в Throttled → восстановление в Active.
    #[test]
    fn test_recovery_from_throttled() {
        let mut monitor = AdaptiveLeaseMonitor::new("test", 3, Duration::from_secs(30));
        for _ in 0..3 { monitor.record_result(&TxResult::Ignored); }
        assert_eq!(monitor.state, LeaseState::Throttled);
        monitor.record_result(&TxResult::Critical("important!".into()));
        assert_eq!(monitor.state, LeaseState::Active);
    }

    /// Stopped — терминальное, восстановление невозможно.
    #[test]
    fn test_stopped_state_is_terminal() {
        let mut monitor = AdaptiveLeaseMonitor::new("test", 3, Duration::from_secs(30));
        for _ in 0..9 { monitor.record_result(&TxResult::Ignored); }
        assert_eq!(monitor.state, LeaseState::Stopped);
        monitor.record_result(&TxResult::Committed("late".into()));
        assert_eq!(monitor.state, LeaseState::Stopped);
    }

    /// FleetLeaseMonitor отслеживает нескольких акторов независимо.
    #[test]
    fn test_fleet_monitor_tracks_multiple() {
        let mut fleet = FleetLeaseMonitor::new();
        fleet.register("actor_a", 3, Duration::from_secs(30));
        fleet.register("actor_b", 3, Duration::from_secs(30));
        assert_eq!(fleet.active_count(), 2);
        for _ in 0..9 { fleet.record("actor_b", &TxResult::Ignored); }
        assert_eq!(fleet.active_count(), 1, "actor_b должен быть Stopped");
    }

    /// Продуктивность = committed / (committed + ignored).
    #[test]
    fn test_productivity_metric() {
        let mut monitor = AdaptiveLeaseMonitor::new("test", 10, Duration::from_secs(30));
        monitor.record_result(&TxResult::Committed("a".into()));
        monitor.record_result(&TxResult::Ignored);
        monitor.record_result(&TxResult::Committed("b".into()));
        monitor.record_result(&TxResult::Ignored);
        assert!((monitor.productivity() - 0.5).abs() < 1e-10);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// § 2. LR Finder: Алгоритм поиска порога Лесли Смита
// ─────────────────────────────────────────────────────────────────────────────
mod lr_finder_tests {
    use crate::kan_intelligence::pipeline::lr_finder::*;

    /// Конфигурация по умолчанию: min < max, шаги > 0.
    #[test]
    fn test_finder_config_default() {
        let config = FinderConfig::default();
        assert!(config.min_val > 0.0);
        assert!(config.max_val > config.min_val);
        assert!(config.num_steps > 0);
    }

    /// Sweep с квадратичной функцией потерь → валидный результат.
    #[test]
    fn test_find_optimal_returns_valid_result() {
        let config = FinderConfig {
            min_val: 0.1, max_val: 1.0, num_steps: 20,
            smoothing: 0.9, diverge_factor: 4.0,
        };
        let result = find_optimal_threshold("test", &config, |t| (t - 0.5).powi(2));
        assert!(!result.points.is_empty());
        assert!(result.optimal_threshold > 0.0);
        assert_eq!(result.name, "test");
    }

    /// Монотонно убывающая потеря → крутейший градиент отрицательный.
    #[test]
    fn test_find_optimal_for_monotonic_loss() {
        let config = FinderConfig {
            min_val: 0.01, max_val: 1.0, num_steps: 30,
            smoothing: 0.0, diverge_factor: 100.0,
        };
        let result = find_optimal_threshold("mono", &config, |t| 1.0 / (1.0 + t));
        assert!(result.steepest_gradient <= 0.0);
    }

    /// Без расхождения и сглаживания → ровно num_steps точек.
    #[test]
    fn test_sweep_produces_correct_num_points() {
        let config = FinderConfig {
            min_val: 0.1, max_val: 1.0, num_steps: 10,
            smoothing: 0.0, diverge_factor: 100.0,
        };
        let result = find_optimal_threshold("count", &config, |_| 0.5);
        assert_eq!(result.points.len(), 10);
    }

    /// Потеря взлетает после t>1.0 → ранняя остановка sweep.
    #[test]
    fn test_early_divergence_stops_sweep() {
        let config = FinderConfig {
            min_val: 0.01, max_val: 10.0, num_steps: 50,
            smoothing: 0.0, diverge_factor: 2.0,
        };
        let result = find_optimal_threshold("diverge", &config, |t| {
            if t < 1.0 { 0.1 } else { t * 100.0 }
        });
        assert!(result.points.len() < 50, "Должен остановиться рано, получили {} точек", result.points.len());
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// § 3. Checkpoint: Атомарное сохранение / восстановление / сравнение
// ─────────────────────────────────────────────────────────────────────────────
mod checkpoint_tests {
    use super::*;
    use crate::kan_intelligence::pipeline::checkpoint::*;

    /// Чекпоинт срабатывает на кратных интервалу шагах, не при step=0.
    #[test]
    fn test_should_checkpoint_at_interval() {
        let mgr = CheckpointManager::new("/tmp/test-kan-ckpt-interval", 5);
        assert!(!mgr.should_checkpoint(0));
        assert!(!mgr.should_checkpoint(3));
        assert!(mgr.should_checkpoint(5));
        assert!(mgr.should_checkpoint(10));
        let _ = std::fs::remove_dir_all("/tmp/test-kan-ckpt-interval");
    }

    /// Roundtrip: все поля переживают JSON сериализацию.
    #[test]
    fn test_save_and_restore_roundtrip() {
        let dir = "/tmp/test-kan-ckpt-roundtrip";
        let _ = std::fs::remove_dir_all(dir);
        let mut mgr = CheckpointManager::new(dir, 1);
        let mut dirty: HashMap<u64, String> = HashMap::new();
        dirty.insert(100, "Scanning".into());
        let mut committed: HashMap<u64, String> = HashMap::new();
        committed.insert(99, "Done".into());
        let mut actors: HashMap<String, String> = HashMap::new();
        actors.insert("AST".into(), "Active".into());

        let path = mgr.save_checkpoint(&dirty, &committed, &actors, 42, 7, 3).unwrap();
        assert!(path.exists());
        let restored = CheckpointManager::restore_checkpoint(&path).unwrap();
        assert_eq!(restored.epoch, 1);
        assert_eq!(restored.dirty_state.get(&100).unwrap(), "Scanning");
        assert_eq!(restored.committed_state.get(&99).unwrap(), "Done");
        assert_eq!(restored.actor_states.get("AST").unwrap(), "Active");
        assert_eq!(restored.metrics.total_mrs_processed, 42);
        assert_eq!(restored.metrics.total_vulnerabilities_found, 7);
        assert_eq!(restored.metrics.total_fixes_applied, 3);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// find_latest возвращает файл с наибольшей эпохой.
    #[test]
    fn test_find_latest_checkpoint() {
        let dir = "/tmp/test-kan-ckpt-latest";
        let _ = std::fs::remove_dir_all(dir);
        let mut mgr = CheckpointManager::new(dir, 1);
        let empty_u64: HashMap<u64, String> = HashMap::new();
        let empty_str: HashMap<String, String> = HashMap::new();
        mgr.save_checkpoint(&empty_u64, &empty_u64, &empty_str, 1, 0, 0).unwrap();
        mgr.save_checkpoint(&empty_u64, &empty_u64, &empty_str, 2, 0, 0).unwrap();
        let latest = mgr.find_latest_checkpoint().unwrap();
        assert!(latest.to_string_lossy().contains("epoch_0002"));
        let _ = std::fs::remove_dir_all(dir);
    }

    /// Сравнение с самим собой → exact=true.
    #[test]
    fn test_compare_identical_checkpoints() {
        let ckpt = PipelineCheckpoint {
            epoch: 1, timestamp: "test".into(),
            dirty_state: HashMap::new(), committed_state: HashMap::new(),
            actor_states: HashMap::new(),
            metrics: CheckpointMetrics {
                total_mrs_processed: 0, total_vulnerabilities_found: 0,
                total_fixes_applied: 0, uptime_seconds: 0.0,
            },
        };
        let diff = compare_checkpoints(&ckpt, &ckpt);
        assert!(diff.exact, "Само-сравнение должно быть exact");
    }

    /// Разные dirty_state → НЕ exact.
    #[test]
    fn test_compare_different_checkpoints() {
        let ckpt1 = PipelineCheckpoint {
            epoch: 1, timestamp: "t1".into(),
            dirty_state: HashMap::from([(1, "a".into())]),
            committed_state: HashMap::new(), actor_states: HashMap::new(),
            metrics: CheckpointMetrics {
                total_mrs_processed: 0, total_vulnerabilities_found: 0,
                total_fixes_applied: 0, uptime_seconds: 0.0,
            },
        };
        let ckpt2 = PipelineCheckpoint {
            epoch: 2, timestamp: "t2".into(),
            dirty_state: HashMap::from([(2, "b".into())]),
            committed_state: HashMap::new(), actor_states: HashMap::new(),
            metrics: CheckpointMetrics {
                total_mrs_processed: 0, total_vulnerabilities_found: 0,
                total_fixes_applied: 0, uptime_seconds: 0.0,
            },
        };
        let diff = compare_checkpoints(&ckpt1, &ckpt2);
        assert!(!diff.exact);
        assert!(!diff.dirty_match);
    }
}
