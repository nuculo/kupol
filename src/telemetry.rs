use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Warning,
    Error,
    Fatal,
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let icon = match self {
            Severity::Info => "ℹ️",
            Severity::Warning => "⚠️",
            Severity::Error => "🚨",
            Severity::Fatal => "💀",
        };
        write!(f, "{}", icon)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReportEntry {
    pub severity: Severity,
    pub message: String,
    pub file_path: Option<String>,
    pub line_number: Option<usize>,
    pub fix_suggestion: Option<String>,
}

/// The unified Validation Report (Replacing Ignition's `vcontext`)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ValidationReport {
    pub entries: Vec<ReportEntry>,
}

impl ValidationReport {
    pub fn new() -> Self {
        Self::default()
    }
    
    pub fn add(&mut self, severity: Severity, msg: impl Into<String>, path: Option<String>, fix: Option<String>) {
        self.entries.push(ReportEntry { 
            severity, 
            message: msg.into(), 
            file_path: path, 
            line_number: None, 
            fix_suggestion: fix 
        });
    }

    pub fn add_error(&mut self, msg: impl Into<String>) {
        self.add(Severity::Error, msg, None, None);
    }
    
    pub fn add_warning(&mut self, msg: impl Into<String>) {
        self.add(Severity::Warning, msg, None, None);
    }
    
    pub fn merge(&mut self, other: ValidationReport) {
        self.entries.extend(other.entries);
    }
    
    pub fn has_fatal(&self) -> bool {
        self.entries.iter().any(|e| matches!(e.severity, Severity::Fatal))
    }
    
    /// Compiles all entries into a beautiful Github-Flavored Markdown comment
    /// groups exactly like Ignition's grouped stdout format
    pub fn generate_markdown(&mut self) -> String {
        if self.entries.is_empty() { return "✅ All checks passed successfully.".into(); }
        
        // Sort highest priority first
        self.entries.sort_by(|a, b| b.severity.cmp(&a.severity));
        
        let mut md = String::from("## 📝 DevSecOps Swarm Validation Report\n\n");
        md.push_str("> Generated via Ignition-inspired `vcontext` telemetry. Grouped for readability.\n\n");
        
        for entry in &self.entries {
            md.push_str(&format!("### {} {}\n", entry.severity, entry.message));
            if let (Some(path), Some(fix)) = (&entry.file_path, &entry.fix_suggestion) {
                md.push_str(&format!("**File**: `{}`\n", path));
                md.push_str(&format!("**Auto-Fix Suggested**:\n```rust\n{}\n```\n", fix));
            }
            md.push('\n');
        }
        
        md
    }
}
