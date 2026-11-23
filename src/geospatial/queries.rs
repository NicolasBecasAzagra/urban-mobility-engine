//! Geospatial queries on the S2 index
//!
//! Provides functions to query GPS points by:
//! - Exact cell
//! - Radius (spherical cap)
//! - Bounding box (lat/lng rectangle)
//! - k nearest neighbors (k-NN)

use s2::cap::Cap;
use s2::cellid::CellID;
use s2::latlng::LatLng;
use s2::point::Point;
use s2::rect::Rect;
use s2::region::RegionCoverer;
use s2::s1::angle::Deg;
use s2::s1::chordangle::ChordAngle;

use crate::geospatial::s2_index::S2Index;
use crate::ingestion::model::GpsPoint;

/// Queries the GPS points in a specific S2 cell
///
/// # Arguments
/// * `index` - S2 index to query
/// * `cell_id` - ID of the cell to look up
///
/// # Returns
/// Reference to the vector of points in that cell, or an empty slice if it does not exist
pub fn query_cell<'a>(index: &'a S2Index, cell_id: &CellID) -> &'a [GpsPoint] {
    index.get_cell(cell_id).map_or(&[], |v| v.as_slice())
}

/// Queries GPS points within a radius of a center point
///
/// Uses `S2Cap` (spherical cap) to cover the circular area
/// and `S2RegionCoverer` to find the intersected cells
///
/// # Arguments
/// * `index` - S2 index to query
/// * `center_lat` - Center latitude in degrees
/// * `center_lon` - Center longitude in degrees
/// * `radius_km` - Search radius in kilometers
///
/// # Returns
/// Vector of references to points within the radius
pub fn query_radius(
    index: &S2Index,
    center_lat: f64,
    center_lon: f64,
    radius_km: f64,
) -> Vec<&GpsPoint> {
    // Earth radius in km
    const EARTH_RADIUS_KM: f64 = 6371.0;

    // Convert the center to a Point
    let center_latlng = LatLng {
        lat: Deg(center_lat).into(),
        lng: Deg(center_lon).into(),
    };
    let center_point = Point::from(center_latlng);

    // Create a spherical cap with angle = radius / earth_radius
    let angle_rad = radius_km / EARTH_RADIUS_KM;
    let angle: s2::s1::angle::Angle = s2::s1::angle::Rad(angle_rad).into();
    let cap = Cap::from_center_chordangle(&center_point, &ChordAngle::from(angle));

    // Cover the region with S2 cells
    let coverer = RegionCoverer {
        min_level: 0,
        max_level: index.level() as u8,
        level_mod: 1,
        max_cells: 20,
    };
    let covering = coverer.covering(&cap);

    // Look up points in the covered cells
    let mut results = Vec::new();
    for covered_cell_id in covering.0.iter() {
        // We need to find all index cells that intersect
        // with the covering cells
        for (index_cell_id, points) in index.cells() {
            // An index cell intersects if either one contains the other
            if cells_intersect(covered_cell_id, index_cell_id) {
                for point in points {
                    // Fine filter: real distance < radius
                    let point_latlng = LatLng {
                        lat: Deg(point.latitude).into(),
                        lng: Deg(point.longitude).into(),
                    };
                    let dist =
                        haversine_km(center_lat, center_lon, point.latitude, point.longitude);
                    if dist <= radius_km {
                        results.push(point);
                    }
                    let _ = point_latlng; // suppress warning
                }
            }
        }
    }

    results
}

/// Queries GPS points within a bounding box (lat/lng rectangle)
///
/// Uses `S2Rect` + `RegionCoverer` to find the intersected cells
///
/// # Arguments
/// * `index` - S2 index to query
/// * `lat_min`, `lat_max` - Latitude range
/// * `lon_min`, `lon_max` - Longitude range
pub fn query_rect(
    index: &S2Index,
    lat_min: f64,
    lat_max: f64,
    lon_min: f64,
    lon_max: f64,
) -> Vec<&GpsPoint> {
    let lo = LatLng {
        lat: Deg(lat_min).into(),
        lng: Deg(lon_min).into(),
    };
    let hi = LatLng {
        lat: Deg(lat_max).into(),
        lng: Deg(lon_max).into(),
    };
    let rect = &Rect::from(lo) + &hi;

    let coverer = RegionCoverer {
        min_level: 0,
        max_level: index.level() as u8,
        level_mod: 1,
        max_cells: 50,
    };
    let covering = coverer.covering(&rect);

    let mut results = Vec::new();
    for covered_cell_id in covering.0.iter() {
        for (index_cell_id, points) in index.cells() {
            if cells_intersect(covered_cell_id, index_cell_id) {
                for point in points {
                    // Fine filter: point inside the rectangle
                    if point.latitude >= lat_min
                        && point.latitude <= lat_max
                        && point.longitude >= lon_min
                        && point.longitude <= lon_max
                    {
                        results.push(point);
                    }
                }
            }
        }
    }

    results
}

/// Finds the k nearest points to a given point (spatial k-NN)
///
/// # Arguments
/// * `index` - S2 index to query
/// * `center_lat` - Center latitude
/// * `center_lon` - Center longitude
/// * `k` - Number of neighbors to return
///
/// # Returns
/// Vector of `(point, distance_km)` tuples sorted by ascending distance
pub fn query_knn(
    index: &S2Index,
    center_lat: f64,
    center_lon: f64,
    k: usize,
) -> Vec<(&GpsPoint, f64)> {
    // Simple strategy: compute the distance to all points and take the k smallest
    // For large datasets this can be optimized with S2 ring search
    let mut distances: Vec<(&GpsPoint, f64)> = index
        .cells()
        .values()
        .flat_map(|points| {
            points.iter().map(|point| {
                let dist = haversine_km(center_lat, center_lon, point.latitude, point.longitude);
                (point, dist)
            })
        })
        .collect();

    distances.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
    distances.truncate(k);
    distances
}

