//! Errors specific to the GPS data ingestion module
//!
//! Defines all the errors that can occur while reading,
//! parsing and validating Madrid traffic data

use thiserror::Error;

/// Errors of the GPS data ingestion module
///
/// Each variant represents a specific kind of failure during
/// the ingestion process, from I/O errors to validation
/// of geographic coordinates
#[derive(Debug, Error)]
pub enum IngestionError {
    /// Error while reading or parsing a CSV file
    #[error("Error reading CSV: {0}")]
    CsvRead(#[from] csv::Error),

    /// Generic I/O error (file not found, permissions, etc.)
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Timestamp with an invalid or unparseable format
    #[error("Invalid timestamp '{value}': {source}")]
    InvalidTimestamp {
        value: String,
        source: chrono::ParseError,
    },

    /// Coordinates outside the Madrid bounding box
    #[error("Coordinates outside Madrid: lat={lat}, lon={lon}")]
    OutOfBounds { lat: f64, lon: f64 },

    /// Invalid value in a CSV field
    #[error("Invalid data in field '{field}': {reason}")]
    InvalidField { field: String, reason: String },

    /// Measurement point ID with no associated location in the shapefile
    #[error("ID {id} has no associated location")]
    MissingLocation { id: u32 },

    /// Error in the HTTP request to the real-time traffic API
    #[error("HTTP error: {0}")]
    HttpRequest(String),

    /// Error parsing the API XML response
    #[error("XML error: {0}")]
    XmlParse(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_display_csv() {
        let err = IngestionError::InvalidField {
            field: "intensidad".to_string(),
            reason: "negative value".to_string(),
        };
        assert!(err.to_string().contains("intensidad"));
        assert!(err.to_string().contains("negative value"));
    }

    #[test]
    fn test_error_display_bounds() {
        let err = IngestionError::OutOfBounds {
            lat: 41.0,
            lon: -4.0,
        };
        assert!(err.to_string().contains("41"));
        assert!(err.to_string().contains("-4"));
    }

    #[test]
    fn test_error_display_missing_location() {
        let err = IngestionError::MissingLocation { id: 9999 };
        assert!(err.to_string().contains("9999"));
    }
}
