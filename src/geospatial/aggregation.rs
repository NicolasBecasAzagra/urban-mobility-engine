//! Statistical aggregations per S2 cell
//!
//! Computes basic statistics (count, means) per cell
//! or grouped by an S2 level different from the index level

use std::collections::HashMap;

use s2::cellid::CellID;

use crate::geospatial::s2_index::S2Index;
use crate::ingestion::model::GpsPoint;

/// Aggregated statistics for an S2 cell
#[derive(Debug, Clone)]
pub struct CellStats {
    /// S2 cell ID
    pub cell_id: CellID,
    /// Number of GPS points in the cell
    pub point_count: usize,
    /// Mean intensity (vehicles/hour)
    pub avg_intensidad: f64,
    /// Mean occupancy (%)
    pub avg_ocupacion: f64,
    /// Mean load (`carga`, degree of congestion)
    pub avg_carga: f64,
    /// Mean speed (M-30 only, `None` if there is no data)
    pub avg_velocidad: Option<f64>,
}

impl CellStats {
    /// Computes statistics from a vector of GPS points
    fn from_points(cell_id: CellID, points: &[GpsPoint]) -> Self {
        let count = points.len();
        if count == 0 {
            return CellStats {
                cell_id,
                point_count: 0,
                avg_intensidad: 0.0,
                avg_ocupacion: 0.0,
                avg_carga: 0.0,
                avg_velocidad: None,
            };
        }

        let sum_intensidad: i64 = points.iter().map(|p| p.intensidad as i64).sum();
        let sum_ocupacion: i64 = points.iter().map(|p| p.ocupacion as i64).sum();
        let sum_carga: i64 = points.iter().map(|p| p.carga as i64).sum();

        let velocidades: Vec<i32> = points.iter().filter_map(|p| p.velocidad).collect();
        let avg_velocidad = if velocidades.is_empty() {
            None
        } else {
            Some(velocidades.iter().map(|v| *v as f64).sum::<f64>() / velocidades.len() as f64)
        };

        CellStats {
            cell_id,
            point_count: count,
            avg_intensidad: sum_intensidad as f64 / count as f64,
            avg_ocupacion: sum_ocupacion as f64 / count as f64,
            avg_carga: sum_carga as f64 / count as f64,
            avg_velocidad,
        }
    }
}

/// Computes statistics for all cells in the index
///
/// Returns a vector of `CellStats` with one entry per cell
/// that contains at least one point
pub fn aggregate_all(index: &S2Index) -> Vec<CellStats> {
    index
        .iter_cells()
        .map(|(cell_id, points)| CellStats::from_points(*cell_id, points))
        .collect()
}

/// Groups the points by an S2 level different from the index level
///
/// Useful to obtain summaries at a larger scale. For example,
/// if the index is at level 15, it can be grouped by level 12
/// to see per-district statistics
///
/// # Arguments
/// * `index` - S2 index with the data
/// * `target_level` - Grouping level (must be ≤ the index level)
pub fn aggregate_by_level(index: &S2Index, target_level: u64) -> HashMap<CellID, CellStats> {
    assert!(
        target_level <= index.level(),
        "target_level ({}) must be ≤ the index level ({})",
        target_level,
        index.level()
    );

    // Group points by parent cell at target_level
    let mut grouped: HashMap<CellID, Vec<&GpsPoint>> = HashMap::new();
    for (cell_id, points) in index.iter_cells() {
        let parent = cell_id.parent(target_level);
        for point in points {
            grouped.entry(parent).or_default().push(point);
        }
    }

    // Compute stats for each group
    grouped
        .into_iter()
        .map(|(cell_id, points)| {
            let owned_points: Vec<GpsPoint> = points.into_iter().cloned().collect();
            (cell_id, CellStats::from_points(cell_id, &owned_points))
        })
        .collect()
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingestion::model::{DataQuality, RoadType};
    use chrono::NaiveDateTime;

    fn make_point(
        id: u32,
        lat: f64,
        lon: f64,
        intensidad: i32,
        velocidad: Option<i32>,
    ) -> GpsPoint {
        GpsPoint {
            id,
            timestamp: NaiveDateTime::parse_from_str("2025-12-01 08:00:00", "%Y-%m-%d %H:%M:%S")
                .unwrap(),
            latitude: lat,
            longitude: lon,
            road_type: if velocidad.is_some() {
                RoadType::M30
            } else {
                RoadType::Urban
            },
            intensidad,
            ocupacion: 10,
            carga: 15,
            velocidad,
            quality: DataQuality::Normal,
        }
    }

    #[test]
    fn test_aggregate_all() {
        let mut index = S2Index::new(15);
        index.insert(make_point(1, 40.42, -3.70, 500, None));
        index.insert(make_point(2, 40.42, -3.70, 700, None));
        index.insert(make_point(3, 40.38, -3.68, 300, None));

        let stats = aggregate_all(&index);
        assert!(!stats.is_empty());

        let total_points: usize = stats.iter().map(|s| s.point_count).sum();
        assert_eq!(total_points, 3);
    }

    #[test]
    fn test_cell_stats_averages() {
        let points = vec![
            make_point(1, 40.42, -3.70, 400, None),
            make_point(2, 40.42, -3.70, 600, None),
        ];

        let cell_id = CellID(0); // dummy
        let stats = CellStats::from_points(cell_id, &points);

        assert_eq!(stats.point_count, 2);
        assert!((stats.avg_intensidad - 500.0).abs() < 0.01);
        assert!(stats.avg_velocidad.is_none());
    }

    #[test]
    fn test_cell_stats_with_velocity() {
        let points = vec![
            make_point(1, 40.42, -3.70, 400, Some(60)),
            make_point(2, 40.42, -3.70, 600, Some(80)),
        ];

        let cell_id = CellID(0);
        let stats = CellStats::from_points(cell_id, &points);
        assert!((stats.avg_velocidad.unwrap() - 70.0).abs() < 0.01);
    }

    #[test]
    fn test_aggregate_by_level() {
        let mut index = S2Index::new(17);
        // Nearby points → should be grouped at level 15
        index.insert(make_point(1, 40.4200, -3.7000, 500, None));
        index.insert(make_point(2, 40.4201, -3.7001, 700, None));
        index.insert(make_point(3, 40.3800, -3.6800, 300, None));

        let grouped = aggregate_by_level(&index, 15);
        assert!(!grouped.is_empty());

        // Verify that all CellIDs are level 15
        for cell_id in grouped.keys() {
            assert_eq!(cell_id.level(), 15);
        }
    }

    #[test]
    fn test_cell_stats_empty() {
        let cell_id = CellID(0);
        let stats = CellStats::from_points(cell_id, &[]);
        assert_eq!(stats.point_count, 0);
        assert_eq!(stats.avg_intensidad, 0.0);
    }
}
