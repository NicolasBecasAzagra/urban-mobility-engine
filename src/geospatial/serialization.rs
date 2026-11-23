//! Serialization/deserialization of the S2 index in binary format
//!
//! Allows saving and loading the S2 index state to
//! avoid reprocessing the data on every run
//!
//! Uses `bincode` for efficient binary serialization
//!
//! ## Example
//!
//! ```no_run
//! use tfg::geospatial::serialization;
//! use tfg::geospatial::S2Index;
//!
//! let index = S2Index::new(15);
//! serialization::save_index(&index, "index.bin").unwrap();
//! let loaded = serialization::load_index("index.bin").unwrap();
//! ```

use std::collections::HashMap;
use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::geospatial::s2_index::S2Index;
use crate::ingestion::model::GpsPoint;

/// Serializable representation of the S2 index
///
/// The `s2` crate's `CellID` does not implement `Serialize`, so
/// we store its inner `u64` value (field `.0`)
#[derive(Serialize, Deserialize)]
struct SerializableIndex {
    /// S2 level of the index
    level: u64,
    /// Cell (as u64) → GPS points map
    cells: HashMap<u64, Vec<GpsPoint>>,
}

impl From<&S2Index> for SerializableIndex {
    fn from(index: &S2Index) -> Self {
        let cells: HashMap<u64, Vec<GpsPoint>> = index
            .cells()
            .iter()
            .map(|(cell_id, points)| (cell_id.0, points.clone()))
            .collect();

        SerializableIndex {
            level: index.level(),
            cells,
        }
    }
}

impl SerializableIndex {
    /// Rebuilds the `S2Index` from the serializable representation
    fn into_s2_index(self) -> S2Index {
        let mut index = S2Index::new(self.level);
        for (_cell_u64, points) in self.cells {
            for point in points {
                index.insert(point);
            }
        }
        index
    }
}

/// Serializes the S2 index to a binary file with `bincode`
///
/// # Arguments
/// * `index` - S2 index to save
/// * `path` - Path of the output file
///
/// # Errors
/// Returns an error if the file cannot be created or if
/// serialization fails
pub fn save_index<P: AsRef<Path>>(index: &S2Index, path: P) -> Result<()> {
    let path = path.as_ref();
    let file =
        File::create(path).with_context(|| format!("Could not create file: {}", path.display()))?;
    let writer = BufWriter::new(file);

    let serializable = SerializableIndex::from(index);
    bincode::serialize_into(writer, &serializable)
        .with_context(|| format!("Error serializing index to: {}", path.display()))?;

    tracing::info!(
        "S2 index saved to '{}' ({} cells, {} points)",
        path.display(),
        index.cell_count(),
        index.len()
    );
    Ok(())
}

/// Loads an S2 index from a binary file
///
/// # Arguments
/// * `path` - Path of the file to read
///
/// # Errors
/// Returns an error if the file does not exist, is not readable,
/// or the format is invalid
pub fn load_index<P: AsRef<Path>>(path: P) -> Result<S2Index> {
    let path = path.as_ref();
    let file =
        File::open(path).with_context(|| format!("Could not open file: {}", path.display()))?;
    let reader = BufReader::new(file);

    let serializable: SerializableIndex = bincode::deserialize_from(reader)
        .with_context(|| format!("Error deserializing index from: {}", path.display()))?;

    let index = serializable.into_s2_index();
    tracing::info!(
        "S2 index loaded from '{}' ({} cells, {} points)",
        path.display(),
        index.cell_count(),
        index.len()
    );
    Ok(index)
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingestion::model::{DataQuality, RoadType};
    use chrono::NaiveDateTime;
    use std::fs;

    fn make_point(id: u32, lat: f64, lon: f64) -> GpsPoint {
        GpsPoint {
            id,
            timestamp: NaiveDateTime::parse_from_str("2025-12-01 08:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
            latitude: lat,
            longitude: lon,
            road_type: RoadType::Urban,
            intensidad: 500,
            ocupacion: 10,
            carga: 15,
            velocidad: None,
            quality: DataQuality::Normal,
        }
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let mut index = S2Index::new(15);
        index.insert(make_point(1, 40.4168, -3.7038));
        index.insert(make_point(2, 40.4530, -3.6883));
        index.insert(make_point(3, 40.3880, -3.7024));

        let path = "/tmp/test_s2_index.bin";

        // Save
        save_index(&index, path).unwrap();
        assert!(Path::new(path).exists());

        // Load
        let loaded = load_index(path).unwrap();

        // Verify
        assert_eq!(loaded.level(), index.level());
        assert_eq!(loaded.len(), index.len());
        assert_eq!(loaded.cell_count(), index.cell_count());

        // Cleanup
        fs::remove_file(path).ok();
    }

    #[test]
    fn test_roundtrip_preserves_data() {
        let mut index = S2Index::new(15);
        index.insert(make_point(42, 40.4168, -3.7038));

        let path = "/tmp/test_s2_roundtrip_data.bin";
        save_index(&index, path).unwrap();
        let loaded = load_index(path).unwrap();

        // Verify point data is preserved
        let original_pts = index.get_at_coords(40.4168, -3.7038).unwrap();
        let loaded_pts = loaded.get_at_coords(40.4168, -3.7038).unwrap();

        assert_eq!(original_pts.len(), loaded_pts.len());
        assert_eq!(original_pts[0].id, loaded_pts[0].id);
        assert_eq!(original_pts[0].intensidad, loaded_pts[0].intensidad);
        assert!((original_pts[0].latitude - loaded_pts[0].latitude).abs() < 1e-10);

        fs::remove_file(path).ok();
    }

    #[test]
    fn test_save_empty_index() {
        let index = S2Index::new(15);
        let path = "/tmp/test_s2_empty.bin";

        save_index(&index, path).unwrap();
        let loaded = load_index(path).unwrap();

        assert_eq!(loaded.len(), 0);
        assert_eq!(loaded.cell_count(), 0);
        assert_eq!(loaded.level(), 15);

        fs::remove_file(path).ok();
    }

    #[test]
    fn test_load_nonexistent_file() {
        let result = load_index("/tmp/nonexistent_s2_index.bin");
        assert!(result.is_err());
    }
}
