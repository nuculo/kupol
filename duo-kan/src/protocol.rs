//! # 📨 YDB-протоколы и сообщения
//!
//! Все типы сообщений, состояний и протоколов, вдохновлённые YDB:
//!
//! - **GraphDelta** — Атомарное изменение графа (добавление/удаление ребра)
//! - **ActorLifecycle** — Жизненный цикл актора (Active → Draining → Expired → Removed)
//! - **Message** — Единый enum всех сообщений в системе
//! - **DynBitMap** — Аллокатор O(1) из YDB node_broker_impl.h
//! - **NodeInfo / StateData** — Состояние зарегистрированных узлов
//! - **NodeDelta** — Дельта для подписчиков  

use std::collections::HashMap;
use std::time::Instant;

use crate::models::{AstPatch, Entity, EntityGraph, MergeRequestEvent};

// ─────────────────────────────────────────────────────────────────────────────
// § 1. Дельта-протокол графа
// ─────────────────────────────────────────────────────────────────────────────

/// Атомарное изменение графа сущностей.
/// Используется для инкрементальной передачи изменений между акторами.
#[derive(Debug, Clone)]
pub enum GraphDelta {
    /// Добавлено ребро: (источник, цель)
    EdgeAdded(String, String),
    /// Удалено ребро: (источник, цель)
    EdgeRemoved(String, String),
}

// ─────────────────────────────────────────────────────────────────────────────
// § 2. Жизненный цикл актора (из YDB ENodeState, node_broker_impl.h:42)
// ─────────────────────────────────────────────────────────────────────────────

/// Состояния жизненного цикла актора, вдохновлённые YDB ENodeState.
#[derive(Debug, Clone, PartialEq)]
pub enum ActorLifecycle {
    /// Нормальная работа
    Active,
    /// Получен PoisonPill, завершаем последнее сообщение
    Draining,
    /// Истёк таймаут аренды (lease)
    Expired,
    /// Полностью очищен
    Removed,
}

// ─────────────────────────────────────────────────────────────────────────────
// § 3. Единый enum сообщений системы
// ─────────────────────────────────────────────────────────────────────────────

/// Все типы сообщений, циркулирующих между акторами платформы.
#[derive(Debug, Clone)]
pub enum Message {
    // --- Триггеры ---
    /// Новый Merge Request от GitLab webhook
    MergeRequestCreated { mr: MergeRequestEvent },

    // --- Рабочие запросы ---
    /// Запрос на AST-анализ файлов MR
    AnalyzeAST { mr_id: u64, files: Vec<String> },
    /// Запрос на сканирование безопасности
    ScanSecurity { mr_id: u64, files: Vec<String> },
    /// Запрос на проверку архитектурного дрифта
    CheckDrift { mr_id: u64, title: String, deltas: Vec<GraphDelta> },

    // --- Результаты EntityGraph ---
    /// Граф сущностей построен и готов к анализу
    GraphReady { mr_id: u64, files: Vec<String>, graph: Box<EntityGraph> },

    // --- Результаты (дельта-протокол) ---
    /// AST-дельта: изменения в графе
    AstDelta { mr_id: u64, title: String, deltas: Vec<GraphDelta> },
    /// Обнаружен архитектурный дрифт (нарушения правил + семантический анализ)
    DriftDetected { mr_id: u64, violations: Vec<(String, String)>, semantic_drift: Option<String>, plantuml: String },
    /// Дрифт не обнаружен — MR прошёл проверку
    DriftPassed { mr_id: u64 },
    /// Найдены уязвимости безопасности
    SecurityVulnFound { mr_id: u64, vulns: Vec<String>, patches: Vec<String>, plantuml: String },
    /// Сканирование безопасности пройдено
    SecurityScanPassed { mr_id: u64 },
    /// Запрос на публикацию комментария в MR
    PostReviewComment { mr_id: u64, payload: String },
    /// AST-фикс готов к применению
    FixReady { file_path: String, diff: String },

    // --- Swarm Fan-in и AST-патчи ---
    /// Отчёт безопасности от Swarm
    SecurityReport { mr_id: u64, issues: Vec<String>, plantuml: String },
    /// Патч от AstFixAgent
    FixPatch { mr_id: u64, patch: AstPatch },
    /// Отчёт код-ревью
    ReviewReport { mr_id: u64, comments: Vec<String> },
    /// Агрегированный результат всех Swarm-агентов
    AggregatedResult {
        mr_id: u64,
        patch: Option<AstPatch>,
        issues: Vec<String>,
        comments: Vec<String>,
        failed_entities: Vec<Entity>,
        plantuml: String,
    },

