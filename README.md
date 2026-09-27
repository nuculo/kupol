# KUPOL

Security dome over your repo.

KUPOL is the product shell around the **duo-agents** engine (Duo Architecture Guardian): multi-agent security and architecture review for a codebase. Engine **source** lives in this repo. The compiled `duo-agents` binary (~15MB) is **not** in git — download it from [GitHub Releases](https://github.com/rustoman-AI/kupol/releases).

Upstream engine license in-tree is **MIT** (see `LICENSE`). The release tarball is the supported binary distribution.

## Architecture (CTO)

Closed core / open shell, unchanged from the engine:

| Piece | Path |
| --- | --- |
| Open shell (CLI, actors, Axum, dashboard) | `src/`, `dashboard/` |
| Closed core (KAN, scan, policy, blast-radius) | `duo-kan/` |
| Product + architecture draft | `docs/Arch/` |
| Compiled engine | GitHub Releases (`duo-agents-v1.0-Release.tar.gz`) |

```
GitLab Duo / your repo
        │
        ▼
   KUPOL (this product)
        │
        ▼
   duo-agents engine  →  duo-kan core
```

## Quick start

**From source**

```bash
cargo build --release
./run.sh
./run.sh scan /path/to/code
```

**From the v1.0 release tarball**

```bash
tar -xzf duo-agents-v1.0-Release.tar.gz
# binary: release_build/duo-agents
# architecture notes: release_build/docs/
```

Do not `git add` `duo-agents` or the `.tar.gz`.

## Engine README

The original Duo Architecture Guardian write-up (hackathon agents, CI, stack) is in `docs/duo-agents-readme.md`.
