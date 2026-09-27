//! Scan Pipeline — promptfoo-inspired security scanning engine
//!
//! Architecture: Plugin → Grade → Report (аналог Plugin → Strategy → Grader из promptfoo)

pub mod plugins;
pub mod graders;
pub mod report;
pub mod moe_router;
pub mod phi;
pub mod dropout;
pub mod checkpoint;
pub mod wal;
pub mod dag;
pub mod stream;
pub mod dsl;
pub mod fuzzy;
pub mod sparse_router;
pub mod crdt;
pub mod ode;
pub mod tensor_profile;
pub mod quantize;
pub mod gadt_rule;
pub mod graph_kan;
pub mod forward_ad;
pub mod lora_edge;
pub mod what_if;
pub mod gravity;
pub mod accel_detector;
pub mod basis_cache;
pub mod surge_queue;
pub mod adaptive_grid;
pub mod redshift;
pub mod kv_cache;
pub mod reverse_ad;
pub mod scan_policy;
pub mod frozen_trainable;
pub mod topk_sampler;

use serde::{Serialize, Deserialize};
use chrono::{DateTime, Utc};

// ─────────────────────────────────────────────────────────────────────────────
// § Core Types
// ─────────────────────────────────────────────────────────────────────────────

/// Severity уязвимости (CVSS-подобная шкала)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl Severity {
    pub fn emoji(&self) -> &str {
        match self {
            Severity::Critical => "🔴",
            Severity::High     => "🟠",
            Severity::Medium   => "🟡",
            Severity::Low      => "🟢",
            Severity::Info     => "ℹ️",
        }
    }

    pub fn score(&self) -> f64 {
        match self {
            Severity::Critical => 9.0,
            Severity::High     => 7.0,
            Severity::Medium   => 5.0,
            Severity::Low      => 3.0,
            Severity::Info     => 1.0,
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "critical" => Severity::Critical,
            "high"     => Severity::High,
            "medium"   => Severity::Medium,
            "low"      => Severity::Low,
            _          => Severity::Info,
        }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", match self {
            Severity::Critical => "CRITICAL",
            Severity::High     => "HIGH",
            Severity::Medium   => "MEDIUM",
            Severity::Low      => "LOW",
            Severity::Info     => "INFO",
        })
    }
}

/// Отдельная находка (finding) от плагина
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Finding {
    pub plugin: String,
    pub severity: Severity,
    pub title: String,
    pub description: String,
    pub file: String,
    pub line: Option<usize>,
    pub code_snippet: Option<String>,
    pub suggestion: Option<String>,
    pub cwe: Option<String>,
}

/// Результат сканирования
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub target: String,
    pub duration_ms: u64,
    pub findings: Vec<Finding>,
    pub summary: ScanSummary,
}

/// Сводка сканирования
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanSummary {
    pub total_files: usize,
    pub total_findings: usize,
    pub critical: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub info: usize,
    pub risk_score: f64,
}

