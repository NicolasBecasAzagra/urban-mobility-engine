// Urban Mobility Analysis Engine in Rust
//
// This crate implements a geospatial processing engine
// that computes urban traffic metrics on an S2 grid

pub mod cli;
pub mod config;
pub mod export;
pub mod geospatial;
pub mod ingestion;
pub mod metrics;
pub mod progress;
pub mod tui;
pub mod utils;

// Re-export common types for convenience
pub use config::Config;
