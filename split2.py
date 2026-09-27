import os

with open('src/main.rs', 'r') as f:
    content = f.read()

# Since run_demos() is the last function, split from its header
start_marker = "// ─────────────────────────────────────────────────────────────────────────────\n// § Демо-сценарии (вынесены из main)"

idx = content.find(start_marker)

if idx != -1:
    main_content = content[:idx]
    demos_content = content[idx:]

    header = """use anyhow::Result;
use tokio::time::Duration;
use tokio::sync::{mpsc, broadcast};
use tracing::{info, warn, error};
use std::collections::HashMap;
use axum::{routing::{get, post}, Json, Router};
use tower_http::cors::{Any, CorsLayer};
use tokio::net::TcpListener;
use crate::{*, protocol::*, models::*, actor::*, actors::*, orchestrator::*};

"""
    demos_content = demos_content.replace('async fn run_demos()', 'pub async fn run_demos()')

    with open('src/demos.rs', 'w') as f:
        f.write(header + demos_content)
        
    with open('src/main.rs', 'w') as f:
        f.write(main_content)
    
    print("Extraction successful (Method 2).")
else:
    print("Could not find start marker.")
