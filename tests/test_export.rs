//! Integration tests of the export module (US-25)
//!
//! Validate that the three export formats (JSON, CSV, GeoJSON)
//! generate valid files with the expected content

use std::fs;
use std::path::Path;

use chrono::NaiveDateTime;

use tfg::export::{export_csv, export_geojson, export_json};
use tfg::geospatial::s2_index::S2Index;
use tfg::ingestion::model::{DataQuality, GpsPoint, RoadType};
use tfg::metrics::delay_factor::compute_all_cells;
use tfg::metrics::model::CellDelayReport;
use tfg::metrics::statistics::compute_all_statistics;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn parse_dt(s: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
}

fn make_point(id: u32, lat: f64, lng: f64, carga: i32, velocidad: Option<i32>) -> GpsPoint {
    GpsPoint {
        id,
        timestamp: parse_dt("2025-12-01 08:00:00"),
        latitude: lat,
        longitude: lng,
        road_type: if velocidad.is_some() {
            RoadType::M30
        } else {
            RoadType::Urban
        },
        intensidad: 500,
        ocupacion: carga / 2,
        carga,
        velocidad,
        quality: DataQuality::Normal,
    }
}

/// Builds an S2Index with points in two areas of Madrid
fn build_test_index() -> S2Index {
    let points = vec![
        // Zone 1: Retiro (urban)
        make_point(1, 40.4155, -3.6844, 80, None),
        make_point(2, 40.4156, -3.6845, 85, None),
        make_point(3, 40.4157, -3.6846, 90, None),
        make_point(4, 40.4158, -3.6847, 75, None),
        make_point(5, 40.4159, -3.6848, 70, None),
        // Zone 2: southern M-30
        make_point(6, 40.3900, -3.6900, 40, Some(80)),
        make_point(7, 40.3901, -3.6901, 45, Some(75)),
        make_point(8, 40.3902, -3.6902, 50, Some(70)),
        make_point(9, 40.3903, -3.6903, 55, Some(65)),
        make_point(10, 40.3904, -3.6904, 60, Some(60)),
    ];
    S2Index::build_from_iterator(points, 15)
}

fn default_config() -> tfg::config::MetricsConfig {
    tfg::config::MetricsConfig {
        min_observations: 2,
        m30_reference_speed: 90.0,
        urban_reference_speed: 50.0,
    }
}

// ---------------------------------------------------------------------------
// Tests: export_json
// ---------------------------------------------------------------------------

#[test]
fn test_export_json_creates_file() {
    let index = build_test_index();
    let config = default_config();
    let reports = compute_all_cells(&index, &config);
    let stats = compute_all_statistics(&index, &config);

    let out_path = Path::new("/tmp/tfg_test_export_json.json");
    let summary = export_json(&index, &reports, &stats, &[], out_path, "Test Source").unwrap();

    assert!(out_path.exists(), "The JSON file must exist");
    assert!(summary.bytes_written > 0, "Bytes must have been written");
    assert!(summary.items_exported > 0, "Cells must have been exported");

    // Clean up
    let _ = fs::remove_file(out_path);
}

#[test]
fn test_export_json_valid_structure() {
    let index = build_test_index();
    let config = default_config();
    let reports = compute_all_cells(&index, &config);
    let stats = compute_all_statistics(&index, &config);

    let out_path = Path::new("/tmp/tfg_test_export_json_struct.json");
    export_json(&index, &reports, &stats, &[], out_path, "Test").unwrap();

    let content = fs::read_to_string(out_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).expect("valid JSON");

    // Check the root structure
    assert!(json["pipeline"].is_object(), "Must have a 'pipeline' field");
    assert!(json["cells"].is_array(), "Must have a 'cells' field");

    // Check the pipeline metadata
    let pipeline = &json["pipeline"];
    assert!(pipeline["total_gps_points"].as_u64().unwrap() > 0);
    assert!(pipeline["s2_cell_count"].as_u64().unwrap() > 0);
    assert!(pipeline["source"].as_str().unwrap() == "Test");
    assert!(!pipeline["timestamp"].as_str().unwrap().is_empty());

    let _ = fs::remove_file(out_path);
}

