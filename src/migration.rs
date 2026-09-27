use serde::{Deserialize, Serialize};

/// The unified Migration Trait (Replacing Go's Reflection with Rust's Type Safety)
pub trait MigrateConfig<To> {
    fn migrate(self) -> To;
}

// =====================================================================
// 📦 1. LEGACY SCHEMA (v1) - What users wrote 6 months ago
// =====================================================================
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityRuleV1 {
    pub banned_functions: Vec<String>,
    pub strict_layering: bool,
}

// =====================================================================
// 🚀 2. CURRENT SCHEMA (v2) - Our advanced Semantic Swarm payload
// =====================================================================
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SecurityRuleV2 {
    pub forbidden_calls: Option<Vec<String>>,
    pub architecture_layers: Option<Vec<String>>,
    pub require_review: Option<bool>,
    pub max_taint_depth: Option<usize>,
}

// =====================================================================
// 🔄 3. THE TRANSLATOR ENGINE (Ignition translate.go equivalent)
// =====================================================================
impl MigrateConfig<SecurityRuleV2> for SecurityRuleV1 {
    fn migrate(self) -> SecurityRuleV2 {
        tracing::info!("🔄 [MigrationEngine] Upgrading legacy SecurityRuleV1 payload to SecurityRuleV2 in memory...");
        
        SecurityRuleV2 {
            // 🎯 Identical conceptual fields get seamlessly mapped
            forbidden_calls: Some(self.banned_functions),
            require_review: Some(self.strict_layering),
            
            // ⚠️ Breaking structural additions receive sensible defaults
            architecture_layers: Some(vec!["*".to_string()]),
            max_taint_depth: Some(5), // New heuristic injected seamlessly
        }
    }
}

// =====================================================================
// 🛠️ 4. AUTO-UPGRADING CONFIG PARSER
// =====================================================================
/// Parses YAML and automatically climbs the version migration ladder
pub fn parse_and_migrate(yaml_content: &str) -> SecurityRuleV2 {
    // 1. Try parsing directly into the newest format
    if let Ok(v2) = serde_yaml::from_str::<SecurityRuleV2>(yaml_content) {
        tracing::debug!("✅ [MigrationEngine] Config is already v2.");
        return v2;
    }
    
    // 2. Fallback to v1 and perform memory translation!
    if let Ok(v1) = serde_yaml::from_str::<SecurityRuleV1>(yaml_content) {
        tracing::warn!("⚠️ [MigrationEngine] Detected legacy v1 config! Initiating memory translation.");
        return v1.migrate();
    }
    
    // 3. Complete failure fallback (empty default)
    tracing::error!("❌ [MigrationEngine] Failed to parse config across all known versions. Resorting to failsafe defaults.");
    SecurityRuleV2::default()
}
