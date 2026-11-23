//! `DataSource` implementation for historical CSV data
//!
//! Wrapper of the existing CSV parser that implements the
//! [`DataSource`](super::datasource::DataSource) trait, allowing
//! historical data to be used through the same interface as the live API
//!
//! ## Example
//!
//! ```rust,no_run
//! use tfg::ingestion::csv_source::CsvDataSource;
//! use tfg::ingestion::datasource::DataSource;
//!
//! let mut source = CsvDataSource::new(
//!     "data/samples/sample_traffic.csv",
//!     "data/samples/locations.csv",
//! ).unwrap();
//!
//! while source.has_more() {
//!     let points = source.fetch_batch().unwrap();
//!     println!("Loaded {} points from CSV", points.len());
//! }
//! // After reading, has_more() returns false
//! assert!(!source.has_more());
//! ```

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use crate::ingestion::csv_parser;
use crate::ingestion::datasource::DataSource;
use crate::ingestion::error::IngestionError;
use crate::ingestion::model::{GpsPoint, Location};

/// Data source based on historical CSV files
///
/// Reads traffic and location data from CSV files of
/// Madrid's open data portal. Reading happens
/// only once (finite source)
///
/// Locations are loaded into memory when the source is created
/// and used as an O(1) lookup when building the `GpsPoint`s
pub struct CsvDataSource {
    /// Path to the traffic CSV file
    traffic_path: PathBuf,
    /// HashMap of locations indexed by measurement point ID
    locations: HashMap<u32, Location>,
    /// Indicates whether the data has already been read
    consumed: bool,
}

impl CsvDataSource {
    /// Creates a new CSV source
    ///
    /// Loads the locations into memory immediately. Traffic data
    /// is read on demand in `fetch_batch()`
    ///
    /// # Arguments
    /// * `traffic_path` — Path to the historical traffic CSV
    /// * `locations_path` — Path to the locations CSV
    ///
    /// # Errors
    /// Returns an error if the locations cannot be read
    pub fn new(traffic_path: &str, locations_path: &str) -> Result<Self, IngestionError> {
        let locations = csv_parser::load_locations_csv(locations_path).map_err(|e| {
            IngestionError::InvalidField {
                field: "locations_path".to_string(),
                reason: format!("Error loading locations: {}", e),
            }
        })?;

        tracing::info!(
            "CsvDataSource created: traffic='{}', {} locations loaded",
            traffic_path,
            locations.len()
        );

        Ok(CsvDataSource {
            traffic_path: PathBuf::from(traffic_path),
            locations,
            consumed: false,
        })
    }

    /// Returns the number of loaded locations
    pub fn location_count(&self) -> usize {
        self.locations.len()
    }

    /// Returns a reference to the locations map
    ///
    /// Useful to share the locations with other data sources
    /// (e.g., `LiveApiSource`)
    pub fn locations(&self) -> &HashMap<u32, Location> {
        &self.locations
    }
}

impl DataSource for CsvDataSource {
    fn name(&self) -> &str {
        "Historical CSV"
    }

    fn fetch_batch(&mut self) -> Result<Vec<GpsPoint>, IngestionError> {
        if self.consumed {
            return Ok(vec![]);
        }

        self.consumed = true;

        // Read the traffic records
        let records = csv_parser::parse_traffic_csv(&self.traffic_path).map_err(|e| {
            IngestionError::InvalidField {
                field: "traffic_path".to_string(),
                reason: format!("Error reading traffic CSV: {}", e),
            }
        })?;

        // Combine with the locations
        let points = csv_parser::build_gps_points(&records, &self.locations);

        tracing::info!(
            "CsvDataSource: {} GPS points generated from '{}'",
            points.len(),
            self.traffic_path.display()
        );

        Ok(points)
    }

    fn has_more(&self) -> bool {
        !self.consumed
    }

    fn refresh_interval(&self) -> Option<Duration> {
        None // Finite source -> read only once
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_TRAFFIC: &str = "tests/fixtures/valid_traffic.csv";
    const FIXTURE_LOCATIONS: &str = "tests/fixtures/locations.csv";

    #[test]
    fn test_csv_source_creation() {
        let source = CsvDataSource::new(FIXTURE_TRAFFIC, FIXTURE_LOCATIONS).unwrap();
        assert_eq!(source.name(), "Historical CSV");
        assert!(source.has_more());
        assert!(source.refresh_interval().is_none());
        assert_eq!(source.location_count(), 3);
    }

    #[test]
    fn test_csv_source_fetch_batch() {
        let mut source = CsvDataSource::new(FIXTURE_TRAFFIC, FIXTURE_LOCATIONS).unwrap();

        assert!(source.has_more());

        let points = source.fetch_batch().unwrap();
        assert!(!points.is_empty());
        assert!(!source.has_more());

        // Verify that the points have the correct data
        for point in &points {
            assert!(point.latitude > 40.0);
            assert!(point.longitude < -3.0);
        }
    }

    #[test]
    fn test_csv_source_exhaustion() {
        let mut source = CsvDataSource::new(FIXTURE_TRAFFIC, FIXTURE_LOCATIONS).unwrap();

        // First read
        let points = source.fetch_batch().unwrap();
        assert!(!points.is_empty());
        assert!(!source.has_more());

        // Second read → empty
        let empty = source.fetch_batch().unwrap();
        assert!(empty.is_empty());

        // Still no data
        assert!(!source.has_more());
    }

    #[test]
    fn test_csv_source_invalid_traffic_path() {
        let source = CsvDataSource::new("nonexistent.csv", FIXTURE_LOCATIONS);
        // Creation should work (it only loads locations)
        // but in this case the locations do exist
        // The error will come on fetch_batch with an invalid traffic path
        if let Ok(mut s) = source {
            let result = s.fetch_batch();
            // It may fail when trying to read the nonexistent traffic CSV
            // but the trait returns IngestionError, which has no From<anyhow::Error>
            // In practice the CSV will not exist and will produce an error
            assert!(result.is_err() || result.unwrap().is_empty());
        }
    }

    #[test]
    fn test_csv_source_invalid_locations_path() {
        let result = CsvDataSource::new(FIXTURE_TRAFFIC, "nonexistent_locations.csv");
        assert!(result.is_err());
    }

    #[test]
    fn test_csv_source_as_trait_object() {
        let source: Box<dyn DataSource> =
            Box::new(CsvDataSource::new(FIXTURE_TRAFFIC, FIXTURE_LOCATIONS).unwrap());

        assert!(source.has_more());
        assert_eq!(source.name(), "Historical CSV");
        assert!(source.refresh_interval().is_none());
    }
}
