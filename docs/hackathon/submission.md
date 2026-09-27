# Duo Architecture Guardian — Hackathon Submission

## Project Title
**Duo Architecture Guardian**

## Tagline
AI-powered multi-agent security and architecture analysis for the GitLab SDLC.

## Description

### The Problem
AI accelerates code generation, but security reviews, architecture compliance, and vulnerability management remain manual bottlenecks. Developers ship faster but review processes don't scale — creating the "AI Paradox" where speed increases risk.

### The Solution
Duo Architecture Guardian is a multi-agent system built on the GitLab Duo Agent Platform that automates the entire security review pipeline:

1. **Architecture Scanner Agent** analyzes repository structure, detects cyclic dependencies, and identifies architecture drift
2. **Security Reviewer Agent** performs taint analysis (tracking data from sources to sinks), correlates vulnerabilities, and maps findings to CWE identifiers
3. **Report Generator Agent** synthesizes analysis into structured MR notes, auto-creates GitLab issues for critical findings, and links vulnerabilities to merge requests

The system goes beyond simple scanning — it computes **blast-radius** (how many downstream components are affected by a vulnerability) using CycloneDX trust-decay models, giving teams quantified risk assessment.

### What Makes It Different
- **Multi-agent flow**: Not a single chatbot, but a pipeline of specialized agents that work together
- **Blast-radius computation**: Quantifies the *impact* of vulnerabilities, not just their existence
- **Kolmogorov-Arnold Networks**: Uses KAN (a mathematical framework beyond standard neural networks) for pattern recognition in code analysis
- **12 Rust actors**: Production-grade async actor system with 2-phase execution, circuit breakers, and work-stealing scheduling
- **Full SDLC integration**: From commit scanning in CI/CD to interactive analysis in Duo Chat

### GitLab Duo Agent Platform Usage
- **Custom Agent**: Security analysis agent with 15+ GitLab API tools (list_vulnerabilities, create_issue, post_duo_code_review, etc.)
- **Custom Flow**: 3-agent pipeline (architecture_scanner → security_reviewer → report_generator) with ambient execution
- **AGENTS.md**: Repo-level agent context providing architecture knowledge
- **Chat Rules**: Custom MR review policies and code conventions
- **CI/CD Pipeline**: 3-stage pipeline (build → test → security-review) with automatic MR scanning
- **MR Triggers**: Auto-run the security flow on merge request events

### Built With
- Rust, Tokio, Axum
- GitLab Duo Agent Platform
- Kolmogorov-Arnold Networks (KAN)
- React, TailwindCSS, Vite
- CycloneDX (blast-radius)
- rust-analyzer APIs (semantic analysis)

## Demo Video
[TODO: Upload 3-minute demo to YouTube/Vimeo]

## Project URL
[TODO: Add GitLab project URL in gitlab-ai-hackathon group]

## Hackathon Category Targets
- 🏆 **Grand Prize**: Complete multi-agent security platform
- 🧠 **Most Technically Impressive**: KAN intelligence + 12-actor architecture + blast-radius engine
- 💡 **Most Impactful**: Solves the security bottleneck in AI-accelerated development
- 🟢 **Green Agent Prize**: Efficient Rust binary (~15MB), minimal compute, no unnecessary LLM calls
