//! # 🧪 Интеграционные тесты ML Core
//!
//! Тесты для 4 примитивов машинного обучения, портированных из Clojure KAN:
//!
//! - **Normalizing Flow** (`normalizing_flow.rs`) — Обратимая оценка плотности
//!   для обнаружения аномалий в MR. Тесты проверяют биективность coupling-слоёв,
//!   roundtrip потока (x → z → x), конечность log-вероятности и классификацию аномалий.
//!
//! - **Operator Algebra** (`operator_algebra.rs`) — AST-трансформы как первоклассные
//!   алгебраические операторы. Тесты проверяют все 7 элементарных операторов,
//!   композицию (A∘B), анализ коммутативности [A,B], подобие кода и обучение весов.
//!
//! - **Symbolic Discovery** (`symbolic.rs`) — Автоматическое обнаружение O(1)
//!   замороженных правил из выводов тяжёлого агента. Тесты проверяют жизненный цикл
//!   зонд→заморозка, пороговый гейтинг, совпадение/промах fast_pass и счётчик зондов.
//!
//! - **Hybrid State** (`hybrid_state.rs`) — Мутабельное ядро на UnsafeCell
//!   с заморозкой в иммутабельную аудит-оболочку. Тесты проверяют накопление,
//!   отслеживание severity, семантику заморозки и запись большого объёма (10K).

// ─────────────────────────────────────────────────────────────────────────────
// § 1. Normalizing Flow: Обратимость и обнаружение аномалий
// ─────────────────────────────────────────────────────────────────────────────
mod normalizing_flow_tests {
    use crate::kan_intelligence::ml_core::normalizing_flow::*;

    /// Проверяет основной математический инвариант coupling-слоя:
    /// для любого входа x, inverse(forward(x)) должен точно воспроизвести x.
    /// Это фундамент normalizing flows — без обратимости оценка плотности ломается.
    #[test]
    fn test_coupling_layer_invertibility() {
        let layer = CouplingLayer::new(8, 0);
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 7.0, 8.0];
        let (y, log_det) = layer.forward(&x);
        let x_reconstructed = layer.inverse(&y);

