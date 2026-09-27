//! # 🌉 MCP Bridge Actor (Anthropic Model Context Protocol)
//!
//! Форматирование JSON-RPC 2.0 запросов для вызова инструментов
//! через MCP Server (например, создание Jira-тикета).

use async_trait::async_trait;
use serde::Serialize;
use tracing::info;

use crate::protocol::*;
use crate::actor::*;

// ─────────────────────────────────────────────────────────────────────────────
// § MCP JSON-RPC 2.0 типы и актор
// ─────────────────────────────────────────────────────────────────────────────

/// Запрос вызова инструмента по протоколу MCP JSON-RPC 2.0.
#[derive(Debug, Serialize)]
pub struct McpCallToolRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: McpCallToolParams,
    pub id: u64,
}

/// Параметры вызова инструмента MCP.
#[derive(Debug, Serialize)]
pub struct McpCallToolParams {
    pub name: String,
    pub arguments: serde_json::Value,
}

/// Актор MCP Bridge: формирует JSON-RPC payload и отправляет в MCP Server.
pub struct MCPBridgeActor {
    lifecycle: ActorLifecycle,
    /// Счётчик ID транзакций (монотонно растущий)
    tx_id_counter: u64,
}

impl MCPBridgeActor {
    pub fn new() -> Self { Self { lifecycle: ActorLifecycle::Active, tx_id_counter: 1 } }
}

#[async_trait]
impl Actor for MCPBridgeActor {
    fn name(&self) -> &'static str { "MCPBridgeActor" }
    fn lifecycle(&self) -> &ActorLifecycle { &self.lifecycle }
    fn set_lifecycle(&mut self, s: ActorLifecycle) { self.lifecycle = s; }

    async fn execute(&mut self, msg: Message) -> TxResult {
        if let Message::ExecuteMCPTool { tool_name, args } = msg {
            info!("🌉 [MCP:Execute] Формирование JSON-RPC 2.0 для инструмента '{}'...", tool_name);
            let req = McpCallToolRequest {
                jsonrpc: "2.0".into(),
                method: "tools/call".into(),
                params: McpCallToolParams { name: tool_name, arguments: args },
                id: self.tx_id_counter,
            };
            self.tx_id_counter += 1;
            return TxResult::McpPayloadReady { payload: serde_json::to_string(&req).unwrap() };
        }
        TxResult::Ignored
    }

    async fn complete(&mut self, result: TxResult, _ctx: &mut ActorContext) {
        if let TxResult::McpPayloadReady { payload } = result {
            info!("🌉 [MCP:Complete] Payload отправлен в Jira MCP Server:");
            println!("{}\n", payload);
        }
    }
}
