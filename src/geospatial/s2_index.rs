//! S2 geospatial index for GPS traffic data
//!
//! Implements an index based on `HashMap<CellID, Vec<GpsPoint>>`
//! that maps each S2 cell to the GPS points it contains
//!
//! ## Resolution levels for Madrid
//!
//! | Level | Edge size   | Engine use      |
//! |-------|-------------|-----------------|
//! | 12 | ~3.3 km | Macro view of Madrid |
//! | 14 | ~830 m | Neighborhoods |
//! | 15 | ~318 m | Main streets (base) |
//! | 16 | ~159 m | City blocks |
//! | 17 | ~79 m | Bottlenecks (subdivision) |

use rayon::prelude::*;
use std::collections::HashMap;

use s2::cellid::CellID;
use s2::latlng::LatLng;
use s2::s1::angle::Deg;

use crate::ingestion::model::GpsPoint;

/// Geospatial index that stores GPS points keyed by S2 cell
///
/// Each GPS point is assigned to an S2 cell at the configured
/// resolution level. Multiple points can fall into the same cell
///
/// ## Example
///
/// ```
/// use tfg::geospatial::s2_index::S2Index;
///
/// let index = S2Index::new(15); // Level 15 ≈ 318m
/// assert_eq!(index.cell_count(), 0);
/// ```
#[derive(Debug)]
pub struct S2Index {
    /// HashMap of S2 cells → contained GPS points
    cells: HashMap<CellID, Vec<GpsPoint>>,
    /// S2 resolution level of the index
    level: u64,
    /// Total number of inserted points
    total_points: usize,
}

impl S2Index {
    /// Creates a new empty S2 index at the given resolution level
    ///
    /// # Arguments
    /// * `level` - S2 level (0–30). Level 15 recommended for Madrid
    ///
    /// # Panics
    /// Panics if `level > 30`
    pub fn new(level: u64) -> Self {
        assert!(level <= 30, "S2 level must be between 0 and 30");
        S2Index {
            cells: HashMap::new(),
            level,
            total_points: 0,
        }
    }

    /// Converts lat/lng coordinates to a `CellID` at the index level
    ///
    /// # Arguments
    /// * `latitude` - Latitude in decimal degrees (WGS84)
    /// * `longitude` - Longitude in decimal degrees (WGS84)
    pub fn coords_to_cell_id(&self, latitude: f64, longitude: f64) -> CellID {
        let latlng = LatLng {
            lat: Deg(latitude).into(),
            lng: Deg(longitude).into(),
        };
        let cell_id = CellID::from(latlng);
        cell_id.parent(self.level)
    }

    /// Inserts a GPS point into the index
    ///
    /// The point is assigned to the matching S2 cell according to its
    /// latitude and longitude coordinates
    pub fn insert(&mut self, point: GpsPoint) {
        let cell_id = self.coords_to_cell_id(point.latitude, point.longitude);
        self.cells.entry(cell_id).or_default().push(point);
        self.total_points += 1;
    }

    /// Inserts multiple GPS points from an iterator
    pub fn insert_many(&mut self, points: impl IntoIterator<Item = GpsPoint>) {
        for point in points {
            self.insert(point);
        }
    }

    /// Builds a complete S2 index from an iterator of GPS points
    ///
    /// # Arguments
    /// * `points` - Iterator of GPS points
    /// * `level` - S2 resolution level
    pub fn build_from_iterator(points: impl IntoIterator<Item = GpsPoint>, level: u64) -> Self {
        let mut index = S2Index::new(level);
        index.insert_many(points);
        index
    }

    /// Returns the total number of S2 cells with at least one point
    pub fn cell_count(&self) -> usize {
        self.cells.len()
    }

    /// Returns the total number of GPS points in the index
    pub fn len(&self) -> usize {
        self.total_points
    }

    /// Indicates whether the index is empty
    pub fn is_empty(&self) -> bool {
        self.total_points == 0
    }

