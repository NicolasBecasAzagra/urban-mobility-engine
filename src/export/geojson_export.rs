//! Export to standard GeoJSON with exact S2 cell polygons
//!
//! Generates a valid GeoJSON `FeatureCollection` (RFC 7946) where each
//! S2 cell is a `Feature` with its exact polygon and the metrics
//! as properties
//!
//! The resulting file is compatible with:
//! - **QGIS** (drag and drop)
//! - **Leaflet / Mapbox** (load as a layer)
//! - **Kepler.gl** (advanced visualization)
//! - **geojson.io** (quick inspection)
//!
//! ## Schema
//!
//! ```json
//! {
//!   "type": "FeatureCollection",
//!   "features": [
//!     {
//!       "type": "Feature",
//!       "geometry": {
//!         "type": "Polygon",
//!         "coordinates": [[[-3.72, 40.42], [-3.71, 40.42], ...]]
//!       },
//!       "properties": {
//!         "cell_id": "3F8A...",
//!         "point_count": 12,
//!         "avg_srf": 0.42,
//!         "congestion_level": "E",
//!         "road_type": "Urban",
//!         "observation_count": 12
//!       }
//!     }
//!   ]
//! }
//! ```

use std::fs;
use std::path::Path;

use s2::cell::Cell;
use s2::cellid::CellID;
use s2::latlng::LatLng;
use serde_json::{json, Value};

use crate::export::{ExportError, ExportSummary};
use crate::geospatial::s2_index::S2Index;
use crate::metrics::model::CellDelayReport;

/// Exports the S2 index and the metrics to a GeoJSON file
///
/// Each S2 cell becomes a Feature with its exact polygon
/// (4 vertices plus the closing one) and the available metrics as properties
/// Cells without metrics are included with `avg_srf: null`
///
/// # Arguments
/// * `index` — S2 index with all indexed points
/// * `reports` — Per-cell delay_factor reports
/// * `output_path` — Path of the output GeoJSON file
pub fn export_geojson(
    index: &S2Index,
    reports: &[CellDelayReport],
    output_path: &Path,
) -> Result<ExportSummary, ExportError> {
    // Lookup of reports by cell_id
    let reports_map: std::collections::HashMap<CellID, &CellDelayReport> =
        reports.iter().map(|r| (r.cell_id, r)).collect();

    let mut features: Vec<Value> = Vec::with_capacity(index.cell_count());

    for (cell_id, cell_points) in index.iter_leaf_cells() {
        // Skip cells that have points but not enough to generate reports (insufficient data)
        if !cell_points.is_empty() && !reports_map.contains_key(cell_id) {
            continue;
        }

        // S2 polygon vertices in [lng, lat] order (GeoJSON convention)
        let cell_obj = Cell::from(cell_id);
        let mut coordinates: Vec<[f64; 2]> = cell_obj
            .vertices()
            .iter()
            .map(|p| {
                let ll = LatLng::from(p);
                // GeoJSON uses [longitude, latitude]
                [ll.lng.deg(), ll.lat.deg()]
            })
            .collect();

        // Close the ring (RFC 7946: first and last points must coincide)
        if let Some(first) = coordinates.first().copied() {
            coordinates.push(first);
        }

        // Centroid
        let (lat, lng) = if cell_points.is_empty() {
            let center = LatLng::from(cell_obj.center());
            (center.lat.deg(), center.lng.deg())
        } else {
            let n = cell_points.len() as f64;
            (
                cell_points.iter().map(|p| p.latitude).sum::<f64>() / n,
                cell_points.iter().map(|p| p.longitude).sum::<f64>() / n,
            )
        };

        // Properties
        let mut properties = json!({
            "cell_id": format!("{:016X}", cell_id.0),
            "lat": lat,
            "lng": lng,
            "point_count": cell_points.len(),
        });

        if let Some(report) = reports_map.get(cell_id) {
            properties["avg_srf"] = json!(report.avg_delay_factor);
            properties["min_srf"] = json!(report.min_delay_factor);
            properties["max_srf"] = json!(report.max_delay_factor);
            properties["std_srf"] = json!(report.std_delay_factor);
            properties["congestion_level"] = json!(format!("{:?}", report.congestion_level));
            properties["road_type"] = json!(format!("{:?}", report.road_type_dominant));
            properties["observation_count"] = json!(report.observation_count);
        } else {
            properties["avg_srf"] = Value::Null;
            properties["congestion_level"] = Value::Null;
        }

        features.push(json!({
            "type": "Feature",
            "geometry": {
                "type": "Polygon",
                "coordinates": [coordinates],
            },
            "properties": properties,
        }));
    }

    let geojson = json!({
        "type": "FeatureCollection",
        "features": features,
    });

    let output_str = serde_json::to_string_pretty(&geojson)
        .map_err(|e| ExportError::Serialize(e.to_string()))?;

    // Create parent directories if they do not exist
    if let Some(parent) = output_path.parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }

    let bytes = output_str.len();
    fs::write(output_path, &output_str)?;

    Ok(ExportSummary {
        output_path: output_path.to_path_buf(),
        bytes_written: bytes,
        items_exported: features.len(),
    })
}
