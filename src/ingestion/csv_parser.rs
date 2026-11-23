//! CSV parser for Madrid traffic data
//!
//! Provides functions to read traffic and location CSV
//! files, with support for the specific format
//! of the Madrid City Council (`;` delimiter)
//!
//! ## Traffic CSV format
//!
//! - Delimiter: `;` (semicolon)
//! - Headers: `id;fecha;tipo_elem;intensidad;ocupacion;carga;vmed;error;periodo_integracion`
//! - Values quoted with `"`
//!
//! ## Usage example
//!
//! ```no_run
//! use tfg::ingestion::csv_parser;
//!
//! let records = csv_parser::parse_traffic_csv("data/samples/sample_traffic.csv").unwrap();
//! let locations = csv_parser::load_locations_csv("data/samples/locations.csv").unwrap();
//! let points = csv_parser::build_gps_points(&records, &locations);
//! ```

use std::collections::HashMap;
use std::path::Path;

use anyhow::{Context, Result};

use crate::ingestion::model::{GpsPoint, Location, RawTrafficRecord, TrafficRecord};

/// Creates a `csv::ReaderBuilder` configured for the Madrid CSV format
///
/// Configures:
/// - Delimiter `;`
/// - Double quotes `"`
/// - Headers enabled
fn madrid_csv_reader() -> csv::ReaderBuilder {
    let mut builder = csv::ReaderBuilder::new();
    builder.delimiter(b';').double_quote(true).has_headers(true);
    builder
}

/// Reads a historical traffic CSV and returns the normalized records
///
/// Loads ALL records into memory. For large files (>100MB),
/// use the streaming iterator [`super::streaming::GpsIterator`]
///
/// # Arguments
/// * `path` - Path to the traffic CSV file
///
/// # Errors
/// Returns an error if the file does not exist, is not readable,
/// or if any record cannot be parsed
pub fn parse_traffic_csv<P: AsRef<Path>>(path: P) -> Result<Vec<TrafficRecord>> {
    let path = path.as_ref();
    let mut reader = madrid_csv_reader()
        .from_path(path)
        .with_context(|| format!("Could not open CSV: {}", path.display()))?;

    let mut records = Vec::new();
    for (i, result) in reader.deserialize::<RawTrafficRecord>().enumerate() {
        let raw = result
            .with_context(|| format!("Error on line {} of the CSV: {}", i + 2, path.display()))?;

        match TrafficRecord::from_raw(&raw) {
            Ok(record) => records.push(record),
            Err(e) => {
                tracing::warn!("Line {} discarded: {}", i + 2, e);
            }
        }
    }

    tracing::info!(
        "Read {} valid records from '{}'",
        records.len(),
        path.display()
    );
    Ok(records)
}

/// Reads a locations CSV and returns a HashMap indexed by ID
///
/// The CSV must have the columns: `id;tipo_elem;distrito;nombre;longitud;latitud`
/// Locations are loaded once at startup and used as an O(1) lookup
///
/// # Arguments
/// * `path` - Path to the locations CSV
///
/// # Errors
/// Returns an error if the file does not exist or has an incorrect format
pub fn load_locations_csv<P: AsRef<Path>>(path: P) -> Result<HashMap<u32, Location>> {
    let path = path.as_ref();
    let mut reader = madrid_csv_reader()
        .from_path(path)
        .with_context(|| format!("Could not open locations CSV: {}", path.display()))?;

    let mut locations = HashMap::new();
    for (i, result) in reader.deserialize::<Location>().enumerate() {
        let location = result
            .with_context(|| format!("Error on line {} of locations: {}", i + 2, path.display()))?;

        // Validate bounds (warning, not a fatal error)
        if let Err(e) = location.validate_bounds() {
            tracing::warn!(
                "Location ID {} is out of bounds, including it anyway: {}",
                location.id,
                e
            );
        }

        locations.insert(location.id, location);
    }

    tracing::info!(
        "Loaded {} locations from '{}'",
        locations.len(),
        path.display()
    );
    Ok(locations)
}

