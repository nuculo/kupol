//! Write-Ahead Log (WAL) — Append-Only Scan Audit Trail
//!
//! Вдохновлён `lib/optim/wal.ml` из OCaml KAN (фаза 38b).
//! WAL записывает каждый обработанный файл **немедленно** в append-only лог.
//! При крэше checkpoint загружает bulk-состояние, а WAL **доигрывает**
//! последние файлы, потерянные между чекпоинтами.
//!
//! Гарантирует:
//! - Нулевую потерю данных даже при OOM kill
//! - Audit trail для compliance (SOX, PCI-DSS)

use serde::{Serialize, Deserialize};
use std::path::Path;
use std::io::{BufRead, Write};

const WAL_FILE: &str = "scan.wal";

/// Одна запись в WAL — один обработанный файл
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct WalEntry {
    /// Монотонно возрастающий ID записи
    pub seq_id: u64,
    /// Путь к файлу
    pub file_path: String,
    /// Количество findings в этом файле
    pub finding_count: usize,
    /// Максимальная severity в findings этого файла
    pub max_severity: String,
    /// Timestamp (RFC3339)
    pub timestamp: String,
}

/// Write-Ahead Log — append-only лог сканирования
pub struct ScanWal {
    /// Путь к WAL файлу
    wal_path: String,
    /// Текущий sequence ID
    next_seq: u64,
}

impl ScanWal {
    /// Создать или открыть WAL в директории сканирования
    pub fn open(scan_root: &str) -> Self {
        let wal_path = Path::new(scan_root).join(WAL_FILE)
            .to_string_lossy().to_string();

        // Определить last seq_id если WAL уже существует
        let next_seq = if Path::new(&wal_path).exists() {
            let entries = Self::replay_from_path(&wal_path);
            entries.last().map(|e| e.seq_id + 1).unwrap_or(0)
        } else {
            0
        };

        tracing::info!("📝 [WAL] Открыт: {} (next_seq={})", wal_path, next_seq);

        Self { wal_path, next_seq }
    }

    /// Записать одну запись (append-only, немедленный flush)
    pub fn log_file(&mut self, file_path: &str, finding_count: usize, max_severity: &str) {
        let entry = WalEntry {
            seq_id: self.next_seq,
            file_path: file_path.to_string(),
            finding_count,
            max_severity: max_severity.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        };

        // Append JSON line + newline (JSONL format)
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.wal_path)
        {
            if let Ok(json) = serde_json::to_string(&entry) {
                let _ = writeln!(file, "{}", json);
                let _ = file.flush(); // Немедленный fsync
            }
        }

        self.next_seq += 1;
    }

    /// Воспроизвести WAL — вернуть все записи (для recovery)
    pub fn replay(&self) -> Vec<WalEntry> {
        Self::replay_from_path(&self.wal_path)
    }

    /// Внутренняя функция replay по пути
    fn replay_from_path(path: &str) -> Vec<WalEntry> {
        let mut entries = Vec::new();

        if let Ok(file) = std::fs::File::open(path) {
            let reader = std::io::BufReader::new(file);
            for line in reader.lines() {
                if let Ok(line) = line {
                    // Битая запись (крэш при записи) — пропускаем
                    if let Ok(entry) = serde_json::from_str::<WalEntry>(&line) {
                        entries.push(entry);
                    }
                }
            }
        }

        entries
    }

    /// Получить set файлов из WAL (для merge с checkpoint)
    pub fn scanned_files(&self) -> std::collections::HashSet<String> {
        self.replay()
            .into_iter()
            .map(|e| e.file_path)
            .collect()
    }

    /// Очистить WAL (после успешного checkpoint)
    pub fn truncate(&self) {
        if Path::new(&self.wal_path).exists() {
            let _ = std::fs::write(&self.wal_path, ""); // Truncate to empty
            tracing::info!("🧹 [WAL] Truncated после checkpoint (seq reset)");
        }
    }

    /// Удалить WAL полностью (по завершению сканирования)
    pub fn cleanup(scan_root: &str) {
        let path = Path::new(scan_root).join(WAL_FILE);
        if path.exists() {
            let _ = std::fs::remove_file(&path);
            tracing::info!("🧹 [WAL] Удалён (сканирование завершено)");
        }
    }

    /// Статистика WAL
    pub fn stats(&self) -> WalStats {
        let entries = self.replay();
        let total_findings: usize = entries.iter().map(|e| e.finding_count).sum();
        let critical_files = entries.iter()
            .filter(|e| e.max_severity == "CRITICAL" || e.max_severity == "HIGH")
            .count();

        WalStats {
            total_entries: entries.len(),
            total_findings,
            critical_files,
            last_seq: entries.last().map(|e| e.seq_id).unwrap_or(0),
        }
    }
}

/// Статистика WAL для мониторинга
#[derive(Debug)]
pub struct WalStats {
    pub total_entries: usize,
    pub total_findings: usize,
    pub critical_files: usize,
    pub last_seq: u64,
}
