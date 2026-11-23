//! Export to structured JSON compatible with `dashboard.html`
//!
//! Produces the same format as `demo.rs` but as a library function
//! reusable by the CLI and the live scheduler
//!
//! ## Output schema
//!
//! ```json
//! {
//!   "pipeline": {
//!     "total_gps_points": 4821,
//!     "s2_cell_count": 241,
//!     "cells_with_metrics": 198,
//!     "cells_with_stats": 145,
//!     "source": "Madrid Real-Time API",
//!     "timestamp": "2026-05-01T18:30:00"
//!   },
//!   "cells": [
//!     {
//!       "cell_id": "3F8A1C0000000000",
//!       "vertices": [[40.42, -3.72], ...],
//!       "lat": 40.4234, "lng": -3.7012,
//!       "point_count": 12,
//!       "metrics": { "avg_delay_factor": 0.42, "congestion_level": "E", ... },
//!       "stats": { "mean": 0.41, "percentile_95": 0.35, ... }
//!     }
//!   ]
//! }
//! ```

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use s2::cell::Cell;
use s2::cellid::CellID;
use s2::latlng::LatLng;
use serde::Serialize;

use crate::export::{ExportError, ExportSummary};
use crate::geospatial::s2_index::S2Index;
use crate::metrics::anomaly::AnomalyAlert;
use crate::metrics::model::CellDelayReport;
use crate::metrics::statistics::CellStatistics;

// ---------------------------------------------------------------------------
// Serialization structs (same as demo.rs, now as a module)
// ---------------------------------------------------------------------------

/// Root of the exported JSON document
#[derive(Serialize)]
pub struct MotorOutput {
    pub pipeline: PipelineInfo,
    /// Anomaly alerts of the current tick (live mode only)
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tick_alerts: Vec<AlertOutput>,
    pub cells: Vec<CellOutput>,
}

/// Metadata of the pipeline that ran the export
#[derive(Serialize)]
pub struct PipelineInfo {
    pub total_gps_points: usize,
    pub s2_cell_count: usize,
    pub cells_with_metrics: usize,
    pub cells_with_stats: usize,
    /// Name of the data source ("Historical CSV" or "Madrid Real-Time API")
    pub source: String,
    /// ISO 8601 timestamp of the export moment
    pub timestamp: String,
}

/// Data of an exported S2 cell
#[derive(Serialize)]
pub struct CellOutput {
    pub cell_id: String,
    /// Exact S2 polygon vertices: [[lat, lng]; 4]
    pub vertices: Vec<[f64; 2]>,
    pub lat: f64,
    pub lng: f64,
    pub point_count: usize,
    pub metrics: Option<CellMetrics>,
    pub stats: Option<CellStatsOutput>,
}

/// delay_factor metrics of a cell
#[derive(Serialize)]
pub struct CellMetrics {
    pub avg_delay_factor: f64,
    pub min_delay_factor: f64,
    pub max_delay_factor: f64,
    pub std_delay_factor: f64,
    /// "A", "B", "C", "D", "E" or "F"
    pub congestion_level: String,
    /// "M30" or "Urban"
    pub road_type: String,
    pub observation_count: usize,
}

/// Extended statistics of a cell
#[derive(Serialize)]
pub struct CellStatsOutput {
    pub mean: f64,
    pub median: f64,
    pub std_dev: f64,
    pub percentile_25: f64,
    pub percentile_75: f64,
    pub percentile_95: f64,
    pub iqr: f64,
    pub anomaly_count: usize,
    pub congestion_level: String,
}

/// Serializable anomaly alert for the output JSON
#[derive(Serialize)]
pub struct AlertOutput {
    pub cell_id: String,
    pub observed_srf: f64,
    pub baseline_p50: f64,
    pub baseline_p5: f64,
    pub baseline_p95: f64,
    pub deviation: f64,
    /// "Severe", "Moderate" or "Improvement"
    pub severity: String,
    pub description: String,
}

// ---------------------------------------------------------------------------
// export_json — main public function
// ---------------------------------------------------------------------------

