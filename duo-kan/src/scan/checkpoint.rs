//! Scan Checkpoint/Resume — Crash-Resilient Incremental Scanning
//!
//! Вдохновлён `lib/kan/checkpoint.ml` из OCaml KAN.
//! При сканировании огромных монорепо (10к+ файлов), CI runner может быть
//! прерван по таймауту. Мы сериализуем состояние каждые N файлов в
//! `.duo-checkpoint.json` и восстанавливаемся с места остановки.

use serde::{Serialize, Deserialize};
use std::collections::HashSet;
use std::path::Path;
use crate::scan::Finding;

const CHECKPOINT_FILE: &str = ".duo-checkpoint.json";

/// Сериализуемое состояние сканирования
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct ScanCheckpoint {
    /// Файлы, которые уже были просканированы
    pub scanned_files: HashSet<String>,
    /// Промежуточные findings
    pub findings: Vec<Finding>,
    /// Временная метка создания чекпоинта
    pub timestamp: String,
    /// Общее количество файлов в задании
    pub total_files: usize,
}

impl ScanCheckpoint {
    /// Создать пустой чекпоинт
    pub fn new(total_files: usize) -> Self {
        Self {
            scanned_files: HashSet::new(),
            findings: Vec::new(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            total_files,
        }
    }

    /// Попытаться загрузить чекпоинт с диска
    pub fn load(scan_root: &str) -> Option<Self> {
        let path = Path::new(scan_root).join(CHECKPOINT_FILE);
        if path.exists() {
            if let Ok(data) = std::fs::read_to_string(&path) {
                if let Ok(ckpt) = serde_json::from_str::<ScanCheckpoint>(&data) {
                    tracing::info!(
                        "♻️  [Checkpoint] Восстановлен чекпоинт: {} файлов уже просканировано, {} findings",
                        ckpt.scanned_files.len(),
                        ckpt.findings.len()
                    );
                    return Some(ckpt);
                }
            }
        }
        None
    }

    /// Сохранить чекпоинт на диск
    pub fn save(&self, scan_root: &str) -> std::io::Result<()> {
        let path = Path::new(scan_root).join(CHECKPOINT_FILE);
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(&path, json)?;
        tracing::info!(
            "💾 [Checkpoint] Сохранён: {}/{} файлов, {} findings",
            self.scanned_files.len(),
            self.total_files,
            self.findings.len()
        );
        Ok(())
    }

    /// Удалить чекпоинт (по завершению сканирования)
    pub fn cleanup(scan_root: &str) {
        let path = Path::new(scan_root).join(CHECKPOINT_FILE);
        if path.exists() {
            let _ = std::fs::remove_file(&path);
            tracing::info!("🧹 [Checkpoint] Чекпоинт удалён (сканирование завершено)");
        }
    }

    /// Проверить, был ли файл уже просканирован
    pub fn is_scanned(&self, file_path: &str) -> bool {
        self.scanned_files.contains(file_path)
    }

    /// Отметить файл как просканированный и добавить findings
    pub fn mark_scanned(&mut self, file_path: &str, new_findings: &[Finding]) {
        self.scanned_files.insert(file_path.to_string());
        self.findings.extend(new_findings.iter().cloned());
    }

    /// Прогресс в процентах
    pub fn progress_pct(&self) -> f64 {
        if self.total_files == 0 { return 100.0; }
        (self.scanned_files.len() as f64 / self.total_files as f64) * 100.0
    }
}
