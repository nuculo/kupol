# vuln-lab

Small, intentionally vulnerable files used to demonstrate the KUPOL scanner. Not a real application — each file exists to trigger a specific check.

| File | Triggers |
|------|----------|
| `query.rs` | `sql-injection` — SQL built with `format!()` |
| `leaked.py` | `hardcoded-secrets` — an `sk-` style API key |
| `query.py` | sample of an f-string SQL pattern |

Scan just this folder:

```bash
kupol scan fixtures/vuln-lab --fail-on high
```
