# KUPOL

Security dome over your repo.

KUPOL scans a local codebase for security findings and prints a report. It is a standalone product at [kupol.app](https://kupol.app). Not affiliated with GitLab.

The public command is `kupol`. The engine crate in this repo is still named `duo-agents` so `cargo build` stays unchanged. Compiled engine binaries and `*.tar.gz` are not in git — they live in [Releases](https://github.com/nuculo/kupol/releases).

## Install from Releases

1. Download `duo-agents-v1.0-Release.tar.gz` from [Releases](https://github.com/nuculo/kupol/releases).
2. Extract. The engine binary is `release_build/duo-agents`.
3. From this repo:

```bash
chmod +x install.sh bin/kupol
./install.sh ./release_build/duo-agents
```

`install.sh` copies `bin/kupol` to `~/.local/bin`. If you pass an engine path, it copies that binary too (as `duo-agents`). Put `~/.local/bin` on your `PATH`.

Or point at an unpacked engine without copying it:

```bash
export KUPOL_BIN=/path/to/duo-agents
./bin/kupol scan .
```

## Install from source

```bash
git clone https://github.com/nuculo/kupol.git
cd kupol
cargo build --release
./install.sh ./target/release/duo-agents
```

## Commands

```bash
kupol scan [path] [-f table|json|markdown] [-o file]
kupol serve
kupol init
kupol mcp
kupol info
kupol demo
kupol help
```

`kupol` locates the engine in this order: `$KUPOL_BIN`, `duo-agents` on `PATH`, `./target/release/duo-agents`, `./release_build/duo-agents`.

## License

[MIT](LICENSE)

https://kupol.app
