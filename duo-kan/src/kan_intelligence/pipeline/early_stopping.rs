//! ⏳ Early Stopping for Actor Lease
//!
//! Inspired by Clojure_KAN `early_stopping.clj`:
//!   - Patience counter tracks N epochs without validation improvement
//!   - Saves best weights, halts training when patience exhausted
//!   - Restores best model checkpoint
//!
//! For Duo: Instead of static 30s Lease TTLs, we track consecutive
//! `TxResult::Ignored` responses. When an actor shows no progress
//! for `patience` rounds, its lease shrinks → Draining → freed.
//! When it delivers useful results, lease expands back.

use std::collections::HashMap;
use std::time::{Duration, Instant};
use tracing::info;

// =============================================================================
// Tx Result (what an actor returns after processing)
// =============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum TxResult {
    /// Actor did useful work
    Committed(String),
    /// Actor found nothing to do
    Ignored,
    /// Actor produced a critical finding
    Critical(String),
}

// =============================================================================
// Lease State Machine
// =============================================================================

#[derive(Debug, Clone, PartialEq)]
pub enum LeaseState {
    /// Full capacity, healthy lease
    Active,
    /// Lease shrinking, receiving fewer tasks
    Throttled,
    /// Draining: finishing current work, no new tasks
    Draining,
    /// Actor fully stopped
    Stopped,
}

impl std::fmt::Display for LeaseState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LeaseState::Active => write!(f, "🟢 Active"),
            LeaseState::Throttled => write!(f, "🟡 Throttled"),
            LeaseState::Draining => write!(f, "🔴 Draining"),
            LeaseState::Stopped => write!(f, "⏹️  Stopped"),
        }
    }
}

// =============================================================================
// Adaptive Lease Monitor
// =============================================================================

/// Monitors actor productivity and adaptively adjusts lease duration.
///
/// Like early_stopping.clj's patience counter:
/// - `patience`: how many consecutive Ignored results before throttling
/// - `no_progress_counter`: current count of consecutive Ignored
/// - `best_metric`: best productivity score seen
/// - `state`: current lease state
#[derive(Debug, Clone)]
pub struct AdaptiveLeaseMonitor {
    pub actor_name: String,
    /// Max consecutive Ignored results before state transition
    pub patience: usize,
    /// Current consecutive Ignored count
    pub no_progress_counter: usize,
    /// Best productivity metric ever seen
    pub best_metric: f64,
    /// Current lease duration
    pub lease_ttl: Duration,
    /// Original (max) lease duration
    pub max_lease_ttl: Duration,
    /// Minimum lease duration before Draining
    pub min_lease_ttl: Duration,
    /// Current state
    pub state: LeaseState,
    /// Total useful results delivered
    pub total_committed: u64,
    /// Total ignored results
    pub total_ignored: u64,
    /// When this monitor was created
    pub created_at: Instant,
}

impl AdaptiveLeaseMonitor {
    pub fn new(actor_name: &str, patience: usize, initial_ttl: Duration) -> Self {
        Self {
            actor_name: actor_name.to_string(),
            patience,
            no_progress_counter: 0,
            best_metric: 0.0,
            lease_ttl: initial_ttl,
            max_lease_ttl: initial_ttl,
            min_lease_ttl: Duration::from_millis(500),
            state: LeaseState::Active,
            total_committed: 0,
            total_ignored: 0,
            created_at: Instant::now(),
        }
    }

