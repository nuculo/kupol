# KUPOL

Security dome over your repo.

KUPOL sits on top of your codebase and the **duo-agents** engine: it watches, gates, and reports — without putting the AGPL engine binary into git.

## Layout (CTO)

| Path | What belongs here |
| --- | --- |
| `docs/` | Product draft and architecture notes |
| `engine/` | Engine *source* and wrappers we own (not the 15MB binary) |
| GitHub Releases | `duo-agents` / `duo-agents.exe` — download, do not commit |

Paste or copy the engine and product draft into this tree when you have them. The binary stays out of the repo.

## Engine binary

Do **not** `git add` `duo-agents`. After a tagged release:

```bash
gh release create v0.1.0 ./duo-agents.exe --title "v0.1.0" --notes "Engine binary (AGPL). Source and product live in this repo."
```

## Status

Scaffold only. Engine and product draft are not in this clone yet.
