//! Flat CSV export of per-cell S2 metrics
//!
//! Generates a CSV file with one row per cell that has metrics,
//! suitable for analysis in Excel, Python (pandas) or R
//!
//! ## Columns
//!
//! ```text
//! cell_id,lat,lng,avg_srf,min_srf,max_srf,std_srf,
//! congestion_level,road_type,observation_count
//! ```

use std::fmt::Write as FmtWrite;
use std::fs;
use std::path::Path;

use s2::cell::Cell;
use s2::cellid::CellID;
use s2::latlng::LatLng;

use crate::export::{ExportError, ExportSummary};
use crate::geospatial::s2_index::S2Index;
use crate::metrics::model::CellDelayReport;

/// Exports the metric reports to a CSV file
///
/// Only cells with computed metrics are exported
/// (i.e., with enough observations according to the engine threshold)
///
/// # Arguments
/// * `reports` — Per-cell delay_factor reports
/// * `index` — S2 index used to resolve coordinates of empty cells
/// * `output_path` — Path of the output CSV file
pub fn export_csv(
    reports: &[CellDelayReport],
    index: &S2Index,
    output_path: &Path,
) -> Result<ExportSummary, ExportError> {
    // Precompute centroids from the index
    let centroids: std::collections::HashMap<CellID, (f64, f64)> = index
        .iter_cells()
        .map(|(cell_id, points)| {
            let (lat, lng) = if points.is_empty() {
                let center = LatLng::from(Cell::from(cell_id).center());
                (center.lat.deg(), center.lng.deg())
            } else {
                let n = points.len() as f64;
                (
                    points.iter().map(|p| p.latitude).sum::<f64>() / n,
                    points.iter().map(|p| p.longitude).sum::<f64>() / n,
                )
            };
            (*cell_id, (lat, lng))
        })
        .collect();

    let mut csv = String::new();

    // Header
    writeln!(
        csv,
        "cell_id,lat,lng,avg_srf,min_srf,max_srf,std_srf,congestion_level,road_type,observation_count"
    )
    .map_err(|e| ExportError::Serialize(e.to_string()))?;

    // Rows — one per cell with metrics
    for report in reports {
        let (lat, lng) = centroids
            .get(&report.cell_id)
            .copied()
            .unwrap_or((0.0, 0.0));

        writeln!(
            csv,
            "{:016X},{:.6},{:.6},{:.6},{:.6},{:.6},{:.6},{:?},{:?},{}",
            report.cell_id.0,
            lat,
            lng,
            report.avg_delay_factor,
            report.min_delay_factor,
            report.max_delay_factor,
            report.std_delay_factor,
            report.congestion_level,
            report.road_type_dominant,
            report.observation_count,
        )
        .map_err(|e| ExportError::Serialize(e.to_string()))?;
    }

    // Create parent directories if they do not exist
    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let bytes = csv.len();
    fs::write(output_path, &csv)?;

    Ok(ExportSummary {
        output_path: output_path.to_path_buf(),
        bytes_written: bytes,
        items_exported: reports.len(),
    })
}