    /// Feed a TxResult into the monitor. Returns the new LeaseState.
    ///
    /// Like `check-stop!` in early_stopping.clj:
    /// - Improvement → reset counter, save best, expand lease
    /// - No improvement → increment counter
    /// - Counter >= patience → throttle or drain
    pub fn record_result(&mut self, result: &TxResult) -> &LeaseState {
        if self.state == LeaseState::Stopped {
            return &self.state;
        }

        match result {
            TxResult::Committed(_) | TxResult::Critical(_) => {
                // Improvement! Reset counter, potentially expand lease
                self.no_progress_counter = 0;
                self.total_committed += 1;

                let metric = self.total_committed as f64
                    / (self.total_committed + self.total_ignored).max(1) as f64;

                if metric > self.best_metric {
                    self.best_metric = metric;
                }

                // Expand lease (recovery from throttled state)
                if self.state == LeaseState::Throttled {
                    self.lease_ttl = self.max_lease_ttl;
                    self.state = LeaseState::Active;
                    info!(
                        "⏳ [EarlyStop] '{}' recovered → {} (lease: {:?})",
                        self.actor_name, self.state, self.lease_ttl
                    );
                }
            }
            TxResult::Ignored => {
                self.no_progress_counter += 1;
                self.total_ignored += 1;

                if self.no_progress_counter >= self.patience {
                    match self.state {
                        LeaseState::Active => {
                            // First threshold: shrink lease by 50%
                            self.lease_ttl = Duration::from_millis(
                                (self.lease_ttl.as_millis() as u64 / 2).max(self.min_lease_ttl.as_millis() as u64)
                            );
                            self.state = LeaseState::Throttled;
                            info!(
                                "⏳ [EarlyStop] '{}' no progress for {} rounds → {} (lease: {:?})",
                                self.actor_name, self.patience, self.state, self.lease_ttl
                            );
                        }
                        LeaseState::Throttled => {
                            // Second threshold: start draining
                            self.state = LeaseState::Draining;
                            self.lease_ttl = self.min_lease_ttl;
                            info!(
                                "⏳ [EarlyStop] '{}' still idle → {} (lease: {:?})",
                                self.actor_name, self.state, self.lease_ttl
                            );
                        }
                        LeaseState::Draining => {
                            // Third threshold: fully stop
                            self.state = LeaseState::Stopped;
                            self.lease_ttl = Duration::ZERO;
                            info!(
                                "⏳ [EarlyStop] '{}' patience exhausted → {}",
                                self.actor_name, self.state
                            );
                        }
                        LeaseState::Stopped => {}
                    }
                    // Reset counter for next patience window
                    self.no_progress_counter = 0;
                }
            }
        }

        &self.state
    }

    /// Current productivity ratio (committed / total)
    pub fn productivity(&self) -> f64 {
        let total = self.total_committed + self.total_ignored;
        if total == 0 { 0.0 } else { self.total_committed as f64 / total as f64 }
    }
}

// =============================================================================
// Fleet Monitor (tracks all actors)
// =============================================================================

pub struct FleetLeaseMonitor {
    pub monitors: HashMap<String, AdaptiveLeaseMonitor>,
}

impl FleetLeaseMonitor {
    pub fn new() -> Self {
        Self { monitors: HashMap::new() }
    }

    pub fn register(&mut self, name: &str, patience: usize, ttl: Duration) {
        self.monitors.insert(
            name.to_string(),
            AdaptiveLeaseMonitor::new(name, patience, ttl),
        );
    }

    pub fn record(&mut self, name: &str, result: &TxResult) -> Option<LeaseState> {
        self.monitors.get_mut(name).map(|m| m.record_result(result).clone())
    }

    pub fn active_count(&self) -> usize {
        self.monitors.values().filter(|m| m.state != LeaseState::Stopped).count()
    }

    pub fn summary(&self) {
        info!("⏳ [Fleet] ─── Actor Lease Summary ───");
        for (name, m) in &self.monitors {
            info!(
                "⏳ [Fleet]   {} | {} | Lease: {:?} | Productivity: {:.0}% | Committed: {} | Ignored: {}",
                name, m.state, m.lease_ttl, m.productivity() * 100.0,
                m.total_committed, m.total_ignored
            );
        }
        info!(
            "⏳ [Fleet]   Active actors: {}/{}",
            self.active_count(),
            self.monitors.len()
        );
    }
}

