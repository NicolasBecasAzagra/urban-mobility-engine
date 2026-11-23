//! Integration tests of the near-real-time (live) mode
//!
//! Verify the complete pipeline:
//! `XML fixture → LiveApiSource → PollingScheduler → metrics`
//!
//! The tests use local fixtures (no network). The `#[ignore]` test
//! at the end validates against the real Madrid API

use std::time::Duration;

use tfg::config::{GeospatialConfig, MetricsConfig};
use tfg::ingestion::datasource::DataSource;
use tfg::ingestion::error::IngestionError;
use tfg::ingestion::live_api::LiveApiSource;
use tfg::ingestion::model::{DataQuality, GpsPoint, RoadType};
use tfg::ingestion::scheduler::PollingScheduler;

// ===========================================================================
// Constants
// ===========================================================================

const FIXTURE_XML: &str = "tests/fixtures/sample_api_response.xml";
const FIXTURE_LOCATIONS: &str = "tests/fixtures/locations.csv";

fn test_config() -> MetricsConfig {
    MetricsConfig {
        m30_reference_speed: 90.0,
        urban_reference_speed: 50.0,
        min_observations: 1,
    }
}

fn test_geospatial_config() -> GeospatialConfig {
    GeospatialConfig {
        base_level: 15,
        subdivision_level: 17,
        delay_threshold: 1.5,
        min_gps_count: 50,
    }
}

// ===========================================================================
// 1. LiveApiSource with an XML fixture
// ===========================================================================

#[test]
fn test_live_source_parse_fixture() {
    let source = LiveApiSource::new("http://fake-endpoint", FIXTURE_LOCATIONS).unwrap();
    let xml = std::fs::read_to_string(FIXTURE_XML).unwrap();
    let points = source.parse_xml_response(&xml).unwrap();

    // The fixture has 5 <pm>, but only 3 IDs have a location in locations.csv
    assert_eq!(points.len(), 3, "3 points with a known location");

    // Check the road types
    let m30_count = points
        .iter()
        .filter(|p| p.road_type == RoadType::M30)
        .count();
    let urban_count = points
        .iter()
        .filter(|p| p.road_type == RoadType::Urban)
        .count();
    assert_eq!(m30_count, 1, "1 M-30 point (has speed)");
    assert_eq!(urban_count, 2, "2 urban points (no speed)");
}

#[test]
fn test_live_source_point_quality() {
    let source = LiveApiSource::new("http://fake-endpoint", FIXTURE_LOCATIONS).unwrap();
    let xml = std::fs::read_to_string(FIXTURE_XML).unwrap();
    let points = source.parse_xml_response(&xml).unwrap();

    // All returned points must have Normal quality
    // (points with error=E have no location in our fixture)
    for point in &points {
        assert_eq!(point.quality, DataQuality::Normal);
    }
}

#[test]
fn test_live_source_coordinates_in_madrid() {
    let source = LiveApiSource::new("http://fake-endpoint", FIXTURE_LOCATIONS).unwrap();
    let xml = std::fs::read_to_string(FIXTURE_XML).unwrap();
    let points = source.parse_xml_response(&xml).unwrap();

    // Madrid bounding box: lat ∈ [40.3, 40.6], lon ∈ [-3.9, -3.5]
    for point in &points {
        assert!(
            point.latitude > 40.3 && point.latitude < 40.6,
            "Latitude {} outside Madrid for point {}",
            point.latitude,
            point.id
        );
        assert!(
            point.longitude > -3.9 && point.longitude < -3.5,
            "Longitude {} outside Madrid for point {}",
            point.longitude,
            point.id
        );
    }
}

#[test]
fn test_live_source_m30_has_velocity() {
    let source = LiveApiSource::new("http://fake-endpoint", FIXTURE_LOCATIONS).unwrap();
    let xml = std::fs::read_to_string(FIXTURE_XML).unwrap();
    let points = source.parse_xml_response(&xml).unwrap();

    for point in &points {
        match point.road_type {
            RoadType::M30 => assert!(
                point.velocidad.is_some(),
                "M-30 point {} must have a speed",
                point.id
            ),
            RoadType::Urban => assert!(
                point.velocidad.is_none(),
                "Urban point {} must not have a speed",
                point.id
            ),
        }
    }
}

