# 🛡️ Duo Architecture Guardian

> AI-powered multi-agent security and architecture analysis platform for the GitLab SDLC.
> Built with Rust + GitLab Duo Agent Platform for the **GitLab AI Hackathon 2026**.

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

## The Problem

AI writes code faster than ever. But **security, compliance, and architecture reviews** remain manual bottlenecks. Teams ship vulnerable code not because tools don't exist, but because review processes don't scale.

## The Solution

**Duo Architecture Guardian** is a multi-agent system that automates the security review pipeline:

| Agent | Role |
|-------|------|
| 🏗️ **Architecture Scanner** | Analyzes module dependencies, detects cyclic deps, identifies architecture drift |
| 🔒 **Security Reviewer** | Taint analysis (source→sink), vulnerability correlation, CWE mapping |
| 📊 **Report Generator** | Creates structured MR notes, auto-creates issues for critical findings |
| 💥 **Blast-Radius Engine** | Computes impact radius through dependency graphs using CycloneDX trust-decay |
| 🧠 **KAN Intelligence** | 12 Kolmogorov-Arnold Network modules for pattern recognition |

## Architecture

```
┌──────────────────────────── GitLab Duo Agent Platform ─────────────────────────────┐
│                                                                                     │
│  Custom Agent (Duo Chat)          Custom Flow (3-agent pipeline)                   │
│  ┌─────────────────────┐          ┌────────────┐  ┌──────────────┐  ┌───────────┐ │
│  │ Architecture         │          │ Architecture│→│ Security     │→│ Report    │ │
│  │ Guardian Agent       │          │ Scanner     │  │ Reviewer     │  │ Generator │ │
│  └─────────────────────┘          └────────────┘  └──────────────┘  └───────────┘ │
│                                                                                     │
└─────────────────────────────────────┬───────────────────────────────────────────────┘
                                      │ GitLab API (tools)
                                      ▼
┌──────────────────────────── Rust Engine (duo-agents) ──────────────────────────────┐
│                                                                                     │
│  12 Actors:  AST Analyzer │ Security Scanner │ Drift Detector │ GitLab Client      │
│              MCP Bridge   │ Swarm (×6)       │ Node Broker    │ Orchestrator       │
│                                                                                     │
│  duo-kan:   Scan Pipeline │ Policy Engine │ Blast-Radius │ KAN Intelligence        │
│             Semantic Engine │ Strategy │ Babylonian Scanner │ Crossover             │
│                                                                                     │
│  Server:    Axum REST API │ WebSocket │ React Dashboard                             │
│                                                                                     │
└─────────────────────────────────────────────────────────────────────────────────────┘
```

## Quick Start

### 1. Build & Run

```bash
# Build
cargo build --release

# Run Web UI (http://localhost:3000)
./run.sh

# Scan a project
./run.sh scan /path/to/code

# Export report
./run.sh scan-json src/ > report.json
```

### 2. GitLab Duo Integration

**Custom Agent** — follow [docs/hackathon/custom-agent-prompt.md](docs/hackathon/custom-agent-prompt.md):
1. Go to your project → Automate → Agents → New Agent
2. Paste the system prompt from the doc
3. Select the listed tools
4. Enable the agent → use in Duo Chat: `@Duo Architecture Guardian analyze this MR`

**Custom Flow** — follow [docs/hackathon/custom-flow-config.yaml](docs/hackathon/custom-flow-config.yaml):
1. Go to your project → Automate → Flows → New Flow
2. Paste the YAML config
3. Optionally set up MR trigger for auto-review

### 3. CI/CD Pipeline

The included `.gitlab-ci.yml` runs automatically:
- **Build** → Compiles the Rust binary
- **Test** → Runs cargo test
- **Security Review** → Scans code on MRs and posts results as MR notes

```bash
# Generate CI config for your own project
duo-agents init .
```

## GitLab Duo Agent Platform Features Used

| Feature | How We Use It |
|---------|---------------|
| **Custom Agent** | Security analysis agent with 15+ GitLab API tools |
| **Custom Flow** | 3-agent pipeline: scan → review → report |
| **AGENTS.md** | Repo-level agent context and architecture knowledge |
| **Chat Rules** | MR review policies and code conventions |
| **Triggers** | Auto-run flow on MR events |
| **CI/CD** | Build, test, and scan pipeline |

## Tech Stack

- **Language**: Rust 🦀
- **Async Runtime**: Tokio
- **Web Framework**: Axum + Tower
- **Frontend**: React + TailwindCSS + Vite
- **AI**: Kolmogorov-Arnold Networks (KAN)
- **Protocol**: JSON-RPC 2.0 (MCP), WebSocket
- **Analysis**: `syn` + `ra_ap_*` (rust-analyzer APIs)

## Team

**RED Team** — GitLab AI Hackathon 2026

## License

[MIT](LICENSE)
