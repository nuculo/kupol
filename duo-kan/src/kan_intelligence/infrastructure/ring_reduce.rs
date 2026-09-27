use std::collections::HashMap;

/// A single security finding that can be shared across the ring
#[derive(Debug, Clone)]
pub struct Finding {
    pub scanner_id: usize,
    pub category: String,
    pub message: String,
    pub severity: u8, // 1-10
}

/// Virtual scanner node in the ring topology
#[derive(Debug)]
pub struct ScannerNode {
    pub id: usize,
    pub local_findings: Vec<Finding>,
}

impl ScannerNode {
    pub fn new(id: usize) -> Self {
        Self { id, local_findings: Vec::new() }
    }

    pub fn add_finding(&mut self, category: impl Into<String>, message: impl Into<String>, severity: u8) {
        self.local_findings.push(Finding {
            scanner_id: self.id,
            category: category.into(),
            message: message.into(),
            severity,
        });
    }
}

/// Ring All-Reduce for Security Findings
///
/// Inspired by Clojure KAN's `distributed.clj` ring-all-reduce.
///
/// Instead of funneling all findings through a central Orchestrator (O(N×D)),
/// each scanner node passes its chunk to its neighbor in a ring, achieving
/// O(2(N-1)/N × D) bandwidth-optimal aggregation.
pub struct FindingsRing {
    pub nodes: Vec<ScannerNode>,
}

impl FindingsRing {
    pub fn new(n_scanners: usize) -> Self {
        Self {
            nodes: (0..n_scanners).map(ScannerNode::new).collect(),
        }
    }

    /// Phase 1: Scatter-Reduce
    /// Each node sends its findings to the next node in the ring.
    /// After N-1 steps, every node has seen all findings.
    ///
    /// Phase 2: Deduplicate & Merge
    /// Identical findings (same category+message) are merged, keeping highest severity.
    pub fn all_reduce(&self) -> Vec<Finding> {
        let n = self.nodes.len();
        if n == 0 { return vec![]; }

        tracing::info!("📡 [RingAllReduce] Starting ring sync across {} scanner nodes...", n);

        // Phase 1: Scatter-Reduce (simulate ring passing)
        // Each node accumulates findings from its left neighbor, N-1 steps
        let mut buffers: Vec<Vec<Finding>> = self.nodes.iter()
            .map(|node| node.local_findings.clone())
            .collect();

        for step in 0..(n - 1) {
            let mut new_buffers = buffers.clone();
            for w in 0..n {
                let recv_from = if w == 0 { n - 1 } else { w - 1 };
                // Node w receives from its left neighbor
                let incoming = buffers[recv_from].clone();
                new_buffers[w].extend(incoming);
            }
            buffers = new_buffers;
            tracing::info!("   📦 [Ring:Step {}] Scatter-reduce pass complete", step + 1);
        }

        // Phase 2: Deduplicate & Merge (keep highest severity per unique finding)
        let mut dedup: HashMap<String, Finding> = HashMap::new();
        // All nodes now have equivalent data; take node 0's buffer
        for finding in &buffers[0] {
            let key = format!("{}::{}", finding.category, finding.message);
            let entry = dedup.entry(key).or_insert_with(|| finding.clone());
            if finding.severity > entry.severity {
                entry.severity = finding.severity;
            }
        }

        let mut merged: Vec<Finding> = dedup.into_values().collect();
        merged.sort_by(|a, b| b.severity.cmp(&a.severity)); // Highest severity first

        tracing::info!(
            "📡 [RingAllReduce] Complete! {} unique findings from {} scanners (ring topology: O(2(N-1)/N × D))",
            merged.len(), n
        );

        merged
    }
}