// ===========================================================================
// 2. Scheduler with MockSource (fixture-based)
// ===========================================================================

/// MockSource that serves XML fixture data as if it were an API
struct XmlFixtureSource {
    source: LiveApiSource,
    xml_body: String,
    ticks_remaining: usize,
}

impl XmlFixtureSource {
    fn new(ticks: usize) -> Self {
        let source = LiveApiSource::new("http://fake", FIXTURE_LOCATIONS).unwrap();
        let xml = std::fs::read_to_string(FIXTURE_XML).unwrap();
        XmlFixtureSource {
            source,
            xml_body: xml,
            ticks_remaining: ticks,
        }
    }
}

impl DataSource for XmlFixtureSource {
    fn name(&self) -> &str {
        "XML Fixture Source"
    }

    fn fetch_batch(&mut self) -> Result<Vec<GpsPoint>, IngestionError> {
        if self.ticks_remaining > 0 {
            self.ticks_remaining -= 1;
            self.source.parse_xml_response(&self.xml_body)
        } else {
            Ok(vec![])
        }
    }

    fn has_more(&self) -> bool {
        self.ticks_remaining > 0
    }

    fn refresh_interval(&self) -> Option<Duration> {
        None // No sleep in tests
    }
}

#[test]
fn test_scheduler_with_xml_fixture_single_tick() {
    let source = XmlFixtureSource::new(1);
    let mut scheduler =
        PollingScheduler::new(Box::new(source), test_geospatial_config(), test_config());

    let report = scheduler.tick().unwrap();

    assert_eq!(report.tick_number, 1);
    assert_eq!(report.points_fetched, 3); // 3 points with a location
    assert_eq!(report.total_points_indexed, 3);
    assert!(report.total_cells > 0);
    assert!(
        !report.cell_reports.is_empty(),
        "Must generate metric reports"
    );
}

#[test]
fn test_scheduler_with_xml_fixture_incremental() {
    let source = XmlFixtureSource::new(3);
    let mut scheduler =
        PollingScheduler::new(Box::new(source), test_geospatial_config(), test_config());

    let reports = scheduler.run(None).unwrap();

    assert_eq!(reports.len(), 3);

    // Check incremental accumulation: each tick adds 3 points
    assert_eq!(reports[0].total_points_indexed, 3);
    assert_eq!(reports[1].total_points_indexed, 6);
    assert_eq!(reports[2].total_points_indexed, 9);

    // Cells may not grow linearly (same points → same cells)
    assert!(reports[2].total_cells >= reports[0].total_cells);
}

#[test]
fn test_scheduler_metrics_consistency() {
    let source = XmlFixtureSource::new(2);
    let mut scheduler =
        PollingScheduler::new(Box::new(source), test_geospatial_config(), test_config());

    let reports = scheduler.run(None).unwrap();

    for report in &reports {
        // Every report must have coherent metrics
        assert!(
            report.points_fetched <= report.total_points_indexed,
            "Fetched points cannot exceed the total indexed"
        );

        for cell_report in &report.cell_reports {
            // delay_factor (SRF) must be in [0, 1+]
            assert!(
                cell_report.avg_delay_factor >= 0.0,
                "SRF cannot be negative: {}",
                cell_report.avg_delay_factor
            );
            assert!(
                cell_report.avg_delay_factor <= 2.0,
                "SRF > 2.0 is suspicious: {}",
                cell_report.avg_delay_factor
            );
        }
    }
}

// ===========================================================================
// 3. Full pipeline: parse → index → compute → verify
// ===========================================================================

