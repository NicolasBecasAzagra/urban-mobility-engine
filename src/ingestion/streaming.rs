//! Streaming parser for large GPS datasets
//!
//! Provides a lazy iterator (`GpsIterator`) that processes
//! CSV records one by one without loading the whole file into memory
//! Ideal for files with 11M+ records (~800MB)
//!
//! ## Usage example
//!
//! ```no_run
//! use tfg::ingestion::streaming::GpsIterator;
//!
//! let iter = GpsIterator::new(
//!     "data/raw/12-2025.csv",
//!     "data/samples/locations.csv",
//!     true, // skip_invalid
//! ).unwrap();
//!
//! for result in iter {
//!     match result {
//!         Ok(point) => println!("GPS: {:?}", point),
//!         Err(e) => eprintln!("Error: {}", e),
//!     }
//! }
//! ```

use std::collections::HashMap;
use std::io::Read;
use std::path::Path;

use anyhow::{Context, Result};

use crate::ingestion::csv_parser::load_locations_csv;
use crate::ingestion::model::{GpsPoint, Location, RawTrafficRecord, TrafficRecord};

/// Lazy iterator that yields `GpsPoint`s one by one from a traffic CSV
///
/// Internally wraps a `csv::DeserializeRecordsIntoIter` and performs
/// the conversion + location lookup on the fly. It does not load the whole
/// file into memory
///
/// ## Error behavior
///
/// - If `skip_invalid` is `true`: invalid records are discarded
///   silently (with a log warning). The iterator never returns `Err`
/// - If `skip_invalid` is `false`: errors are propagated as `Err`
pub struct GpsIterator<R: Read> {
    /// Internal CSV iterator
    inner: csv::DeserializeRecordsIntoIter<R, RawTrafficRecord>,
    /// Location lookup by ID
    locations: HashMap<u32, Location>,
    /// If true, invalid records are skipped; if false, they are propagated as errors
    skip_invalid: bool,
    /// Counters for logging
    stats: IteratorStats,
}

/// Iterator statistics for reporting
#[derive(Debug, Default)]
struct IteratorStats {
    /// Total records processed
    processed: u64,
    /// Valid records emitted
    valid: u64,
    /// Records discarded due to a parse error
    parse_errors: u64,
    /// Records discarded for lacking a location
    missing_location: u64,
}

impl GpsIterator<std::fs::File> {
    /// Creates a new `GpsIterator` reading from the given file paths
    ///
    /// # Arguments
    /// * `traffic_path` - Path to the historical traffic CSV
    /// * `locations_path` - Path to the locations CSV
    /// * `skip_invalid` - Whether to skip invalid records or propagate errors
    ///
    /// # Errors
    /// Returns an error if the files cannot be opened
    pub fn new<P1: AsRef<Path>, P2: AsRef<Path>>(
        traffic_path: P1,
        locations_path: P2,
        skip_invalid: bool,
    ) -> Result<Self> {
        let traffic_path = traffic_path.as_ref();

        // Load locations first (single in-memory load, ~5K entries)
        let locations = load_locations_csv(locations_path)?;

        // Create the streaming CSV reader
        let reader = csv::ReaderBuilder::new()
            .delimiter(b';')
            .double_quote(true)
            .has_headers(true)
            .from_path(traffic_path)
            .with_context(|| format!("Could not open CSV: {}", traffic_path.display()))?;

        Ok(GpsIterator {
            inner: reader.into_deserialize(),
            locations,
            skip_invalid,
            stats: IteratorStats::default(),
        })
    }
}

impl<R: Read> GpsIterator<R> {
    /// Creates a `GpsIterator` from a generic reader (useful for tests)
    pub fn from_reader(reader: R, locations: HashMap<u32, Location>, skip_invalid: bool) -> Self {
        let csv_reader = csv::ReaderBuilder::new()
            .delimiter(b';')
            .double_quote(true)
            .has_headers(true)
            .from_reader(reader);

        GpsIterator {
            inner: csv_reader.into_deserialize(),
            locations,
            skip_invalid,
            stats: IteratorStats::default(),
        }
    }

    /// Returns the current statistics of the iterator
    pub fn stats(&self) -> (u64, u64, u64, u64) {
        (
            self.stats.processed,
            self.stats.valid,
            self.stats.parse_errors,
            self.stats.missing_location,
        )
    }
}

impl<R: Read> Iterator for GpsIterator<R> {
    type Item = Result<GpsPoint>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let result = self.inner.next()?;
            self.stats.processed += 1;

