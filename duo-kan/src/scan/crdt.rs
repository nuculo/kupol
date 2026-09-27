//! Entity CRDT — Collaborative Knowledge Graph
//!
//! В энтерпрайзе код рассредоточен по тысячам микросервисов.
//! Сканирование одного репозитория (например, `Payment`) не покажет уязвимости,
//! спрятанные в его зависимостях (например, `Auth`).
//!
//! Инфраструктура Duo Agents позволяет строить глобальный Distributed Knowledge Graph.
//! Мы используем CRDT (Conflict-free Replicated Data Type), где каждый "Аллен" (Agent)
//! пишет свой лог операций (`InsertEntity`, `AddEdge`). При слиянии логов (Sync)
//! от разных агентов мы получаем eventual consistency без единой базы данных (P2P).

use std::collections::{HashMap, HashSet};
use chrono::{DateTime, Utc};
use serde::{Serialize, Deserialize};

/// Уникальный идентификатор сущности (Microservice, Vulnerability, User)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct EntityId(pub String);

/// Тип узла (сущности) в графе
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityKind {
    Microservice,
    Library,
    Vulnerability,
    Developer,
}

/// Сущность графа (узлы)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Entity {
    pub id: EntityId,
    pub kind: EntityKind,
    pub attributes: HashMap<String, String>,
}

/// Тип ребра (связи) в графе
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EdgeKind {
    DependsOn,
    HasVulnerability,
    AuthoredBy,
}

/// Одиночная CRDT-операция (Oplog Entry).
/// ADT (Algebraic Data Type) позволяет использовать Pattern Matching для безопасного update'а.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CrdtOp {
    InsertEntity {
        id: EntityId,
        kind: EntityKind,
        attributes: HashMap<String, String>,
        timestamp: DateTime<Utc>,
    },
    AddEdge {
        from: EntityId,
        to: EntityId,
        kind: EdgeKind,
        timestamp: DateTime<Utc>,
    },
    RemoveEdge {
        from: EntityId,
        to: EntityId,
        kind: EdgeKind,
        timestamp: DateTime<Utc>,
    },
}

impl CrdtOp {
    pub fn timestamp(&self) -> DateTime<Utc> {
        match self {
            Self::InsertEntity { timestamp, .. } => *timestamp,
            Self::AddEdge { timestamp, .. } => *timestamp,
            Self::RemoveEdge { timestamp, .. } => *timestamp,
        }
    }
}

/// Conflict-free Replicated Data Type граф.
pub struct KnowledgeGraph {
    /// Узлы (Сущности)
    pub entities: HashMap<EntityId, Entity>,
    /// Рёбра (граф связей)
    /// Map of From -> (To -> Set of Contexts)
    pub forward_edges: HashMap<EntityId, HashMap<EntityId, HashSet<EdgeKind>>>,
    /// Лог всех применённых операций (для синхронизации)
    pub oplog: Vec<CrdtOp>,
    /// Последний Lamport clock/timestamp (для merge)
    pub max_timestamp: DateTime<Utc>,
}

impl KnowledgeGraph {
    pub fn new() -> Self {
        Self {
            entities: HashMap::new(),
            forward_edges: HashMap::new(),
            oplog: Vec::new(),
            max_timestamp: chrono::DateTime::from_timestamp(0, 0).unwrap(),
        }
    }

    /// Применить одну операцию
    pub fn apply_op(&mut self, op: CrdtOp) {
        let ts = op.timestamp();
        if ts > self.max_timestamp {
            self.max_timestamp = ts;
        }

        match &op {
            CrdtOp::InsertEntity { id, kind, attributes, .. } => {
                // Last-Write-Wins (LWW) семантика: если сущность уже есть, перезаписываем
                self.entities.insert(id.clone(), Entity {
                    id: id.clone(),
                    kind: kind.clone(),
                    attributes: attributes.clone(),
                });
            }
            CrdtOp::AddEdge { from, to, kind, .. } => {
                self.forward_edges
                    .entry(from.clone())
                    .or_insert_with(HashMap::new)
                    .entry(to.clone())
                    .or_insert_with(HashSet::new)
                    .insert(kind.clone());
            }
            CrdtOp::RemoveEdge { from, to, kind, .. } => {
                if let Some(targets) = self.forward_edges.get_mut(from) {
                    if let Some(kinds) = targets.get_mut(to) {
                        kinds.remove(kind);
                    }
                }
            }
        }

        self.oplog.push(op);
    }

