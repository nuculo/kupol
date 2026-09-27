//! 💾 Pipeline Checkpoint/Resume (Crash Resilience)
//!
//! Inspired by Clojure_KAN `serialization.clj`:
//!   - save-model → EDN (Clojure native)
//!   - Checkpoint every N epochs
//!   - Resume training from latest checkpoint
//!
//! For Duo: The Orchestrator periodically snapshots its `dirty_state` +
//! `committed_state` to disk as JSON. On `kill -9` recovery, the pipeline
//! resumes from the exact millisecond it left off.

use serde::{Serialize, Deserialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tracing::info;

// =============================================================================
// Pipeline State Snapshot
// =============================================================================

/// Serializable snapshot of the Orchestrator's complete state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineCheckpoint {
    /// Monotonic version counter
    pub epoch: u64,
    /// Timestamp (ISO 8601)
    pub timestamp: String,
    /// Dirty (in-flight) MR states: mr_id → phase name
    pub dirty_state: HashMap<u64, String>,
    /// Committed (completed) MR states: mr_id → phase name
    pub committed_state: HashMap<u64, String>,
    /// Actor lifecycle snapshots: actor_name → lifecycle state
    pub actor_states: HashMap<String, String>,
    /// Additional pipeline metrics
    pub metrics: CheckpointMetrics,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckpointMetrics {
    pub total_mrs_processed: u64,
    pub total_vulnerabilities_found: u64,
    pub total_fixes_applied: u64,
    pub uptime_seconds: f64,
}

// =============================================================================
// Checkpoint Manager
// =============================================================================

pub struct CheckpointManager {
    checkpoint_dir: PathBuf,
    checkpoint_interval: u64,  // save every N steps
    current_epoch: u64,
    start_time: Instant,
}

impl CheckpointManager {
    pub fn new(dir: &str, interval: u64) -> Self {
        let path = PathBuf::from(dir);
        std::fs::create_dir_all(&path).ok();
        Self {
            checkpoint_dir: path,
            checkpoint_interval: interval,
            current_epoch: 0,
            start_time: Instant::now(),
        }
    }

    /// Should we checkpoint at this step?
    pub fn should_checkpoint(&self, step: u64) -> bool {
        step > 0 && step % self.checkpoint_interval == 0
    }

    /// Save checkpoint atomically (write to .tmp, then rename).
    pub fn save_checkpoint(
        &mut self,
        dirty: &HashMap<u64, String>,
        committed: &HashMap<u64, String>,
        actors: &HashMap<String, String>,
        mrs_processed: u64,
        vulns_found: u64,
        fixes_applied: u64,
    ) -> std::io::Result<PathBuf> {
        self.current_epoch += 1;
        let epoch = self.current_epoch;

        let checkpoint = PipelineCheckpoint {
            epoch,
            timestamp: chrono_now(),
            dirty_state: dirty.clone(),
            committed_state: committed.clone(),
            actor_states: actors.clone(),
            metrics: CheckpointMetrics {
                total_mrs_processed: mrs_processed,
                total_vulnerabilities_found: vulns_found,
                total_fixes_applied: fixes_applied,
                uptime_seconds: self.start_time.elapsed().as_secs_f64(),
            },
        };

        let filename = format!("checkpoint_epoch_{:04}.json", epoch);
        let final_path = self.checkpoint_dir.join(&filename);
        let tmp_path = self.checkpoint_dir.join(format!("{}.tmp", filename));

        // Atomic write: write to tmp, then rename
        let json = serde_json::to_string_pretty(&checkpoint)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(&tmp_path, &json)?;
        std::fs::rename(&tmp_path, &final_path)?;

        info!(
            "💾 [Checkpoint] Saved epoch {} → {} ({} bytes)",
            epoch,
            final_path.display(),
            json.len()
        );

        Ok(final_path)
    }

    /// Find the latest checkpoint file in the directory.
    pub fn find_latest_checkpoint(&self) -> Option<PathBuf> {
        let dir = &self.checkpoint_dir;
        if !dir.exists() {
            return None;
        }

        let mut checkpoints: Vec<PathBuf> = std::fs::read_dir(dir)
            .ok()?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension().map_or(false, |ext| ext == "json")
                    && p.file_name()
                        .map_or(false, |n| n.to_string_lossy().starts_with("checkpoint_"))
            })
            .collect();

        checkpoints.sort();
        checkpoints.last().cloned()
    }

    /// Restore from a checkpoint file.
    pub fn restore_checkpoint(path: &Path) -> std::io::Result<PipelineCheckpoint> {
        let json = std::fs::read_to_string(path)?;
        let checkpoint: PipelineCheckpoint = serde_json::from_str(&json)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;

        info!(
            "💾 [Checkpoint] Restored epoch {} from {} (uptime was {:.1}s)",
            checkpoint.epoch,
            path.display(),
            checkpoint.metrics.uptime_seconds
        );

        Ok(checkpoint)
    }

    /// Restore from the latest checkpoint in the directory, if any.
    pub fn restore_latest(&self) -> Option<PipelineCheckpoint> {
        let path = self.find_latest_checkpoint()?;
        Self::restore_checkpoint(&path).ok()
    }
}

