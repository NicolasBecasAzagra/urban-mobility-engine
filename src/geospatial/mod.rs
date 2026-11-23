//! S2 geospatial indexing module
//!
//! Provides the indexing system and spatial queries
//! based on Google's S2 Geometry library
//!
//! ## Structure
//!
//! - [`s2_index`] — S2 index with a `CellID → Vec<GpsPoint>` HashMap
//! - [`queries`] — Geospatial queries (cell, radius, polygon, k-NN)
//! - [`aggregation`] — Statistical aggregations per cell/level
//! - [`serialization`] — Binary-format persistence of the index

pub mod aggregation;
pub mod queries;
pub mod s2_index;
pub mod serialization;

// Re-export main types
pub use s2_index::S2Index;
