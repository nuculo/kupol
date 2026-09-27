//! # 🧪 Набор интеграционных тестов KAN Intelligence
//!
//! Комплексные тесты всех 12 модулей Clojure KAN, портированных в Rust.
//! Разбиты по подсистемам в соответствии с иерархией продакшн-модулей:
//!
//! ```text
//! kan_intelligence/
//! ├── ml_core/           → test_ml_core.rs       (24 теста)
//! │   ├── normalizing_flow  — Обратимая оценка плотности
//! │   ├── operator_algebra  — AST-трансформы как алгебраические операторы
//! │   ├── symbolic          — O(1) обнаружение правил (зонд→заморозка→быстрый_проход)
//! │   └── hybrid_state      — UnsafeCell мутабельное ядро → иммутабельная оболочка
//! │
//! ├── security_rules/    → test_security_rules.rs (11 тестов)
//! │   ├── evolution_nas     — Эволюционный NAS по Regex/AST/TaintFlow типам
//! │   └── lora_rules        — Низкоранговая адаптация замороженных правил
//! │
//! ├── pipeline/          → test_pipeline.rs       (19 тестов)
//! │   ├── early_stopping    — Машина состояний аренды с терпением
//! │   ├── lr_finder         — Алгоритм поиска порога Лесли Смита
//! │   └── checkpoint        — Атомарное сохранение/восстановление в JSON
//! │
//! └── infrastructure/    → test_infrastructure.rs (18 тестов)
//!     ├── lazy_dag          — Отложенное исполнение DAG в стиле XLA
//!     ├── ring_reduce       — Оптимальная по пропускной способности агрегация находок
//!     └── streaming         — Асинхронный 3-стадийный конвейер (каналы tokio)
//! ```
//!
//! **Итого: 72 юнит-теста + 8 асинхронных интеграционных = 80 тестов**
//!
//! Запуск: `cargo test kan_intelligence`

mod test_ml_core;
mod test_security_rules;
mod test_pipeline;
mod test_infrastructure;
