use serde::{Deserialize, Serialize};

// =============================================================================
// 🔌 PhiFunction Protocol (from Clojure KAN phi_protocol.clj)
//    Plug-and-play trait for scanning engines. Hot-swappable at runtime.
// =============================================================================

/// The ScanEngine trait — our PhiFunction equivalent.
/// Each implementation provides a different scanning strategy.
/// Actors can hot-swap engines at runtime via `make_engine()`.
pub trait ScanEngine: Send + Sync {
    /// Unique name of this engine
    fn name(&self) -> &'static str;
    
    /// Execute a scan on the given source code. Returns list of findings.
    fn scan(&self, source: &str) -> Vec<String>;
    
    /// Generate a structured report from findings
    fn report(&self, findings: &[String]) -> String;
    
    /// Confidence score (0.0 - 1.0) of the engine for the given code
    fn confidence(&self, source: &str) -> f64;
}

// =============================================================================
// 🔍 DFS Taint Analysis Engine (like BSplinePhi — the default, most precise)
// =============================================================================
pub struct DfsTaintEngine;

impl ScanEngine for DfsTaintEngine {
    fn name(&self) -> &'static str { "DFS Taint Analysis" }
    
    fn scan(&self, source: &str) -> Vec<String> {
        let mut findings = Vec::new();
        if source.contains("raw_query") {
            findings.push("SQL Injection: raw_query() used without sanitization".into());
        }
        if source.contains("exec(") || source.contains("eval(") {
            findings.push("Code Injection: exec/eval detected in data path".into());
        }
        findings
    }
    
    fn report(&self, findings: &[String]) -> String {
        format!("### 🔍 DFS Taint Analysis\n{}", 
            findings.iter().map(|f| format!("- 🚨 {}", f)).collect::<Vec<_>>().join("\n"))
    }
    
    fn confidence(&self, source: &str) -> f64 {
        // High confidence on Rust/SQL code, lower on templates
        if source.contains("fn ") || source.contains("query") { 0.95 } else { 0.6 }
    }
}

// =============================================================================
// 🎯 Pattern Matching Engine (like PolyPhi — fast, for simple checks)
// =============================================================================
pub struct PatternMatchEngine;

impl ScanEngine for PatternMatchEngine {
    fn name(&self) -> &'static str { "Pattern Matching" }
    
    fn scan(&self, source: &str) -> Vec<String> {
        let mut findings = Vec::new();
        let patterns = [
            ("unwrap()", "Potential panic: unwrap() without error handling"),
            ("unsafe {", "Unsafe block detected"),
            ("TODO", "Unresolved TODO marker"),
            (".clone()", "Excessive cloning (potential perf issue)"),
        ];
        for (pat, msg) in patterns {
            if source.contains(pat) {
                findings.push(msg.to_string());
            }
        }
        findings
    }
    
    fn report(&self, findings: &[String]) -> String {
        format!("### 🎯 Pattern Matching\n{}", 
            findings.iter().map(|f| format!("- ⚠️ {}", f)).collect::<Vec<_>>().join("\n"))
    }
    
    fn confidence(&self, _source: &str) -> f64 { 0.7 } // Always moderate
}

// =============================================================================
// 🧠 LLM-Assisted Review Engine (like RationalPhi — powerful, expensive)
// =============================================================================
pub struct LlmReviewEngine;

impl ScanEngine for LlmReviewEngine {
    fn name(&self) -> &'static str { "LLM-Assisted Review" }
    
    fn scan(&self, source: &str) -> Vec<String> {
        let mut findings = Vec::new();
        // Simulated LLM analysis (would call GitLab Duo API in production)
        if source.len() > 200 {
            findings.push("Complexity: Function exceeds 200 chars, consider splitting".into());
        }
        if source.contains("pub") && !source.contains("///") {
            findings.push("Documentation: Public API lacks doc comments".into());
        }
        findings
    }
    
    fn report(&self, findings: &[String]) -> String {
        format!("### 🧠 LLM-Assisted Review\n{}", 
            findings.iter().map(|f| format!("- 💡 {}", f)).collect::<Vec<_>>().join("\n"))
    }
    
