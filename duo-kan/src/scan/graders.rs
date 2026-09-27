//! Graders — CVSS-подобная система оценки рисков
//!
//! Вдохновлено promptfoo riskScoring.ts

use super::{Finding, Severity, ScanResult};

/// Рассчитать общий Risk Score для результата сканирования
/// Формула: Impact_Base + Exploitation_Modifier + Complexity_Penalty
pub fn calculate_risk_score(result: &ScanResult) -> f64 {
    result.summary.risk_score
}

/// Категоризация общего уровня Risk
pub fn risk_label(score: f64) -> &'static str {
    match score as u32 {
        9..=10 => "🔴 CRITICAL",
        7..=8  => "🟠 HIGH",
        4..=6  => "🟡 MEDIUM",
        1..=3  => "🟢 LOW",
        _      => "ℹ️  NONE",
    }
}

/// Фильтрация findings по минимальному severity
pub fn filter_by_severity(findings: &[Finding], min: &Severity) -> Vec<Finding> {
    findings.iter()
        .filter(|f| &f.severity >= min)
        .cloned()
        .collect()
}
