//! Errors of the traffic metrics module
//!
//! Defines the error types specific to the computation
//! of delay_factor and temporal aggregations

use thiserror::Error;

/// Errors that can occur during metric computation
#[derive(Debug, Error)]
pub enum MetricsError {
    /// The cell does not have enough observations for a reliable computation
    #[error("Insufficient data: cell has {actual} observations, minimum required: {required}")]
    InsufficientData {
        /// Number of observations in the cell
        actual: usize,
        /// Minimum required by the configuration
        required: usize,
    },

    /// No reference speed is available to compute the SRF
    #[error("No reference speed for measurement point {sensor_id}")]
    NoReferenceSpeed {
        /// ID of the sensor without a reference speed
        sensor_id: u32,
    },

    /// Metric value outside the expected range
    #[error("Invalid metric: {metric} = {value} (expected {expected_range})")]
    InvalidMetric {
        /// Name of the metric
        metric: String,
        /// Value obtained
        value: f64,
        /// Description of the expected range
        expected_range: String,
    },

    /// No data with a valid speed in the cell (M-30)
    #[error("No valid speed data in the cell")]
    NoSpeedData,
}
