import re

with open('src/main.rs', 'r') as f:
    content = f.read()

# Find the start of run_demos() including the section header
match = re.search(r'// ─────────────────────────────────────────────────────────────────────────────\n// § Демо-сценарии \(вынесены из main\)\n// ─────────────────────────────────────────────────────────────────────────────\n\nasync fn run_demos\(\) -> anyhow::Result<\(\)> \{', content)

if match:
    start_idx = match.start()
    end_idx = content.find('}\n\n', start_idx) + 2 # End of function
    if end_idx < start_idx + 10:
         end_idx = len(content)
         
    demos_content = content[start_idx:end_idx]
    main_content = content[:start_idx] + content[end_idx:]

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
        
    # main.rs needs `mod demos;` at the top
    main_content = main_content.replace('pub mod kan_intelligence;', 'pub mod kan_intelligence;\n/// Демонстрационные сценарии\npub mod demos;')
    # and call `crate::demos::run_demos().await?;` instead of `run_demos().await?;`
    main_content = main_content.replace('run_demos().await?;', 'crate::demos::run_demos().await?;')

    with open('src/main.rs', 'w') as f:
        f.write(main_content)
    
    print("Extraction successful.")
else:
    print("Could not find run_demos()")
