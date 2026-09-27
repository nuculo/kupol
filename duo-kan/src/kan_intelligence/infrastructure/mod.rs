//! Infrastructure — Distributed compute and data-flow primitives.
//!
//! - `lazy_dag`: Lazy DAG with dead-code elimination and fusion (from `lazy_graph.clj`)
//! - `ring_reduce`: Ring All-Reduce gradient synchronization (from `ring_reduce.clj`)
//! - `streaming`: Lazy/chunked data streaming pipeline (from `streaming.clj`)

pub mod lazy_dag;
pub mod ring_reduce;
pub mod streaming;
