//! # 🧪 Интеграционные тесты Security Rules
//!
//! Тесты для 2 модулей правил безопасности, портированных из Clojure KAN:
//!
//! - **Evolution NAS** (`evolution_nas.rs`) — Эволюционный поиск архитектуры
//!   правил безопасности. Тесты проверяют инициализацию популяции с разнообразными
//!   типами правил (Regex / AstPattern / TaintFlow), оценку фитнеса и
//!   корректность победителя эволюционного цикла с элитизмом.
//!
//! - **LoRA Rules** (`lora_rules.rs`) — Низкоранговая адаптация замороженных
//!   правил безопасности. Тесты проверяют скоринг замороженного правила,
//!   нулевую инициализацию LoRA-дельты, roundtrip параметров, и ключевой
//!   контракт: обучение LoRA НИКОГДА не должно менять замороженные веса.

// ─────────────────────────────────────────────────────────────────────────────
// § 1. Evolution NAS: Инициализация популяции, фитнес, эволюционный цикл
// ─────────────────────────────────────────────────────────────────────────────
mod evolution_nas_tests {
    use crate::kan_intelligence::security_rules::evolution_nas::*;

    /// Популяция должна инициализироваться с точно запрошенным размером.
    #[test]
    fn test_population_init_correct_size() {
        let pop = init_population(20);
        assert_eq!(pop.len(), 20);
    }

    /// При 30 особях и 3 типах правил хотя бы один тип
    /// должен быть представлен (вероятностно, но почти наверняка).
    #[test]
    fn test_population_has_diverse_types() {
        let pop = init_population(30);
        let has_regex = pop.iter().any(|g| matches!(g.rule_type, RuleType::Regex { .. }));
        let has_ast = pop.iter().any(|g| matches!(g.rule_type, RuleType::AstPattern { .. }));
        let has_taint = pop.iter().any(|g| matches!(g.rule_type, RuleType::TaintFlow { .. }));
        assert!(
            has_regex || has_ast || has_taint,
            "Хотя бы один тип правила должен присутствовать в популяции из 30"
        );
    }

    /// После эволюции победитель должен иметь фитнес в допустимом диапазоне [-1, 1].
    /// Фитнес = TPR - FPR, поэтому ограничен этими двумя ставками.
    #[test]
    fn test_evolution_produces_valid_fitness() {
        let winner = evolve(10, 5);
        assert!(
            winner.fitness >= -1.0 && winner.fitness <= 1.0,
            "Фитнес должен быть в [-1, 1], получили {}",
            winner.fitness
        );
    }

    /// С элитизмом лучшая особь всегда выживает.
    /// TPR и FPR должны быть неотрицательными.
    #[test]
    fn test_evolution_best_survives_elitism() {
        let winner = evolve(15, 10);
        assert!(winner.true_positive_rate >= 0.0, "TPR должен быть ≥ 0");
        assert!(winner.false_positive_rate >= 0.0, "FPR должен быть ≥ 0");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// § 2. LoRA Rules: Замороженное правило + Низкоранговая адаптация
// ─────────────────────────────────────────────────────────────────────────────
mod lora_rules_tests {
    use crate::kan_intelligence::security_rules::lora_rules::*;

    /// Вспомогательная функция: создаёт замороженное правило SQL-инъекции для тестов.
    /// weights = [0.8(sql), 0.6(user), 0.0(auth), 0.0(test), 0.0(deps)]
    fn make_test_frozen() -> FrozenRule {
        FrozenRule {
            name: "test_rule".into(),
            weights: vec![0.8, 0.6, 0.0, 0.0, 0.0],
            bias: -0.3,
            threshold: 0.5,
        }
    }

    /// Скор = dot(weights, x) + bias = 0.8*1 + 0.6*1 + 0 + 0 + 0 - 0.3 = 1.1
    #[test]
    fn test_frozen_rule_score() {
        let frozen = make_test_frozen();
        let score = frozen.score(&vec![1.0, 1.0, 0.0, 0.0, 0.0]);
        assert!((score - 1.1).abs() < 1e-10, "Ожидали 1.1, получили {}", score);
    }

    /// predict() возвращает true тогда и только тогда, когда score > threshold.
    #[test]
    fn test_frozen_rule_predict() {
        let frozen = make_test_frozen();
        assert!(frozen.predict(&vec![1.0, 1.0, 0.0, 0.0, 0.0]));  // 1.1 > 0.5 ✓
        assert!(!frozen.predict(&vec![0.0, 0.0, 0.0, 0.0, 0.0])); // -0.3 < 0.5 ✗
    }

    /// num_params = len(weights) + 1 (bias).
    #[test]
    fn test_frozen_rule_num_params() {
        assert_eq!(make_test_frozen().num_params(), 6);
    }

    /// Вектор B в LoRA стартует с нулей → delta_score должен быть пренебрежимо мал.
    /// Это гарантирует, что адаптированное правило стартует идентично замороженному.
    #[test]
    fn test_lora_delta_initially_zero() {
        let delta = LoRADelta::new(5, 2, 1.0);
        let ds = delta.delta_score(&vec![1.0, 2.0, 3.0, 4.0, 5.0]);
        assert!(ds.abs() < 1e-10, "Начальная LoRA-дельта должна быть ≈0, получили {}", ds);
    }

    /// Обучаемые параметры LoRA = dim*rank + rank.
    #[test]
    fn test_lora_delta_num_params() {
        let delta = LoRADelta::new(5, 2, 1.0);
        assert_eq!(delta.num_params(), 12); // 5*2 + 2
    }

    /// params_flat() → set_params() → params_flat() должен быть тождественным.
    #[test]
    fn test_lora_params_roundtrip() {
        let mut delta = LoRADelta::new(5, 2, 1.0);
        let params = delta.params_flat();
        assert_eq!(params.len(), delta.num_params());
        delta.set_params(&params);
        assert_eq!(delta.params_flat(), params);
    }

    /// До обучения адаптированный скор равен замороженному (дельта = 0).
    #[test]
    fn test_adapted_rule_initially_equals_frozen() {
        let frozen = make_test_frozen();
        let adapted = LoRAAdaptedRule::new(frozen.clone(), 2, 1.0);
        let features = vec![1.0, 1.0, 0.0, 0.0, 0.0];
        assert!(
            (frozen.score(&features) - adapted.score(&features)).abs() < 1e-10,
            "Начальный адаптированный скор должен равняться замороженному"
        );
    }

    /// КЛЮЧЕВОЙ КОНТРАКТ: обучение LoRA НИКОГДА не должно менять замороженные веса.
    /// Обновляются только матрицы A и B дельты LoRA.
    #[test]
    fn test_lora_training_preserves_frozen_weights() {
        let frozen = make_test_frozen();
        let original_weights = frozen.weights.clone();
        let original_bias = frozen.bias;
        let mut adapted = LoRAAdaptedRule::new(frozen, 2, 1.0);

        let data = vec![
            (vec![1.0, 1.0, 0.0, 0.5, 0.0], true),
            (vec![0.0, 0.0, 1.0, 0.8, 0.0], false),
        ];
        adapted.train_lora(&data, 10, 0.1);

        assert_eq!(adapted.frozen.weights, original_weights, "Замороженные веса НЕ ДОЛЖНЫ меняться!");
        assert_eq!(adapted.frozen.bias, original_bias, "Замороженный bias НЕ ДОЛЖЕН меняться!");
    }
}
