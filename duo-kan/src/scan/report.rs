//! Report — экспорт результатов сканирования
//!
//! Форматы: Terminal Table, JSON, Markdown

use super::{ScanResult, Severity};
use colored::Colorize;
use tabled::{Table, Tabled, settings::Style};

#[derive(Tabled)]
struct FindingRow {
    #[tabled(rename = "")]
    emoji: String,
    #[tabled(rename = "Severity")]
    severity: String,
    #[tabled(rename = "Plugin")]
    plugin: String,
    #[tabled(rename = "Title")]
    title: String,
    #[tabled(rename = "File")]
    file: String,
    #[tabled(rename = "Line")]
    line: String,
    #[tabled(rename = "CWE")]
    cwe: String,
}

/// Вывод красивой таблицы в терминал (как в promptfoo)
pub fn print_table(result: &ScanResult) {
    println!();
    println!("{}", "━".repeat(70).dimmed());
    println!("  {} {}", "🛡️  Duo Architecture Guardian".bold(), format!("v{}", env!("CARGO_PKG_VERSION")).dimmed());
    println!("{}", "━".repeat(70).dimmed());
    println!();
    println!("  {} {}",  "ID:".dimmed(),       &result.id);
    println!("  {} {}", "Target:".dimmed(),     &result.target);
    println!("  {} {}ms", "Duration:".dimmed(), result.duration_ms);
    println!("  {} {}",  "Files:".dimmed(),     result.summary.total_files);
    println!("  {} {}",  "Findings:".dimmed(),  result.summary.total_findings);
    println!();

    if result.findings.is_empty() {
        println!("  {} No security issues found!", "✅".green());
        println!();
        return;
    }

    let rows: Vec<FindingRow> = result.findings.iter().map(|f| {
        let severity_str = match f.severity {
            Severity::Critical => f.severity.to_string().red().bold().to_string(),
            Severity::High     => f.severity.to_string().yellow().bold().to_string(),
            Severity::Medium   => f.severity.to_string().yellow().to_string(),
            Severity::Low      => f.severity.to_string().green().to_string(),
            Severity::Info     => f.severity.to_string().dimmed().to_string(),
        };

        // Shorten file path (safe for UTF-8)
        let short_file = if f.file.chars().count() > 35 {
            let skip = f.file.chars().count() - 32;
            format!("...{}", f.file.chars().skip(skip).collect::<String>())
        } else {
            f.file.clone()
        };

        // Safe truncation for title
        let short_title = if f.title.chars().count() > 40 {
            format!("{}...", f.title.chars().take(37).collect::<String>())
        } else {
            f.title.clone()
        };

        FindingRow {
            emoji: f.severity.emoji().to_string(),
            severity: severity_str,
            plugin: f.plugin.clone(),
            title: short_title,
            file: short_file,
            line: f.line.map(|l| l.to_string()).unwrap_or_default(),
            cwe: f.cwe.clone().unwrap_or_default(),
        }
    }).collect();

    let table = Table::new(rows).with(Style::rounded()).to_string();
    println!("{}", table);

    // Summary
    println!();
    println!("{}", "━".repeat(70).dimmed());
    print!("  Results: ");
    if result.summary.critical > 0 { print!("{} ", format!("🔴 {} critical", result.summary.critical).red().bold()); }
    if result.summary.high > 0     { print!("{} ", format!("🟠 {} high", result.summary.high).yellow().bold()); }
    if result.summary.medium > 0   { print!("{} ", format!("🟡 {} medium", result.summary.medium).yellow()); }
    if result.summary.low > 0      { print!("{} ", format!("🟢 {} low", result.summary.low).green()); }
    if result.summary.info > 0     { print!("{} ", format!("ℹ️  {} info", result.summary.info).dimmed()); }
    println!();

    let risk_label = super::graders::risk_label(result.summary.risk_score);
    println!("  Risk Score: {} ({}/10.0)", risk_label, format!("{:.1}", result.summary.risk_score).bold());
    println!("{}", "━".repeat(70).dimmed());
    println!();
}

/// Экспорт в JSON
pub fn to_json(result: &ScanResult) -> String {
    serde_json::to_string_pretty(result).unwrap_or_default()
}

/// Экспорт в Markdown
pub fn to_markdown(result: &ScanResult) -> String {
    let mut md = String::new();
    md.push_str(&format!("# 🛡️ Security Scan Report\n\n"));
    md.push_str(&format!("- **ID:** {}\n", result.id));
    md.push_str(&format!("- **Target:** {}\n", result.target));
    md.push_str(&format!("- **Date:** {}\n", result.timestamp.format("%Y-%m-%d %H:%M:%S UTC")));
    md.push_str(&format!("- **Duration:** {}ms\n", result.duration_ms));
    md.push_str(&format!("- **Risk Score:** {:.1}/10.0\n\n", result.summary.risk_score));

    md.push_str("## Summary\n\n");
    md.push_str(&format!("| Severity | Count |\n|----------|-------|\n"));
    if result.summary.critical > 0 { md.push_str(&format!("| 🔴 Critical | {} |\n", result.summary.critical)); }
    if result.summary.high > 0 { md.push_str(&format!("| 🟠 High | {} |\n", result.summary.high)); }
    if result.summary.medium > 0 { md.push_str(&format!("| 🟡 Medium | {} |\n", result.summary.medium)); }
    if result.summary.low > 0 { md.push_str(&format!("| 🟢 Low | {} |\n", result.summary.low)); }
    if result.summary.info > 0 { md.push_str(&format!("| ℹ️ Info | {} |\n", result.summary.info)); }

    md.push_str("\n## Findings\n\n");
    for f in &result.findings {
        md.push_str(&format!("### {} {} — {}\n\n", f.severity.emoji(), f.severity, f.title));
        md.push_str(&format!("- **Plugin:** {}\n", f.plugin));
        md.push_str(&format!("- **File:** `{}`", f.file));
        if let Some(line) = f.line { md.push_str(&format!(":L{}", line)); }
        md.push_str("\n");
        md.push_str(&format!("- **Description:** {}\n", f.description));
        if let Some(cwe) = &f.cwe { md.push_str(&format!("- **CWE:** {}\n", cwe)); }
        if let Some(snippet) = &f.code_snippet {
            md.push_str(&format!("\n```\n{}\n```\n", snippet));
        }
        if let Some(suggestion) = &f.suggestion {
            md.push_str(&format!("\n> 💡 {}\n", suggestion));
        }
        md.push_str("\n---\n\n");
    }
    md
}
