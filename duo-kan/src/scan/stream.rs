//! SSE Streaming Findings — Real-Time Scan Output
//!
//! Вдохновлён `bin/server.ml` из OCaml KAN (фаза 34).
//! Вместо batch-ответа после завершения сканирования,
//! каждый finding стримится **мгновенно** через SSE-подобные события.
//! UX как у ChatGPT — "живая" лента находок.

use std::sync::mpsc;
use std::time::Instant;
use serde::Serialize;

// ── SSE Event Types ──────────────────────────────────────────────────────────

/// SSE-событие сканирования
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum ScanEvent {
    /// Начало сканирования
    #[serde(rename = "scan_start")]
    ScanStart {
        total_files: usize,
        scan_id: String,
        timestamp: String,
    },

    /// Прогресс (файл обработан)
    #[serde(rename = "progress")]
    Progress {
        file_path: String,
        files_done: usize,
        total_files: usize,
        percent: f64,
    },

    /// Найден finding (стримится мгновенно!)
    #[serde(rename = "finding")]
    Finding {
        severity: String,
        rule_id: String,
        message: String,
        file_path: String,
        line: usize,
        cwe_id: String,
    },

    /// Severity summary (обновляется с каждым finding)
    #[serde(rename = "severity_update")]
    SeverityUpdate {
        critical: usize,
        high: usize,
        medium: usize,
        low: usize,
        info: usize,
    },

    /// Сканирование завершено
    #[serde(rename = "scan_done")]
    ScanDone {
        total_findings: usize,
        total_files: usize,
        duration_ms: u64,
        risk_score: f64,
    },
}

impl ScanEvent {
    /// Форматировать как SSE data line
    pub fn to_sse(&self) -> String {
        match serde_json::to_string(self) {
            Ok(json) => format!("data: {}\n\n", json),
            Err(_) => "data: {\"type\":\"error\"}\n\n".to_string(),
        }
    }
}

// ── Finding Stream ───────────────────────────────────────────────────────────

/// Потоковый эмиттер findings — канал для SSE
pub struct FindingStream {
    sender: mpsc::Sender<ScanEvent>,
    start_time: Instant,
    /// Счётчики severity (обновляются на лету)
    pub critical: usize,
    pub high: usize,
    pub medium: usize,
    pub low: usize,
    pub info: usize,
    pub total_findings: usize,
    pub files_done: usize,
    pub total_files: usize,
}

/// Приёмник SSE-событий
pub struct FindingReceiver {
    receiver: mpsc::Receiver<ScanEvent>,
}

impl FindingReceiver {
    /// Получить следующее событие (блокирующее)
    pub fn recv(&self) -> Option<ScanEvent> {
        self.receiver.recv().ok()
    }

    /// Попытаться получить событие без блокировки
    pub fn try_recv(&self) -> Option<ScanEvent> {
        self.receiver.try_recv().ok()
    }

    /// Собрать все события в вектор (non-blocking drain)
    pub fn drain_all(&self) -> Vec<ScanEvent> {
        let mut events = Vec::new();
        while let Ok(event) = self.receiver.try_recv() {
            events.push(event);
        }
        events
    }
}

impl FindingStream {
    /// Создать пару (stream, receiver) — аналог Go channel
    pub fn new(total_files: usize, scan_id: &str) -> (Self, FindingReceiver) {
        let (sender, receiver) = mpsc::channel();

        let stream = Self {
            sender: sender.clone(),
            start_time: Instant::now(),
            critical: 0,
            high: 0,
            medium: 0,
            low: 0,
            info: 0,
            total_findings: 0,
            files_done: 0,
            total_files,
        };

        // Emit scan_start
        let _ = sender.send(ScanEvent::ScanStart {
            total_files,
            scan_id: scan_id.to_string(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        });

        (stream, FindingReceiver { receiver })
    }

    /// Стримить finding (мгновенная отправка!)
    pub fn emit_finding(&mut self, severity: &str, rule_id: &str, message: &str, file_path: &str, line: usize, cwe_id: &str) {
        // Обновить счётчики
        match severity {
            "CRITICAL" => self.critical += 1,
            "HIGH" => self.high += 1,
            "MEDIUM" => self.medium += 1,
            "LOW" => self.low += 1,
            _ => self.info += 1,
        }
        self.total_findings += 1;

        // Emit finding event
        let _ = self.sender.send(ScanEvent::Finding {
            severity: severity.to_string(),
            rule_id: rule_id.to_string(),
            message: message.to_string(),
            file_path: file_path.to_string(),
            line,
            cwe_id: cwe_id.to_string(),
        });

        // Emit severity update
        let _ = self.sender.send(ScanEvent::SeverityUpdate {
            critical: self.critical,
            high: self.high,
            medium: self.medium,
            low: self.low,
            info: self.info,
        });
    }

    /// Пометить файл как обработанный (прогресс)
    pub fn emit_progress(&mut self, file_path: &str) {
        self.files_done += 1;
        let percent = self.files_done as f64 / self.total_files as f64 * 100.0;

        let _ = self.sender.send(ScanEvent::Progress {
            file_path: file_path.to_string(),
            files_done: self.files_done,
            total_files: self.total_files,
            percent,
        });
    }

    /// Завершить сканирование
    pub fn finish(self, risk_score: f64) {
        let duration_ms = self.start_time.elapsed().as_millis() as u64;

        let _ = self.sender.send(ScanEvent::ScanDone {
            total_findings: self.total_findings,
            total_files: self.total_files,
            duration_ms,
            risk_score,
        });
        // sender drops here → receiver will get None on next recv
    }
}
