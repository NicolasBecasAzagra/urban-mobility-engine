//! `DataSource` trait for data source abstraction
//!
//! Defines a common interface that lets the engine operate
//! identically on historical (CSV) or real-time (API) data
//!
//! ## Design
//!
//! The trait follows the **Strategy** pattern: the engine receives a
//! `Box<dyn DataSource>` and runs the pipeline without knowing
//! the concrete source. This allows:
//!
//! - Using CSV for historical analysis and validation
//! - Using the real-time API for near-real-time mode
//! - Using mocks in tests with no network/disk dependency
//!
//! ## Example
//!
//! ```rust,no_run
//! use tfg::ingestion::datasource::DataSource;
//! use tfg::ingestion::csv_source::CsvDataSource;
//!
//! let mut source = CsvDataSource::new(
//!     "data/samples/sample_traffic.csv",
//!     "data/samples/locations.csv",
//! ).unwrap();
//!
//! while source.has_more() {
//!     let points = source.fetch_batch().unwrap();
//!     println!("Received {} points", points.len());
//! }
//! ```

use std::time::Duration;

use crate::ingestion::error::IngestionError;
use crate::ingestion::model::GpsPoint;

/// Trait that abstracts the engine's data source
///
/// Allows using historical (CSV) or real-time (API) data
/// interchangeably without changing the pipeline logic
///
/// ## Implementations
///
/// - [`super::csv_source::CsvDataSource`] — Historical data from CSV
/// - `LiveApiSource` (future) — Real-time traffic API
pub trait DataSource {
    /// Descriptive name of the source for logging
    fn name(&self) -> &str;

    /// Fetches a batch of GPS points from the source
    ///
    /// - **CSV**: reads all the records of the file
    /// - **API**: performs an HTTP request and parses the response
    ///
    /// # Errors
    /// Returns an error if reading or parsing fails
    fn fetch_batch(&mut self) -> Result<Vec<GpsPoint>, IngestionError>;

    /// Indicates whether the source has more data available
    ///
    /// - **CSV**: `false` after the first complete read
    /// - **API**: always `true` (infinite source)
    fn has_more(&self) -> bool;

    /// Refresh period in seconds
    ///
    /// - **CSV**: `None` (finite source, read once)
    /// - **API**: `Some(300)` (every 5 minutes)
    fn refresh_interval(&self) -> Option<Duration>;
}

#[cfg(test)]
mod tests {
    use super::*;

    /// DataSource mock for tests with no disk/network dependency
    struct MockDataSource {
        data: Vec<Vec<GpsPoint>>,
        cursor: usize,
    }

    impl MockDataSource {
        fn new(batches: Vec<Vec<GpsPoint>>) -> Self {
            MockDataSource {
                data: batches,
                cursor: 0,
            }
        }

        fn empty() -> Self {
            MockDataSource {
                data: vec![],
                cursor: 0,
            }
        }
    }

    impl DataSource for MockDataSource {
        fn name(&self) -> &str {
            "Mock DataSource"
        }

        fn fetch_batch(&mut self) -> Result<Vec<GpsPoint>, IngestionError> {
            if self.cursor < self.data.len() {
                let batch = self.data[self.cursor].clone();
                self.cursor += 1;
                Ok(batch)
            } else {
                Ok(vec![])
            }
        }

        fn has_more(&self) -> bool {
            self.cursor < self.data.len()
        }

        fn refresh_interval(&self) -> Option<Duration> {
            None
        }
    }

    // Helpers to create test points
    fn make_test_point(id: u32, carga: i32) -> GpsPoint {
        use crate::ingestion::model::{DataQuality, RoadType};
        use chrono::NaiveDateTime;

        GpsPoint {
            id,
            timestamp: NaiveDateTime::parse_from_str("2025-12-01 08:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
            latitude: 40.45,
            longitude: -3.72,
            road_type: RoadType::Urban,
            intensidad: 300,
            ocupacion: 15,
            carga,
            velocidad: None,
            quality: DataQuality::Normal,
        }
    }

    #[test]
    fn test_mock_datasource_single_batch() {
        let points = vec![make_test_point(1, 30), make_test_point(2, 50)];
        let mut source = MockDataSource::new(vec![points]);

        assert!(source.has_more());
        assert_eq!(source.name(), "Mock DataSource");
        assert!(source.refresh_interval().is_none());

        let batch = source.fetch_batch().unwrap();
        assert_eq!(batch.len(), 2);
        assert!(!source.has_more());
    }

    #[test]
    fn test_mock_datasource_multiple_batches() {
        let batch1 = vec![make_test_point(1, 30)];
        let batch2 = vec![make_test_point(2, 50), make_test_point(3, 70)];
        let mut source = MockDataSource::new(vec![batch1, batch2]);

        assert!(source.has_more());

        let b1 = source.fetch_batch().unwrap();
        assert_eq!(b1.len(), 1);
        assert!(source.has_more());

        let b2 = source.fetch_batch().unwrap();
        assert_eq!(b2.len(), 2);
        assert!(!source.has_more());
    }

    #[test]
    fn test_mock_datasource_empty() {
        let mut source = MockDataSource::empty();

        assert!(!source.has_more());
        let batch = source.fetch_batch().unwrap();
        assert!(batch.is_empty());
    }

    #[test]
    fn test_datasource_as_trait_object() {
        // Verify that DataSource works as a dynamic (dyn) object
        let points = vec![make_test_point(1, 30)];
        let source: Box<dyn DataSource> = Box::new(MockDataSource::new(vec![points]));

        assert!(source.has_more());
        assert_eq!(source.name(), "Mock DataSource");
    }
}
