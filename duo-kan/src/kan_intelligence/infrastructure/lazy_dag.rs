use std::collections::{HashMap, HashSet};

/// Operation types that the Orchestrator can schedule
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum DagOp {
    AstAnalysis,
    SecurityScan,
    DriftDetection,
    CodeReview,
    AstFix,
    PostComment,
    CreateJira,
}

impl DagOp {
    pub fn name(&self) -> &'static str {
        match self {
            DagOp::AstAnalysis => "AST Analysis",
            DagOp::SecurityScan => "Security Scan",
            DagOp::DriftDetection => "Drift Detection",
            DagOp::CodeReview => "Code Review",
            DagOp::AstFix => "AST Fix",
            DagOp::PostComment => "Post Comment",
            DagOp::CreateJira => "Create Jira",
        }
    }
}

/// A lazy node in the execution DAG (recorded but NOT executed)
#[derive(Debug, Clone)]
pub struct DagNode {
    pub id: usize,
    pub op: DagOp,
    pub deps: Vec<usize>,       // IDs of prerequisite nodes
    pub target_files: Vec<String>,
    pub executed: bool,
}

/// The Lazy DAG Planner (XLA-inspired, from Clojure KAN's `lazy_graph.clj`)
///
/// 1. RECORD operations (don't execute!)
/// 2. OPTIMIZE the graph (dead code elimination, dedup, fusion)
/// 3. EXECUTE the optimized graph
#[derive(Debug)]
pub struct LazyDag {
    pub nodes: Vec<DagNode>,
    next_id: usize,
}

impl LazyDag {
    pub fn new() -> Self {
        Self { nodes: Vec::new(), next_id: 0 }
    }

