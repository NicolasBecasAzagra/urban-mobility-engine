//! Multi-format export module of the mobility engine
//!
//! Provides functions to export the engine results
//! (S2 metrics, statistics, geospatial indexes) to three
//! standard formats:
//!
//! - [`json_export`] — Structured JSON compatible with `dashboard.html`
//! - [`csv_export`] — Flat CSV for analysis in Excel / Python / R
//! - [`geojson_export`] — Standard GeoJSON for QGIS / Leaflet / Mapbox
//!
//! ## Quick example
//!
//! ```rust,no_run
//! use std::path::Path;
//! use tfg::export::{export_json, export_csv, export_geojson};
//!
//! // After running the pipeline...
//! // export_json(&index, &reports, &stats, Path::new("demo/motor_output.json")).unwrap();
//! // export_csv(&reports, &index, Path::new("demo/metrics.csv")).unwrap();
//! // export_geojson(&index, &reports, Path::new("demo/madrid.geojson")).unwrap();
//! ```

pub mod csv_export;
pub mod error;
pub mod geojson_export;
pub mod json_export;

pub use csv_export::export_csv;
pub use error::ExportError;
pub use geojson_export::export_geojson;
pub use json_export::export_json;

/// Summary of the result of an export
#[derive(Debug, Clone)]
pub struct ExportSummary {
    /// Path of the generated file
    pub output_path: std::path::PathBuf,
    /// Bytes written to disk
    pub bytes_written: usize,
    /// Number of exported items (cells, rows...)
    pub items_exported: usize,
}

impl std::fmt::Display for ExportSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} ({} items, {} bytes)",
            self.output_path.display(),
            self.items_exported,
            self.bytes_written
        )
    }
}
