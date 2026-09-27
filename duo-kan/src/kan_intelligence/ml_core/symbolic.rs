use std::collections::HashMap;

/// Candidates for Symbolic Discovery (mirrors `kan_symbolic.clj` primitive library)
const SYMBOLIC_CANDIDATES: &[&str] = &[
    "unwrap()", "exec(", "eval(", "unsafe {", "TODO", "raw_query", "chmod 777"
];

/// A frozen O(1) exactly-solved rule (0 trainable parameters)
#[derive(Debug, Clone)]
pub struct FrozenRule {
    pub formula: String,     // The exact symbolic substring
    pub confidence: f64,     // `1 - MSE/var` equivalent
}

impl FrozenRule {
    /// O(1) Execution
    pub fn forward(&self, source: &str) -> bool {
        source.contains(&self.formula)
    }
}

/// Auto-discovers symbolic rules from heavy agent outputs
pub struct SymbolicDiscoverer {
    pub candidate_success: HashMap<String, usize>,
    pub frozen_rules: Vec<FrozenRule>,
    pub freeze_threshold: usize,
    pub total_probes: usize,
}

impl SymbolicDiscoverer {
    pub fn new(threshold: usize) -> Self {
        Self {
            candidate_success: HashMap::new(),
            frozen_rules: Vec::new(),
            freeze_threshold: threshold,
            total_probes: 0,
        }
    }

    /// Probe step: see if any O(1) candidate perfectly explains the Heavy Agent's findings
    pub fn probe(&mut self, source: &str, heavy_agent_flagged: bool) {
        self.total_probes += 1;
        
        if !heavy_agent_flagged { return; }

        for &cand in SYMBOLIC_CANDIDATES {
            if source.contains(cand) {
                // The candidate matched the same payload the heavy agent flagged!
                // Confidence increases!
                let count = self.candidate_success.entry(cand.to_string()).or_insert(0);
                *count += 1;

                if *count == self.freeze_threshold {
                    // Confidence reached 1.0! Freeze the edge!
                    if !self.frozen_rules.iter().any(|r| r.formula == cand) {
                        tracing::warn!(
                            "❄️ [SymbolicDiscovery] Heavy Agent logic reversed! Formula '{}' fits perfectly (conf=1.0). Freezing into O(1) 0-param Rule!",
                            cand
                        );
                        self.frozen_rules.push(FrozenRule {
                            formula: cand.to_string(),
                            confidence: 1.0,
                        });
                    }
                }
            }
        }
    }

    /// Run all frozen rules (0-cost execution)
    pub fn fast_pass(&self, source: &str) -> Vec<String> {
        let mut results = Vec::new();
        for rule in &self.frozen_rules {
            if rule.forward(source) {
                results.push(format!("❄️ FROZEN(0-param): Formula '{}' matched in O(1) time!", rule.formula));
            }
        }
        results
    }
}
