//! Security Rules — Evolutionary and adaptive security rule engines.
//!
//! - `evolution_nas`: Genetic algorithm NAS for security rule structure search (from `evolution.clj`)
//! - `lora_rules`: Low-Rank Adaptation for fine-tuning frozen security rules (from `lora.clj`)

pub mod evolution_nas;
pub mod lora_rules;
