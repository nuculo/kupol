use std::cell::UnsafeCell;

/// 🟢 THE IMMUTABLE SHELL
/// This is the Event Sourcing struct that is safely passed between threads/actors.
/// It is clones, immutable, and perfect for strict Audit Logs.
#[derive(Debug, Clone)]
pub struct ImmutableAuditShell {
    pub findings_count: usize,
    pub max_severity: String,
    pub scan_time_micros: u64,
}

/// 🔴 THE MUTABLE CORE
/// This represents the internal state of a Scan Node.
/// Inspired by `tensor_v2.clj` (which wraps raw mutable double-arrays in Clojure closures),
/// we use `UnsafeCell` to strip away ALL Rust borrowing overhead (`RefCell` checks, locks, etc.).
/// This allows 0-cost `O(1)` pointer writes in a tight spin loop while parsing ASTs.
pub struct MutablePerformanceCore {
    raw_findings_count: UnsafeCell<usize>,
    max_severity: UnsafeCell<u8>, // 0=None, 1=Info, 2=Warn, 3=Critical
}

// Ensure it can be moved into a Tokio actor
unsafe impl Send for MutablePerformanceCore {}

impl MutablePerformanceCore {
    pub fn new() -> Self {
        Self {
            raw_findings_count: UnsafeCell::new(0),
            max_severity: UnsafeCell::new(0),
        }
    }

    /// Blazing fast unchecked mutation. 
    /// Used inside the heavy 11M msg/sec scan loop.
    pub fn record_finding_fast(&self, severity: u8) {
        unsafe {
            let count_ptr = self.raw_findings_count.get();
            *count_ptr += 1;
            
            let sev_ptr = self.max_severity.get();
            if severity > *sev_ptr {
                *sev_ptr = severity;
            }
        }
    }

    /// FREEZE AND SEAL
    /// Converts the unsafe mutable internal state into the strictly safe Immutable Shell.
    pub fn freeze(&self, time_micros: u64) -> ImmutableAuditShell {
        let count;
        let sev;
        unsafe {
            count = *self.raw_findings_count.get();
            sev = *self.max_severity.get();
        }

        let severity_str = match sev {
            0 => "NONE",
            1 => "INFO",
            2 => "WARN",
            _ => "CRITICAL",
        };

        ImmutableAuditShell {
            findings_count: count,
            max_severity: severity_str.to_string(),
            scan_time_micros: time_micros,
        }
    }
}
