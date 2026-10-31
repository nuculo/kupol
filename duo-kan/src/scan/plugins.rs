//! Security Plugins — модульная система обнаружения уязвимостей
//!
//! Вдохновлено promptfoo RedteamPluginBase: каждый плагин = scan() + severity

use super::{Finding, Severity};

/// Trait для плагинов безопасности (аналог RedteamPluginBase из promptfoo)
pub trait SecurityPlugin: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn scan(&self, file_path: &str, content: &str) -> Vec<Finding>;
}

/// Получить все доступные плагины
pub fn all_plugins() -> Vec<Box<dyn SecurityPlugin>> {
    vec![
        Box::new(UnsafeCodePlugin),
        Box::new(SqlInjectionPlugin),
        Box::new(HardcodedSecretsPlugin),
        Box::new(UnwrapPlugin),
        Box::new(TodoFixmePlugin),
        Box::new(DeprecatedApiPlugin),
        Box::new(InputValidationPlugin),
        Box::new(CryptoWeaknessPlugin),
    ]
}

// ═════════════════════════════════════════════════════════════════════════════
// Plugin 1: Unsafe Code Detection
// ═════════════════════════════════════════════════════════════════════════════
pub struct UnsafeCodePlugin;

impl SecurityPlugin for UnsafeCodePlugin {
    fn name(&self) -> &str { "unsafe-code" }
    fn description(&self) -> &str { "Detects unsafe blocks and raw pointers" }

    fn scan(&self, file_path: &str, content: &str) -> Vec<Finding> {
        if !file_path.ends_with(".rs") { return vec![]; }
        let mut findings = Vec::new();

        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.starts_with("///") { continue; }

            if trimmed.contains("unsafe {") || trimmed.contains("unsafe fn") {
                findings.push(Finding {
                    plugin: self.name().into(),
                    severity: Severity::High,
                    title: "Unsafe block".into(),
                    description: "unsafe code bypasses the borrow checker and can cause undefined behavior.".into(),
                    file: file_path.into(),
                    line: Some(i + 1),
                    code_snippet: Some(trimmed.to_string()),
                    suggestion: Some("Prefer a safe alternative, or add a `// SAFETY:` comment explaining the invariants.".into()),
                    cwe: Some("CWE-676".into()),
                });
            }

            if trimmed.contains("*mut ") || trimmed.contains("*const ") {
                findings.push(Finding {
                    plugin: self.name().into(),
                    severity: Severity::Medium,
                    title: "Raw pointer".into(),
                    description: "Raw pointers can lead to use-after-free and double-free bugs.".into(),
                    file: file_path.into(),
                    line: Some(i + 1),
                    code_snippet: Some(trimmed.to_string()),
                    suggestion: Some("Use `&T`/`&mut T` or `Box<T>` instead of raw pointers.".into()),
                    cwe: Some("CWE-416".into()),
                });
            }
        }
        findings
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Plugin 2: SQL Injection
// ═════════════════════════════════════════════════════════════════════════════
pub struct SqlInjectionPlugin;

impl SecurityPlugin for SqlInjectionPlugin {
    fn name(&self) -> &str { "sql-injection" }
    fn description(&self) -> &str { "Detects SQL built from string interpolation" }