        for (a, b) in x.iter().zip(x_reconstructed.iter()) {
            assert!(
                (a - b).abs() < 1e-10,
                "Нарушение обратимости: {} vs {} (Δ={})",
                a, b, (a - b).abs()
            );
        }
        assert!(log_det.is_finite(), "log_det должен быть конечным, получили {}", log_det);
    }

    /// Сквозной roundtrip через 4-слойный поток.
    /// Проверяет, что композиция нескольких coupling-слоёв сохраняет обратимость.
    #[test]
    fn test_flow_forward_inverse_roundtrip() {
        let flow = NormalizingFlow::new(8, 4);
        let x = vec![0.5, -0.3, 1.2, -0.8, 0.1, 0.7, -0.5, 0.9];
        let (z, log_det) = flow.forward(&x);
        let x_back = flow.inverse(&z);

        let max_err: f64 = x.iter().zip(x_back.iter())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f64, f64::max);

        assert!(
            max_err < 1e-10,
            "Ошибка roundtrip потока слишком велика: {:.2e}",
            max_err
        );
        assert!(log_det.is_finite());
    }

    /// log p(x) всегда должен быть конечным числом (не NaN и не ±Inf).
    /// Бесконечные log-вероятности обрушат детектор аномалий.
    #[test]
    fn test_flow_log_prob_returns_finite() {
        let flow = NormalizingFlow::new(8, 4);
        let x = vec![0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5];
        let log_p = flow.log_prob(&x);
        assert!(log_p.is_finite(), "log_prob должен быть конечным, получили {}", log_p);
    }

    /// MRFeatures должен иметь ровно 8 измерений (соответствует потоку).
    #[test]
    fn test_mr_features_dimension() {
        assert_eq!(MRFeatures::dim(), 8);
        let mr = MRFeatures {
            files_changed: 5.0, lines_added: 100.0, lines_deleted: 20.0,
            author_tenure_days: 365.0, test_coverage_pct: 80.0,
            review_approvals: 2.0, is_dependency_update: 0.0, commit_count: 3.0,
        };
        assert_eq!(mr.to_vec().len(), 8);
    }

    /// «Нормальный» MR должен классифицироваться как Normal при очень низких порогах.
    #[test]
    fn test_anomaly_classification_normal_mr() {
        let flow = NormalizingFlow::new(8, 4);
        let mr = MRFeatures {
            files_changed: 5.0, lines_added: 100.0, lines_deleted: 20.0,
            author_tenure_days: 365.0, test_coverage_pct: 80.0,
            review_approvals: 2.0, is_dependency_update: 0.0, commit_count: 3.0,
        };
        let verdict = classify_mr(&flow, &mr, -1e10, -1e20);
        assert!(matches!(verdict, AnomalyVerdict::Normal { .. }));
    }

    /// MR, похожий на supply-chain-атаку, должен классифицироваться как Critical
    /// при очень высоких порогах (каждый MR проваливает проверку).
    #[test]
    fn test_anomaly_classification_critical_mr() {
        let flow = NormalizingFlow::new(8, 4);
        let mr = MRFeatures {
            files_changed: 150.0, lines_added: 5000.0, lines_deleted: 2.0,
            author_tenure_days: 2.0, test_coverage_pct: 0.0,
            review_approvals: 0.0, is_dependency_update: 1.0, commit_count: 1.0,
        };
        let verdict = classify_mr(&flow, &mr, 1e10, 1e10);
        assert!(matches!(verdict, AnomalyVerdict::Critical { .. }));
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// § 2. Operator Algebra: AST-трансформы как первоклассные операторы
// ─────────────────────────────────────────────────────────────────────────────
mod operator_algebra_tests {
    use crate::kan_intelligence::ml_core::operator_algebra::*;

    /// Вспомогательная функция: создаёт минимальный фрагмент кода с 2 функциями.
    fn sample_code() -> CodeFragment {
        CodeFragment {
            functions: vec![
                FnDef { name: "process".into(), body: vec!["let x = read()".into()], is_public: false },
                FnDef { name: "query".into(), body: vec!["db.exec(sql)".into()], is_public: false },
            ],
        }
    }

    /// Оператор Identity должен возвращать вход без изменений.
    #[test]
    fn test_identity_operator() {
        let code = sample_code();
        assert_eq!(id_op().apply(&code), code, "Id должен быть тождественным");
    }

    /// Rename добавляет суффикс "_v2" к имени каждой функции.
    #[test]
    fn test_rename_operator() {
        let renamed = rename_op().apply(&sample_code());
        assert_eq!(renamed.functions[0].name, "process_v2");
        assert_eq!(renamed.functions[1].name, "query_v2");
    }

    /// Uppercase переводит имена функций в верхний регистр.
    #[test]
    fn test_uppercase_operator() {
        let upper = uppercase_op().apply(&sample_code());
        assert_eq!(upper.functions[0].name, "PROCESS");
        assert_eq!(upper.functions[1].name, "QUERY");
    }

    /// Prefix добавляет "safe_" в начало имён функций.
    #[test]
    fn test_prefix_operator() {
        let prefixed = prefix_op().apply(&sample_code());
        assert_eq!(prefixed.functions[0].name, "safe_process");
    }

    /// Extract делает все функции публичными.
    #[test]
    fn test_extract_operator_makes_public() {
        let code = sample_code();
        assert!(!code.functions[0].is_public);
        let extracted = extract_op().apply(&code);
        assert!(extracted.functions[0].is_public);
        assert!(extracted.functions[1].is_public);
    }

    /// Inline объединяет все строки тела через "; " в одну строку.
    #[test]
    fn test_inline_operator_merges_body() {
        let code = CodeFragment {
            functions: vec![FnDef {
                name: "multi".into(),
                body: vec!["line1".into(), "line2".into(), "line3".into()],
                is_public: false,
            }],
        };
        let inlined = inline_op().apply(&code);
        assert_eq!(inlined.functions[0].body.len(), 1);
        assert_eq!(inlined.functions[0].body[0], "line1; line2; line3");
    }

    /// Secure оборачивает тело каждой функции защитными комментариями валидации/санитизации.
    #[test]
    fn test_secure_operator_adds_guards() {
        let secured = secure_op().apply(&sample_code());
        let body = &secured.functions[0].body;
        assert!(body[0].contains("SECURITY: validated"));
        assert!(body.last().unwrap().contains("SECURITY: sanitized"));
    }

    /// Композиция: (Upper ∘ Prefix)(x) = Upper(Prefix(x)).
    /// "process" → prefix → "safe_process" → upper → "SAFE_PROCESS"
    #[test]
    fn test_composition_a_then_b() {
        let composed = op_compose(&uppercase_op(), &prefix_op());
        let result = composed.apply(&sample_code());
        assert_eq!(result.functions[0].name, "SAFE_PROCESS");
    }

    /// Identity должен коммутировать с любым оператором: [Id, X] = 0.
    #[test]
    fn test_identity_commutes_with_everything() {
        let code = sample_code();
        let dist = commutator_distance(&id_op(), &rename_op(), &code);
        assert!(dist < 0.01, "Id должен коммутировать с Rename, расстояние {}", dist);
    }

    /// Сходство фрагмента кода с самим собой должно быть ровно 1.0.
    #[test]
    fn test_similarity_identical() {
        let code = sample_code();
        assert!((code.similarity(&code) - 1.0).abs() < 1e-10);
    }

    /// После обучения веса операторов должны быть нормализованы (сумма ≈ 1.0).
    #[test]
    fn test_weight_training_converges() {
        let input = sample_code();
        let target = CodeFragment {
            functions: vec![
                FnDef { name: "safe_process".into(), body: vec!["let x = read()".into()], is_public: true },
                FnDef { name: "safe_query".into(), body: vec!["db.exec(sql)".into()], is_public: true },
            ],
        };
        let ops = vec![id_op(), rename_op(), prefix_op(), extract_op()];
        let weights = train_operator_weights(&ops, &input, &target, 50, 0.5);
        assert_eq!(weights.len(), 4);
        let sum: f64 = weights.iter().sum();
        assert!((sum - 1.0).abs() < 0.01, "Веса должны суммироваться в 1.0, получили {}", sum);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// § 3. Symbolic Discovery: Жизненный цикл зонд → заморозка → быстрый проход
// ─────────────────────────────────────────────────────────────────────────────
mod symbolic_tests {
    use crate::kan_intelligence::ml_core::symbolic::*;

    /// Кандидат НЕ должен замораживаться, если не достиг порогового количества.
    #[test]
    fn test_probe_does_not_freeze_below_threshold() {
        let mut disc = SymbolicDiscoverer::new(5);
        for _ in 0..4 {
            disc.probe("let x = unsafe { ptr.read() };", true);
        }
        assert!(disc.frozen_rules.is_empty(), "Не должен замораживаться до порога (4 < 5)");
    }

    /// Кандидат ОБЯЗАН заморозиться ровно при достижении порогового количества.
    #[test]
    fn test_probe_freezes_at_threshold() {
        let mut disc = SymbolicDiscoverer::new(3);
        for _ in 0..3 {
            disc.probe("let x = unsafe { ptr.read() };", true);
        }
        assert!(!disc.frozen_rules.is_empty(), "Должен заморозиться при пороге=3");
        let has_unsafe = disc.frozen_rules.iter().any(|r| r.formula.contains("unsafe"));
        assert!(has_unsafe, "Замороженные правила должны содержать паттерн 'unsafe {{' ");
    }

    /// Когда тяжёлый агент НЕ помечает код — зондирование пропускается.
    #[test]
    fn test_probe_ignores_when_not_flagged() {
        let mut disc = SymbolicDiscoverer::new(1);
        disc.probe("unsafe { ptr }", false);
        assert!(disc.frozen_rules.is_empty(), "Не должен зондировать, когда агент не пометил");
    }

    /// fast_pass должен находить совпадение замороженного правила в исходном коде.
    #[test]
    fn test_fast_pass_matches_frozen() {
        let mut disc = SymbolicDiscoverer::new(1);
        disc.probe("eval( malicious_code )", true);
        let results = disc.fast_pass("eval( user.input() )");
        assert!(!results.is_empty(), "fast_pass должен найти 'eval(' в коде");
    }

    /// fast_pass должен возвращать пустой результат для безопасного кода.
    #[test]
    fn test_fast_pass_empty_on_no_match() {
        let mut disc = SymbolicDiscoverer::new(1);
        disc.probe("eval(x)", true);
        let results = disc.fast_pass("let x = 42;");
        assert!(results.is_empty(), "fast_pass не должен срабатывать на безопасном коде");
    }

    /// FrozenRule::forward делает простую проверку на подстроку.
    #[test]
    fn test_frozen_rule_forward() {
        let rule = FrozenRule { formula: "TODO".into(), confidence: 1.0 };
        assert!(rule.forward("// TODO: fix this"));
        assert!(!rule.forward("// completed task"));
    }

    /// total_probes должен считать каждый вызов probe, независимо от флага.
    #[test]
    fn test_discoverer_total_probes_count() {
        let mut disc = SymbolicDiscoverer::new(100);
        disc.probe("code1", true);
        disc.probe("code2", false);
        disc.probe("code3", true);
        assert_eq!(disc.total_probes, 3);
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// § 4. Hybrid State: Мутабельное ядро → Иммутабельная аудит-оболочка
// ─────────────────────────────────────────────────────────────────────────────
mod hybrid_state_tests {
    use crate::kan_intelligence::ml_core::hybrid_state::*;

    /// Новое ядро при заморозке должно давать 0 находок и severity "NONE".
    #[test]
    fn test_core_starts_empty() {
        let core = MutablePerformanceCore::new();
        let shell = core.freeze(0);
        assert_eq!(shell.findings_count, 0);
        assert_eq!(shell.max_severity, "NONE");
    }

    /// Множественные вызовы record_finding_fast должны накапливать счётчик
    /// и отслеживать максимальный уровень severity.
    #[test]
    fn test_record_findings_accumulate() {
        let core = MutablePerformanceCore::new();
        core.record_finding_fast(1); // INFO
        core.record_finding_fast(2); // WARN
        core.record_finding_fast(1); // INFO
        let shell = core.freeze(100);
        assert_eq!(shell.findings_count, 3);
        assert_eq!(shell.max_severity, "WARN");
    }

    /// Severity 3+ отображается как "CRITICAL" при заморозке.
    #[test]
    fn test_max_severity_critical() {
        let core = MutablePerformanceCore::new();
        core.record_finding_fast(1);
        core.record_finding_fast(3); // CRITICAL
        core.record_finding_fast(2);
        let shell = core.freeze(200);
        assert_eq!(shell.max_severity, "CRITICAL");
    }

    /// Время scan_time_micros из freeze() должно сохраняться в оболочке.
    #[test]
    fn test_freeze_captures_time() {
        let core = MutablePerformanceCore::new();
        let shell = core.freeze(12345);
        assert_eq!(shell.scan_time_micros, 12345);
    }

    /// ImmutableAuditShell должна поддерживать Clone (необходимо для event sourcing).
    #[test]
    fn test_immutable_shell_is_cloneable() {
        let core = MutablePerformanceCore::new();
        core.record_finding_fast(2);
        let shell = core.freeze(42);
        let cloned = shell.clone();
        assert_eq!(shell.findings_count, cloned.findings_count);
        assert_eq!(shell.max_severity, cloned.max_severity);
    }

    /// Стресс-тест: 10K быстрых записей не должны вызвать панику или повреждение данных.
    /// Валидирует использование UnsafeCell при последовательной нагрузке.
    #[test]
    fn test_high_volume_recording() {
        let core = MutablePerformanceCore::new();
        for _ in 0..10_000 {
            core.record_finding_fast(1);
        }
        let shell = core.freeze(999);
        assert_eq!(shell.findings_count, 10_000);
        assert_eq!(shell.max_severity, "INFO");
    }
}