    // --- YDB Lifecycle (из actor_tracker.h + extend_lease.cpp) ---
    /// Heartbeat от актора (подтверждение жизни)
    Heartbeat { actor_name: &'static str },
    /// Запрос на graceful shutdown
    PoisonPill,
    /// Подтверждение получения PoisonPill
    PoisonTaken { actor_name: &'static str },
    /// Истёк lease актора — требуется самовосстановление
    ActorLeaseExpired { actor_name: &'static str, mr_id: u64 },

    // --- YDB Node Broker Protocol (TTxRegisterNode) ---
    /// Запрос на регистрацию нового узла
    RegisterNodeRequest { host: String, port: u16, fixed_node_id: bool },
    /// Проверка статуса всех зарегистрированных узлов
    CheckNodesStatus,
    /// Подписка на дельты изменений узлов
    SubscribeNodes { subscriber_name: &'static str },
    /// Обновление состояния узла
    UpdateNodeState { node_id: u32, state: ActorLifecycle },

    // --- MCP Bridge (Anthropic Model Context Protocol) ---
    /// Вызов инструмента через MCP JSON-RPC 2.0
    ExecuteMCPTool { tool_name: String, args: serde_json::Value },
}

// ─────────────────────────────────────────────────────────────────────────────
// § 4. DynBitMap — Аллокатор ID O(1) из YDB (TDynBitMap, node_broker_impl.h)
// ─────────────────────────────────────────────────────────────────────────────

/// Битовая карта для быстрого выделения/освобождения ID узлов.
/// 1 = свободен, 0 = занят. Выделение — O(1) через trailing_zeros.
pub struct DynBitMap {
    bits: Vec<u64>,
}

impl DynBitMap {
    /// Создать битовую карту с заданной ёмкостью (все биты = свободны).
    pub fn new(capacity: usize) -> Self {
        let size = (capacity + 63) / 64;
        Self { bits: vec![u64::MAX; size] } // 1 = FREE, 0 = USED
    }

    /// Найти и занять первый свободный бит. Возвращает None если все заняты.
    pub fn first_non_zero_bit(&mut self) -> Option<u32> {
        for (i, block) in self.bits.iter_mut().enumerate() {
            if *block > 0 {
                let bit_idx = block.trailing_zeros();
                *block &= !(1 << bit_idx); // Очистить бит (пометить как занятый)
                return Some((i * 64 + bit_idx as usize) as u32);
            }
        }
        None
    }

    /// Освободить ранее занятый бит (вернуть ID в пул).
    pub fn free_bit(&mut self, bit_idx: u32) {
        let block_idx = (bit_idx / 64) as usize;
        let local_bit = bit_idx % 64;
        self.bits[block_idx] |= 1 << local_bit;
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// § 5. Состояние зарегистрированных узлов (из TDirtyState / TState в YDB)
// ─────────────────────────────────────────────────────────────────────────────

/// Информация о зарегистрированном узле (акторе) в кластере.
#[derive(Debug, Clone)]
pub struct NodeInfo {
    /// Уникальный ID узла, выделенный из DynBitMap
    pub node_id: u32,
    /// Сетевой адрес узла (host:port)
    pub address: String,
    /// Время истечения аренды
    pub expire: Instant,
    /// Текущее состояние жизненного цикла
    pub state: ActorLifecycle,
    /// Версия эпохи при последнем изменении
    pub version: u64,
}

/// Состояние реестра узлов. Используется в 2-фазной схеме Dirty→Committed.
#[derive(Clone)]
pub struct StateData {
    /// Зарегистрированные узлы: NodeID → NodeInfo
    pub nodes: HashMap<u32, NodeInfo>,
    /// Обратный индекс: host:port → NodeID
    pub hosts: HashMap<String, u32>,
    /// Текущая версия эпохи (монотонно возрастающая)
    pub epoch_version: u64,
}

impl StateData {
    /// Создать пустой реестр.
    pub fn new() -> Self {
        Self { nodes: HashMap::new(), hosts: HashMap::new(), epoch_version: 0 }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// § 6. Дельты для подписчиков Node Broker
// ─────────────────────────────────────────────────────────────────────────────

/// Дельта-запись для инкрементальной рассылки подписчикам.
#[derive(Clone, Debug)]
pub enum NodeDelta {
    /// Добавлен новый узел
    NodeAdded(u32, NodeInfo),
    /// Обновлено состояние существующего узла
    NodeUpdated(u32, ActorLifecycle),
}