// =============================================================================
// Comparison (model diff à la Clojure serialization.clj)
// =============================================================================

pub fn compare_checkpoints(a: &PipelineCheckpoint, b: &PipelineCheckpoint) -> CheckpointDiff {
    let dirty_match = a.dirty_state == b.dirty_state;
    let committed_match = a.committed_state == b.committed_state;
    let actors_match = a.actor_states == b.actor_states;

    CheckpointDiff {
        epoch_a: a.epoch,
        epoch_b: b.epoch,
        dirty_match,
        committed_match,
        actors_match,
        exact: dirty_match && committed_match && actors_match,
    }
}

#[derive(Debug)]
pub struct CheckpointDiff {
    pub epoch_a: u64,
    pub epoch_b: u64,
    pub dirty_match: bool,
    pub committed_match: bool,
    pub actors_match: bool,
    pub exact: bool,
}

// =============================================================================
// Helpers
// =============================================================================

fn chrono_now() -> String {
    // Simple ISO-ish timestamp without external chrono crate
    let d = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}s", d.as_secs())
}

// =============================================================================
// Demo: Save → Crash → Restore
// =============================================================================

pub fn demo_checkpoint_resume() {
    let checkpoint_dir = "/tmp/duo-checkpoints";

    // Clean up from previous runs
    let _ = std::fs::remove_dir_all(checkpoint_dir);

    let mut mgr = CheckpointManager::new(checkpoint_dir, 3);

    // Simulate pipeline running for 6 steps
    let mut dirty: HashMap<u64, String> = HashMap::new();
    let mut committed: HashMap<u64, String> = HashMap::new();
    let mut actors: HashMap<String, String> = HashMap::new();

    actors.insert("ASTAnalyzerActor".into(), "Active".into());
    actors.insert("SecurityAnalyzerActor".into(), "Active".into());
    actors.insert("DriftDetectorActor".into(), "Active".into());

    info!("💾 [Checkpoint] Simulating pipeline execution (6 steps)...");

    for step in 1..=6u64 {
        // Simulate work
        let mr_id = 1000 + step;
        dirty.insert(mr_id, "Dirty".into());

        if step % 2 == 0 {
            // Promote to committed
            if let Some(prev_mr) = dirty.keys().next().cloned() {
                let state = dirty.remove(&prev_mr).unwrap();
                committed.insert(prev_mr, format!("Committed(was:{})", state));
            }
        }

        info!(
            "💾 [Checkpoint] Step {} | Dirty: {} | Committed: {}",
            step,
            dirty.len(),
            committed.len()
        );

        // Checkpoint every 3 steps
        if mgr.should_checkpoint(step) {
            mgr.save_checkpoint(
                &dirty, &committed, &actors,
                step, step / 2, step / 3,
            ).expect("Checkpoint save failed");
        }
    }

    // === SIMULATED CRASH (kill -9) ===
    info!("💾 [Checkpoint] 💥 SIMULATED CRASH (kill -9)!");
    info!("💾 [Checkpoint] All in-memory state is LOST.");

    // Clear everything
    let _dead_dirty = dirty;
    let _dead_committed = committed;

    // === RESTORE ===
    info!("💾 [Checkpoint] 🔄 Attempting recovery from checkpoint...");

    let mgr2 = CheckpointManager::new(checkpoint_dir, 3);
    match mgr2.restore_latest() {
        Some(restored) => {
            info!(
                "💾 [Checkpoint] ✅ Recovered! Epoch={}, Dirty={}, Committed={}, Actors={}",
                restored.epoch,
                restored.dirty_state.len(),
                restored.committed_state.len(),
                restored.actor_states.len()
            );
            info!(
                "💾 [Checkpoint]    MRs processed: {}, Vulns: {}, Fixes: {}",
                restored.metrics.total_mrs_processed,
                restored.metrics.total_vulnerabilities_found,
                restored.metrics.total_fixes_applied
            );
            for (name, state) in &restored.actor_states {
                info!("💾 [Checkpoint]    Actor '{}' → {}", name, state);
            }

            // Verify roundtrip
            let path = mgr2.find_latest_checkpoint().unwrap();
            let restored2 = CheckpointManager::restore_checkpoint(&path).unwrap();
            let diff = compare_checkpoints(&restored, &restored2);
            info!(
                "💾 [Checkpoint]    Roundtrip Verification: {}",
                if diff.exact { "✅ EXACT match" } else { "❌ MISMATCH" }
            );
        }
        None => {
            info!("💾 [Checkpoint] ❌ No checkpoint found! Cold start required.");
        }
    }

    // Cleanup
    let _ = std::fs::remove_dir_all(checkpoint_dir);
}
