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
    fn description(&self) -> &str { "Обнаружение unsafe блоков и raw pointers" }

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
                    title: "Использование unsafe блока".into(),
                    description: "unsafe код обходит проверки borrow checker и может вызвать UB".into(),
                    file: file_path.into(),
                    line: Some(i + 1),
                    code_snippet: Some(trimmed.to_string()),
                    suggestion: Some("Рассмотрите safe-альтернативы или добавьте // SAFETY: комментарий".into()),
                    cwe: Some("CWE-676".into()),
                });
            }

            if trimmed.contains("*mut ") || trimmed.contains("*const ") {
                findings.push(Finding {
                    plugin: self.name().into(),
                    severity: Severity::Medium,
                    title: "Raw pointer detected".into(),
                    description: "Raw pointers могут вызвать use-after-free и double-free".into(),
                    file: file_path.into(),
                    line: Some(i + 1),
                    code_snippet: Some(trimmed.to_string()),
                    suggestion: Some("Используйте &T/&mut T или Box<T> вместо raw pointers".into()),
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
    fn description(&self) -> &str { "Обнаружение SQL-инъекций (Powered by PhiProtocol)" }

    fn scan(&self, file_path: &str, content: &str) -> Vec<Finding> {
        use crate::scan::phi::{PhiRule, select_phi_engine};

        let rules = [
            PhiRule { pattern: "format!(\"SELECT", severity: Severity::Critical, description: "SQL query built via format!()", suggestion: "Используйте параметризованные запросы", cwe: "CWE-89" },
            PhiRule { pattern: "format!(\"INSERT", severity: Severity::Critical, description: "SQL insert built via format!()", suggestion: "Используйте параметризованные запросы", cwe: "CWE-89" },
            PhiRule { pattern: "format!(\"UPDATE", severity: Severity::Critical, description: "SQL update built via format!()", suggestion: "Используйте параметризованные запросы", cwe: "CWE-89" },
            PhiRule { pattern: ".execute(&format!", severity: Severity::Critical, description: "Dynamic SQL execution", suggestion: "Используйте параметризованные запросы", cwe: "CWE-89" },
            PhiRule { pattern: "raw_query(", severity: Severity::High, description: "Potential raw SQL query", suggestion: "Используйте ORM type-safe queries", cwe: "CWE-89" },
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
    fn description(&self) -> &str { "Обнаружение захардкоженных секретов и ключей API" }

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
                        title: format!("Обнаружен секрет: {}", desc),
                        description: "Секреты не должны храниться в коде".into(),
                        file: file_path.into(),
                        line: Some(i + 1),
                        code_snippet: Some(format!("{}...", &trimmed[..trimmed.len().min(60)])),
                        suggestion: Some("Перенесите секреты в переменные окружения или Vault".into()),
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
    fn description(&self) -> &str { "Обнаружение .unwrap() без обработки ошибок" }

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
                    title: "Использование .unwrap() без обработки ошибок".into(),
                    description: "unwrap() вызывает panic при None/Err — опасно в production".into(),
                    file: file_path.into(),
                    line: Some(i + 1),
                    code_snippet: Some(trimmed.to_string()),
                    suggestion: Some("Используйте match, if let, ?, или .unwrap_or_default()".into()),
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
    fn description(&self) -> &str { "Обнаружение незакрытых TODO, FIXME, HACK, XXX" }

    fn scan(&self, file_path: &str, content: &str) -> Vec<Finding> {
        let mut findings = Vec::new();
        let markers = [
            ("FIXME", Severity::Medium, "Требует исправления"),
            ("HACK", Severity::Medium, "Временное решение"),
            ("XXX", Severity::Medium, "Опасный участок"),
            ("TODO: security", Severity::High, "Отложенная задача безопасности"),
            ("TODO", Severity::Info, "Незакрытая задача"),
        ];

        for (i, line) in content.lines().enumerate() {
            let upper = line.to_uppercase();
            for (marker, severity, desc) in &markers {
                if upper.contains(marker) {
                    findings.push(Finding {
                        plugin: self.name().into(),
                        severity: severity.clone(),
                        title: format!("{} маркер", marker),
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
    fn description(&self) -> &str { "Обнаружение устаревших и опасных API" }

    fn scan(&self, file_path: &str, content: &str) -> Vec<Finding> {
        let mut findings = Vec::new();
        let deprecated = [
            ("std::mem::transmute", Severity::High, "transmute обходит систему типов"),
            ("std::mem::uninitialized", Severity::Critical, "Создание неинициализированной памяти — UB"),
            ("eval(", Severity::Critical, "Динамическое выполнение кода"),
            ("exec(", Severity::High, "Выполнение системных команд"),
            ("os.system(", Severity::High, "Python: системная команда"),
            ("subprocess.call(", Severity::Medium, "Python: subprocess без проверки"),
            ("dangerouslySetInnerHTML", Severity::High, "React: XSS уязвимость"),
            ("innerHTML", Severity::Medium, "Прямой DOM-инъекция"),
        ];

        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") || trimmed.starts_with("#") { continue; }

            for (pattern, severity, desc) in &deprecated {
                if trimmed.contains(pattern) {
                    findings.push(Finding {
                        plugin: self.name().into(),
                        severity: severity.clone(),
                        title: format!("Опасный API: {}", pattern),
                        description: desc.to_string(),
                        file: file_path.into(),
                        line: Some(i + 1),
                        code_snippet: Some(trimmed.to_string()),
                        suggestion: Some("Используйте безопасные альтернативы".into()),
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
    fn description(&self) -> &str { "Обнаружение отсутствия валидации входных данных" }

    fn scan(&self, file_path: &str, content: &str) -> Vec<Finding> {
        let mut findings = Vec::new();
        let patterns = [
            ("from_utf8_unchecked", Severity::High, "UTF-8 без проверки может вызвать UB"),
            ("parse().unwrap()", Severity::Medium, "Парсинг без обработки ошибок"),
            ("as_str().unwrap()", Severity::Low, "Преобразование без проверки типа"),
            ("from_raw_parts", Severity::Critical, "Создание данных из raw pointer"),
        ];

        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") { continue; }

            for (pattern, severity, desc) in &patterns {
                if trimmed.contains(pattern) {
                    findings.push(Finding {
                        plugin: self.name().into(),
                        severity: severity.clone(),
                        title: "Отсутствие валидации входных данных".into(),
                        description: desc.to_string(),
                        file: file_path.into(),
                        line: Some(i + 1),
                        code_snippet: Some(trimmed.to_string()),
                        suggestion: Some("Добавьте валидацию и обработку ошибок".into()),
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
    fn description(&self) -> &str { "Обнаружение слабой криптографии" }

    fn scan(&self, file_path: &str, content: &str) -> Vec<Finding> {
        let mut findings = Vec::new();
        // Patterns that need word-boundary matching to avoid false positives (e.g. "DES" in "description")
        let weak_crypto = [
            ("MD5(", Severity::High, "MD5 небезопасен для hashing паролей"),
            ("md5(", Severity::High, "MD5 небезопасен для hashing паролей"),
            ("SHA1(", Severity::Medium, "SHA1 считается устаревшим"),
            ("sha1(", Severity::Medium, "SHA1 считается устаревшим"),
            ("DES.encrypt", Severity::High, "DES-шифрование взломано"),
            ("DES.decrypt", Severity::High, "DES-шифрование взломано"),
            ("DESCipher", Severity::High, "DES-шифрование взломано"),
            ("RC4.encrypt", Severity::High, "RC4 считается небезопасным"),
            ("RC4.decrypt", Severity::High, "RC4 считается небезопасным"),
            ("\"ECB\"", Severity::Medium, "ECB mode не обеспечивает семантическую безопасность"),
            ("Math.random()", Severity::Medium, "Math.random() предсказуем"),
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
                        title: format!("Слабая криптография: {}", pattern),
                        description: desc.to_string(),
                        file: file_path.into(),
                        line: Some(i + 1),
                        code_snippet: Some(trimmed.to_string()),
                        suggestion: Some("Используйте AES-256-GCM, SHA-256+, bcrypt/argon2".into()),
                        cwe: Some("CWE-327".into()),
                    });
                }
            }
        }
        findings
    }
}
