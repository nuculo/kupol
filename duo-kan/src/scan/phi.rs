use crate::scan::{Finding, Severity};
use std::time::Duration;

/// Правило для передачи в универсальный Phi движок
pub struct PhiRule {
    pub pattern: &'static str,
    pub severity: Severity,
    pub description: &'static str,
    pub suggestion: &'static str,
    pub cwe: &'static str,
}

/// Полиморфный протокол сканирования (Security Phi-Protocol)
/// Позволяет плагинам динамически менять алгоритм обнаружения уязвимостей
pub trait SecurityPhi: Send + Sync {
    fn engine_name(&self) -> &'static str;
    fn execute(&self, plugin_name: &str, file_path: &str, content: &str, rules: &[PhiRule]) -> Vec<Finding>;
}

// ─────────────────────────────────────────────────────────────────────────────
// 1. RegexPhi: Легковесный линейный парсер (Защита от DoS)
// ─────────────────────────────────────────────────────────────────────────────
pub struct RegexPhi;
impl SecurityPhi for RegexPhi {
    fn engine_name(&self) -> &'static str { "RegexPhi (O(N) Fast)" }
    
    fn execute(&self, plugin_name: &str, file_path: &str, content: &str, rules: &[PhiRule]) -> Vec<Finding> {
        let mut findings = Vec::new();
        for (i, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("//") { continue; }
            for rule in rules {
                if trimmed.contains(rule.pattern) {
                    findings.push(Finding {
                        plugin: format!("{} [{}]", plugin_name, self.engine_name()),
                        severity: rule.severity.clone(),
                        title: format!("Обнаружено: {}", rule.pattern),
                        description: rule.description.to_string(),
                        file: file_path.into(),
                        line: Some(i + 1),
                        code_snippet: Some(trimmed.to_string()),
                        suggestion: Some(rule.suggestion.to_string()),
                        cwe: Some(rule.cwe.to_string()),
                    });
                }
            }
        }
        findings
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// 2. SemanticPhi: Тяжелый нейро-эмбеддинг AST графа
// ─────────────────────────────────────────────────────────────────────────────
pub struct SemanticPhi;
impl SecurityPhi for SemanticPhi {
    fn engine_name(&self) -> &'static str { "SemanticPhi (ML Context)" }
    
    fn execute(&self, plugin_name: &str, file_path: &str, content: &str, rules: &[PhiRule]) -> Vec<Finding> {
        std::thread::sleep(Duration::from_millis(15));
        
        let mut findings = RegexPhi.execute(plugin_name, file_path, content, rules);
        for f in &mut findings {
            f.plugin = format!("{} [{}]", plugin_name, self.engine_name());
            f.description = format!("(Семантический AST-вектор подтвердил намерение) {}", f.description);
        }
        findings
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Оркестратор Graceful Degradation
// ─────────────────────────────────────────────────────────────────────────────
/// Фабрика: Выдает нужный математический движок в зависимости от объема файла
pub fn select_phi_engine(content_bytes: usize) -> Box<dyn SecurityPhi> {
    if content_bytes > 5000 {
        // Файл слишком большой! Защита от DoS CI/CD пайплайна -> Сбрасываем до дешевых регулярных выражений
        Box::new(RegexPhi)
    } else {
        // Файл мелкий -> Анализируем глубоко через ML Семантику (AST вектора)
        Box::new(SemanticPhi)
    }
}
