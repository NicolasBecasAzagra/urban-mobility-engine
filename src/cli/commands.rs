//! Implementation of the CLI subcommands
//!
//! Each `run_*` function implements a complete subcommand with
//! visual progress, logging and result export

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use s2::cellid::CellID;

use crate::cli::opts::{DemoArgs, ExportArgs, ExportFormat, LiveArgs};
use crate::config::Config;
use crate::export::{export_csv, export_geojson, export_json};
use crate::geospatial::s2_index::S2Index;
use crate::ingestion::csv_parser;
use crate::ingestion::live_api::LiveApiSource;
use crate::ingestion::scheduler::PollingScheduler;
use crate::metrics::{anomaly::AnomalyDetector, delay_factor, statistics};
use crate::progress::{self, TickSummaryData};

// ---------------------------------------------------------------------------
// run_demo — Complete historical pipeline
// ---------------------------------------------------------------------------

/// Runs the complete historical pipeline: CSV → S2 → metrics → JSON
///
/// This is the CLI equivalent of `cargo run --bin demo`, but with
/// configurable paths and output to any directory
pub fn run_demo(args: &DemoArgs, config: &Config) -> Result<()> {
    progress::demo_header();

    // Phase 1: CSV ingestion
    progress::phase_header(1, "Task 14", "Spatial data ingestion and cleaning");

    let sp = progress::spinner("Reading CSV data...");
    let records = csv_parser::parse_traffic_csv(&args.data)
        .with_context(|| format!("Could not read CSV: {}", args.data.display()))?;
    let locations = csv_parser::load_locations_csv(&args.locations)
        .with_context(|| format!("Could not read locations: {}", args.locations.display()))?;
    sp.finish_and_clear();

    let bar = progress::pipeline_bar(records.len() as u64, "GpsPoints");
    let points = csv_parser::build_gps_points(&records, &locations);
    bar.finish_and_clear();

    progress::phase_done(&format!(
        "{} records → {} locations → {} GpsPoints",
        records.len(),
        locations.len(),
        points.len()
    ));

    // Phase 2: S2 indexing
    progress::phase_header(2, "Task 15", "Geospatial Indexing (Google S2)");

    let sp = progress::spinner("Building S2 index with Madrid coverage...");
    let mut index = S2Index::build_from_iterator(points, config.geospatial.base_level);

    // Coverage of the whole Madrid bounding box
    inject_madrid_coverage(&mut index, config.geospatial.base_level);
    sp.finish_and_clear();

    progress::phase_done(&format!(
        "S2 index ready: {} cells (base level {})",
        index.cell_count(),
        index.level()
    ));

    // Phase 3: SRF computation + adaptive subdivision
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

    // Phase 4: Statistics
    progress::phase_header(4, "Task 17+18", "Statistics: Percentiles and Anomalies");

    let sp = progress::spinner("Computing per-cell statistics...");
    let reports = delay_factor::compute_all_cells(&index, &config.metrics);
    let all_stats = statistics::compute_all_statistics(&index, &config.metrics);
    sp.finish_and_clear();

    progress::phase_done(&format!(
        "{} cells with SRF | {} cells with percentiles",
        reports.len(),
        all_stats.len()
    ));

    // Phase 5: JSON export
    progress::phase_header(5, "Task 19", "JSON export for the dashboard");

    let output_path = resolve_output_path(&args.output, "motor_output.json");
    let sp = progress::spinner("Serializing and writing JSON...");
    let summary = export_json(
        &index,
        &reports,
        &all_stats,
        &[],
        &output_path,
        "Historical CSV",
    )?;
    sp.finish_and_clear();

    progress::pipeline_done(
        output_path.to_str().unwrap_or("demo/motor_output.json"),
        summary.bytes_written,
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// run_live — Near-real-time mode
// ---------------------------------------------------------------------------

/// Runs near-real-time mode, connecting to the Madrid API
///
/// Flow:
/// 1. Optional warm-start: preloads a CSV into the S2 index
/// 2. Creates `LiveApiSource` with the endpoint from config.toml
/// 3. Runs `max_ticks` ticks (or unbounded if ticks == 0)
/// 4. After each tick: exports JSON compatible with dashboard.html
pub fn run_live(args: &LiveArgs, config: &Config) -> Result<()> {
    let live_config = config
        .live
        .as_ref()
        .with_context(|| "Missing [live] section in config.toml")?;

    let max_ticks = if args.ticks == 0 {
        None
    } else {
        Some(args.ticks)
    };

    progress::live_header(&live_config.endpoint, args.warm_start.is_some(), max_ticks);

    // Warm-start: preload the historical CSV into the index before polling
    let initial_index = if let Some(warm_path) = &args.warm_start {
        progress::phase_header(0, "Warm-start", "Historical data preload");
        let sp = progress::spinner("Loading historical CSV into the S2 index...");

        let records = csv_parser::parse_traffic_csv(warm_path)
            .with_context(|| format!("Could not read warm-start CSV: {}", warm_path.display()))?;
        let locations = csv_parser::load_locations_csv(&args.locations)
            .with_context(|| format!("Could not read locations: {}", args.locations.display()))?;
        let points = csv_parser::build_gps_points(&records, &locations);
        let idx = S2Index::build_from_iterator(points, config.geospatial.base_level);
        sp.finish_and_clear();

        progress::phase_done(&format!(
            "{} points preloaded into {} cells",
            idx.len(),
            idx.cell_count()
        ));
        Some(idx)
    } else {
        None
    };

    // Build the AnomalyDetector from the historical baseline (if warm-start is used)
    let anomaly_detector: Option<AnomalyDetector> = if let Some(ref idx) = initial_index {
        let sp = progress::spinner("Computing historical baseline for anomaly detection...");
        let hist_stats = statistics::compute_all_statistics(idx, &config.metrics);
        let detector = AnomalyDetector::from_statistics(&hist_stats);
        sp.finish_and_clear();
        progress::phase_done(&format!(
            "Baseline: {} cells with historical percentiles",
            detector.baseline_size()
        ));
        Some(detector)
    } else {
        None
    };

    // Create LiveApiSource
    let sp = progress::spinner("Initializing connection to the Madrid API...");
    let source = LiveApiSource::new(&live_config.endpoint, &live_config.locations_path)
        .with_context(|| "Could not create LiveApiSource")?;
    sp.finish_and_clear();

    progress::phase_done(&format!(
        "API: {} | Locations: {}",
        live_config.endpoint, live_config.locations_path
    ));

    // Create the scheduler (with or without a preloaded index)
    let scheduler_base = match initial_index {
        Some(idx) => {
            let s = PollingScheduler::new(
                Box::new(source),
                config.geospatial.clone(),
                config.metrics.clone(),
            );
            let _ = idx;
            s
        }
        None => PollingScheduler::new(
            Box::new(source),
            config.geospatial.clone(),
            config.metrics.clone(),
        ),
    };

    // Attach the anomaly detector if available
    let mut scheduler = if let Some(detector) = anomaly_detector {
        scheduler_base.with_anomaly_detector(detector)
    } else {
        scheduler_base
    };

    // Manual tick loop (so we can export and show progress on every tick)
    let mut tick_num = 0usize;

    loop {
        tick_num += 1;

        if let Some(max) = max_ticks {
            if tick_num > max {
                break;
            }
        }

        // Spinner during the fetch
        let sp = progress::spinner(&format!(
            "Tick {}/{} — Connecting to {}...",
            tick_num,
            max_ticks
                .map(|m| m.to_string())
                .unwrap_or_else(|| "∞".to_string()),
            live_config.endpoint
        ));
        let tick_start = Instant::now();
        let tick_result = scheduler.tick();
        sp.finish_and_clear();

        match tick_result {
            Ok(report) => {
                let elapsed_ms = tick_start.elapsed().as_millis();

                // Compute the mean SRF of the tick
                let avg_srf = if report.cell_reports.is_empty() {
                    1.0
                } else {
                    report
                        .cell_reports
                        .iter()
                        .map(|r| r.avg_delay_factor)
                        .sum::<f64>()
                        / report.cell_reports.len() as f64
                };

                // Export JSON after every tick
                let output_path = resolve_output_path(&args.output, "live_output.json");
                let all_stats =
                    statistics::compute_all_statistics(scheduler.index(), &config.metrics);
                let export_sp = progress::spinner("Exporting JSON...");
                let export_result = export_json(
                    scheduler.index(),
                    &report.cell_reports,
                    &all_stats,
                    &report.anomaly_alerts,
                    &output_path,
                    "Madrid Real-Time API",
                );
                export_sp.finish_and_clear();

                match export_result {
                    Ok(s) => {
                        progress::phase_done(&format!(
                            "-> {} ({} bytes)",
                            output_path.display(),
                            s.bytes_written
                        ));

                        // Show detected alerts
                        if !report.anomaly_alerts.is_empty() {
                            println!("  Anomalies detected: {}", report.anomaly_alerts.len());
                            for alert in report.anomaly_alerts.iter().take(3) {
                                println!("    [{}]", alert.description());
                            }
                        }
                    }
                    Err(e) => {
                        eprintln!("  Warning: Error exporting: {}", e);
                    }
                }

                // Show the tick summary
                progress::tick_summary(&TickSummaryData {
                    tick: tick_num,
                    max: max_ticks,
                    points_fetched: report.points_fetched,
                    total_indexed: report.total_points_indexed,
                    total_cells: report.total_cells,
                    congested: report.congested_cells(),
                    avg_srf,
                    elapsed_ms,
                });

                // Wait between ticks (only if there are more ticks to run)
                let has_more = max_ticks.is_none_or(|m| tick_num < m);
                if has_more {
                    let interval_secs = live_config.poll_interval_secs;
                    progress::tick_waiting(interval_secs);
                    std::thread::sleep(std::time::Duration::from_secs(interval_secs));
                }
            }
            Err(e) => {
                eprintln!("  ✗ Error in tick {}: {}", tick_num, e);
                break;
            }
        }
    }

    println!();
    println!(
        "  ✅ Live mode finished. {} ticks completed.",
        tick_num.saturating_sub(1)
    );
    println!("  📂 Last output: {}", args.output.display());

    Ok(())
}

// ---------------------------------------------------------------------------
// run_export — Result export
// ---------------------------------------------------------------------------

/// Runs the historical pipeline and exports to the chosen format
pub fn run_export(args: &ExportArgs, config: &Config) -> Result<()> {
    progress::demo_header();
    progress::phase_header(1, "Export", "Running pipeline for export");

    // Minimal pipeline: CSV → S2 → metrics
    let sp = progress::spinner("Loading data...");
    let records = csv_parser::parse_traffic_csv(&args.data)
        .with_context(|| format!("Could not read CSV: {}", args.data.display()))?;
    let locations = csv_parser::load_locations_csv(&args.locations)
        .with_context(|| format!("Could not read locations: {}", args.locations.display()))?;
    let points = csv_parser::build_gps_points(&records, &locations);
    let mut index = S2Index::build_from_iterator(points, config.geospatial.base_level);

    // Adaptive subdivision
    let max_level = 17u64;
    loop {
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
    }

    // Compute final SRF
    let reports = delay_factor::compute_all_cells(&index, &config.metrics);
    sp.finish_and_clear();

    progress::phase_done(&format!(
        "{} GpsPoints | {} cells | {} SRF reports",
        index.len(),
        index.cell_count(),
        reports.len()
    ));

    // Export to the chosen format
    let sp = progress::spinner("Exporting...");
    let summary = match args.format {
        ExportFormat::Json => {
            let all_stats = statistics::compute_all_statistics(&index, &config.metrics);
            let path = resolve_output_path(&args.output, "motor_output.json");
            export_json(&index, &reports, &all_stats, &[], &path, "Historical CSV")?
        }
        ExportFormat::Csv => {
            let path = resolve_output_path(&args.output, "metrics.csv");
            export_csv(&reports, &index, &path)?
        }
        ExportFormat::Geojson => {
            let path = resolve_output_path(&args.output, "madrid.geojson");
            export_geojson(&index, &reports, &path)?
        }
    };
    sp.finish_and_clear();

    progress::pipeline_done(
        summary.output_path.to_str().unwrap_or("output"),
        summary.bytes_written,
    );

    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Resolves the output path:
/// - If `output` is a directory (or ends in `/`), appends the default file name
/// - If `output` has an extension, it is used directly
fn resolve_output_path(output: &Path, default_name: &str) -> PathBuf {
    let s = output.to_string_lossy();
    if s.ends_with('/') || s.ends_with('\\') || output.is_dir() {
        output.join(default_name)
    } else if output.extension().is_some() {
        output.to_path_buf()
    } else {
        output.join(default_name)
    }
}

/// Injects empty cells to cover the Madrid bounding box
/// Ensures the map shows the whole city even if not all
/// sensors have transmitted data
fn inject_madrid_coverage(index: &mut S2Index, level: u64) {
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
            let cell_id = CellID::from(latlng).parent(level);
            index.insert_empty_cell(cell_id);
        }
    }
}
