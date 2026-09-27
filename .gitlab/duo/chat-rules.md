# Duo Architecture Guardian — Chat Rules

## Merge Request Review Rules

When reviewing merge requests in this project:

1. **Security First**: Always check for taint-flow vulnerabilities (SQL injection, XSS, command injection, path traversal)
2. **Architecture Compliance**: Verify that new code follows the actor-based architecture (no direct cross-module dependencies)
3. **Blast-Radius**: Estimate how many downstream modules are affected by the change
4. **Test Coverage**: Ensure new actors have corresponding test modules
5. **Unsafe Code**: Any `unsafe` block must have a `// SAFETY:` comment explaining the invariant

## Code Style

- Use `anyhow::Result<()>` for error handling, not `unwrap()` in production code
- Actors must implement the `Actor` trait with 2-phase execution
- All public APIs must have doc comments (`///`)
- Use `tracing` macros (`info!`, `warn!`, `error!`) for logging, not `println!`

## Response Format

When analyzing code, structure your response as:
1. **🔍 Findings**: List of detected issues with severity (Critical/High/Medium/Low/Info)
2. **📊 Blast-Radius**: Impact assessment with affected modules
3. **🛡️ Recommendations**: Specific fixes with code examples
4. **✅ Compliance**: Status against applicable security frameworks