// ===========================================================================
// Helper functions
// ===========================================================================

/// Checks whether two S2 cells intersect (one contains the other)
fn cells_intersect(a: &CellID, b: &CellID) -> bool {
    // One cell contains the other if the parent ID of the lower-level cell
    // matches the higher-level cell
    let (higher, lower) = if a.level() <= b.level() {
        (a, b)
    } else {
        (b, a)
    };
    lower.parent(higher.level()) == *higher
}

/// Haversine distance between two points in kilometers
fn haversine_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const EARTH_RADIUS_KM: f64 = 6371.0;

    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();

    let lat1_rad = lat1.to_radians();
    let lat2_rad = lat2.to_radians();

    let a =
        (dlat / 2.0).sin().powi(2) + lat1_rad.cos() * lat2_rad.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();

    EARTH_RADIUS_KM * c
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingestion::model::{DataQuality, RoadType};
    use chrono::NaiveDateTime;

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

    fn build_test_index() -> S2Index {
        let mut index = S2Index::new(15);
        // Points in Madrid
        index.insert(make_point(1, 40.4168, -3.7038)); // Center
        index.insert(make_point(2, 40.4530, -3.6883)); // Chamartin
        index.insert(make_point(3, 40.3880, -3.7024)); // Usera
        index.insert(make_point(4, 40.4250, -3.7640)); // Moncloa
        index.insert(make_point(5, 40.4175, -3.7040)); // Center (very close to 1)
        index
    }

    #[test]
    fn test_query_cell() {
        let index = build_test_index();
        let cell_id = index.coords_to_cell_id(40.4168, -3.7038);
        let points = query_cell(&index, &cell_id);
        assert!(!points.is_empty());
    }

    #[test]
    fn test_query_cell_empty() {
        let index = build_test_index();
        // Point outside Madrid → empty cell
        let cell_id = index.coords_to_cell_id(41.0, -4.0);
        let points = query_cell(&index, &cell_id);
        assert!(points.is_empty());
    }

    #[test]
    fn test_query_radius() {
        let index = build_test_index();
        // Search within a 1km radius of the center of Madrid
        let results = query_radius(&index, 40.4168, -3.7038, 1.0);
        // It must include at least the center point
        assert!(
            !results.is_empty(),
            "Should find points within a 1km radius"
        );
    }

    #[test]
    fn test_query_radius_large() {
        let index = build_test_index();
        // Large radius (10km) → all points
        let results = query_radius(&index, 40.42, -3.70, 10.0);
        assert_eq!(
            results.len(),
            5,
            "A 10km radius must cover all of the Madrid test data"
        );
    }

    #[test]
    fn test_query_radius_small() {
        let index = build_test_index();
        // Very small radius (0.01km = 10m) → few or no points
        let results = query_radius(&index, 40.50, -3.60, 0.01);
        assert!(
            results.is_empty(),
            "A 10m radius away from the data must be empty"
        );
    }

    #[test]
    fn test_query_rect() {
        let index = build_test_index();
        // Bounding box covering all of the Madrid test data
        let results = query_rect(&index, 40.35, 40.50, -3.80, -3.65);
        assert_eq!(results.len(), 5, "The rect must cover all test points");
    }

    #[test]
    fn test_query_rect_partial() {
        let index = build_test_index();
        // Bounding box covering only part of Madrid
        let results = query_rect(&index, 40.40, 40.43, -3.72, -3.69);
        assert!(
            results.len() < 5,
            "The partial rect must not cover all points"
        );
    }

    #[test]
    fn test_query_knn() {
        let index = build_test_index();
        let results = query_knn(&index, 40.4168, -3.7038, 3);

        assert_eq!(results.len(), 3, "k-NN must return exactly k results");

        // Results must be sorted by distance
        for i in 1..results.len() {
            assert!(
                results[i].1 >= results[i - 1].1,
                "k-NN must be sorted by distance"
            );
        }
    }

    #[test]
    fn test_query_knn_closest() {
        let index = build_test_index();
        let results = query_knn(&index, 40.4168, -3.7038, 1);

        assert_eq!(results.len(), 1);
        // The nearest point to the center (40.4168, -3.7038) must be ID 1 or 5
        assert!(
            results[0].0.id == 1 || results[0].0.id == 5,
            "The nearest neighbor to the center must be ID 1 or 5"
        );
    }

    #[test]
    fn test_haversine_km() {
        // Madrid → Barcelona ≈ 505 km
        let dist = haversine_km(40.4168, -3.7038, 41.3851, 2.1734);
        assert!(
            (dist - 505.0).abs() < 10.0,
            "Madrid-Barcelona ≈ 505km, got {}",
            dist
        );
    }

    #[test]
    fn test_cells_intersect() {
        let index = S2Index::new(15);
        let cell15 = index.coords_to_cell_id(40.4168, -3.7038);
        let child = cell15.children()[0]; // Child at level 16

        assert!(cells_intersect(&cell15, &child));
        assert!(cells_intersect(&child, &cell15));

        // Different cells at same level
        let other = index.coords_to_cell_id(40.50, -3.60);
        assert!(!cells_intersect(&cell15, &other));
    }
}
