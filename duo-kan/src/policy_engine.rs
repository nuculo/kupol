use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecurityRule {
    pub forbidden_calls: Option<Vec<String>>,
    pub architecture_layers: Option<Vec<String>>,
    pub require_review: Option<bool>,
    pub max_taint_depth: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SwarmConfig {
    pub global: SecurityRule,
    pub enterprise: SecurityRule,
    pub project: SecurityRule,
}

impl SwarmConfig {
    /// Ignition-inspired Multi-Tier Merge Strategy:
    /// latest.Merge(base, system, user)
    /// 1. Global (Base Hardcoded Defaults)
    /// 2. Enterprise (System Dashboard Rules)
    /// 3. Project (User `security.yml` from MR)
    pub fn merge(base: &SecurityRule, system: &SecurityRule, user: &SecurityRule) -> SecurityRule {
        SecurityRule {
            // Arrays are inclusively aggregated across all tiers
            forbidden_calls: Self::merge_vec(&base.forbidden_calls, &system.forbidden_calls, &user.forbidden_calls),
            architecture_layers: Self::merge_vec(&base.architecture_layers, &system.architecture_layers, &user.architecture_layers),
            
            // Booleans and Scalars are strictly overriden Top-Down (User -> System -> Base)
            require_review: user.require_review.or(system.require_review).or(base.require_review),
            max_taint_depth: user.max_taint_depth.or(system.max_taint_depth).or(base.max_taint_depth),
        }
    }

    fn merge_vec(base: &Option<Vec<String>>, system: &Option<Vec<String>>, user: &Option<Vec<String>>) -> Option<Vec<String>> {
        let mut result = Vec::new();
        if let Some(b) = base { result.extend(b.clone()); }
        if let Some(s) = system { result.extend(s.clone()); }
        if let Some(u) = user { result.extend(u.clone()); }
        
        if result.is_empty() { 
            None 
        } else {
            result.sort();
            result.dedup();
            Some(result)
        }
    }
}