    /// Returns the S2 resolution level of the index
    pub fn level(&self) -> u64 {
        self.level
    }

    /// Returns the GPS points in a specific cell
    ///
    /// # Arguments
    /// * `cell_id` - ID of the S2 cell to query
    pub fn get_cell(&self, cell_id: &CellID) -> Option<&Vec<GpsPoint>> {
        self.cells.get(cell_id)
    }

    /// Returns the GPS points in the cell that contains the given coordinates
    pub fn get_at_coords(&self, latitude: f64, longitude: f64) -> Option<&Vec<GpsPoint>> {
        let cell_id = self.coords_to_cell_id(latitude, longitude);
        self.cells.get(&cell_id)
    }

    /// Subdivides a list of cells into their 4 child cells (increasing
    /// the resolution by 1 S2 level)
    /// This allows finer granularity (zoom-in) in areas the engine
    /// has detected as problematic or heavily congested
    pub fn subdivide_cells(&mut self, cell_ids: &[CellID]) {
        for &cell_id in cell_ids {
            if let Some(points) = self.cells.remove(&cell_id) {
                let current_level = cell_id.level();
                if current_level >= 30 {
                    // Cannot subdivide any further
                    self.cells.insert(cell_id, points);
                    continue;
                }

                let mut children_map: HashMap<CellID, Vec<GpsPoint>> = HashMap::new();
                for p in points {
                    let latlng = LatLng {
                        lat: Deg(p.latitude).into(),
                        lng: Deg(p.longitude).into(),
                    };
                    let child_id = CellID::from(latlng).parent(current_level + 1);
                    children_map.entry(child_id).or_default().push(p);
                }

                for (child, child_points) in children_map {
                    self.cells.insert(child, child_points);
                }
            }
        }
    }

    /// Injects an empty cell into the index (used to render the full grid)
    pub fn insert_empty_cell(&mut self, cell_id: CellID) {
        self.cells.entry(cell_id).or_default();
    }

    /// Returns an iterator over all cells and their points
    pub fn iter_cells(&self) -> impl Iterator<Item = (&CellID, &Vec<GpsPoint>)> {
        self.cells.iter()
    }

    /// Returns a parallel iterator over all cells and their points
    pub fn par_iter_cells(&self) -> impl ParallelIterator<Item = (&CellID, &Vec<GpsPoint>)> {
        self.cells.par_iter()
    }

    /// Returns all CellIDs of the index
    pub fn cell_ids(&self) -> Vec<CellID> {
        self.cells.keys().copied().collect()
    }

    /// Direct access to the internal HashMap (for advanced queries)
    pub fn cells(&self) -> &HashMap<CellID, Vec<GpsPoint>> {
        &self.cells
    }

    /// Checks whether a cell has any child (at any lower level)
    /// present in the index. Used to avoid visual overlap
    /// when exporting cells of different S2 levels
    pub fn has_children_in_index(&self, cell_id: &CellID) -> bool {
        let level = cell_id.level();
        // Check whether any other cell in the index is a child of this one
        self.cells.keys().any(|other| {
            let other_level = other.level();
            other_level > level && other.parent(level) == *cell_id
        })
    }