impl ScanSummary {
    pub fn from_findings(findings: &[Finding], total_files: usize) -> Self {
        let critical = findings.iter().filter(|f| f.severity == Severity::Critical).count();
        let high = findings.iter().filter(|f| f.severity == Severity::High).count();
        let medium = findings.iter().filter(|f| f.severity == Severity::Medium).count();
        let low = findings.iter().filter(|f| f.severity == Severity::Low).count();
        let info = findings.iter().filter(|f| f.severity == Severity::Info).count();

        // CVSS-like risk score: max(finding scores) + distribution penalty
        let max_score = findings.iter()
            .map(|f| f.severity.score())
            .fold(0.0_f64, f64::max);

        let distribution_penalty = if critical > 2 { 1.0 }
            else if critical > 0 && high > 2 { 0.5 }
            else { 0.0 };

        let risk_score = (max_score + distribution_penalty).min(10.0);

        ScanSummary {
            total_files,
            total_findings: findings.len(),
            critical, high, medium, low, info,
            risk_score,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// § Scan Pipeline
// ─────────────────────────────────────────────────────────────────────────────

/// Запуск полного сканирования
pub fn run_scan(path: &str) -> ScanResult {
    let start = std::time::Instant::now();
    let scan_id = format!("scan-{}", uuid::Uuid::new_v4().to_string().split('-').next().unwrap());

    // Сбор файлов
    let files = collect_files(path);
    let total_files = files.len();

    // Запуск всех плагинов
    let all_plugins = plugins::all_plugins();

    // ♻️ Checkpoint: попытка восстановления
    let ckpt = checkpoint::ScanCheckpoint::load(path)
        .unwrap_or_else(|| checkpoint::ScanCheckpoint::new(total_files));

    // Восстановленные findings из предыдущего прогона
    let restored_findings: Vec<Finding> = ckpt.findings.clone();

    // 📝 WAL: открыть Write-Ahead Log + merge с checkpoint
    let scan_wal = wal::ScanWal::open(path);
    let wal_files = scan_wal.scanned_files(); // Файлы из WAL (между чекпоинтами)

    let ckpt_mutex = std::sync::Mutex::new(ckpt);
    let wal_mutex = std::sync::Mutex::new(scan_wal);
    let path_owned = path.to_string();

    // 🚀 Rayon Parallel Scan: каждый файл на отдельном ядре CPU
    use rayon::prelude::*;

    let parallel_findings: Vec<Finding> = files.par_iter()
        .filter(|file_path| {
            // ♻️ Пропустить уже просканированные файлы (чекпоинт + WAL)
            let ckpt = ckpt_mutex.lock().unwrap();
            !ckpt.is_scanned(file_path) && !wal_files.contains(*file_path)
        })
        .flat_map(|file_path| {
            let mut file_findings = Vec::new();
            if let Ok(content) = std::fs::read_to_string(file_path) {
                // Анализ файла через Sparse Router (выбираем Top-K плагинов)
                let active_expert_names = moe_router::MoeRouter::assign_experts(file_path, &content);

                // 🎲 Security Dropout
                let dropout_layer = dropout::SecurityDropout::production();
                let (masked_experts, _dropped) = dropout_layer.apply_mask(&active_expert_names);

                for plugin in &all_plugins {
                    if masked_experts.contains(&plugin.name()) {
                        let mut plugin_findings = plugin.scan(file_path, &content);
                        file_findings.append(&mut plugin_findings);
                    }
                }

                // 📝 WAL: немедленная запись (до чекпоинта!)
                let max_sev = file_findings.iter()
                    .map(|f| &f.severity)
                    .max()
                    .map(|s| format!("{:?}", s))
                    .unwrap_or_else(|| "NONE".to_string());
                if let Ok(mut w) = wal_mutex.lock() {
                    w.log_file(file_path, file_findings.len(), &max_sev);
                }

                // 💾 Thread-safe checkpoint update
                if let Ok(mut ckpt) = ckpt_mutex.lock() {
                    ckpt.mark_scanned(file_path, &file_findings);
                    // Периодический сброс каждые 50 файлов
                    if ckpt.scanned_files.len() % 50 == 0 {
                        let _ = ckpt.save(&path_owned);
                        // WAL truncate после успешного checkpoint
                        if let Ok(w) = wal_mutex.lock() {
                            w.truncate();
                        }
                    }
                }
            }
            file_findings
        })
        .collect();

    // Merge restored + parallel findings
    let mut findings: Vec<Finding> = restored_findings;
    findings.extend(parallel_findings);

    // 🧹 Сканирование завершено — удалить чекпоинт + WAL
    checkpoint::ScanCheckpoint::cleanup(path);
    wal::ScanWal::cleanup(path);

    // Сортировка по severity (critical first)
    findings.sort_by(|a, b| b.severity.cmp(&a.severity));

    let summary = ScanSummary::from_findings(&findings, total_files);
    let duration = start.elapsed().as_millis() as u64;

    ScanResult {
        id: scan_id,
        timestamp: Utc::now(),
        target: path.to_string(),
        duration_ms: duration,
        findings,
        summary,
    }
}

/// Сбор файлов для сканирования
fn collect_files(path: &str) -> Vec<String> {
    let mut files = Vec::new();
    let extensions = ["rs", "ts", "tsx", "js", "py", "go", "rb", "java", "yaml", "yml", "toml"];

    if std::path::Path::new(path).is_file() {
        files.push(path.to_string());
    } else {
        for ext in &extensions {
            let pattern = format!("{}/**/*.{}", path, ext);
            if let Ok(entries) = glob::glob(&pattern) {
                for entry in entries.flatten() {
                    let p = entry.to_string_lossy().to_string();
                    // skip target/, node_modules/, .git/
                    if !p.contains("/target/") && !p.contains("/node_modules/") && !p.contains("/.git/") {
                        files.push(p);
                    }
                }
            }
        }
    }
    files
}
