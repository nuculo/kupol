//! # KAN Intelligence Layer
//!
//! All ML/AI primitives ported from Clojure KAN (Kolmogorov-Arnold Networks).
//! Organized into four subsystems:
//!
//! - **`ml_core`**: Normalizing flows, operator algebra, symbolic discovery, hybrid state
//! - **`security_rules`**: Evolutionary NAS, LoRA-adapted frozen rules
//! - **`pipeline`**: Early stopping, LR finder, checkpoint/resume
//! - **`infrastructure`**: Lazy DAG execution, ring-reduce, streaming

pub mod ml_core;
pub mod security_rules;
pub mod pipeline;
pub mod infrastructure;

#[cfg(test)]
mod tests;
