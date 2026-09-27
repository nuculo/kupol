# KUPOL — agent instructions

You are working on **KUPOL**, a local security scanner for a git repo. Public site: https://kupol.app. Public CLI: `kupol`. Not affiliated with GitLab.

## Role

- Taint-style checks: data from user/network/files toward SQL, shell, and crypto sinks
- Blast-radius: impact of a change through the dependency graph
- Architecture drift: module dependency patterns that do not match the intended layers
- Compliance-oriented reporting: OWASP, CWE, and similar labels already in the scanner

## Layout

Rust workspace (do not rename the engine crate):

- `duo-agents` — binary: actors, HTTP server, WebSocket, dashboard static files
- `duo-kan` — library in this same public repo: scan pipeline, policy engine, semantic analysis

Key paths:

- `src/actors/` — ast_analyzer, security, drift_detector, gitlab (optional git host client), mcp_bridge, swarm, node_broker
- `src/orchestrator.rs` — DAG-style multi-actor flow
- `src/server/` — Axum REST + WebSocket
- `duo-kan/src/scan/` — scan plugins
- `dashboard/` — React monitoring UI
- `bin/kupol` — public wrapper around the engine binary

## How to analyze code

1. Parse with `syn` and build an `EntityGraph`
2. Run plugins from `scan::plugins::all_plugins()`
3. Compare module deps against declared layers
4. For a finding, compute blast-radius on the graph
5. Report as table, JSON, or Markdown

## Conventions

- Russian comments in engine modules are existing style; do not mass-rewrite them
- Actors use a 2-phase Execute/Complete model
- Errors: `anyhow::Result<()>`
- Async: Tokio; HTTP: Axum

## Review checklist

1. `unsafe` without a safety comment
2. SQL built by string interpolation
3. Hardcoded secrets
4. Homegrown crypto
5. Known-bad dependencies in `Cargo.lock`
