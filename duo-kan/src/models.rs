//! # 📋 Модели сущностей
//!
//! Базовые типы данных, используемые во всей платформе Duo Agent:
//!
//! - **AstPatch** — Набор файловых патчей для автоматического хилинга
//! - **EntityKind / Entity** — Узел графа сущностей (функция, структура, эндпоинт, ...)
//! - **EdgeKind / EntityGraph** — Ориентированный граф зависимостей (вызовы, потоки данных)
//! - **MergeRequestEvent** — Входящее событие от GitLab webhook

use serde::{Deserialize, Serialize};
use petgraph::graph::{Graph, NodeIndex};
use crate::kan::bspline::BSpline;
// § 1. AST-патч: набор файл→содержимое для автоматических правок
// ─────────────────────────────────────────────────────────────────────────────

/// Пакет файловых изменений, генерируемых Swarm-агентами.
/// Каждый элемент `files` — кортеж (путь, новое_содержимое).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AstPatch {
    pub files: Vec<(String, String)>, // (путь, новое_содержимое)
}

// ─────────────────────────────────────────────────────────────────────────────
// § 2. Граф сущностей: узлы и рёбра
// ─────────────────────────────────────────────────────────────────────────────

/// Вид узла в графе сущностей.
/// Используется для маршрутизации DFS-обхода при taint-анализе.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum EntityKind {
    /// Функция или метод
    Function,
    /// Структура / тип данных
    Struct,
    /// Rust-модуль
    Module,
    /// HTTP-эндпоинт (вход в систему)
    Endpoint,
    /// Прямой SQL/DB вызов (потенциально опасный сток)
    DBQuery,
    /// Функция санитизации (прерывает taint-путь)
    Sanitizer,
}

/// Узел графа: сущность исходного кода с типом, именем и файлом.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: u32,
    pub kind: EntityKind,
    pub name: String,
    pub file: String,
}

/// Тип ребра между сущностями.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EdgeKind {
    /// Прямой вызов: A вызывает B
    Calls,
    /// Поток данных: данные из A попадают в B
    DataFlow,
    /// Зависимость: A зависит от B
    DependsOn,
}

/// Умное ребро графа (G-KAN), содержащее обучаемую функцию активации (сплайн).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KanEdge {
    pub kind: EdgeKind,
    pub spline: BSpline,
}

/// Ориентированный граф сущностей исходного кода.
/// Основа для taint-анализа, blast-radius и визуализации PlantUML.
#[derive(Debug, Clone)]
pub struct EntityGraph {
    pub graph: Graph<Entity, KanEdge>,
}

impl EntityGraph {
    /// Создать пустой граф.
    pub fn new() -> Self { Self { graph: Graph::new() } }

    /// Добавить сущность, возвращает индекс узла.
    pub fn add(&mut self, e: Entity) -> NodeIndex { self.graph.add_node(e) }

    /// Соединить два узла умным KAN-ребром.
    pub fn connect(&mut self, a: NodeIndex, b: NodeIndex, k: EdgeKind) {
        let spline = match k {
            EdgeKind::Calls => BSpline::amplifier(),      // Вызовы усиливают риск (взрывной радиус)
            EdgeKind::DataFlow => BSpline::neutral(),     // Поток данных (линейная передача)
            EdgeKind::DependsOn => BSpline::dampener(),   // Зависимости (угасание риска)
        };
        self.graph.add_edge(a, b, KanEdge { kind: k, spline });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// § 3. Событие Merge Request от GitLab webhook
// ─────────────────────────────────────────────────────────────────────────────

/// Событие создания/обновления Merge Request.
/// Приходит от GitLab webhook и запускает DAG-обработку.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergeRequestEvent {
    /// Числовой ID merge request в GitLab
    pub mr_id: u64,
    /// Заголовок MR (используется для семантического анализа дрифта)
    pub title: String,
    /// Автор MR
    pub author: String,
    /// Список изменённых файлов
    pub changed_files: Vec<String>,
}
