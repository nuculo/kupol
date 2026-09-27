use arc_swap::ArcSwap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

/// =========================================================================
/// 1. ETS-Like Lock-Free Shared Memory (`arc-swap`)
/// =========================================================================
/// Telecom-grade zero-copy sharing between actors (e.g. 60 Babylonian micro-actors)
/// Allows multiple concurrent readers with ZERO lock contention or cloning overhead.
pub struct EtsTable<T> {
    data: ArcSwap<T>,
}

impl<T> EtsTable<T> {
    pub fn new(initial: T) -> Self {
        Self {
            data: ArcSwap::from_pointee(initial),
        }
    }

    /// Read lock-free (yields an Arc pointer)
    pub fn read(&self) -> Arc<T> {
        self.data.load().clone()
    }

    /// Write (swaps the pointer atomically using Seqlock/RCU mechanisms natively)
    pub fn store(&self, value: T) {
        self.data.store(Arc::new(value));
    }
}

/// =========================================================================
/// 2. WAL Engine (Write-Ahead-Log for Actor Persistence)
/// =========================================================================
/// Simulates low-latency Linux `io_uring` appends for 0-data-loss resilience.
/// Used by NodeBrokerActor to sync Dirty -> Committed states to disk.
pub struct WalEngine {
    file: File,
}

impl WalEngine {
    pub async fn new(path: &str) -> anyhow::Result<Self> {
        let file = tokio::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .await?;
        Ok(Self { file })
    }

    /// O_DIRECT style asynchronous append with immediate fsync
    pub async fn append_log(&mut self, payload: &str) -> anyhow::Result<()> {
        self.file.write_all(payload.as_bytes()).await?;
        self.file.write_all(b"\n").await?;
        self.file.sync_data().await?; // Critical for WAL safety!
        Ok(())
    }
}

/// =========================================================================
/// 3. Backpressure Circuit Breaker
/// =========================================================================
/// Protects the Swarm from cascading failure during Monorepo/DDoS avalanches.
pub struct CircuitBreaker {
    pub max_requests_per_sec: usize,
    pub open_duration: Duration,
    
    current_count: usize,
    window_start: Instant,
    open_until: Option<Instant>,
}

impl CircuitBreaker {
    pub fn new(max_requests_per_sec: usize, open_duration: Duration) -> Self {
        Self {
            max_requests_per_sec,
            open_duration,
            current_count: 0,
            window_start: Instant::now(),
            open_until: None,
        }
    }

    /// Fast path evaluation: true if allowed, false if rejected due to backpressure.
    pub fn allow_request(&mut self) -> bool {
        let now = Instant::now();

        // 1. Check if circuit is currently OPEN (Blocking all)
        if let Some(until) = self.open_until {
            if now < until {
                return false; // Fast Fail
            } else {
                // Cooldown period elapsed. Transition to HALF-OPEN/CLOSED
                tracing::info!("🟢 [CircuitBreaker] Cooldown elapsed. Circuit CLOSED.");
                self.open_until = None;
                self.window_start = now;
                self.current_count = 0;
            }
        }

        // 2. Sliding window reset
        if now.duration_since(self.window_start).as_secs() >= 1 {
            self.window_start = now;
            self.current_count = 0;
        }

        self.current_count += 1;

        // 3. Trip check
        if self.current_count > self.max_requests_per_sec {
            tracing::error!(
                "🚥 [CircuitBreaker] OVERLOAD ({} req/s)! Tripping circuit OPEN for {:?}", 
                self.current_count, self.open_duration
            );
            self.open_until = Some(now + self.open_duration);
            return false;
        }

        true
    }
}