            // Try to deserialize the raw record
            let raw = match result {
                Ok(raw) => raw,
                Err(e) => {
                    self.stats.parse_errors += 1;
                    if self.skip_invalid {
                        tracing::trace!(
                            "Record {} discarded (CSV error): {}",
                            self.stats.processed,
                            e
                        );
                        continue;
                    }
                    return Some(Err(e.into()));
                }
            };

            // Convert to a normalized TrafficRecord
            let record = match TrafficRecord::from_raw(&raw) {
                Ok(record) => record,
                Err(e) => {
                    self.stats.parse_errors += 1;
                    if self.skip_invalid {
                        tracing::trace!(
                            "Record {} (ID {}) discarded (validation): {}",
                            self.stats.processed,
                            raw.id,
                            e
                        );
                        continue;
                    }
                    return Some(Err(e.into()));
                }
            };

            // Location lookup
            match self.locations.get(&record.id) {
                Some(location) => {
                    self.stats.valid += 1;
                    let point = GpsPoint::from_record_and_location(&record, location);
                    return Some(Ok(point));
                }
                None => {
                    self.stats.missing_location += 1;
                    if self.skip_invalid {
                        continue;
                    }
                    return Some(Err(anyhow::anyhow!(
                        "ID {} has no associated location",
                        record.id
                    )));
                }
            }
        }
    }
}

impl<R: Read> Drop for GpsIterator<R> {
    fn drop(&mut self) {
        let (processed, valid, parse_errors, missing) = self.stats();
        if processed > 0 {
            tracing::info!(
                "GpsIterator: {} processed, {} valid, {} parse errors, {} without location",
                processed,
                valid,
                parse_errors,
                missing
            );
        }
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
    fn test_gps_iterator_basic() {
        let iter = GpsIterator::new(FIXTURE_TRAFFIC, FIXTURE_LOCATIONS, true).unwrap();
        let points: Vec<_> = iter.filter_map(|r| r.ok()).collect();

        assert!(!points.is_empty());
        for point in &points {
            assert!(point.latitude > 40.0);
            assert!(point.longitude < -3.0);
        }
    }

    #[test]
    fn test_gps_iterator_skip_invalid() {
        let iter = GpsIterator::new(
            "tests/fixtures/invalid_traffic.csv",
            FIXTURE_LOCATIONS,
            true,
        )
        .unwrap();
        let points: Vec<_> = iter.filter_map(|r| r.ok()).collect();

        // Some invalid records are skipped, the iterator does not fail
        assert!(points.len() < 4);
    }

    #[test]
    fn test_gps_iterator_stats() {
        let mut iter = GpsIterator::new(FIXTURE_TRAFFIC, FIXTURE_LOCATIONS, true).unwrap();

        // Consume all items
        for _ in iter.by_ref() {}

        let (processed, valid, _parse_errors, _missing) = iter.stats();
        assert!(processed > 0);
        assert!(valid > 0);
        assert!(valid <= processed);
    }

    #[test]
    fn test_gps_iterator_from_reader() {
        let csv_data = b"id;\"fecha\";\"tipo_elem\";\"intensidad\";\"ocupacion\";\"carga\";\"vmed\";\"error\";periodo_integracion\n\
            1006;\"2025-12-01 00:00:00\";\"C30\";\"240\";\"0\";\"0\";\"70\";\"N\";5\n";

        let mut locations = HashMap::new();
        locations.insert(
            1006,
            Location {
                id: 1006,
                tipo_elem: "M30".to_string(),
                distrito: Some(14),
                nombre: "PM20001".to_string(),
                longitud: -3.66,
                latitud: 40.41,
            },
        );

        let iter = GpsIterator::from_reader(&csv_data[..], locations, true);
        let points: Vec<_> = iter.filter_map(|r| r.ok()).collect();
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].id, 1006);
    }

    #[test]
    fn test_gps_iterator_propagate_errors() {
        let csv_data = b"id;\"fecha\";\"tipo_elem\";\"intensidad\";\"ocupacion\";\"carga\";\"vmed\";\"error\";periodo_integracion\n\
            1006;\"2025-12-01 00:00:00\";\"C30\";\"240\";\"0\";\"0\";\"70\";\"N\";5\n";

        let locations = HashMap::new(); // empty → missing location

        let iter = GpsIterator::from_reader(&csv_data[..], locations, false);
        let results: Vec<_> = iter.collect();
        assert_eq!(results.len(), 1);
        assert!(results[0].is_err()); // Missing location → error
    }
}
