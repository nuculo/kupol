use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::io::{self, BufRead};
use tracing::error;

use crate::scan::run_scan;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    params: Option<Value>,
    id: Option<Value>,
}

#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    result: Option<Value>,
    error: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    id: Option<Value>,
}

fn send_response(id: Option<Value>, result: Option<Value>, error: Option<Value>) {
    let response = JsonRpcResponse {
        jsonrpc: "2.0".to_string(),
        result,
        error,
        id,
    };
    let json_str = serde_json::to_string(&response).unwrap();
    // MCP requires messages separated by newlines on stdout
    println!("{}", json_str);
}

fn send_error(id: Option<Value>, code: i32, message: &str) {
    send_response(
        id,
        None,
        Some(json!({
            "code": code,
            "message": message
        })),
    );
}

pub async fn run_stdio_server() -> Result<()> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let mut line = String::new();

    // Loop indefinitely waiting for JSON-RPC messages on stdin
    while reader.read_line(&mut line).unwrap_or(0) > 0 {
        let input = line.trim();
        if input.is_empty() {
            line.clear();
            continue;
        }

        match serde_json::from_str::<JsonRpcRequest>(input) {
            Ok(req) => handle_request(req).await,
            Err(e) => {
                // Parse error
                error!("Failed to parse JSON-RPC: {}", e);
                send_error(None, -32700, "Parse error");
            }
        }
        line.clear();
    }

    Ok(())
}

async fn handle_request(req: JsonRpcRequest) {
    let id = req.id.clone();
    match req.method.as_str() {
        "initialize" => {
            send_response(
                id,
                Some(json!({
                    "protocolVersion": "2024-11-05",
                    "capabilities": {
                        "tools": {}
                    },
                    "serverInfo": {
                        "name": "duo-agents",
                        "version": "0.1.0"
                    }
                })),
                None,
            );
        }
        "notifications/initialized" => {
            // Acknowledge but silently ignore
        }
        "tools/list" => {
            send_response(
                id,
                Some(json!({
                    "tools": [
                        {
                            "name": "scan_codebase",
                            "description": "Scans the specified local codebase path for security vulnerabilities using the Duo Architecture Guardian engine.",
                            "inputSchema": {
                                "type": "object",
                                "properties": {
                                    "path": {
                                        "type": "string",
                                        "description": "The local filesystem path to the codebase to scan (e.g., './src' or '/absolute/path')."
                                    }
                                },
                                "required": ["path"]
                            }
                        }
                    ]
                })),
                None,
            );
        }
        "tools/call" => {
            if let Some(params) = req.params {
                let name = params.get("name").and_then(|v| v.as_str()).unwrap_or("");
                let arguments = params.get("arguments");

                if name == "scan_codebase" {
                    let path = arguments
                        .and_then(|args| args.get("path"))
                        .and_then(|p| p.as_str())
                        .unwrap_or(".");

                    // Run scan
                    let result = run_scan(path);
                    
                    // Format output
                    let findings_summary = if result.findings.is_empty() {
                        "No vulnerabilities found.".to_string()
                    } else {
                        format!("Found {} vulnerabilities. Risk Score: {}", result.findings.len(), result.summary.risk_score)
                    };

                    send_response(
                        id,
                        Some(json!({
                            "content": [
                                {
                                    "type": "text",
                                    "text": format!("Scan completed for {}.\n\nResults:\n{}\n\nDetailed JSON:\n{}", path, findings_summary, serde_json::to_string_pretty(&result).unwrap_or_default())
                                }
                            ]
                        })),
                        None,
                    );
                } else {
                    send_error(id, -32601, "Tool not found");
                }
            } else {
                send_error(id, -32602, "Invalid params");
            }
        }
        _ => {
            send_error(id, -32601, "Method not found");
        }
    }
}