/// Exports the engine results to JSON compatible with `dashboard.html`
///
/// # Arguments
/// * `index` — S2 index with all indexed points
/// * `reports` — Per-cell delay_factor reports
/// * `stats` — Extended per-cell statistics (may be empty)
/// * `tick_alerts` — Anomaly alerts of the current tick (empty in demo mode)
/// * `output_path` — Path of the output JSON file
/// * `source_name` — Name of the data source for the metadata
///
/// # Returns
/// `ExportSummary` with the path, bytes written and exported cells
pub fn export_json(
    index: &S2Index,
    reports: &[CellDelayReport],
    stats: &[CellStatistics],
    tick_alerts: &[AnomalyAlert],
    output_path: &Path,
    source_name: &str,
) -> Result<ExportSummary, ExportError> {
    // Lookups by cell_id for O(1) access
    let reports_map: HashMap<CellID, &CellDelayReport> =
        reports.iter().map(|r| (r.cell_id, r)).collect();
    let stats_map: HashMap<CellID, &CellStatistics> =
        stats.iter().map(|s| (s.cell_id, s)).collect();

    let mut cells: Vec<CellOutput> = Vec::with_capacity(index.cell_count());

    for (cell_id, cell_points) in index.iter_leaf_cells() {
        // Skip cells that have points but not enough to generate reports (insufficient data)
        if !cell_points.is_empty() && !reports_map.contains_key(cell_id) {
            continue;
        }

        // Centroid: mean of the points, or the exact geometric center if the cell is empty
        let (avg_lat, avg_lng) = if cell_points.is_empty() {
            let center = LatLng::from(Cell::from(cell_id).center());
            (center.lat.deg(), center.lng.deg())
        } else {
            let n = cell_points.len() as f64;
            (
                cell_points.iter().map(|p| p.latitude).sum::<f64>() / n,
                cell_points.iter().map(|p| p.longitude).sum::<f64>() / n,
            )
        };

        // Exact vertices of the S2 cell (4 corners)
        let cell_obj = Cell::from(cell_id);
        let vertices: Vec<[f64; 2]> = cell_obj
            .vertices()
            .iter()
            .map(|p| {
                let ll = LatLng::from(p);
                [ll.lat.deg(), ll.lng.deg()]
            })
            .collect();

        // delay_factor metrics
        let metrics = reports_map.get(cell_id).map(|r| CellMetrics {
            avg_delay_factor: r.avg_delay_factor,
            min_delay_factor: r.min_delay_factor,
            max_delay_factor: r.max_delay_factor,
            std_delay_factor: r.std_delay_factor,
            congestion_level: format!("{:?}", r.congestion_level),
            road_type: format!("{:?}", r.road_type_dominant),
            observation_count: r.observation_count,
        });

        // Extended statistics
        let stats_out = stats_map.get(cell_id).map(|s| CellStatsOutput {
            mean: s.mean,
            median: s.median,
            std_dev: s.std_dev,
            percentile_25: s.percentile_25,
            percentile_75: s.percentile_75,
            percentile_95: s.percentile_95,
            iqr: s.iqr,
            anomaly_count: s.anomaly_count,
            congestion_level: format!("{:?}", s.congestion_level),
        });

        cells.push(CellOutput {
            cell_id: format!("{:016X}", cell_id.0),
            vertices,
            lat: avg_lat,
            lng: avg_lng,
            point_count: cell_points.len(),
            metrics,
            stats: stats_out,
        });
    }

    // Sort: cells with metrics first, by ascending SRF (most congested on top)
    cells.sort_by(|a, b| {
        let srf_a = a
            .metrics
            .as_ref()
            .map(|m| m.avg_delay_factor)
            .unwrap_or(2.0);
        let srf_b = b
            .metrics
            .as_ref()
            .map(|m| m.avg_delay_factor)
            .unwrap_or(2.0);
        srf_a
            .partial_cmp(&srf_b)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Serialize anomaly alerts
    let tick_alerts_out: Vec<AlertOutput> = tick_alerts
        .iter()
        .map(|a| AlertOutput {
            cell_id: format!("{:016X}", a.cell_id.0),
            observed_srf: a.observed_srf,
            baseline_p50: a.baseline_p50,
            baseline_p5: a.baseline_p5,
            baseline_p95: a.baseline_p95,
            deviation: a.deviation,
            severity: format!("{:?}", a.severity),
            description: a.description(),
        })
        .collect();

    let output = MotorOutput {
        pipeline: PipelineInfo {
            total_gps_points: index.len(),
            s2_cell_count: index.cell_count(),
            cells_with_metrics: reports.len(),
            cells_with_stats: stats.len(),
            source: source_name.to_string(),
            timestamp: chrono::Local::now()
                .naive_local()
                .format("%Y-%m-%dT%H:%M:%S")
                .to_string(),
        },
        tick_alerts: tick_alerts_out,
        cells,
    };

    let items = output.pipeline.s2_cell_count;
    let json =
        serde_json::to_string_pretty(&output).map_err(|e| ExportError::Serialize(e.to_string()))?;

    // Create parent directories if they do not exist
    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?
        }
    }

    let bytes = json.len();
    fs::write(output_path, &json)?;

    Ok(ExportSummary {
        output_path: output_path.to_path_buf(),
        bytes_written: bytes,
        items_exported: items,
    })
}