    fn scan(&self, file_path: &str, content: &str) -> Vec<Finding> {
        use crate::scan::phi::{PhiRule, select_phi_engine};

        let rules = [
            PhiRule { pattern: "format!(\"SELECT", severity: Severity::Critical, description: "SQL query built with format!()", suggestion: "Use parameterized queries instead of string interpolation.", cwe: "CWE-89" },
            PhiRule { pattern: "format!(\"INSERT", severity: Severity::Critical, description: "SQL insert built with format!()", suggestion: "Use parameterized queries instead of string interpolation.", cwe: "CWE-89" },
            PhiRule { pattern: "format!(\"UPDATE", severity: Severity::Critical, description: "SQL update built with format!()", suggestion: "Use parameterized queries instead of string interpolation.", cwe: "CWE-89" },
            PhiRule { pattern: ".execute(&format!", severity: Severity::Critical, description: "Dynamic SQL execution", suggestion: "Use parameterized queries instead of string interpolation.", cwe: "CWE-89" },
            PhiRule { pattern: "raw_query(", severity: Severity::High, description: "Potential raw SQL query", suggestion: "Use a type-safe query builder or parameterized queries.", cwe: "CWE-89" },
        ];

        // 🧠 Graceful Degradation: Оркестратор выдает AST-SemanticPhi для маленьких файлов, 
        // и сваливается до O(N) RegexPhi для гигантских монолитов (защита от DoS пайплайна)
        let engine = select_phi_engine(content.len());
        
        engine.execute(self.name(), file_path, content, &rules)
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Plugin 3: Hardcoded Secrets
// ═════════════════════════════════════════════════════════════════════════════
pub struct HardcodedSecretsPlugin;

impl SecurityPlugin for HardcodedSecretsPlugin {
    fn name(&self) -> &str { "hardcoded-secrets" }
    fn description(&self) -> &str { "Detects hardcoded secrets and API keys" }

    fn scan(&self, file_path: &str, content: &str) -> Vec<Finding> {
        // Skip binary files and lock files
        if file_path.ends_with(".lock") || file_path.ends_with(".sum") { return vec![]; }
        let mut findings = Vec::new();

        let secret_patterns = [
            ("sk-", "OpenAI API Key"),
            ("AKIA", "AWS Access Key"),
            ("ghp_", "GitHub Personal Access Token"),
            ("glpat-", "GitLab Personal Access Token"),
            ("Bearer ", "Bearer Token in code"),
            ("password = \"", "Hardcoded password"),
            ("password: \"", "Hardcoded password (YAML)"),
            ("api_key = \"", "Hardcoded API key"),
            ("apiKey: \"", "Hardcoded API key"),
            ("secret_key", "Secret key reference"),
            ("private_key", "Private key reference"),
        ];

        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.starts_with("#") || trimmed.starts_with("*") { continue; }
            // Skip env var reads and examples
            if trimmed.contains("env::var") || trimmed.contains("process.env") || trimmed.contains("os.getenv") { continue; }
            if trimmed.contains("example") || trimmed.contains("placeholder") { continue; }

            for (pattern, desc) in &secret_patterns {
                if trimmed.contains(pattern) {
                    let severity = if *pattern == "sk-" || *pattern == "AKIA" || *pattern == "ghp_" || *pattern == "glpat-" {
                        Severity::Critical
                    } else if trimmed.contains("password") {
                        Severity::High
                    } else {
                        Severity::Medium
                    };

                    findings.push(Finding {
                        plugin: self.name().into(),
                        severity,
                        title: format!("Hardcoded secret: {}", desc),
                        description: "Secrets should not be committed to source code.".into(),
                        file: file_path.into(),
                        line: Some(i + 1),
                        code_snippet: Some(format!("{}...", &trimmed[..trimmed.len().min(60)])),
                        suggestion: Some("Move secrets to environment variables or a secrets manager.".into()),
                        cwe: Some("CWE-798".into()),
                    });
                    break; // one finding per line
                }
            }
        }
        findings
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Plugin 4: Unwrap() Detection (Rust-specific)
// ═════════════════════════════════════════════════════════════════════════════
pub struct UnwrapPlugin;

impl SecurityPlugin for UnwrapPlugin {
    fn name(&self) -> &str { "unwrap-panic" }
    fn description(&self) -> &str { "Detects .unwrap() without error handling" }

    fn scan(&self, file_path: &str, content: &str) -> Vec<Finding> {
        if !file_path.ends_with(".rs") { return vec![]; }
        let mut findings = Vec::new();

        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") { continue; }
            // Skip test files
            if file_path.contains("/test") || trimmed.contains("#[test]") || trimmed.contains("#[cfg(test)]") { continue; }

            if trimmed.contains(".unwrap()") && !trimmed.contains("// OK:") {
                findings.push(Finding {
                    plugin: self.name().into(),
                    severity: Severity::Low,
                    title: ".unwrap() without error handling".into(),
                    description: "unwrap() panics on None/Err, which is risky in production code.".into(),
                    file: file_path.into(),
                    line: Some(i + 1),
                    code_snippet: Some(trimmed.to_string()),
                    suggestion: Some("Use match, if let, the `?` operator, or .unwrap_or_default().".into()),
                    cwe: Some("CWE-391".into()),
                });
            }
        }
        findings
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Plugin 5: TODO/FIXME/HACK markers
// ═════════════════════════════════════════════════════════════════════════════
pub struct TodoFixmePlugin;

impl SecurityPlugin for TodoFixmePlugin {
    fn name(&self) -> &str { "todo-fixme" }
    fn description(&self) -> &str { "Detects unresolved TODO, FIXME, HACK, XXX markers" }

    fn scan(&self, file_path: &str, content: &str) -> Vec<Finding> {
        let mut findings = Vec::new();
        let markers = [
            ("FIXME", Severity::Medium, "Needs a fix"),
            ("HACK", Severity::Medium, "Temporary workaround"),
            ("XXX", Severity::Medium, "Fragile or risky code"),
            ("TODO: security", Severity::High, "Deferred security task"),
            ("TODO", Severity::Info, "Unresolved task"),
        ];

        for (i, line) in content.lines().enumerate() {
            let upper = line.to_uppercase();
            for (marker, severity, desc) in &markers {
                if upper.contains(marker) {
                    findings.push(Finding {
                        plugin: self.name().into(),
                        severity: severity.clone(),
                        title: format!("{} marker", marker),
                        description: desc.to_string(),
                        file: file_path.into(),
                        line: Some(i + 1),
                        code_snippet: Some(line.trim().to_string()),
                        suggestion: None,
                        cwe: None,
                    });
                    break;
                }
            }
        }
        findings
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Plugin 6: Deprecated API Usage
// ═════════════════════════════════════════════════════════════════════════════
pub struct DeprecatedApiPlugin;

impl SecurityPlugin for DeprecatedApiPlugin {
    fn name(&self) -> &str { "deprecated-api" }
    fn description(&self) -> &str { "Detects deprecated and dangerous APIs" }

    fn scan(&self, file_path: &str, content: &str) -> Vec<Finding> {
        let mut findings = Vec::new();
        let deprecated = [
            ("std::mem::transmute", Severity::High, "transmute bypasses the type system"),
            ("std::mem::uninitialized", Severity::Critical, "Creates uninitialized memory (undefined behavior)"),
            ("eval(", Severity::Critical, "Dynamic code execution"),
            ("exec(", Severity::High, "Executes system commands"),
            ("os.system(", Severity::High, "Python: runs a shell command"),
            ("subprocess.call(", Severity::Medium, "Python: subprocess without validation"),
            ("dangerouslySetInnerHTML", Severity::High, "React: potential XSS"),
            ("innerHTML", Severity::Medium, "Direct DOM injection"),
        ];

        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.starts_with("#") { continue; }

            for (pattern, severity, desc) in &deprecated {
                if trimmed.contains(pattern) {
                    findings.push(Finding {
                        plugin: self.name().into(),
                        severity: severity.clone(),
                        title: format!("Dangerous API: {}", pattern),
                        description: desc.to_string(),
                        file: file_path.into(),
                        line: Some(i + 1),
                        code_snippet: Some(trimmed.to_string()),
                        suggestion: Some("Replace with a safe alternative.".into()),
                        cwe: Some("CWE-477".into()),
                    });
                }
            }
        }
        findings
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Plugin 7: Input Validation
// ═════════════════════════════════════════════════════════════════════════════
pub struct InputValidationPlugin;

impl SecurityPlugin for InputValidationPlugin {
    fn name(&self) -> &str { "input-validation" }
    fn description(&self) -> &str { "Detects missing input validation" }

    fn scan(&self, file_path: &str, content: &str) -> Vec<Finding> {
        let mut findings = Vec::new();
        let patterns = [
            ("from_utf8_unchecked", Severity::High, "Unchecked UTF-8 can cause undefined behavior"),
            ("parse().unwrap()", Severity::Medium, "Parsing without error handling"),
            ("as_str().unwrap()", Severity::Low, "Conversion without a type check"),
            ("from_raw_parts", Severity::Critical, "Builds a slice from a raw pointer"),
        ];

        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") { continue; }

            for (pattern, severity, desc) in &patterns {
                if trimmed.contains(pattern) {
                    findings.push(Finding {
                        plugin: self.name().into(),
                        severity: severity.clone(),
                        title: "Missing input validation".into(),
                        description: desc.to_string(),
                        file: file_path.into(),
                        line: Some(i + 1),
                        code_snippet: Some(trimmed.to_string()),
                        suggestion: Some("Validate the input and handle errors explicitly.".into()),
                        cwe: Some("CWE-20".into()),
                    });
                }
            }
        }
        findings
    }
}

// ═════════════════════════════════════════════════════════════════════════════
// Plugin 8: Crypto Weakness
// ═════════════════════════════════════════════════════════════════════════════
pub struct CryptoWeaknessPlugin;

impl SecurityPlugin for CryptoWeaknessPlugin {
    fn name(&self) -> &str { "crypto-weakness" }
    fn description(&self) -> &str { "Detects weak cryptography" }

    fn scan(&self, file_path: &str, content: &str) -> Vec<Finding> {
        let mut findings = Vec::new();
        // Patterns that need word-boundary matching to avoid false positives (e.g. "DES" in "description")
        let weak_crypto = [
            ("MD5(", Severity::High, "MD5 is unsafe for password hashing"),
            ("md5(", Severity::High, "MD5 is unsafe for password hashing"),
            ("SHA1(", Severity::Medium, "SHA-1 is considered obsolete"),
            ("sha1(", Severity::Medium, "SHA-1 is considered obsolete"),
            ("DES.encrypt", Severity::High, "DES encryption is broken"),
            ("DES.decrypt", Severity::High, "DES encryption is broken"),
            ("DESCipher", Severity::High, "DES encryption is broken"),
            ("RC4.encrypt", Severity::High, "RC4 is considered insecure"),
            ("RC4.decrypt", Severity::High, "RC4 is considered insecure"),
            ("\"ECB\"", Severity::Medium, "ECB mode is not semantically secure"),
            ("Math.random()", Severity::Medium, "Math.random() is predictable"),
        ];

        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.starts_with("#") { continue; }

            for (pattern, severity, desc) in &weak_crypto {
                if trimmed.contains(pattern) {
                    // Skip import/use statements mentioning crypto libraries
                    if trimmed.starts_with("use ") || trimmed.starts_with("import ") { continue; }
                    findings.push(Finding {
                        plugin: self.name().into(),
                        severity: severity.clone(),
                        title: format!("Weak cryptography: {}", pattern),
                        description: desc.to_string(),
                        file: file_path.into(),
                        line: Some(i + 1),
                        code_snippet: Some(trimmed.to_string()),
                        suggestion: Some("Use AES-256-GCM, SHA-256+, or bcrypt/argon2.".into()),
                        cwe: Some("CWE-327".into()),
                    });
                }
            }
        }
        findings
    }
}
