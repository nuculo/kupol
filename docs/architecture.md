# Architecture overview

A high-level map of how KUPOL scans a repository. This is a product-level overview, not an internals dump — see `docs/Arch/` for historical engine notes.

## Pieces

- **`kupol`** (`bin/kupol`, `bin/kupol.ps1`) — the public command. A thin wrapper that locates the engine binary and adds the `--fail-on` CI gate.
- **Engine** (`duo-agents`) — the compiled binary. Hosts the CLI, the scan pipeline, the optional Web UI/API, and the MCP server. The crate is named `duo-agents` for historical reasons; users never type it.
- **`duo-kan`** — the library crate in this repo that holds the scan pipeline, the security checks (plugins), severity grading, and report formatting.
- **`dashboard/`** — an optional React monitoring UI served by `kupol serve`.

## The scan pipeline

```
kupol scan <path>
   │
   ▼
read .kupol.yml ──► ignore rules, min_severity, fail_on
   │
   ▼
collect files ─────► walk the tree, skip target/ node_modules/ .git/ + ignores
   │
   ▼
run checks ────────► each plugin inspects a file and emits findings
   │
   ▼
grade ─────────────► assign severity, compute a risk score
   │
   ▼
report ────────────► table (terminal), JSON, or Markdown
   │
   ▼
CI gate ───────────► exit 1 if any finding ≥ --fail-on / fail_on
```

## Security checks

Each check is a plugin implementing a small `SecurityPlugin` trait (`name`, `description`, `scan`). The built-in set covers:

| Check | Looks for | Example CWE |
|-------|-----------|-------------|
| `sql-injection` | SQL built from string interpolation | CWE-89 |
| `hardcoded-secrets` | API keys, tokens, passwords in code | CWE-798 |
| `crypto-weakness` | MD5/SHA-1/DES/RC4, ECB mode, weak RNG | CWE-327 |
| `unsafe-code` | `unsafe` blocks and raw pointers (Rust) | CWE-676 |
| `deprecated-api` | dangerous/deprecated APIs (`eval`, `transmute`, …) | CWE-477 |
| `input-validation` | unchecked conversions and parsing | CWE-20 |
| `unwrap-panic` | `.unwrap()` without error handling (Rust) | CWE-391 |
| `todo-fixme` | unresolved TODO/FIXME/HACK markers | — |

Findings carry a plugin name, severity, title, description, file and line, an optional code snippet, a suggested fix, and a CWE reference where applicable.

## Severity and risk

Severities are `info < low < medium < high < critical`. The report includes an overall risk score derived from the highest-severity finding plus a penalty when many critical/high findings cluster together. `min_severity` filters the report; `fail_on` / `--fail-on` decides the process exit code for CI.

## Output formats

- **table** — a colorized terminal table (severity colors are applied when the terminal supports them).
- **json** — machine-readable, stable shape; used by the `kupol` wrapper's CI gate.
- **markdown** — a report suitable for posting to a pull/merge request.

Logs go to stderr so that stdout stays clean for reports and JSON.

## Beyond the scanner

The engine also ships a multi-agent demo (`kupol demo`), an Axum Web UI/API (`kupol serve`), and a Model Context Protocol server (`kupol mcp`) so editors and AI agents can call the scanner as a tool. These are optional; the core product is `kupol scan`.
