//! # 📡 Node Broker Actor (YDB TTxRegisterNode)
//!
//! Управление динамическим пулом акторов-узлов:
//! - Регистрация с O(1) аллокацией ID через DynBitMap
//! - 2-фазная схема Dirty → Committed
//! - Delta Log для инкрементальной рассылки подписчикам

use async_trait::async_trait;
use std::collections::HashMap;
use tokio::time::Duration;
use std::time::Instant;
use tracing::{info, error};

use crate::protocol::*;
use crate::actor::*;

// ─────────────────────────────────────────────────────────────────────────────
// § NodeBrokerActor — регистрация узлов, DynBitMap, Delta Log
// ─────────────────────────────────────────────────────────────────────────────

/// Актор-брокер узлов (вдохновлён TTxRegisterNode из YDB).
/// Управляет регистрацией, обновлением и освобождением ID акторов.
pub struct NodeBrokerActor {
    lifecycle: ActorLifecycle,
    /// Битовая карта свободных ID
    free_ids: DynBitMap,
    /// Dirty-состояние (ещё не подтверждённое)
    dirty: StateData,
    /// Committed-состояние (подтверждённое)
    committed: StateData,
    /// Лог дельт для инкрементальной рассылки
    pub delta_log: Vec<(u64, NodeDelta)>,
    /// Подписчики: имя → последняя отправленная версия
    pub subscribers: HashMap<&'static str, u64>,
}

impl NodeBrokerActor {
    pub fn new() -> Self {
        Self {
            lifecycle: ActorLifecycle::Active,
            free_ids: DynBitMap::new(1024), // Пул из 1024 ID акторов
            dirty: StateData::new(),
            committed: StateData::new(),
            delta_log: Vec::new(),
            subscribers: HashMap::new(),
        }
    }

    /// Разослать дельты всем подписчикам, у которых версия отстаёт.
    fn push_deltas_to_subscribers(&mut self) {
        for (sub, sent_version) in self.subscribers.iter_mut() {
            if *sent_version < self.committed.epoch_version {
                let pending: Vec<&NodeDelta> = self.delta_log
                    .iter()
                    .filter(|(v, _)| *v > *sent_version)
                    .map(|(_, d)| d)
                    .collect();

                info!("   📦 [Broker:Delta] Формирование дельты для '{}' (v{} → v{}): {} изменений.",
                      sub, sent_version, self.committed.epoch_version, pending.len());

                *sent_version = self.committed.epoch_version;
            }
        }
    }
}

#[async_trait]
impl Actor for NodeBrokerActor {
    fn name(&self) -> &'static str { "NodeBrokerActor" }
    fn lifecycle(&self) -> &ActorLifecycle { &self.lifecycle }
    fn set_lifecycle(&mut self, s: ActorLifecycle) { self.lifecycle = s; }

    async fn execute(&mut self, msg: Message) -> TxResult {
        match msg {
            Message::SubscribeNodes { subscriber_name } => {
                info!("   🎧 [Broker:Subscribe] '{}' подписался на v{}", subscriber_name, self.committed.epoch_version);
                self.subscribers.insert(subscriber_name, self.committed.epoch_version);
                TxResult::Ignored
            }
            Message::RegisterNodeRequest { host, port, .. } => {
                let addr = format!("{}:{}", host, port);
                info!("📡 [Broker:Execute] Запрос регистрации от {}...", addr);

                // 1. TTxRegisterNode: выделение ID из битовой карты FreeIds
                let node_id = match self.free_ids.first_non_zero_bit() {
                    Some(id) => id,
                    None => {
                        error!("   ❌ Нет свободных ID узлов!");
                        return TxResult::Ignored;
                    }
                };

                // 2. Мутация только Dirty-состояния
                let version = self.dirty.epoch_version + 1;
                let node_info = NodeInfo {
                    node_id,
                    address: addr.clone(),
                    expire: Instant::now() + Duration::from_secs(3600),
                    state: ActorLifecycle::Active,
                    version,
                };

                info!("   ✅ Выделен NodeID={} (O(1) через DynBitMap). Обновление Dirty до v{}.", node_id, version);
                self.dirty.nodes.insert(node_id, node_info);
                self.dirty.hosts.insert(addr.clone(), node_id);
                self.dirty.epoch_version = version;

                TxResult::RegisterNode { node_id, address: addr, version }
            }
            Message::UpdateNodeState { node_id, state } => {
                let version = self.dirty.epoch_version + 1;
                info!("📡 [Broker:Execute] Обновление NodeID={} до состояния {:?}", node_id, state);
                if let Some(node) = self.dirty.nodes.get_mut(&node_id) {
                    node.state = state.clone();
                } else if let Some(node) = self.committed.nodes.get(&node_id) {
                    let mut new_node = node.clone();
                    new_node.state = state.clone();
                    new_node.version = version;
                    self.dirty.nodes.insert(node_id, new_node);
                }
                self.dirty.epoch_version = version;
                TxResult::UpdateNode { node_id, state, version }
            }
            Message::CheckNodesStatus => {
                info!("📋 [Broker:Status] {} узлов зарегистрировано в Committed.", self.committed.nodes.len());
                TxResult::Ignored
            }
            _ => TxResult::Ignored,
        }
    }

    async fn complete(&mut self, result: TxResult, _ctx: &mut ActorContext) {
        if let TxResult::RegisterNode { node_id, address, version } = result {
            // 3. Фаза Complete: продвижение в Committed
            info!("📡 [Broker:Complete] Продвижение NodeID={} в Committed! Рассылка подписчикам.", node_id);
            let info = self.dirty.nodes[&node_id].clone();

            // Запись в Delta Log
            self.delta_log.push((version, NodeDelta::NodeAdded(node_id, info.clone())));

            self.committed.nodes.insert(node_id, info);
            self.committed.hosts.insert(address, node_id);
            self.committed.epoch_version = version;

            self.push_deltas_to_subscribers();
        } else if let TxResult::UpdateNode { node_id, state, version } = result {
            info!("📡 [Broker:Complete] Продвижение NodeID={} в состояние {:?}! Рассылка подписчикам.", node_id, state);
            if let Some(info) = self.committed.nodes.get_mut(&node_id) {
                info.state = state.clone();
                info.version = version;
                self.delta_log.push((version, NodeDelta::NodeUpdated(node_id, state.clone())));
                self.committed.epoch_version = version;

                if state == ActorLifecycle::Removed {
                    self.free_ids.free_bit(node_id);
                }
                self.push_deltas_to_subscribers();
            }
        }
    }
}
