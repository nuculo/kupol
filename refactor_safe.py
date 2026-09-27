import os
import shutil

# 1. Clean up broken dirs
if os.path.exists('src/demos'):
    shutil.rmtree('src/demos')

# 2. Re-create directories
os.makedirs('src/server', exist_ok=True)
os.makedirs('src/demos', exist_ok=True)

# 3. Read the original main.rs top part
with open('src/main.rs', 'r') as f:
    main_content = f.read()

# We need everything up to `async fn run_server`
server_idx = main_content.find("async fn run_server(port: u16)")
if server_idx == -1:
    server_idx = main_content.find("// ─────────────────────────────────────────────────────────────────────────────\n// § API Server")

main_top = main_content[:server_idx]
server_part = main_content[server_idx:]

# But wait, run_demos was after server if I reordered it?
# In original file, run_demos was the last function.
# Let's just accurately recreate the files.
# `src/demos.rs` has the proper `run_demos`!
with open('src/demos.rs', 'r') as f:
    demos_full = f.read()

# remove pub mod demos from main_top if it's there
main_top = main_top.replace("pub mod demos;", "")
main_top = main_top.replace("pub mod kan_intelligence;\n/// Демонстрационные сценарии\n", "pub mod kan_intelligence;\n")

# Extract only the `run_demos` body from `src/demos.rs`
demo_start = demos_full.find("pub async fn run_demos")
if demo_start == -1:
    demo_start = demos_full.find("async fn run_demos")

demos_code = demos_full[demo_start:].replace("pub async fn run_demos", "pub async fn run_demos")

# Write src/demos/mod.rs
with open('src/demos/mod.rs', 'w') as f:
    f.write("""use anyhow::Result;
use tokio::time::Duration;
use tokio::sync::{mpsc, broadcast};
use tracing::{info, warn, error};
use std::collections::HashMap;

use crate::{*, protocol::*, models::*, actor::*, actors::*, orchestrator::*};

""")
    f.write(demos_code)

# Extract Server code
# Find the end of server code in main_top
# Actually, server code might be mixed in `main_top`? 
# In original, `run_server` was around line 286, and `api_*` were up to 430.
# Then `run_demos` started. Let's find `run_server` in main_content.
run_server_idx = main_content.find("async fn run_server(")
run_demos_idx_in_main = main_content.find("async fn run_demos(")
if run_demos_idx_in_main == -1:
    run_demos_idx_in_main = len(main_content)

server_code = main_content[run_server_idx:run_demos_idx_in_main]
server_code = server_code.replace("async fn run_server", "pub async fn run_server")
server_code = server_code.replace("async fn api_health", "pub async fn api_health")
server_code = server_code.replace("async fn api_scan", "pub async fn api_scan")
server_code = server_code.replace("async fn api_get_scans", "pub async fn api_get_scans")
server_code = server_code.replace("async fn api_get_plugins", "pub async fn api_get_plugins")
server_code = server_code.replace("async fn api_graph", "pub async fn api_graph")

with open('src/server/mod.rs', 'w') as f:
    f.write("""use anyhow::Result;
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

""")
    f.write(server_code)

# Write core main.rs
main_part = main_content[:run_server_idx]
# Fix module declarations
mod_decls = "pub mod server;\npub mod demos;\n\n"
if "pub mod cli;" in main_part:
    main_part = main_part.replace("pub mod cli;", "pub mod cli;\n" + mod_decls)

# Ensure AppState stays in main.rs because it's used in server.rs and main.rs?
# AppState can stay in main.rs so we `use crate::AppState` in server/mod.rs.

# We must also change calls in main()
main_part = main_part.replace("run_server(port).await", "crate::server::run_server(port).await")
main_part = main_part.replace("run_demos().await", "crate::demos::run_demos().await")
main_part = main_part.replace("crate::demos::run_demos().await?;", "crate::demos::run_demos().await?;")

with open('src/main.rs', 'w') as f:
    f.write(main_part)

# Cleanup
if os.path.exists('src/demos.rs'):
    os.remove('src/demos.rs')

print("Refactored into src/server/mod.rs and src/demos/mod.rs successfully!")