    fn confidence(&self, _source: &str) -> f64 { 0.85 } // High but expensive
}

// =============================================================================
// 🏭 Factory (mirrors phi_protocol.clj `make-phi`)
// =============================================================================

/// Scanning strategies available to the SecurityAnalyzerActor
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScanStrategy {
    DfsTaintAnalysis,
    PatternMatching,
    LlmAssistedReview,
}

impl ScanStrategy {
    /// Returns the next strategy in the mutation cycle
    pub fn mutate(&self) -> Self {
        match self {
            ScanStrategy::DfsTaintAnalysis => ScanStrategy::PatternMatching,
            ScanStrategy::PatternMatching => ScanStrategy::LlmAssistedReview,
            ScanStrategy::LlmAssistedReview => ScanStrategy::DfsTaintAnalysis,
        }
    }
}

/// Factory: create ScanEngine by strategy type (like `make-phi` in Clojure KAN)
pub fn make_engine(strategy: ScanStrategy) -> Box<dyn ScanEngine> {
    match strategy {
        ScanStrategy::DfsTaintAnalysis => Box::new(DfsTaintEngine),
        ScanStrategy::PatternMatching => Box::new(PatternMatchEngine),
        ScanStrategy::LlmAssistedReview => Box::new(LlmReviewEngine),
    }
}

// =============================================================================
// 🐜 Agent-Edge (now holds a pluggable ScanEngine!)
// =============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentMode {
    Explore,
    Exploit,
}

/// Agent-Edge state inspired by Clojure KAN's `agent_kan.clj`
pub struct AgentEdge {
    pub strategy: ScanStrategy,
    pub engine: Box<dyn ScanEngine>,  // Hot-swappable engine!
    pub mode: AgentMode,
    pub history: Vec<usize>,
    pub stagnation_window: usize,
    pub mutations: usize,
    pub age: usize,
}

impl AgentEdge {
    pub fn new() -> Self {
        let strategy = ScanStrategy::DfsTaintAnalysis;
        Self {
            strategy,
            engine: make_engine(strategy),
            mode: AgentMode::Explore,
            history: Vec::new(),
            stagnation_window: 3,
            mutations: 0,
            age: 0,
        }
    }
    
    /// Detect stagnation: no improvement in the last N scans
    fn is_stagnating(&self) -> bool {
        if self.history.len() < self.stagnation_window { return false; }
        let recent: Vec<_> = self.history.iter().rev().take(self.stagnation_window).collect();
        let best = recent.iter().max().unwrap();
        let worst = recent.iter().min().unwrap();
        best == worst
    }
    
    /// Core step: observe result, choose strategy, possibly mutate + hot-swap engine
    pub fn step(&mut self, vulns_found: usize) {
        self.history.push(vulns_found);
        self.age += 1;
        
        if vulns_found > 0 {
            self.mode = AgentMode::Exploit;
            tracing::info!("🐜 [AgentEdge] Mode: Exploit (engine '{}' is productive, confidence: {:.0}%)",
                self.engine.name(), self.engine.confidence("") * 100.0);
        } else if self.is_stagnating() {
            let old_name = self.engine.name();
            self.strategy = self.strategy.mutate();
            self.engine = make_engine(self.strategy); // 🔌 HOT-SWAP!
            self.mutations += 1;
            self.mode = AgentMode::Explore;
            self.history.clear();
            tracing::warn!(
                "🧬 [AgentEdge] MUTATION! Engine '{}' → '{}' (hot-swap via make_engine factory, mutations: {})",
                old_name, self.engine.name(), self.mutations
            );
        } else {
            self.mode = AgentMode::Explore;
            tracing::info!("🐜 [AgentEdge] Mode: Explore (engine '{}')", self.engine.name());
        }
    }
}

// Manual Debug impl since Box<dyn ScanEngine> doesn't derive Debug
impl std::fmt::Debug for AgentEdge {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentEdge")
            .field("strategy", &self.strategy)
            .field("engine", &self.engine.name())
            .field("mode", &self.mode)
            .field("age", &self.age)
            .field("mutations", &self.mutations)
            .finish()
    }
}