    /// Iterator over "leaf" cells: excludes cells that have children
    /// in the index (avoids visual overlap in exports)
    pub fn iter_leaf_cells(&self) -> Vec<(&CellID, &Vec<GpsPoint>)> {
        // Collect all cell_ids so ancestry can be checked
        let all_ids: Vec<CellID> = self.cells.keys().copied().collect();

        self.cells
            .iter()
            .filter(|(cell_id, _)| {
                let level = cell_id.level();
                // Keep this cell only if it has NO children in the index
                !all_ids
                    .iter()
                    .any(|other| other.level() > level && other.parent(level) == **cell_id)
            })
            .collect()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingestion::model::{DataQuality, RoadType};
    use chrono::NaiveDateTime;

    /// Creates a test GPS point at the given coordinates
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
    fn test_new_index() {
        let index = S2Index::new(15);
        assert_eq!(index.cell_count(), 0);
        assert_eq!(index.len(), 0);
        assert!(index.is_empty());
        assert_eq!(index.level(), 15);
    }

    #[test]
    fn test_insert_single_point() {
        let mut index = S2Index::new(15);
        let point = make_point(10011, 40.4485, -3.7253);
        index.insert(point);

        assert_eq!(index.len(), 1);
        assert_eq!(index.cell_count(), 1);
        assert!(!index.is_empty());
    }

    #[test]
    fn test_insert_same_cell() {
        let mut index = S2Index::new(15);
        // Two very close points → same level 15 cell
        index.insert(make_point(1, 40.4485, -3.7253));
        index.insert(make_point(2, 40.4486, -3.7254));

        assert_eq!(index.len(), 2);
        // They could fall in the same cell or different ones depending on the level
        // At level 15 (~318m), points <50m apart → same cell
        assert!(index.cell_count() <= 2);
    }

    #[test]
    fn test_insert_different_cells() {
        let mut index = S2Index::new(15);
        // Points in different areas of Madrid
        index.insert(make_point(1, 40.45, -3.72)); // North
        index.insert(make_point(2, 40.38, -3.68)); // South-East
        index.insert(make_point(3, 40.42, -3.80)); // West

        assert_eq!(index.len(), 3);
        assert!(
            index.cell_count() >= 2,
            "Distant points must fall into different cells"
        );
    }

    #[test]
    fn test_get_at_coords() {
        let mut index = S2Index::new(15);
        index.insert(make_point(10011, 40.4485, -3.7253));

        let points = index.get_at_coords(40.4485, -3.7253).unwrap();
        assert_eq!(points.len(), 1);
        assert_eq!(points[0].id, 10011);
    }

    #[test]
    fn test_coords_to_cell_id() {
        let index = S2Index::new(15);
        let cell_id = index.coords_to_cell_id(40.4485, -3.7253);

        assert!(cell_id.is_valid());
        assert_eq!(cell_id.level(), 15);
    }

    #[test]
    fn test_build_from_iterator() {
        let points = vec![
            make_point(1, 40.45, -3.72),
            make_point(2, 40.38, -3.68),
            make_point(3, 40.42, -3.80),
        ];

        let index = S2Index::build_from_iterator(points, 15);
        assert_eq!(index.len(), 3);
        assert!(index.cell_count() >= 2);
    }

    #[test]
    fn test_iter_cells() {
        let mut index = S2Index::new(15);
        index.insert(make_point(1, 40.45, -3.72));
        index.insert(make_point(2, 40.38, -3.68));

        let all_points: usize = index.iter_cells().map(|(_, pts)| pts.len()).sum();
        assert_eq!(all_points, 2);
    }

    #[test]
    fn test_cell_ids() {
        let mut index = S2Index::new(15);
        index.insert(make_point(1, 40.45, -3.72));

        let ids = index.cell_ids();
        assert_eq!(ids.len(), 1);
        assert!(ids[0].is_valid());
        assert_eq!(ids[0].level(), 15);
    }

    #[test]
    fn test_different_levels() {
        // The same point at different levels produces different cell_ids
        let idx15 = S2Index::new(15);
        let idx17 = S2Index::new(17);

        let cell15 = idx15.coords_to_cell_id(40.4485, -3.7253);
        let cell17 = idx17.coords_to_cell_id(40.4485, -3.7253);

        assert_ne!(cell15, cell17, "Different levels produce different CellIDs");
        assert_eq!(cell15.level(), 15);
        assert_eq!(cell17.level(), 17);

        // Level 17 is a child of level 15
        let parent = cell17.parent(15);
        assert_eq!(
            parent, cell15,
            "A level 17 cell must be a child of a level 15 cell"
        );
    }
}
