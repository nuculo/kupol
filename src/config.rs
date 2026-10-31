//! KUPOL configuration — `.kupol.yml`
//!
//! The scanner reads an optional `.kupol.yml` from the root of the scan target
//! (or the current directory). It controls which paths are ignored, the minimum
//! severity to report, and the default CI fail-on threshold.
//!
//! ```yaml
//! ignore:
//!   - target
//!   - node_modules
//!   - dist
//! min_severity: low
//! fail_on: high
//! ```

use serde::Deserialize;
use std::path::Path;

/// Parsed contents of a `.kupol.yml` file.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct KupolConfig {
    /// Path fragments to skip during a scan (in addition to the built-in
    /// `target/`, `node_modules/`, and `.git/`).
    #[serde(default)]
    pub ignore: Vec<String>,

    /// Minimum severity to include in the report.
    #[serde(default)]
    pub min_severity: Option<String>,

    /// Default CI gate severity (used when `--fail-on` is not passed).
    #[serde(default)]
    pub fail_on: Option<String>,
}

impl KupolConfig {
    /// Load `.kupol.yml` for a scan of `target_path`.
    ///
    /// Looks for the file next to the target (if the target is a directory) and
    /// falls back to the current working directory. Returns the default config
    /// when no file is found or it cannot be parsed.
    pub fn load_for(target_path: &str) -> Self {
        for candidate in config_candidates(target_path) {
            if let Ok(text) = std::fs::read_to_string(&candidate) {
                match serde_yaml::from_str::<KupolConfig>(&text) {
                    Ok(cfg) => return cfg,
                    Err(e) => {
                        tracing::warn!("could not parse {}: {}", candidate, e);
                        return KupolConfig::default();
                    }
                }
            }
        }
        KupolConfig::default()
    }

    /// Returns true if `file_path` matches any configured ignore entry.
    ///
    /// Matching is substring-based on normalized (`/`-separated) paths, so an
    /// entry like `dist` ignores any path segment named `dist`.
    pub fn is_ignored(&self, file_path: &str) -> bool {
        let normalized = file_path.replace('\\', "/");
        self.ignore.iter().any(|raw| {
            let pat = raw.trim().trim_matches('/');
            if pat.is_empty() {
                return false;
            }
            let needle = format!("/{}/", pat);
            normalized == pat
                || normalized.starts_with(&format!("{}/", pat))
                || normalized.contains(&needle)
                || normalized.ends_with(&format!("/{}", pat))
        })
    }
}

fn config_candidates(target_path: &str) -> Vec<String> {
    let mut candidates = Vec::new();
    let target = Path::new(target_path);

    if target.is_dir() {
        candidates.push(target.join(".kupol.yml").to_string_lossy().to_string());
    } else if let Some(parent) = target.parent() {
        if !parent.as_os_str().is_empty() {
            candidates.push(parent.join(".kupol.yml").to_string_lossy().to_string());
        }
    }

    candidates.push(".kupol.yml".to_string());
    candidates
}