// =============================================================================
// Demo: Simulate actor fleet with varying productivity
// =============================================================================

pub fn demo_early_stopping() {
    info!("⏳ [EarlyStop] Simulating fleet of 4 actors with adaptive leases...");

    let mut fleet = FleetLeaseMonitor::new();
    let ttl = Duration::from_secs(30);

    fleet.register("ASTAnalyzerActor", 3, ttl);         // patience=3
    fleet.register("SecurityAnalyzerActor", 3, ttl);     // patience=3
    fleet.register("StyleCheckerActor", 3, ttl);         // patience=3
    fleet.register("ObsoletePluginActor", 3, ttl);       // patience=3

    // Simulate 20 rounds of work
    // AST: consistently productive
    // Security: productive early, then slows down
    // Style: always idle (should get stopped)
    // Obsolete: idle, then suddenly productive (recovery test)

    let scenarios: Vec<(&str, Vec<TxResult>)> = vec![
        ("ASTAnalyzerActor", vec![
            TxResult::Committed("found fn".into()), TxResult::Committed("found struct".into()),
            TxResult::Ignored, TxResult::Committed("found impl".into()),
            TxResult::Committed("found trait".into()), TxResult::Ignored,
            TxResult::Committed("found enum".into()), TxResult::Committed("found mod".into()),
            TxResult::Committed("found use".into()), TxResult::Committed("found type".into()),
            TxResult::Ignored, TxResult::Committed("found const".into()),
            TxResult::Committed("found static".into()), TxResult::Committed("found macro".into()),
            TxResult::Committed("found attr".into()), TxResult::Committed("found lifetime".into()),
        ]),
        ("SecurityAnalyzerActor", vec![
            TxResult::Critical("SQL injection!".into()), TxResult::Committed("checked auth".into()),
            TxResult::Committed("checked XSS".into()), TxResult::Ignored,
            TxResult::Ignored, TxResult::Ignored,                    // patience hit → Throttled
            TxResult::Ignored, TxResult::Ignored, TxResult::Ignored, // patience hit → Draining
            TxResult::Committed("late find".into()),                  // recovery!
            TxResult::Ignored, TxResult::Ignored, TxResult::Ignored, // → Throttled again
            TxResult::Ignored, TxResult::Ignored, TxResult::Ignored, // → Draining
        ]),
        ("StyleCheckerActor", vec![
            TxResult::Ignored, TxResult::Ignored, TxResult::Ignored, // → Throttled
            TxResult::Ignored, TxResult::Ignored, TxResult::Ignored, // → Draining
            TxResult::Ignored, TxResult::Ignored, TxResult::Ignored, // → Stopped
            TxResult::Ignored, TxResult::Ignored, TxResult::Ignored,
            TxResult::Ignored, TxResult::Ignored, TxResult::Ignored,
            TxResult::Ignored,
        ]),
        ("ObsoletePluginActor", vec![
            TxResult::Ignored, TxResult::Ignored, TxResult::Ignored, // → Throttled
            TxResult::Ignored, TxResult::Ignored,
            TxResult::Critical("supply chain attack!".into()),         // recovery!
            TxResult::Committed("verified dep".into()),
            TxResult::Committed("checked license".into()),
            TxResult::Ignored, TxResult::Ignored,
            TxResult::Committed("found CVE".into()),
            TxResult::Ignored, TxResult::Ignored, TxResult::Ignored,
            TxResult::Ignored, TxResult::Ignored,
        ]),
    ];

    let max_rounds = scenarios.iter().map(|(_, v)| v.len()).max().unwrap_or(0);

    for round in 0..max_rounds {
        for (name, results) in &scenarios {
            if let Some(result) = results.get(round) {
                fleet.record(name, result);
            }
        }
    }

    info!("⏳ [EarlyStop] ════════════════════════════════════════");
    fleet.summary();
    info!("⏳ [EarlyStop] ════════════════════════════════════════");
}
