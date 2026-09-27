use std::collections::{HashSet, VecDeque};
use petgraph::graph::NodeIndex;
use petgraph::Direction;
use crate::{EntityGraph, EntityKind};

pub struct TrustDecayAnalyzer {
    pub critical_threshold: usize,
    pub high_threshold: usize,
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum RiskScore {
    Critical,
    High,
    Low,
}

impl TrustDecayAnalyzer {
    pub fn new() -> Self {
        Self {
            critical_threshold: 10,
            high_threshold: 3,
        }
    }

    /// Performs a BFS upward from the tainted node (e.g. `unwrap()` or `unsafe`)
    /// to determine how many upstream endpoints rely on this compromised code base.
    /// Trust decays as we go up, but blast radius is determined by absolute count of Entrypoints.
    pub fn compute_blast_radius(&self, graph: &EntityGraph, tainted_node: NodeIndex) -> (usize, RiskScore) {
        let mut visited = HashSet::new();
        let mut queue = VecDeque::new();
        
        // Track the specific endpoints affected
        let mut affected_endpoints = 0;

        // BFS queue: (NodeIndex, current_risk)
        queue.push_back((tainted_node, 1.0_f64));
        visited.insert(tainted_node);

        while let Some((current, current_risk)) = queue.pop_front() {
            // Find all nodes that point TO current (meaning they depend on the tainted node)
            for edge in graph.graph.edges_directed(current, Direction::Incoming) {
                // petgraph trait: edge.source() points TO current
                use petgraph::visit::EdgeRef;
                let upstream_node = edge.source();
                let kan_edge = edge.weight();
                
                // 🧠 Graph-KAN (G-KAN) Trust Decay: Evaluate risk via B-Spline
                let new_risk = kan_edge.spline.eval(current_risk);

                // Only propagate if the spline hasn't fully dampened the risk
                if new_risk > 0.1 && !visited.contains(&upstream_node) {
                    visited.insert(upstream_node);
                    queue.push_back((upstream_node, new_risk));
                    
                    // Check if this upstream node is an actual Endpoint/API
                    if let Some(entity) = graph.graph.node_weight(upstream_node) {
                        if entity.kind == EntityKind::Endpoint {
                            affected_endpoints += 1;
                        }
                    }
                }
            }
        }

        let score = if affected_endpoints >= self.critical_threshold {
            RiskScore::Critical
        } else if affected_endpoints >= self.high_threshold {
            RiskScore::High
        } else {
            RiskScore::Low
        };

        (affected_endpoints, score)
    }

    /// Generates a recommendation based on risk architecture score.
    pub fn generate_recommendation(&self, score: &RiskScore) -> &'static str {
        match score {
            RiskScore::Critical => "🚨 CRITICAL BLAST RADIUS: Widespread API exposure. Escalating to Staff/Principal Security Engineer.",
            RiskScore::High => "⚠️ HIGH BLAST RADIUS: Multiple subsystems impacted. Escalating to Senior Security Reviewer.",
            RiskScore::Low => "✅ LOW BLAST RADIUS: Localized internal issue. Proceeding with standard Auto-Fix & Junior Review.",
        }
    }
}
