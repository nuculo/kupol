//! Pipeline — Training and execution pipeline utilities.
//!
//! - `early_stopping`: Adaptive patience-based lease monitoring (from `early_stopping.clj`)
//! - `lr_finder`: Leslie Smith LR finder for auto-tuning thresholds (from `learning_rate_finder.clj`)
//! - `checkpoint`: Pipeline checkpoint/resume with EDN-style serialization (from `serialization.clj`)

pub mod early_stopping;
pub mod lr_finder;
pub mod checkpoint;
