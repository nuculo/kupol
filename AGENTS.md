# KUPOL — agent instructions

You are working on **KUPOL**, a local security scanner for a git repo. Public site: https://kupol.app. Public CLI: `kupol`. Not affiliated with GitLab.

## Role

- Taint-style checks: data from user/network/files toward SQL, shell, and crypto sinks
- Blast-radius: impact of a change through the dependency graph
- Architecture drift: module dependency patterns that do not match the intended layers
- Compliance-oriented reporting: OWASP, CWE, and similar labels already in the scanner

## Layout

Rust workspace (do not rename the engine crate):

- `duo-agents` — binary: CLI, actors, HTTP server, WebSocket, dashboard static files. Internal engine name; users only ever type `kupol`.
- `duo-kan` — library in this same public repo: scan pipeline, security checks (plugins), grading, report formatting.

Key paths:

- `src/main.rs` — CLI dispatch (scan, serve, report, init, mcp, info, demo)
- `src/cli.rs` — clap definitions
- `src/config.rs` — `.kupol.yml` loader (ignore, min_severity, fail_on)
- `src/actors/` — ast_analyzer, security, drift_detector, gitlab (optional git host client), mcp_bridge, swarm, node_broker
- `src/orchestrator.rs` — DAG-style multi-actor flow
- `src/server/` — Axum REST + WebSocket
- `src/demos/` — multi-agent demo scenarios (`kupol demo`)
- `duo-kan/src/scan/` — scan pipeline, plugins, report, grading
- `dashboard/` — React monitoring UI
- `bin/kupol`, `bin/kupol.ps1` — public wrapper around the engine binary

## How a scan works

1. `scan::run_scan(path)` reads `.kupol.yml` (ignore rules) and collects files
2. Each plugin from `scan::plugins::all_plugins()` inspects a file and emits findings
3. Findings are graded (severity + risk score) and filtered by `min_severity`
4. The report is printed as table, JSON, or Markdown
5. `--fail-on` (or `fail_on` in `.kupol.yml`) sets the CI exit code

## Conventions

- User-facing text (CLI output, finding titles/descriptions, logs, docs, website) is English and says KUPOL only.
- Russian comments in engine modules are existing style; do not mass-rewrite them. Do not reintroduce Russian into user-facing strings.
- Logs go to stderr via `tracing`; stdout stays clean for reports and JSON-RPC.
- Actors use a 2-phase Execute/Complete model
- Errors: `anyhow::Result<()>`
- Async: Tokio; HTTP: Axum

## Forbidden in product-facing files

Never use these strings in README, website, docs, AGENTS.md, or CLI output:
"GitLab Duo", "Duo Agent Platform", "Duo Architecture Guardian", "Duo Chat".
GitLab is OK only as an optional git/CI host, same as GitHub.

## Review checklist

1. `unsafe` without a safety comment
2. SQL built by string interpolation
3. Hardcoded secrets
4. Homegrown crypto
5. Known-bad dependencies in `Cargo.lock`
