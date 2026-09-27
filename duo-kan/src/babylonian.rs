use tokio::sync::mpsc;

/// A single narrow "Head" in the Babylonian Architecture.
/// Instead of one huge actor analyzing 100 things, one micro-actor analyzes exactly 1 thing.
pub struct MicroScannerHead {
    pub id: usize,
    pub focus_pattern: &'static str,
    pub vulnerability_type: &'static str,
}

/// The Swarm orchestrator for the Babylonian 60-Head architecture
pub struct BabylonianSwarm {
    heads: Vec<MicroScannerHead>,
}

impl BabylonianSwarm {
    pub fn new() -> Self {
        // Base library of vulnerabilities
        let patterns = vec![
            ("unwrap()", "Unsafe Panic"),
            ("unsafe {", "Memory Safety"),
            ("raw_query", "SQL Injection"),
            ("exec(", "Code Injection"),
            ("eval(", "Code Injection"),
            ("chmod 777", "Insecure Permissions"),
            ("TODO", "Tech Debt"),
            ("panic!(", "Hardcoded Panic"),
            ("std::process::Command", "OS Command Injection"),
            ("transmute", "Unsafe Typecast"),
            ("to_string()", "Suboptimal Allocation"),
            ("Mutex::new", "Potential Deadlock Point"),
        ];
        
        // Spawn 60 narrow heads. In a real system we'd have 60 unique rules.
        // For the demo, we cycle through the library to reach exactly 60 heads.
        let mut heads = Vec::new();
        for i in 0..60 {
            let (pat, vul_type) = patterns[i % patterns.len()];
            heads.push(MicroScannerHead {
                id: i,
                focus_pattern: pat, // e.g. "unwrap()"
                vulnerability_type: vul_type,
            });
        }
        
        Self { heads }
    }
    
    /// Fan-Out to 60 concurrent Micro-Actors, then Fan-In the results.
    /// Emulates `financial_timeseries_kan.clj` model B (60 narrow heads).
    pub async fn scan_parallel(&self, source: String) -> Vec<String> {
        // mpsc channel for Fan-In (Gather)
        let (tx, mut rx) = mpsc::channel(100);
        let mut join_handles = Vec::new();
        
        let start_time = tokio::time::Instant::now();
        
        // FAN-OUT (Scatter): Span 60 independent async tasks
        for head in &self.heads {
            let tx_clone = tx.clone();
            let source_clone = source.clone();
            
            let focus = head.focus_pattern;
            let v_type = head.vulnerability_type;
            let id = head.id;
            
            let handle = tokio::spawn(async move {
                // Simulate I/O or parsing jitter (5-15ms) so they don't block
                tokio::time::sleep(tokio::time::Duration::from_millis(5 + (id as u64) % 10)).await;
                
                // Narrow O(1) focus scan
                if source_clone.contains(focus) {
                    let finding = format!("🎯 [Head {:02} | {}] Detected '{}'", id, v_type, focus);
                    let _ = tx_clone.send(finding).await;
                }
            });
            join_handles.push(handle);
        }
        
        // Drop the original sender so `rx.recv()` terminates when all clones are done
        drop(tx);
        
        // FAN-IN (Gather)
        let mut findings = Vec::new();
        while let Some(finding) = rx.recv().await {
            findings.push(finding);
        }
        
        // Await all actors for clean shutdown
        for handle in join_handles {
            let _ = handle.await;
        }
        
        tracing::info!(
            "⛩️  [BabylonianSwarm] Orchestrated 60 Micro-Actors concurrently in {:?}", 
            start_time.elapsed()
        );
        
        findings
    }
}