#[test]
fn test_full_pipeline_xml_to_metrics() {
    // 1. Parse the XML fixture
    let source = LiveApiSource::new("http://fake", FIXTURE_LOCATIONS).unwrap();
    let xml = std::fs::read_to_string(FIXTURE_XML).unwrap();
    let points = source.parse_xml_response(&xml).unwrap();
    assert!(!points.is_empty());

    // 2. Index into S2
    let index = tfg::geospatial::s2_index::S2Index::build_from_iterator(points.clone(), 15);
    assert!(!index.is_empty());
    assert!(index.cell_count() > 0);

    // 3. Compute metrics
    let config = test_config();
    let reports = tfg::metrics::delay_factor::compute_all_cells(&index, &config);

    // 4. Verify results
    assert!(!reports.is_empty(), "Must generate reports for the cells");

    // Every point has data → every cell must have a report
    for report in &reports {
        assert!(report.observation_count > 0);
        assert!(report.avg_delay_factor > 0.0);
    }

    // Check that the M-30 points produce a speed-based SRF
    // M-30 point: id=1006, velocidad=72, ref=90 → SRF = 72/90 ≈ 0.8
    let m30_points: Vec<_> = points
        .iter()
        .filter(|p| p.road_type == RoadType::M30)
        .collect();
    assert_eq!(m30_points.len(), 1);
    assert_eq!(m30_points[0].velocidad, Some(72));
}

#[test]
fn test_full_pipeline_via_scheduler() {
    // Full pipeline via the scheduler (the idiomatic way)
    let source = XmlFixtureSource::new(1);
    let mut scheduler =
        PollingScheduler::new(Box::new(source), test_geospatial_config(), test_config());

    let reports = scheduler.run(Some(1)).unwrap();
    assert_eq!(reports.len(), 1);

    let report = &reports[0];

    // The index must contain exactly the points of the fixture
    let index = scheduler.index();
    assert_eq!(index.len(), 3);

    // Metrics must be present
    assert!(!report.cell_reports.is_empty());

    // Check that Display works (for logging)
    let display = format!("{}", report);
    assert!(display.contains("Tick #1"));
}

// ===========================================================================
// 4. Test with the real API (run manually)
// ===========================================================================

/// Test against the real Madrid API
///
/// Marked `#[ignore]` so it does not run in CI or on every `cargo test`
/// Run manually with: `cargo test test_live_api_real -- --ignored`
#[test]
#[ignore]
fn test_live_api_real() {
    let mut source = LiveApiSource::new(
        "https://informo.madrid.es/informo/tmadrid/pm.xml",
        "data/samples/sample_locations.csv",
    )
    .expect("Must be able to create a LiveApiSource with real locations");

    // Real fetch
    let points = source.fetch_batch().expect("The API must respond");

    println!("=== Real Madrid API ===");
    println!("Points obtained: {}", points.len());

    // Basic validations
    assert!(
        points.len() > 100,
        "The API must return >100 points (it has ~4000+)"
    );

    let m30_count = points
        .iter()
        .filter(|p| p.road_type == RoadType::M30)
        .count();
    let urban_count = points
        .iter()
        .filter(|p| p.road_type == RoadType::Urban)
        .count();
    println!("M-30: {}, Urban: {}", m30_count, urban_count);

    assert!(
        urban_count > m30_count,
        "Must have more urban points than M-30"
    );

    // All coordinates in Madrid
    for point in &points {
        assert!(
            point.latitude > 40.2 && point.latitude < 40.7,
            "Latitude outside Madrid: {} (point {})",
            point.latitude,
            point.id
        );
        assert!(
            point.longitude > -4.0 && point.longitude < -3.4,
            "Longitude outside Madrid: {} (point {})",
            point.longitude,
            point.id
        );
    }

    // Compute metrics over the real data
    let index = tfg::geospatial::s2_index::S2Index::build_from_iterator(points, 15);
    let config = test_config();
    let reports = tfg::metrics::delay_factor::compute_all_cells(&index, &config);

    println!("S2 cells: {}", index.cell_count());
    println!("Metric reports: {}", reports.len());

    let congested = reports.iter().filter(|r| r.avg_delay_factor < 0.5).count();
    println!("Congested cells (SRF < 0.5): {}", congested);

    assert!(!reports.is_empty(), "Must generate reports with real data");
}