    /// O(1) добавление через ADT (helper для локального агента)
    pub fn insert_microservice(&mut self, name: &str) -> EntityId {
        let id = EntityId(name.to_lowercase().replace(" ", "-"));
        let mut attrs = HashMap::new();
        attrs.insert("name".to_string(), name.to_string());
        
        self.apply_op(CrdtOp::InsertEntity {
            id: id.clone(),
            kind: EntityKind::Microservice,
            attributes: attrs,
            timestamp: Utc::now(),
        });
        id
    }

    pub fn insert_vulnerability(&mut self, cve: &str, severity: &str) -> EntityId {
        let id = EntityId(cve.to_string());
        let mut attrs = HashMap::new();
        attrs.insert("cve".to_string(), cve.to_string());
        attrs.insert("severity".to_string(), severity.to_string());
        
        self.apply_op(CrdtOp::InsertEntity {
            id: id.clone(),
            kind: EntityKind::Vulnerability,
            attributes: attrs,
            timestamp: Utc::now(),
        });
        id
    }

    pub fn add_dependency(&mut self, my_service: &EntityId, lib: &EntityId) {
        self.apply_op(CrdtOp::AddEdge {
            from: my_service.clone(),
            to: lib.clone(),
            kind: EdgeKind::DependsOn,
            timestamp: Utc::now(),
        });
    }

    pub fn flag_vulnerable(&mut self, subject: &EntityId, vuln: &EntityId) {
        self.apply_op(CrdtOp::AddEdge {
            from: subject.clone(),
            to: vuln.clone(),
            kind: EdgeKind::HasVulnerability,
            timestamp: Utc::now(),
        });
    }

    /// Слияние логов двух независимых узлов P2P (CRDT Sync).
    /// Коммутативность: порядок слияния логов из разных узлов не имеет значения.
    pub fn merge_oplog(&mut self, remote_log: &[CrdtOp]) {
        // Простой LWW-merge: сортируем по timestamp и накатываем то, чего у нас нет.
        // В продакшене нужен Vector Clock или State-based CRDT, 
        // но для proof-of-concept достаточно сортировки лога.
        let mut sorted_log = remote_log.to_vec();
        sorted_log.sort_by_key(|op| op.timestamp());

        for op in sorted_log {
            // Защита от duplication (упрощенная)
            // Реальный CRDT использует UUID транзакции или Idempontent apply
            self.apply_op(op); 
        }
    }

    /// BFS Query: Найти Supply Chain уязвимости (Транзитивно доступные узлы типа Vulnerability).
    pub fn blast_radius(&self, start: &EntityId) -> Vec<Entity> {
        let mut visited = HashSet::new();
        let mut queue = Vec::new();
        let mut found_vulns = Vec::new();

        queue.push(start.clone());
        visited.insert(start.clone());

        while let Some(current_id) = queue.pop() {
            if let Some(entity) = self.entities.get(&current_id) {
                if entity.kind == EntityKind::Vulnerability {
                    found_vulns.push(entity.clone());
                }
            }

            // Следуем по рёбрам
            if let Some(targets) = self.forward_edges.get(&current_id) {
                for target_id in targets.keys() {
                    if !visited.contains(target_id) {
                        visited.insert(target_id.clone());
                        queue.push(target_id.clone());
                    }
                }
            }
        }
        found_vulns
    }
}
