# KUPOL

A local security scanner for your repository — scans a codebase, reports findings, and fails your CI build on severity.

[kupol.app](https://kupol.app) · MIT licensed · not affiliated with GitLab

KUPOL walks a source tree, runs a set of security checks (SQL injection, hardcoded secrets, weak crypto, unsafe code, and more), and prints a clean report as a table, JSON, or Markdown. Run it on your machine or in CI.

The public command is `kupol`, a thin wrapper around the engine. The engine crate is named `duo-agents` internally so `cargo build` stays unchanged; you never type that name as a user. Compiled binaries are published on [Releases](https://github.com/nuculo/kupol/releases), not committed to git.

## Install

### From a release

1. Download `duo-agents-v1.0-Release.tar.gz` from [Releases](https://github.com/nuculo/kupol/releases) and extract it. The engine binary is `release_build/duo-agents`.
2. Install the wrapper and engine:

   ```bash
   chmod +x install.sh bin/kupol
   ./install.sh ./release_build/duo-agents
   ```

   `install.sh` copies `bin/kupol` to `~/.local/bin` and, if you pass an engine path, copies it too (as `duo-agents`). Make sure `~/.local/bin` is on your `PATH`.

Or point at an unpacked engine without installing:

```bash
export KUPOL_BIN=/path/to/duo-agents
./bin/kupol scan .
```

### From source

```bash
git clone https://github.com/nuculo/kupol.git
cd kupol
cargo build --release
./install.sh ./target/release/duo-agents
```

## Usage

```bash
kupol scan [path] [-f table|json|markdown] [-o file] [--fail-on low|medium|high|critical]
kupol serve     # Web UI + API on http://localhost:3000
kupol init      # generate a CI pipeline template
kupol mcp       # run as a Model Context Protocol server (stdio)
kupol info      # engine information
kupol demo      # multi-agent demo scenarios
kupol help
```

Scan the current repo and print a table:

```bash
kupol scan .
```

Write a Markdown report:

```bash
kupol scan . -f markdown -o kupol-report.md
```

Try it on the bundled, intentionally vulnerable samples:

```bash
kupol scan fixtures/vuln-lab
```

## CI

`--fail-on` is the CI gate: `kupol scan` exits `1` if any finding is at or above the given severity (`critical` > `high` > `medium` > `low`). Without it, the engine falls back to `fail_on` in `.kupol.yml`.

GitHub Actions (see [`.github/workflows/scan.yml`](.github/workflows/scan.yml)) runs on pull requests to `main`:

```yaml
- name: Scan
  run: ./bin/kupol scan . -f markdown -o kupol-report.md --fail-on high
```

A matching GitLab CI template lives in [`.gitlab-ci.yml`](.gitlab-ci.yml). GitHub or GitLab — either host works.

## Configuration — `.kupol.yml`

KUPOL reads an optional `.kupol.yml` from the scan target (or the current directory):

```yaml
ignore:              # skipped in addition to target/, node_modules/, .git/
  - target
  - node_modules
  - dist
min_severity: low    # lowest severity to include in the report
fail_on: high        # default CI gate when --fail-on is omitted
```

## How it works

See [docs/architecture.md](docs/architecture.md) for a high-level overview of the scan pipeline and engine.

## License

[MIT](LICENSE) · [kupol.app](https://kupol.app)
