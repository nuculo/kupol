use anyhow::Result;
use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
    response::IntoResponse,
};
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tokio::sync::broadcast;
use std::sync::{Arc, Mutex};
use tokio::net::TcpListener;
use tracing::{info, warn, error};

use crate::{AppState, scan, orchestrator};

pub async fn run_server(port: u16) -> anyhow::Result<()> {
    let (telemetry_tx, _) = broadcast::channel::<String>(100);
    let state = AppState {
        scan_results: Arc::new(Mutex::new(Vec::new())),
        telemetry_tx: telemetry_tx.clone(),
    };

    let cors = CorsLayer::new().allow_origin(Any).allow_methods(Any).allow_headers(Any);

    // Dashboard static files
    let dashboard_service = ServeDir::new("dashboard/dist");

    let api_routes = Router::new()
        .route("/api/health", get(api_health))
        .route("/api/scan", post(api_scan))
        .route("/api/scans", get(api_get_scans))
        .route("/api/plugins", get(api_get_plugins))
        .route("/api/graph", get(api_graph))
        .with_state((state, telemetry_tx.clone()));

    let ws_routes = Router::new()
        .route("/ws", get(orchestrator::ws_handler))
        .with_state(telemetry_tx);

    let app = api_routes
        .merge(ws_routes)
        // Static dashboard files
        .fallback_service(dashboard_service)
        .layer(cors);

    let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    info!("🌐 Duo Architecture Guardian запущен!");
    info!("   Web UI:  http://localhost:{}", port);
    info!("   API:     http://localhost:{}/api/health", port);
    info!("   WS:      ws://localhost:{}/ws", port);
    axum::serve(listener, app).await?;
    Ok(())
}

// API Handlers
pub async fn api_health() -> impl IntoResponse {
    Json(serde_json::json!({
        "status": "ok",
        "version": env!("CARGO_PKG_VERSION"),
        "name": "Duo Architecture Guardian"
    }))
}

pub async fn api_scan(
    State((state, _)): State<(AppState, broadcast::Sender<String>)>,
    Json(payload): Json<serde_json::Value>,
) -> impl IntoResponse {
    let path = payload["path"].as_str().unwrap_or("src/");
    let result = scan::run_scan(path);
    state.scan_results.lock().unwrap().push(result.clone());
    Json(serde_json::json!(result))
}

pub async fn api_get_scans(
    State((state, _)): State<(AppState, broadcast::Sender<String>)>,
) -> impl IntoResponse {
    let results = state.scan_results.lock().unwrap().clone();
    Json(serde_json::json!(results))
}

pub async fn api_get_plugins() -> impl IntoResponse {
    let plugins: Vec<serde_json::Value> = scan::plugins::all_plugins().iter().map(|p| {
        serde_json::json!({ "name": p.name(), "description": p.description() })
    }).collect();
    Json(serde_json::json!(plugins))
}

pub async fn api_graph() -> impl IntoResponse {
    // Создаем демо-граф из 16 узлов, демонстрирующий Blast Radius "Trust Decay"
    use crate::models::{Entity, EntityKind, EdgeKind, EntityGraph};
    let mut bg = EntityGraph::new();

    let tainted_fn = bg.add(Entity { id: 0, kind: EntityKind::Function, name: "unsafe_raw_query()".into(), file: "db.rs".into() });
    
    let svc_auth = bg.add(Entity { id: 1, kind: EntityKind::Struct, name: "AuthService".into(), file: "auth.rs".into() });
    let svc_user = bg.add(Entity { id: 2, kind: EntityKind::Struct, name: "UserService".into(), file: "user.rs".into() });
    let svc_pay = bg.add(Entity { id: 3, kind: EntityKind::Struct, name: "PaymentService".into(), file: "pay.rs".into() });
    
    bg.connect(svc_auth, tainted_fn, EdgeKind::Calls);
    bg.connect(svc_user, tainted_fn, EdgeKind::Calls);
    bg.connect(svc_pay, tainted_fn, EdgeKind::Calls);

    let mut endpoints = Vec::new();
    for i in 1..=12 {
        let ep = bg.add(Entity { id: 100 + i, kind: EntityKind::Endpoint, name: format!("POST /api/v1/route_{}", i), file: "router.rs".into() });
        endpoints.push(ep);
        let target_svc = if i % 3 == 0 { svc_auth } else if i % 3 == 1 { svc_user } else { svc_pay };
        bg.connect(ep, target_svc, EdgeKind::Calls);
    }

    // Собираем узлы и ребра для React xyflow
    let mut nodes = Vec::new();
    let mut edges = Vec::new();

    for idx in bg.graph.node_indices() {
        let entity = &bg.graph[idx];
        // Blast radius логика: tainted_fn и все кто его вызывает считаются зараженными
        let is_tainted = true; // Для демо мы подсвечиваем весь этот подграф красным (Blast Radius)
        
        nodes.push(serde_json::json!({
            "id": idx.index().to_string(),
            "data": {
                "label": entity.name.clone(),
                "kind": format!("{:?}", entity.kind),
                "isTainted": is_tainted,
                "file": entity.file.clone(),
            },
            "position": { "x": 0, "y": 0 } // Dagre layout расставит их на фронтенде
        }));
    }

    for edge in bg.graph.edge_references() {
        use petgraph::visit::EdgeRef;
        edges.push(serde_json::json!({
            "id": format!("e-{}-{}", edge.source().index(), edge.target().index()),
            "source": edge.source().index().to_string(),
            "target": edge.target().index().to_string(),
            "animated": true,
            "type": "default"
        }));
    }

    Json(serde_json::json!({
        "nodes": nodes,
        "edges": edges
    }))
}


