//! Madrid traffic GPS data ingestion module
//!
//! This module provides the data structures and parsers needed
//! to read, normalize and validate traffic data coming
//! from Madrid's open data portal
//!
//! ## Structure
//!
//! - [`error`] — Ingestion-specific error types
//! - [`model`] — Data models: `GpsPoint`, `TrafficRecord`, `Location`
//! - [`csv_parser`] — CSV parser for traffic data and locations
//! - [`streaming`] — Streaming parser with a lazy iterator
//! - [`datasource`] — `DataSource` trait for source abstraction
//! - [`csv_source`] — `DataSource` implementation for historical CSV
//! - [`live_api`] — `DataSource` implementation for the real-time API
//! - [`scheduler`] — Polling scheduler for near-real-time mode

pub mod csv_parser;
pub mod csv_source;
pub mod datasource;
pub mod error;
pub mod live_api;
pub mod model;
pub mod scheduler;
pub mod streaming;

// Re-export the most used types
pub use csv_source::CsvDataSource;
pub use datasource::DataSource;
pub use error::IngestionError;
pub use live_api::LiveApiSource;
pub use model::{DataQuality, GpsPoint, Location, RawTrafficRecord, RoadType, TrafficRecord};
pub use scheduler::PollingScheduler;