    /// Record a lazy operation (NOT executed yet)
    pub fn record(&mut self, op: DagOp, deps: Vec<usize>, files: Vec<String>) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.nodes.push(DagNode {
            id, op, deps, target_files: files, executed: false,
        });
        tracing::debug!("   📝 [LazyDAG] Recorded node {} ({})", id, self.nodes.last().unwrap().op.name());
        id
    }

    pub fn stats(&self) -> (usize, usize) {
        (self.nodes.len(), self.nodes.iter().filter(|n| !n.executed).count())
    }

    // =========================================================================
    // OPTIMIZATION 1: Dead Code Elimination
    // =========================================================================
    /// Remove nodes that no output node depends on
    pub fn eliminate_dead(&mut self, output_ids: &[usize]) {
        let reachable = self.reachable_from(output_ids);
        let before = self.nodes.len();
        self.nodes.retain(|n| reachable.contains(&n.id));
        let removed = before - self.nodes.len();
        if removed > 0 {
            tracing::info!("   ✂️  [LazyDAG:DCE] Eliminated {} dead nodes", removed);
        }
    }

    fn reachable_from(&self, output_ids: &[usize]) -> HashSet<usize> {
        let mut visited = HashSet::new();
        let mut queue: Vec<usize> = output_ids.to_vec();
        let node_map: HashMap<usize, &DagNode> = self.nodes.iter().map(|n| (n.id, n)).collect();
        while let Some(id) = queue.pop() {
            if visited.insert(id) {
                if let Some(node) = node_map.get(&id) {
                    queue.extend(&node.deps);
                }
            }
        }
        visited
    }

    // =========================================================================
    // OPTIMIZATION 2: Deduplication (same op + same files = merge)
    // =========================================================================
    /// Merge duplicate operations targeting the same files
    pub fn deduplicate(&mut self) {
        let mut seen: HashMap<(DagOp, Vec<String>), usize> = HashMap::new();
        let mut remap: HashMap<usize, usize> = HashMap::new();
        let mut deduped = 0;

        for node in &self.nodes {
            let key = (node.op.clone(), node.target_files.clone());
            if let Some(&canonical_id) = seen.get(&key) {
                remap.insert(node.id, canonical_id);
                deduped += 1;
            } else {
                seen.insert(key, node.id);
            }
        }

        if deduped > 0 {
            // Rewrite deps to point to canonical nodes
            for node in &mut self.nodes {
                node.deps = node.deps.iter()
                    .map(|d| *remap.get(d).unwrap_or(d))
                    .collect();
            }
            // Remove duplicates
            let keep: HashSet<usize> = remap.keys().copied().collect();
            self.nodes.retain(|n| !keep.contains(&n.id));
            tracing::info!("   🔗 [LazyDAG:Dedup] Merged {} duplicate operations", deduped);
        }
    }

    // =========================================================================
    // OPTIMIZATION 3: Fusion (chain of single-dep linear ops → fused node)
    // =========================================================================
    /// Fuse linear chains: A→B where B has exactly 1 dep and A has exactly 1 consumer
    pub fn fuse_chains(&mut self) {
        let mut consumer_count: HashMap<usize, usize> = HashMap::new();
        for node in &self.nodes {
            for dep in &node.deps {
                *consumer_count.entry(*dep).or_insert(0) += 1;
            }
        }

        let mut fused = 0;
        let mut remove_ids: HashSet<usize> = HashSet::new();

        // Find fusable pairs
        let fusable: Vec<(usize, usize)> = self.nodes.iter()
            .filter(|n| n.deps.len() == 1)
            .filter_map(|n| {
                let parent_id = n.deps[0];
                if consumer_count.get(&parent_id) == Some(&1) {
                    Some((parent_id, n.id))
                } else {
                    None
                }
            })
            .collect();

        for (parent_id, child_id) in fusable {
            if remove_ids.contains(&parent_id) || remove_ids.contains(&child_id) { continue; }

            if let (Some(parent), Some(child)) = (
                self.nodes.iter().find(|n| n.id == parent_id),
                self.nodes.iter().find(|n| n.id == child_id),
            ) {
                tracing::info!("   ⚡ [LazyDAG:Fusion] Fused '{}' + '{}' into single dispatch",
                    parent.op.name(), child.op.name());
                fused += 1;
                remove_ids.insert(parent_id);
                // Child inherits parent's deps
            }
        }

        if fused > 0 {
            // Update child deps to skip removed parents
            let parent_deps: HashMap<usize, Vec<usize>> = self.nodes.iter()
                .filter(|n| remove_ids.contains(&n.id))
                .map(|n| (n.id, n.deps.clone()))
                .collect();

            for node in &mut self.nodes {
                node.deps = node.deps.iter()
                    .flat_map(|d| {
                        if let Some(grandparent_deps) = parent_deps.get(d) {
                            grandparent_deps.clone()
                        } else {
                            vec![*d]
                        }
                    })
                    .collect();
            }

            self.nodes.retain(|n| !remove_ids.contains(&n.id));
        }
    }

    // =========================================================================
    // FULL OPTIMIZATION PIPELINE
    // =========================================================================
    pub fn optimize(&mut self, output_ids: &[usize]) {
        let (before_nodes, _) = self.stats();
        tracing::info!("🦥 [LazyDAG] Optimizing DAG ({} nodes)...", before_nodes);

        self.eliminate_dead(output_ids);
        self.deduplicate();
        self.fuse_chains();

        let (after_nodes, _) = self.stats();
        tracing::info!("🦥 [LazyDAG] Optimization complete: {} → {} nodes", before_nodes, after_nodes);
    }

    // =========================================================================
    // EXECUTE (topological order)
    // =========================================================================
    /// Returns the execution order (topologically sorted node IDs)
    pub fn execution_plan(&self) -> Vec<usize> {
        let mut in_degree: HashMap<usize, usize> = self.nodes.iter()
            .map(|n| (n.id, 0))
            .collect();
        for node in &self.nodes {
            for dep in &node.deps {
                if let Some(count) = in_degree.get_mut(dep) {
                    let _ = count; // dep's consumers don't affect its in_degree
                }
            }
            // Actually we track in_degree of each node (how many deps it has)
        }
        // Kahn's algorithm
        let mut in_deg: HashMap<usize, usize> = HashMap::new();
        let mut adj: HashMap<usize, Vec<usize>> = HashMap::new();
        for node in &self.nodes {
            in_deg.insert(node.id, node.deps.len());
            for dep in &node.deps {
                adj.entry(*dep).or_default().push(node.id);
            }
        }

        let mut queue: Vec<usize> = self.nodes.iter()
            .filter(|n| n.deps.is_empty())
            .map(|n| n.id)
            .collect();
        let mut order = Vec::new();

        while let Some(id) = queue.pop() {
            order.push(id);
            if let Some(children) = adj.get(&id) {
                for child in children {
                    if let Some(deg) = in_deg.get_mut(child) {
                        *deg -= 1;
                        if *deg == 0 {
                            queue.push(*child);
                        }
                    }
                }
            }
        }

        order
    }

    /// Pretty-print the execution plan
    pub fn print_plan(&self) {
        let plan = self.execution_plan();
        let node_map: HashMap<usize, &DagNode> = self.nodes.iter().map(|n| (n.id, n)).collect();
        tracing::info!("📋 [LazyDAG] Execution Plan ({} steps):", plan.len());
        for (step, id) in plan.iter().enumerate() {
            if let Some(node) = node_map.get(id) {
                let files_str = if node.target_files.is_empty() {
                    "all".to_string()
                } else {
                    node.target_files.join(", ")
                };
                tracing::info!("   Step {}: {} → [{}]", step + 1, node.op.name(), files_str);
            }
        }
    }
}
