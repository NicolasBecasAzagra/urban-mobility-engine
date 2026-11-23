//! Urban traffic metrics module
//!
//! Implements the computation of the delay_factor (adapted Speed Reduction
//! Factor) per S2 cell, congestion levels and temporal aggregations
//! for urban mobility analysis
//!
//! ## Structure
//!
//! - [`model`] — Data structures: `DelayFactor`, `CongestionLevel`,
//!   `TimeSlot`, `CellDelayReport`
//! - [`error`] — Errors specific to the metrics module
//! - [`delay_factor`] — Per-S2-cell delay_factor computation engine
//! - [`temporal`] — Temporal aggregations (hour, day, week)
//! - [`anomaly`] — Anomaly detection: live SRF vs historical baseline

pub mod anomaly;
pub mod delay_factor;
pub mod error;
pub mod model;
pub mod statistics;
pub mod temporal;
