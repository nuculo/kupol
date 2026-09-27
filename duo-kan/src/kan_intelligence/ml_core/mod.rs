//! ML Core — Foundational ML primitives ported from Clojure KAN.
//!
//! - `normalizing_flow`: Invertible coupling layers for anomaly detection (from `automorphic_kan.clj`)
//! - `operator_algebra`: AST transform operators with commutator verification (from `operator_kan.clj`)
//! - `symbolic`: Symbolic rule discovery and freezing (from `kan_symbolic.clj`)
//! - `hybrid_state`: Dual numeric+symbolic execution with confidence tracking (from `hybrid_kan.clj`)

pub mod normalizing_flow;
pub mod operator_algebra;
pub mod symbolic;
pub mod hybrid_state;
