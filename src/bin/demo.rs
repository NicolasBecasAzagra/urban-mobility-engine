//! Runnable end-to-end pipeline demo with progress bars
//!
//! Run with: `cargo run --bin demo`
//!
//! Full flow:
//!   1. CSV data ingestion → GpsPoints
//!   2. S2 geospatial indexing
//!   3. Per-cell delay_factor (SRF) computation + adaptive subdivision
//!   4. Statistics with percentiles and anomalies
//!   5. JSON export for the HTML visualization

use std::path::Path;

use anyhow::Result;
use s2::cellid::CellID;

use tfg::config::Config;
use tfg::export::export_json;
use tfg::geospatial::s2_index::S2Index;
use tfg::ingestion::csv_parser;
use tfg::metrics::delay_factor;
use tfg::metrics::statistics;
use tfg::progress;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter("tfg=info")
        .without_time()
        .init();

    progress::demo_header();

    // -----------------------------------------------------------------------
    // Phase 1: CSV data ingestion
    // -----------------------------------------------------------------------
    progress::phase_header(1, "Task 14", "Spatial data ingestion and cleaning");

    let config = Config::from_file("config.toml")?;

    let sp = progress::spinner("Reading CSV data...");
    let records = csv_parser::parse_traffic_csv("data/samples/hora_punta.csv")?;
    let locations = csv_parser::load_locations_csv("data/samples/locations.csv")?;
    sp.finish_and_clear();

    let bar = progress::pipeline_bar(records.len() as u64, "GpsPoints");
    let points: Vec<_> = {
        let all = csv_parser::build_gps_points(&records, &locations);
        bar.finish_and_clear();
        all
    };
    bar.finish_and_clear();

    progress::phase_done(&format!(
        "{} records → {} locations → {} GpsPoints processed",
        records.len(),
        locations.len(),
        points.len()
    ));

    // -----------------------------------------------------------------------
    // Phase 2: S2 indexing
    // -----------------------------------------------------------------------
    progress::phase_header(2, "Task 15", "Geospatial Indexing (Google S2)");

    let sp = progress::spinner("Building S2 index...");
    let mut index = S2Index::build_from_iterator(points.clone(), config.geospatial.base_level);

    // Inject empty cells to cover the Madrid bounding box
    let lat_steps = 70u32;
    let lng_steps = 70u32;
    for i in 0..lat_steps {
        for j in 0..lng_steps {
            let lat = 40.35 + (i as f64 / lat_steps as f64) * (40.52 - 40.35);
            let lng = -3.76 + (j as f64 / lng_steps as f64) * (-3.58 - -3.76);
            let latlng = s2::latlng::LatLng {
                lat: s2::s1::angle::Deg(lat).into(),
                lng: s2::s1::angle::Deg(lng).into(),
            };
            let cell_id = s2::cellid::CellID::from(latlng).parent(config.geospatial.base_level);
            index.insert_empty_cell(cell_id);
        }
    }
    sp.finish_and_clear();

    progress::phase_done(&format!(
        "S2 index ready: {} cells (base level {})",
        index.cell_count(),
        index.level()
    ));

    // -----------------------------------------------------------------------
    // Phase 3: SRF computation and adaptive subdivision
    // -----------------------------------------------------------------------
    progress::phase_header(3, "Task 16+19", "Delay Factor and Adaptive Subdivision");

    let sp = progress::spinner("Computing SRF and subdividing congestion hotspots...");
    let max_level = 17u64;
    let mut subdivision_passes = 0;

    while index.level() < max_level {
        let reports = delay_factor::compute_all_cells(&index, &config.metrics);
        let congested: Vec<CellID> = reports
            .iter()
            .filter(|r| r.avg_delay_factor < 0.65 && r.cell_id.level() < max_level)
            .map(|r| r.cell_id)
            .collect();

        if congested.is_empty() {
            break;
        }
        index.subdivide_cells(&congested);
        subdivision_passes += 1;
    }
    sp.finish_and_clear();

    progress::phase_done(&format!(
        "{} subdivision iterations | {} final cells",
        subdivision_passes,
        index.cell_count()
    ));

    // -----------------------------------------------------------------------
    // Phase 4: Statistics
    // -----------------------------------------------------------------------
    progress::phase_header(4, "Task 17+18", "Statistics: Percentiles and Anomalies");

    let sp = progress::spinner("Computing per-cell statistics...");
    let reports = delay_factor::compute_all_cells(&index, &config.metrics);
    let all_stats = statistics::compute_all_statistics(&index, &config.metrics);
    sp.finish_and_clear();

    progress::phase_done(&format!(
        "{} cells with SRF | {} cells with percentiles and anomalies",
        reports.len(),
        all_stats.len()
    ));

    // -----------------------------------------------------------------------
    // Phase 5: JSON export
    // -----------------------------------------------------------------------
    progress::phase_header(5, "Task 19", "JSON export for the dashboard");

    let sp = progress::spinner("Serializing and writing JSON...");
    let output_path = Path::new("demo/motor_output.json");
    let summary = export_json(
        &index,
        &reports,
        &all_stats,
        &[],
        output_path,
        "Historical CSV",
    )?;
    sp.finish_and_clear();

    progress::pipeline_done(
        output_path.to_str().unwrap_or("demo/motor_output.json"),
        summary.bytes_written,
    );

    Ok(())
}
