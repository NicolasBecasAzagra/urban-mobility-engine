//! Per-S2-cell delay_factor computation engine
//!
//! Implements the computation of the adapted Speed Reduction Factor (SRF)
//! for individual GPS points and per-cell aggregated reports
//!
//! ## Formulas
//!
//! **M-30** (speed available):
//! ```text
//! SRF = v_observed / v_reference
//! ```
//!
//! **Urban roads** (no speed, load only):
//! ```text
//! SRF = 1.0 - (carga / 100.0)
//! ```
//!
//! ## Usage example
//!
//! ```rust
//! use tfg::config::MetricsConfig;
//! use tfg::metrics::delay_factor::compute_delay_factor;
//! # use tfg::ingestion::model::{GpsPoint, RoadType, DataQuality};
//! # use chrono::NaiveDateTime;
//! #
//! # let point = GpsPoint {
//! #     id: 1006,
//! #     timestamp: NaiveDateTime::parse_from_str("2025-12-01 08:00:00", "%Y-%m-%d %H:%M:%S").unwrap(),
//! #     latitude: 40.41,
//! #     longitude: -3.70,
//! #     road_type: RoadType::M30,
//! #     intensidad: 500,
//! #     ocupacion: 20,
//! #     carga: 30,
//! #     velocidad: Some(70),
//! #     quality: DataQuality::Normal,
//! # };
//! # let config = MetricsConfig {
//! #     m30_reference_speed: 90.0,
//! #     urban_reference_speed: 50.0,
//! #     min_observations: 5,
//! # };
//!
//! let df = compute_delay_factor(&point, &config).unwrap();
//! // SRF = 70 / 90 ≈ 0.778 → CongestionLevel::C
//! ```

use rayon::prelude::*;
use s2::cellid::CellID;

use crate::config::MetricsConfig;
use crate::geospatial::s2_index::S2Index;
use crate::ingestion::model::{GpsPoint, RoadType};
use crate::metrics::error::MetricsError;
use crate::metrics::model::{CellDelayReport, DelayFactor};

// ---------------------------------------------------------------------------
// compute_delay_factor — per-point computation
// ---------------------------------------------------------------------------

/// Computes the delay_factor (SRF) for an individual GPS point
///
/// # Formulas
///
/// - **M-30**: `SRF = v_observed / v_reference` where `v_reference`
///   is `config.m30_reference_speed` (90 km/h by default)
/// - **Urban**: `SRF = 1.0 - (carga / 100.0)` using the `carga` field
///   as an inverse proxy of traffic fluidity
///
/// # Arguments
/// * `point` — GPS point with traffic data
/// * `config` — Metrics configuration with reference speeds
///
/// # Errors
/// - `NoReferenceSpeed` if the point is M-30 but has no speed
/// - `NoSpeedData` if the M-30 point has a speed ≤ 0
pub fn compute_delay_factor(
    point: &GpsPoint,
    config: &MetricsConfig,
) -> Result<DelayFactor, MetricsError> {
    let srf = match point.road_type {
        RoadType::M30 => {
            // M-30: speed-based SRF
            let v_obs = point.velocidad.ok_or(MetricsError::NoReferenceSpeed {
                sensor_id: point.id,
            })? as f64;

            if v_obs <= 0.0 {
                return Err(MetricsError::NoSpeedData);
            }

            v_obs / config.m30_reference_speed
        }
        RoadType::Urban => {
            // Urban: load-based SRF (inverse proxy)
            // carga 0 = free flow → SRF = 1.0
            // carga 100 = collapse → SRF = 0.0
            let carga = point.carga.clamp(0, 100) as f64;
            1.0 - (carga / 100.0)
        }
    };

    Ok(DelayFactor::new(
        srf,
        point.road_type,
        point.timestamp,
        point.id,
    ))
}

// ---------------------------------------------------------------------------
// compute_cell_delay — aggregated report per S2 cell
// ---------------------------------------------------------------------------