#[test]
fn test_export_json_cells_have_vertices() {
    let index = build_test_index();
    let config = default_config();
    let reports = compute_all_cells(&index, &config);

    let out_path = Path::new("/tmp/tfg_test_export_json_vertices.json");
    export_json(&index, &reports, &[], &[], out_path, "Test").unwrap();

    let content = fs::read_to_string(out_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();

    let cells = json["cells"].as_array().unwrap();
    assert!(!cells.is_empty(), "There must be cells");

    for cell in cells {
        let vertices = cell["vertices"].as_array().unwrap();
        assert_eq!(vertices.len(), 4, "Every S2 cell has exactly 4 vertices");
        for vertex in vertices {
            let coords = vertex.as_array().unwrap();
            assert_eq!(coords.len(), 2, "Every vertex has lat and lng");
            // Check that they are within the approximate geographic range of Madrid
            let lat = coords[0].as_f64().unwrap();
            let lng = coords[1].as_f64().unwrap();
            assert!(lat > 35.0 && lat < 45.0, "Latitude in Spain range: {}", lat);
            assert!(
                lng > -10.0 && lng < 5.0,
                "Longitude in Spain range: {}",
                lng
            );
        }
    }

    let _ = fs::remove_file(out_path);
}

#[test]
fn test_export_json_metrics_present() {
    let index = build_test_index();
    let config = default_config();
    let reports = compute_all_cells(&index, &config);

    let out_path = Path::new("/tmp/tfg_test_export_json_metrics.json");
    export_json(&index, &reports, &[], &[], out_path, "Test").unwrap();

    let content = fs::read_to_string(out_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();

    let cells = json["cells"].as_array().unwrap();
    let cells_with_metrics: Vec<_> = cells.iter().filter(|c| !c["metrics"].is_null()).collect();

    assert!(
        !cells_with_metrics.is_empty(),
        "There must be cells with metrics"
    );

    for cell in &cells_with_metrics {
        let m = &cell["metrics"];
        assert!(m["avg_delay_factor"].is_number());
        assert!(m["congestion_level"].is_string());
        assert!(m["road_type"].is_string());
        assert!(m["observation_count"].is_number());
    }

    let _ = fs::remove_file(out_path);
}

#[test]
fn test_export_json_empty_index() {
    let index = S2Index::new(15);
    let out_path = Path::new("/tmp/tfg_test_export_json_empty.json");
    let result = export_json(&index, &[], &[], &[], out_path, "Empty");
    assert!(result.is_ok(), "Exporting an empty index must not fail");

    let summary = result.unwrap();
    assert_eq!(summary.items_exported, 0);
    assert!(out_path.exists());

    let _ = fs::remove_file(out_path);
}

// ---------------------------------------------------------------------------
// Tests: export_csv
// ---------------------------------------------------------------------------

#[test]
fn test_export_csv_creates_file() {
    let index = build_test_index();
    let config = default_config();
    let reports = compute_all_cells(&index, &config);

    let out_path = Path::new("/tmp/tfg_test_export.csv");
    let summary = export_csv(&reports, &index, out_path).unwrap();

    assert!(out_path.exists());
    assert!(summary.bytes_written > 0);
    assert_eq!(summary.items_exported, reports.len());

    let _ = fs::remove_file(out_path);
}

#[test]
fn test_export_csv_has_correct_headers() {
    let index = build_test_index();
    let config = default_config();
    let reports = compute_all_cells(&index, &config);

    let out_path = Path::new("/tmp/tfg_test_export_headers.csv");
    export_csv(&reports, &index, out_path).unwrap();

    let content = fs::read_to_string(out_path).unwrap();
    let first_line = content.lines().next().unwrap();

    assert!(first_line.contains("cell_id"));
    assert!(first_line.contains("avg_srf"));
    assert!(first_line.contains("congestion_level"));
    assert!(first_line.contains("road_type"));
    assert!(first_line.contains("observation_count"));

    let _ = fs::remove_file(out_path);
}

#[test]
fn test_export_csv_row_count() {
    let index = build_test_index();
    let config = default_config();
    let reports = compute_all_cells(&index, &config);

    let out_path = Path::new("/tmp/tfg_test_export_rows.csv");
    export_csv(&reports, &index, out_path).unwrap();

    let content = fs::read_to_string(out_path).unwrap();
    let data_lines = content.lines().count() - 1; // -1 for the header

    assert_eq!(
        data_lines,
        reports.len(),
        "The CSV must have as many rows as reports"
    );

    let _ = fs::remove_file(out_path);
}

#[test]
fn test_export_csv_srf_values_in_range() {
    let index = build_test_index();
    let config = default_config();
    let reports = compute_all_cells(&index, &config);

    let out_path = Path::new("/tmp/tfg_test_export_srf.csv");
    export_csv(&reports, &index, out_path).unwrap();

    let content = fs::read_to_string(out_path).unwrap();
    for line in content.lines().skip(1) {
        let cols: Vec<&str> = line.split(',').collect();
        if cols.len() >= 4 {
            let avg_srf: f64 = cols[3].parse().unwrap_or(f64::NAN);
            assert!(
                (0.0..=2.0).contains(&avg_srf),
                "SRF outside a reasonable range: {}",
                avg_srf
            );
        }
    }

    let _ = fs::remove_file(out_path);
}

// ---------------------------------------------------------------------------
// Tests: export_geojson
// ---------------------------------------------------------------------------

#[test]
fn test_export_geojson_creates_file() {
    let index = build_test_index();
    let config = default_config();
    let reports = compute_all_cells(&index, &config);

    let out_path = Path::new("/tmp/tfg_test_export.geojson");
    let summary = export_geojson(&index, &reports, out_path).unwrap();

    assert!(out_path.exists());
    assert!(summary.bytes_written > 0);

    let _ = fs::remove_file(out_path);
}

#[test]
fn test_export_geojson_valid_structure() {
    let index = build_test_index();
    let config = default_config();
    let reports = compute_all_cells(&index, &config);

    let out_path = Path::new("/tmp/tfg_test_export_struct.geojson");
    export_geojson(&index, &reports, out_path).unwrap();

    let content = fs::read_to_string(out_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).expect("valid GeoJSON");

    assert_eq!(json["type"].as_str().unwrap(), "FeatureCollection");
    assert!(json["features"].is_array());
    assert!(!json["features"].as_array().unwrap().is_empty());

    let _ = fs::remove_file(out_path);
}

#[test]
fn test_export_geojson_polygon_closed() {
    let index = build_test_index();
    let reports: Vec<CellDelayReport> = vec![];

    let out_path = Path::new("/tmp/tfg_test_export_closed.geojson");
    export_geojson(&index, &reports, out_path).unwrap();

    let content = fs::read_to_string(out_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();

    for feature in json["features"].as_array().unwrap() {
        let coords = &feature["geometry"]["coordinates"][0];
        let ring = coords.as_array().unwrap();

        // RFC 7946: the polygon must be closed (first == last point)
        assert!(
            ring.len() >= 4,
            "Ring must have at least 4 points (3 + closing)"
        );
        let first = &ring[0];
        let last = &ring[ring.len() - 1];
        assert_eq!(first, last, "The ring must be closed (first == last point)");
    }

    let _ = fs::remove_file(out_path);
}

#[test]
fn test_export_geojson_lng_lat_order() {
    // GeoJSON uses [longitude, latitude] (RFC 7946), NOT [lat, lng]
    let index = build_test_index();
    let reports: Vec<CellDelayReport> = vec![];

    let out_path = Path::new("/tmp/tfg_test_export_lnglat.geojson");
    export_geojson(&index, &reports, out_path).unwrap();

    let content = fs::read_to_string(out_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();

    for feature in json["features"].as_array().unwrap() {
        let coords = &feature["geometry"]["coordinates"][0];
        for coord in coords.as_array().unwrap() {
            let lng = coord[0].as_f64().unwrap(); // first value = longitude
            let lat = coord[1].as_f64().unwrap(); // second value = latitude
                                                  // Madrid: lng ≈ -3.7, lat ≈ 40.4 → if swapped, lat would come out as ~-3, which is wrong
            assert!(
                lng < 0.0 && lng > -10.0,
                "First value must be longitude (negative in Madrid): {}",
                lng
            );
            assert!(
                lat > 35.0 && lat < 45.0,
                "Second value must be latitude (40.x in Madrid): {}",
                lat
            );
        }
    }

    let _ = fs::remove_file(out_path);
}

#[test]
fn test_export_geojson_properties_with_metrics() {
    let index = build_test_index();
    let config = default_config();
    let reports = compute_all_cells(&index, &config);

    let out_path = Path::new("/tmp/tfg_test_export_props.geojson");
    export_geojson(&index, &reports, out_path).unwrap();

    let content = fs::read_to_string(out_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();

    let features_with_srf: Vec<_> = json["features"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| !f["properties"]["avg_srf"].is_null())
        .collect();

    assert!(
        !features_with_srf.is_empty(),
        "There must be features with SRF"
    );

    for f in &features_with_srf {
        let props = &f["properties"];
        assert!(props["avg_srf"].is_number());
        assert!(props["congestion_level"].is_string());
        assert!(props["road_type"].is_string());
        assert!(props["observation_count"].is_number());
    }

    let _ = fs::remove_file(out_path);
}

// ---------------------------------------------------------------------------
// Test: compatibility with dashboard.html
// ---------------------------------------------------------------------------

#[test]
fn test_json_export_dashboard_compatible() {
    // Verifies that the exported JSON has EXACTLY the fields
    // that dashboard.html consumes: pipeline.total_gps_points, cells[].vertices,
    // cells[].metrics.avg_delay_factor, cells[].metrics.congestion_level
    let index = build_test_index();
    let config = default_config();
    let reports = compute_all_cells(&index, &config);
    let stats = compute_all_statistics(&index, &config);

    let out_path = Path::new("/tmp/tfg_test_dashboard_compat.json");
    export_json(&index, &reports, &stats, &[], out_path, "Madrid API").unwrap();

    let content = fs::read_to_string(out_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();

    // Fields used by dashboard.html
    assert!(json["pipeline"]["total_gps_points"].is_number());
    assert!(json["pipeline"]["s2_cell_count"].is_number());
    assert!(json["pipeline"]["cells_with_metrics"].is_number());
    assert!(json["pipeline"]["cells_with_stats"].is_number());

    let cells = json["cells"].as_array().unwrap();
    for cell in cells {
        assert!(
            cell["vertices"].is_array(),
            "vertices is required for the map"
        );
        assert!(cell["lat"].is_number());
        assert!(cell["lng"].is_number());
        assert!(cell["point_count"].is_number());
        // metrics may be null if there are few observations
        if !cell["metrics"].is_null() {
            assert!(cell["metrics"]["avg_delay_factor"].is_number());
            assert!(cell["metrics"]["congestion_level"].is_string());
        }
    }

    let _ = fs::remove_file(out_path);
}

#[test]
fn test_export_filters_insufficient_observation_cells() {
    // Build an index with one cell having >= 2 points and another with exactly 1 point
    let points = vec![
        // Cell A: 2 points (enough)
        make_point(1, 40.4155, -3.6844, 40, None),
        make_point(2, 40.4156, -3.6845, 45, None),
        // Cell B: 1 point (not enough)
        make_point(3, 40.5000, -3.6000, 50, None),
    ];
    let index = S2Index::build_from_iterator(points, 15);
    let config = default_config(); // min_observations = 2
    let reports = compute_all_cells(&index, &config);

    // There must be a report for Cell A and none for Cell B
    assert_eq!(reports.len(), 1);

    // 1. Verify JSON Export
    let json_path = Path::new("/tmp/test_filter_1point.json");
    export_json(&index, &reports, &[], &[], json_path, "Test").unwrap();
    let content = fs::read_to_string(json_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();
    let cells = json["cells"].as_array().unwrap();

    // The cell with 1 point must have been excluded, only the one with >= 2 must be exported
    // (or cells with 0 points if injected by madrid coverage, but here there are none with 0 points)
    // So there must be only 1 cell exported (Cell A)
    assert_eq!(cells.len(), 1);
    assert_eq!(cells[0]["point_count"].as_u64().unwrap(), 2);

    let _ = fs::remove_file(json_path);

    // 2. Verify GeoJSON Export
    let geojson_path = Path::new("/tmp/test_filter_1point.geojson");
    export_geojson(&index, &reports, geojson_path).unwrap();
    let geojson_content = fs::read_to_string(geojson_path).unwrap();
    let geojson: serde_json::Value = serde_json::from_str(&geojson_content).unwrap();
    let features = geojson["features"].as_array().unwrap();

    assert_eq!(features.len(), 1);
    assert_eq!(
        features[0]["properties"]["point_count"].as_u64().unwrap(),
        2
    );

    let _ = fs::remove_file(geojson_path);
}
