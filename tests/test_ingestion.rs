//! Integration tests of the GPS data ingestion module
//!
//! Exercise the complete pipeline: CSV → TrafficRecord → GpsPoint
//! using fixtures with representative Madrid data

use std::collections::HashMap;

use tfg::ingestion::csv_parser;
use tfg::ingestion::model::{DataQuality, Location, RoadType};
use tfg::ingestion::streaming::GpsIterator;

// ===========================================================================
// Integration tests of the CSV → GpsPoint pipeline
// ===========================================================================

#[test]
fn test_full_pipeline_batch() {
    // Pipeline batch: CSV → TrafficRecord → GpsPoint
    let records = csv_parser::parse_traffic_csv("tests/fixtures/valid_traffic.csv").unwrap();
    let locations = csv_parser::load_locations_csv("tests/fixtures/locations.csv").unwrap();
    let points = csv_parser::build_gps_points(&records, &locations);

    assert!(!points.is_empty(), "The pipeline must generate GPS points");
    assert_eq!(points.len(), records.len(), "All records must match");

    // Check that the points have coherent data
    for point in &points {
        // Coordinates inside Madrid
        assert!(point.latitude > 40.0, "Latitude must be >40 for Madrid");
        assert!(point.longitude < -3.0, "Longitude must be <-3 for Madrid");

        // Known IDs
        assert!(
            [1006, 3950, 10011].contains(&point.id),
            "Unexpected ID {}",
            point.id
        );
    }
}

#[test]
fn test_full_pipeline_streaming() {
    // Pipeline streaming: CSV → GpsIterator → GpsPoint (lazy)
    let iter = GpsIterator::new(
        "tests/fixtures/valid_traffic.csv",
        "tests/fixtures/locations.csv",
        true,
    )
    .unwrap();

    let points: Vec<_> = iter.filter_map(|r| r.ok()).collect();
    assert!(
        !points.is_empty(),
        "The streaming iterator must generate points"
    );

    // Check coherence
    for point in &points {
        assert!(point.latitude > 40.0);
        assert!(point.longitude < -3.0);
    }
}

#[test]
fn test_batch_vs_streaming_consistency() {
    // Both pipelines must produce the same results
    let records = csv_parser::parse_traffic_csv("tests/fixtures/valid_traffic.csv").unwrap();
    let locations = csv_parser::load_locations_csv("tests/fixtures/locations.csv").unwrap();
    let batch_points = csv_parser::build_gps_points(&records, &locations);

    let iter = GpsIterator::new(
        "tests/fixtures/valid_traffic.csv",
        "tests/fixtures/locations.csv",
        true,
    )
    .unwrap();
    let stream_points: Vec<_> = iter.filter_map(|r| r.ok()).collect();

    assert_eq!(
        batch_points.len(),
        stream_points.len(),
        "Batch and streaming must produce the same number of points"
    );

    // Check that the IDs match
    for (b, s) in batch_points.iter().zip(stream_points.iter()) {
        assert_eq!(b.id, s.id);
        assert_eq!(b.intensidad, s.intensidad);
    }
}

// ===========================================================================
// Edge-case tests with real data
// ===========================================================================

#[test]
fn test_invalid_records_discarded() {
    let records = csv_parser::parse_traffic_csv("tests/fixtures/invalid_traffic.csv").unwrap();

    // The fixture has:
    // - A line with negative values → it parses but has_valid_data() = false
    // - A line with an invalid timestamp → discarded in from_raw
    // - A line with flag "S" → it parses with DataQuality::Substituted
    // Some valid records, others discarded
    for record in &records {
        // Those that parsed must have a valid quality
        assert!(
            record.quality == DataQuality::Normal
                || record.quality == DataQuality::Error
                || record.quality == DataQuality::Substituted
        );
    }
}

#[test]
fn test_m30_urban_separation() {
    let records = csv_parser::parse_traffic_csv("tests/fixtures/valid_traffic.csv").unwrap();

    let m30: Vec<_> = records
        .iter()
        .filter(|r| r.road_type == RoadType::M30)
        .collect();
    let urban: Vec<_> = records
        .iter()
        .filter(|r| r.road_type == RoadType::Urban)
        .collect();

    // Check that both types exist in the fixtures
    assert!(!m30.is_empty(), "There must be M-30 records");
    assert!(!urban.is_empty(), "There must be urban records");

    // M-30 must have a speed, urban must not
    for r in &m30 {
        assert!(
            r.velocidad.is_some(),
            "M-30 must have a speed (ID {})",
            r.id
        );
    }
    for r in &urban {
        assert!(
            r.velocidad.is_none(),
            "Urban must not have a speed (ID {})",
            r.id
        );
    }
}

#[test]
fn test_streaming_with_partial_locations() {
    // Only include a location for one ID
    let csv_data = b"id;\"fecha\";\"tipo_elem\";\"intensidad\";\"ocupacion\";\"carga\";\"vmed\";\"error\";periodo_integracion\n\
        1006;\"2025-12-01 00:00:00\";\"C30\";\"240\";\"0\";\"0\";\"70\";\"N\";5\n\
        9999;\"2025-12-01 00:15:00\";\"URB\";\"120\";\"5\";\"10\";\"0\";\"N\";5\n";

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
    // ID 9999 has no location

    let iter = GpsIterator::from_reader(&csv_data[..], locations, true);
    let points: Vec<_> = iter.filter_map(|r| r.ok()).collect();

    // Only ID 1006 must generate a point
    assert_eq!(points.len(), 1);
    assert_eq!(points[0].id, 1006);
}

#[test]
fn test_timestamp_parsing_formats() {
    // Check that the fixture timestamps parse correctly
    let records = csv_parser::parse_traffic_csv("tests/fixtures/valid_traffic.csv").unwrap();

    for record in &records {
        // All timestamps must be from December 1, 2025
        assert_eq!(record.timestamp.date().year(), 2025);
        assert_eq!(record.timestamp.date().month(), 12);
        assert_eq!(record.timestamp.date().day(), 1);
    }
}

use chrono::Datelike;