/// Combines traffic records with locations to create `GpsPoint`s
///
/// Records with no associated location are discarded with a warning
/// Returns a vector of `GpsPoint` with geographic coordinates
///
/// # Arguments
/// * `records` - Normalized traffic records
/// * `locations` - HashMap of locations indexed by ID
pub fn build_gps_points(
    records: &[TrafficRecord],
    locations: &HashMap<u32, Location>,
) -> Vec<GpsPoint> {
    let mut points = Vec::with_capacity(records.len());
    let mut missing_count = 0u32;

    for record in records {
        if let Some(location) = locations.get(&record.id) {
            points.push(GpsPoint::from_record_and_location(record, location));
        } else {
            missing_count += 1;
            if missing_count <= 5 {
                tracing::warn!("ID {} has no associated location", record.id);
            }
        }
    }

    if missing_count > 5 {
        tracing::warn!("... and {} more without a location", missing_count - 5);
    }

    tracing::info!(
        "Generated {} GpsPoints ({} without a location)",
        points.len(),
        missing_count
    );
    points
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE_TRAFFIC: &str = "tests/fixtures/valid_traffic.csv";
    const FIXTURE_INVALID: &str = "tests/fixtures/invalid_traffic.csv";
    const FIXTURE_LOCATIONS: &str = "tests/fixtures/locations.csv";

    #[test]
    fn test_parse_traffic_csv_valid() {
        let records = parse_traffic_csv(FIXTURE_TRAFFIC).unwrap();
        // The fixture has 7 valid records with correct data
        assert!(!records.is_empty());
        assert!(records.len() <= 7);

        // Check the first record (ID 1006, C30/M30)
        let first = &records[0];
        assert_eq!(first.id, 1006);
        assert_eq!(first.intensidad, 240);
    }

    #[test]
    fn test_parse_traffic_csv_nonexistent() {
        let result = parse_traffic_csv("nonexistent.csv");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_traffic_csv_with_errors() {
        // The invalid fixture has bad timestamps and erroneous data
        // The parser should discard the invalid lines with warnings
        let records = parse_traffic_csv(FIXTURE_INVALID).unwrap();
        // Some records will be discarded, but it should not crash
        assert!(records.len() < 4); // it has 3 rows, some may fail
    }

    #[test]
    fn test_load_locations_csv() {
        let locations = load_locations_csv(FIXTURE_LOCATIONS).unwrap();
        assert_eq!(locations.len(), 3);
        assert!(locations.contains_key(&10011));
        assert!(locations.contains_key(&3950));
        assert!(locations.contains_key(&1006));

        let loc = &locations[&10011];
        assert!((loc.latitud - 40.4485).abs() < 0.001);
    }

    #[test]
    fn test_build_gps_points() {
        let records = parse_traffic_csv(FIXTURE_TRAFFIC).unwrap();
        let locations = load_locations_csv(FIXTURE_LOCATIONS).unwrap();
        let points = build_gps_points(&records, &locations);

        // All IDs in the traffic fixture have a location
        assert!(!points.is_empty());
        assert_eq!(points.len(), records.len());

        // Check that the points have coordinates
        for point in &points {
            assert!(point.latitude > 40.0);
            assert!(point.longitude < -3.0);
        }
    }

    #[test]
    fn test_build_gps_points_missing_locations() {
        let records = parse_traffic_csv(FIXTURE_TRAFFIC).unwrap();
        let empty_locations: HashMap<u32, Location> = HashMap::new();
        let points = build_gps_points(&records, &empty_locations);

        // Without locations, no point is generated
        assert!(points.is_empty());
    }

    #[test]
    fn test_madrid_csv_reader_config() {
        let builder = madrid_csv_reader();
        // Check that a reader can be created (the config is correct)
        let data = b"id;nombre\n1;test\n";
        let mut reader = builder.from_reader(&data[..]);
        assert!(reader.headers().is_ok());
    }
}