/// Computes the aggregated delay_factor report for an S2 cell
///
/// Processes all the GPS points in the cell, computes the individual SRF
/// for each one and produces a `CellDelayReport` with aggregated
/// statistics (mean, min, max, std, dominant congestion level)
///
/// # Arguments
/// * `cell_id` — ID of the S2 cell
/// * `points` — GPS points contained in the cell
/// * `config` — Metrics configuration
///
/// # Errors
/// - `InsufficientData` if the number of valid points is lower
///   than `config.min_observations`
pub fn compute_cell_delay(
    cell_id: CellID,
    points: &[GpsPoint],
    config: &MetricsConfig,
) -> Result<CellDelayReport, MetricsError> {
    // Filter valid points and compute delay_factors
    let factors: Vec<DelayFactor> = points
        .iter()
        .filter_map(|p| compute_delay_factor(p, config).ok())
        .collect();

    // Check the minimum number of observations
    if factors.len() < config.min_observations {
        return Err(MetricsError::InsufficientData {
            actual: factors.len(),
            required: config.min_observations,
        });
    }

    Ok(CellDelayReport::from_factors(cell_id, &factors))
}

// ---------------------------------------------------------------------------
// compute_all_cells — full processing of the S2 index
// ---------------------------------------------------------------------------

/// Computes the delay_factor for all the cells of an S2 index
///
/// Iterates over all the cells of the index and produces a
/// `CellDelayReport` for those that have enough
/// observations. Cells with insufficient data are skipped
/// silently
///
/// # Arguments
/// * `index` — S2 index with GPS points indexed by cell
/// * `config` — Metrics configuration
///
/// # Returns
/// Vector of `CellDelayReport` for cells with enough data,
/// sorted by ascending `avg_delay_factor` (most congested first)
pub fn compute_all_cells(index: &S2Index, config: &MetricsConfig) -> Vec<CellDelayReport> {
    let mut reports: Vec<CellDelayReport> = index
        .par_iter_cells()
        .filter_map(|(cell_id, points)| compute_cell_delay(*cell_id, points, config).ok())
        .collect();

    // Sort by ascending delay_factor: most congested cells first
    reports.par_sort_unstable_by(|a, b| {
        a.avg_delay_factor
            .partial_cmp(&b.avg_delay_factor)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    reports
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingestion::model::{DataQuality, RoadType};
    use chrono::NaiveDateTime;

    fn parse_dt(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    fn default_config() -> MetricsConfig {
        MetricsConfig {
            m30_reference_speed: 90.0,
            urban_reference_speed: 50.0,
            min_observations: 5,
        }
    }

    fn make_m30_point(id: u32, vmed: i32, carga: i32, timestamp: &str) -> GpsPoint {
        GpsPoint {
            id,
            timestamp: parse_dt(timestamp),
            latitude: 40.41,
            longitude: -3.70,
            road_type: RoadType::M30,
            intensidad: 500,
            ocupacion: 20,
            carga,
            velocidad: Some(vmed),
            quality: DataQuality::Normal,
        }
    }

    fn make_urban_point(id: u32, carga: i32, timestamp: &str) -> GpsPoint {
        GpsPoint {
            id,
            timestamp: parse_dt(timestamp),
            latitude: 40.45,
            longitude: -3.72,
            road_type: RoadType::Urban,
            intensidad: 300,
            ocupacion: 15,
            carga,
            velocidad: None,
            quality: DataQuality::Normal,
        }
    }

    // -----------------------------------------------------------------------
    // compute_delay_factor — M-30
    // -----------------------------------------------------------------------

    #[test]
    fn test_m30_free_flow() {
        let config = default_config();
        let point = make_m30_point(1006, 90, 0, "2025-12-01 08:00:00");
        let df = compute_delay_factor(&point, &config).unwrap();

        // SRF = 90 / 90 = 1.0
        assert!((df.value - 1.0).abs() < 0.001);
        assert_eq!(df.congestion, crate::metrics::model::CongestionLevel::A);
        assert_eq!(df.road_type, RoadType::M30);
    }

    #[test]
    fn test_m30_congested() {
        let config = default_config();
        let point = make_m30_point(1006, 45, 60, "2025-12-01 08:30:00");
        let df = compute_delay_factor(&point, &config).unwrap();

        // SRF = 45 / 90 = 0.50 → CongestionLevel::D
        assert!((df.value - 0.50).abs() < 0.001);
        assert_eq!(df.congestion, crate::metrics::model::CongestionLevel::D);
    }

    #[test]
    fn test_m30_above_reference() {
        let config = default_config();
        let point = make_m30_point(1006, 100, 0, "2025-12-01 03:00:00");
        let df = compute_delay_factor(&point, &config).unwrap();

        // SRF = 100 / 90 ≈ 1.111 → CongestionLevel::A
        assert!(df.value > 1.0);
        assert_eq!(df.congestion, crate::metrics::model::CongestionLevel::A);
    }

    #[test]
    fn test_m30_severe_congestion() {
        let config = default_config();
        let point = make_m30_point(1006, 20, 85, "2025-12-01 09:00:00");
        let df = compute_delay_factor(&point, &config).unwrap();

        // SRF = 20 / 90 ≈ 0.222 → CongestionLevel::F
        assert!(df.value < 0.35);
        assert_eq!(df.congestion, crate::metrics::model::CongestionLevel::F);
    }

    #[test]
    fn test_m30_no_speed_data() {
        let config = default_config();
        let mut point = make_m30_point(1006, 0, 50, "2025-12-01 08:00:00");
        point.velocidad = None;

        let result = compute_delay_factor(&point, &config);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MetricsError::NoReferenceSpeed { sensor_id: 1006 }
        ));
    }

    // -----------------------------------------------------------------------
    // compute_delay_factor — Urban
    // -----------------------------------------------------------------------

    #[test]
    fn test_urban_free_flow() {
        let config = default_config();
        let point = make_urban_point(3395, 0, "2025-12-01 03:00:00");
        let df = compute_delay_factor(&point, &config).unwrap();

        // SRF = 1.0 - (0 / 100) = 1.0 → CongestionLevel::A
        assert!((df.value - 1.0).abs() < 0.001);
        assert_eq!(df.congestion, crate::metrics::model::CongestionLevel::A);
        assert_eq!(df.road_type, RoadType::Urban);
    }

    #[test]
    fn test_urban_congested() {
        let config = default_config();
        let point = make_urban_point(3395, 70, "2025-12-01 08:30:00");
        let df = compute_delay_factor(&point, &config).unwrap();

        // SRF = 1.0 - (70 / 100) = 0.30 → CongestionLevel::F
        assert!((df.value - 0.30).abs() < 0.001);
        assert_eq!(df.congestion, crate::metrics::model::CongestionLevel::F);
    }

    #[test]
    fn test_urban_moderate() {
        let config = default_config();
        let point = make_urban_point(3395, 25, "2025-12-01 12:00:00");
        let df = compute_delay_factor(&point, &config).unwrap();

        // SRF = 1.0 - (25 / 100) = 0.75 → CongestionLevel::C
        assert!((df.value - 0.75).abs() < 0.001);
        assert_eq!(df.congestion, crate::metrics::model::CongestionLevel::C);
    }

    #[test]
    fn test_urban_collapse() {
        let config = default_config();
        let point = make_urban_point(3395, 100, "2025-12-01 09:00:00");
        let df = compute_delay_factor(&point, &config).unwrap();

        // SRF = 1.0 - (100 / 100) = 0.0 → CongestionLevel::F
        assert!((df.value - 0.0).abs() < 0.001);
        assert_eq!(df.congestion, crate::metrics::model::CongestionLevel::F);
    }

    #[test]
    fn test_urban_carga_clamped() {
        let config = default_config();
        // Negative load → clamped to 0
        let point = make_urban_point(3395, -5, "2025-12-01 08:00:00");
        let df = compute_delay_factor(&point, &config).unwrap();
        assert!((df.value - 1.0).abs() < 0.001);
    }

    // -----------------------------------------------------------------------
    // compute_cell_delay
    // -----------------------------------------------------------------------

    #[test]
    fn test_cell_delay_report() {
        let config = MetricsConfig {
            m30_reference_speed: 90.0,
            urban_reference_speed: 50.0,
            min_observations: 3,
        };

        let points: Vec<GpsPoint> = vec![
            make_m30_point(1006, 90, 0, "2025-12-01 08:00:00"), // SRF = 1.0
            make_m30_point(1006, 72, 20, "2025-12-01 08:15:00"), // SRF = 0.8
            make_m30_point(1006, 63, 40, "2025-12-01 08:30:00"), // SRF = 0.7
        ];

        let cell_id = CellID(0);
        let report = compute_cell_delay(cell_id, &points, &config).unwrap();

        assert_eq!(report.observation_count, 3);
        // avg = (1.0 + 0.8 + 0.7) / 3 = 0.833
        assert!((report.avg_delay_factor - 0.833).abs() < 0.01);
        assert!((report.min_delay_factor - 0.7).abs() < 0.001);
        assert!((report.max_delay_factor - 1.0).abs() < 0.001);
        assert_eq!(report.road_type_dominant, RoadType::M30);
    }

    #[test]
    fn test_insufficient_data() {
        let config = default_config(); // min_observations = 5

        let points = vec![
            make_m30_point(1006, 90, 0, "2025-12-01 08:00:00"),
            make_m30_point(1006, 72, 20, "2025-12-01 08:15:00"),
        ];

        let result = compute_cell_delay(CellID(0), &points, &config);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MetricsError::InsufficientData {
                actual: 2,
                required: 5
            }
        ));
    }

    #[test]
    fn test_cell_delay_mixed_road_types() {
        let config = MetricsConfig {
            m30_reference_speed: 90.0,
            urban_reference_speed: 50.0,
            min_observations: 2,
        };

        let points = vec![
            make_m30_point(1006, 90, 0, "2025-12-01 08:00:00"),
            make_urban_point(3395, 20, "2025-12-01 08:15:00"),
            make_urban_point(3396, 30, "2025-12-01 08:30:00"),
        ];

        let report = compute_cell_delay(CellID(0), &points, &config).unwrap();
        assert_eq!(report.observation_count, 3);
        // Majority urban → dominant type = Urban
        assert_eq!(report.road_type_dominant, RoadType::Urban);
    }

    // -----------------------------------------------------------------------
    // compute_all_cells
    // -----------------------------------------------------------------------

    #[test]
    fn test_compute_all_cells() {
        let config = MetricsConfig {
            m30_reference_speed: 90.0,
            urban_reference_speed: 50.0,
            min_observations: 2,
        };

        // Create an S2 index with points in several cells
        let mut index = S2Index::new(15);

        // Cell 1: central Madrid — high load (congested)
        for i in 0..3 {
            index.insert(make_urban_point(3395 + i, 70, "2025-12-01 08:00:00"));
        }

        // Cell 2: northern M-30 — free flow
        let mut point1 = make_m30_point(1006, 88, 5, "2025-12-01 08:00:00");
        point1.latitude = 40.50;
        point1.longitude = -3.63;
        index.insert(point1);

        let mut point2 = make_m30_point(1007, 92, 3, "2025-12-01 08:15:00");
        point2.latitude = 40.50;
        point2.longitude = -3.63;
        index.insert(point2);

        let reports = compute_all_cells(&index, &config);

        // There must be at least 1 report (cells with at least 2 observations)
        assert!(!reports.is_empty());

        // Verify they are sorted ascending (most congested first)
        for i in 1..reports.len() {
            assert!(
                reports[i - 1].avg_delay_factor <= reports[i].avg_delay_factor,
                "Reports must be sorted by ascending delay_factor"
            );
        }
    }

    #[test]
    fn test_compute_all_cells_empty_index() {
        let config = default_config();
        let index = S2Index::new(15);

        let reports = compute_all_cells(&index, &config);
        assert!(reports.is_empty());
    }

    #[test]
    fn test_compute_all_cells_skips_insufficient() {
        let config = MetricsConfig {
            m30_reference_speed: 90.0,
            urban_reference_speed: 50.0,
            min_observations: 10, // High threshold → no cell meets it
        };

        let mut index = S2Index::new(15);
        index.insert(make_urban_point(3395, 50, "2025-12-01 08:00:00"));
        index.insert(make_urban_point(3396, 60, "2025-12-01 08:15:00"));

        let reports = compute_all_cells(&index, &config);
        assert!(reports.is_empty());
    }

    // -----------------------------------------------------------------------
    // Custom reference configuration
    // -----------------------------------------------------------------------

    #[test]
    fn test_custom_reference_speed() {
        let config = MetricsConfig {
            m30_reference_speed: 100.0, // Stricter than the default
            urban_reference_speed: 50.0,
            min_observations: 5,
        };

        let point = make_m30_point(1006, 90, 0, "2025-12-01 08:00:00");
        let df = compute_delay_factor(&point, &config).unwrap();

        // SRF = 90 / 100 = 0.90 → CongestionLevel::A (right at the limit)
        assert!((df.value - 0.90).abs() < 0.001);
        assert_eq!(df.congestion, crate::metrics::model::CongestionLevel::A);
    }
}
