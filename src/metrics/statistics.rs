//! Advanced per-S2-cell statistics: percentiles and anomaly detection
//!
//! Extends the delay_factor analysis with percentiles (p25, p50, p75, p95)
//! and anomaly detection based on the IQR (Interquartile Range) method
//!
//! ## IQR method for anomaly detection
//!
//! A value is considered anomalous if it falls outside the range:
//! ```text
//! [Q1 - 1.5·IQR, Q3 + 1.5·IQR]
//! ```
//! where `IQR = Q3 - Q1` (interquartile range)
//!
//! This method is robust against asymmetric distributions,
//! common in urban traffic data (congestion produces
//! heavy tails at low SRF values)

use s2::cellid::CellID;
use serde::{Deserialize, Serialize};

use crate::config::MetricsConfig;
use crate::geospatial::s2_index::S2Index;
use crate::metrics::delay_factor::compute_delay_factor;
use crate::metrics::error::MetricsError;
use crate::metrics::model::{CongestionLevel, DelayFactor};

// ---------------------------------------------------------------------------
// Percentile functions
// ---------------------------------------------------------------------------

/// Computes the `p` percentile of an **already sorted** vector of values
///
/// Uses linear interpolation between the two adjacent values
/// when the index is not an integer (standard NIST method)
///
/// # Arguments
/// * `sorted` — Slice of values sorted in ascending order
/// * `p` — Desired percentile in the range [0.0, 1.0]
///
/// # Panics
/// Panics if `sorted` is empty or if `p` is outside [0.0, 1.0]
pub fn compute_percentile(sorted: &[f64], p: f64) -> f64 {
    assert!(
        !sorted.is_empty(),
        "Cannot compute the percentile of an empty vector"
    );
    assert!(
        (0.0..=1.0).contains(&p),
        "Percentile must be between 0.0 and 1.0, got: {}",
        p
    );

    if sorted.len() == 1 {
        return sorted[0];
    }

    let n = sorted.len() as f64;
    let index = p * (n - 1.0);
    let lower = index.floor() as usize;
    let upper = index.ceil() as usize;
    let fraction = index - lower as f64;

    if lower == upper {
        sorted[lower]
    } else {
        sorted[lower] * (1.0 - fraction) + sorted[upper] * fraction
    }
}

// ---------------------------------------------------------------------------
// CellStatistics — Extended per-cell statistics
// ---------------------------------------------------------------------------

/// Extended delay_factor statistics for an S2 cell
///
/// Includes percentiles, IQR and anomaly count in addition to
/// the basic metrics (mean, std, min, max)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellStatistics {
    /// Identifier of the S2 cell
    pub cell_id: CellID,
    /// Number of observations processed
    pub observation_count: usize,
    /// Mean delay_factor
    pub mean: f64,
    /// Standard deviation of the delay_factor
    pub std_dev: f64,
    /// Minimum observed delay_factor
    pub min: f64,
    /// Maximum observed delay_factor
    pub max: f64,
    /// Median (50th percentile)
    pub median: f64,
    /// 25th percentile (first quartile)
    pub percentile_25: f64,
    /// 75th percentile (third quartile)
    pub percentile_75: f64,
    /// 95th percentile
    pub percentile_95: f64,
    /// Interquartile range (IQR = p75 - p25)
    pub iqr: f64,
    /// Number of anomalous values (outside [p25 - 1.5·IQR, p75 + 1.5·IQR])
    pub anomaly_count: usize,
    /// Congestion level based on the median
    pub congestion_level: CongestionLevel,
}

impl CellStatistics {
    /// Indicates whether the cell has detected anomalies
    pub fn has_anomalies(&self) -> bool {
        self.anomaly_count > 0
    }

    /// Proportion of anomalous observations (0.0–1.0)
    pub fn anomaly_ratio(&self) -> f64 {
        if self.observation_count == 0 {
            return 0.0;
        }
        self.anomaly_count as f64 / self.observation_count as f64
    }

    /// Indicates whether the cell is congested (based on the median)
    pub fn is_congested(&self) -> bool {
        self.congestion_level.is_congested()
    }
}

// ---------------------------------------------------------------------------
// compute_cell_statistics — full per-cell statistics
// ---------------------------------------------------------------------------

/// Computes extended delay_factor statistics for an S2 cell
///
/// Includes mean, std, min, max, percentiles (p25, p50, p75, p95),
/// IQR and the anomaly count using the IQR method
///
/// # Arguments
/// * `cell_id` — ID of the S2 cell
/// * `factors` — Delay_factors computed for the cell
///
/// # Errors
/// - `InsufficientData` if `factors` has fewer than `min_required` elements
pub fn compute_cell_statistics(
    cell_id: CellID,
    factors: &[DelayFactor],
    min_required: usize,
) -> Result<CellStatistics, MetricsError> {
    if factors.len() < min_required {
        return Err(MetricsError::InsufficientData {
            actual: factors.len(),
            required: min_required,
        });
    }

    let values: Vec<f64> = factors.iter().map(|f| f.value).collect();
    let n = values.len() as f64;

    // Basic statistics
    let sum: f64 = values.iter().sum();
    let mean = sum / n;
    let variance: f64 = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
    let std_dev = variance.sqrt();
    let min = values.iter().cloned().fold(f64::INFINITY, f64::min);
    let max = values.iter().cloned().fold(f64::NEG_INFINITY, f64::max);

    // Sort for percentiles
    let mut sorted = values.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

    // Percentiles
    let p25 = compute_percentile(&sorted, 0.25);
    let median = compute_percentile(&sorted, 0.50);
    let p75 = compute_percentile(&sorted, 0.75);
    let p95 = compute_percentile(&sorted, 0.95);
    let iqr = p75 - p25;

    // IQR anomaly detection
    let lower_fence = p25 - 1.5 * iqr;
    let upper_fence = p75 + 1.5 * iqr;
    let anomaly_count = values
        .iter()
        .filter(|v| **v < lower_fence || **v > upper_fence)
        .count();

    Ok(CellStatistics {
        cell_id,
        observation_count: values.len(),
        mean,
        std_dev,
        min,
        max,
        median,
        percentile_25: p25,
        percentile_75: p75,
        percentile_95: p95,
        iqr,
        anomaly_count,
        congestion_level: CongestionLevel::from_srf(median),
    })
}

// ---------------------------------------------------------------------------
// detect_anomalies — returns the anomalous points
// ---------------------------------------------------------------------------

/// Detects and returns the anomalous `DelayFactor`s using the IQR method
///
/// A value is anomalous if it falls outside `[Q1 - 1.5·IQR, Q3 + 1.5·IQR]`
///
/// # Arguments
/// * `factors` — Delay_factors to analyze
/// * `p25` — Precomputed 25th percentile
/// * `p75` — Precomputed 75th percentile
pub fn detect_anomalies(factors: &[DelayFactor], p25: f64, p75: f64) -> Vec<&DelayFactor> {
    let iqr = p75 - p25;
    let lower_fence = p25 - 1.5 * iqr;
    let upper_fence = p75 + 1.5 * iqr;

    factors
        .iter()
        .filter(|f| f.value < lower_fence || f.value > upper_fence)
        .collect()
}

// ---------------------------------------------------------------------------
// compute_all_statistics — full processing of the S2 index
// ---------------------------------------------------------------------------

/// Computes extended statistics for all the cells of an S2 index
///
/// Cells with insufficient data are skipped. The result is sorted
/// by ascending median (most congested cells first)
///
/// # Arguments
/// * `index` — S2 index with GPS points
/// * `config` — Metrics configuration
pub fn compute_all_statistics(index: &S2Index, config: &MetricsConfig) -> Vec<CellStatistics> {
    let mut stats: Vec<CellStatistics> = index
        .iter_cells()
        .filter_map(|(cell_id, points)| {
            // Compute the delay_factors for the cell's points
            let factors: Vec<DelayFactor> = points
                .iter()
                .filter_map(|p| compute_delay_factor(p, config).ok())
                .collect();

            compute_cell_statistics(*cell_id, &factors, config.min_observations).ok()
        })
        .collect();

    // Sort by ascending median (most congested first)
    stats.sort_by(|a, b| {
        a.median
            .partial_cmp(&b.median)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    stats
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingestion::model::{DataQuality, GpsPoint, RoadType};
    use chrono::NaiveDateTime;

    fn parse_dt(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    fn make_factor(value: f64) -> DelayFactor {
        DelayFactor::new(
            value,
            RoadType::Urban,
            parse_dt("2025-12-01 08:00:00"),
            3395,
        )
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
    // compute_percentile
    // -----------------------------------------------------------------------

    #[test]
    fn test_percentile_basic() {
        // Data: [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]
        let data: Vec<f64> = (1..=10).map(|x| x as f64).collect();

        let p25 = compute_percentile(&data, 0.25);
        let p50 = compute_percentile(&data, 0.50);
        let p75 = compute_percentile(&data, 0.75);

        assert!((p25 - 3.25).abs() < 0.01);
        assert!((p50 - 5.50).abs() < 0.01);
        assert!((p75 - 7.75).abs() < 0.01);
    }

    #[test]
    fn test_percentile_median_odd() {
        // Odd-length data: [1, 2, 3, 4, 5]
        let data = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let median = compute_percentile(&data, 0.50);
        assert!((median - 3.0).abs() < 0.001);
    }

    #[test]
    fn test_percentile_median_even() {
        // Even-length data: [1, 2, 3, 4]
        let data = vec![1.0, 2.0, 3.0, 4.0];
        let median = compute_percentile(&data, 0.50);
        assert!((median - 2.5).abs() < 0.001);
    }

    #[test]
    fn test_percentile_extremes() {
        let data = vec![10.0, 20.0, 30.0, 40.0, 50.0];

        let p0 = compute_percentile(&data, 0.0);
        let p100 = compute_percentile(&data, 1.0);

        assert!((p0 - 10.0).abs() < 0.001);
        assert!((p100 - 50.0).abs() < 0.001);
    }

    #[test]
    fn test_percentile_single_value() {
        let data = vec![42.0];
        assert!((compute_percentile(&data, 0.5) - 42.0).abs() < 0.001);
        assert!((compute_percentile(&data, 0.0) - 42.0).abs() < 0.001);
        assert!((compute_percentile(&data, 1.0) - 42.0).abs() < 0.001);
    }

    #[test]
    #[should_panic(expected = "Cannot compute the percentile of an empty vector")]
    fn test_percentile_empty() {
        compute_percentile(&[], 0.5);
    }

    #[test]
    #[should_panic(expected = "Percentile must be between 0.0 and 1.0")]
    fn test_percentile_out_of_range() {
        compute_percentile(&[1.0], 1.5);
    }

    // -----------------------------------------------------------------------
    // compute_cell_statistics
    // -----------------------------------------------------------------------

    #[test]
    fn test_cell_statistics_uniform() {
        // Uniform data → no anomalies, std ≈ 0
        let factors: Vec<DelayFactor> = (0..10).map(|_| make_factor(0.80)).collect();
        let stats = compute_cell_statistics(CellID(0), &factors, 5).unwrap();

        assert_eq!(stats.observation_count, 10);
        assert!((stats.mean - 0.80).abs() < 0.001);
        assert!(stats.std_dev < 0.001);
        assert!((stats.median - 0.80).abs() < 0.001);
        assert!((stats.percentile_25 - 0.80).abs() < 0.001);
        assert!((stats.percentile_75 - 0.80).abs() < 0.001);
        assert!((stats.iqr).abs() < 0.001);
        assert_eq!(stats.anomaly_count, 0);
        assert_eq!(stats.congestion_level, CongestionLevel::B);
    }

    #[test]
    fn test_cell_statistics_with_anomalies() {
        // Normal values around 0.80, with a very low outlier
        let mut factors: Vec<DelayFactor> = (0..20)
            .map(|i| {
                let v = 0.78 + (i as f64 % 5.0) * 0.01; // 0.78–0.82
                make_factor(v)
            })
            .collect();
        // Add an extreme outlier
        factors.push(make_factor(0.10)); // Far below
        factors.push(make_factor(1.50)); // Far above

        let stats = compute_cell_statistics(CellID(0), &factors, 5).unwrap();

        assert!(
            stats.anomaly_count > 0,
            "Should detect at least one anomaly"
        );
        assert!(stats.has_anomalies());
        assert!(stats.anomaly_ratio() > 0.0);
    }

    #[test]
    fn test_cell_statistics_percentiles_ordered() {
        let factors: Vec<DelayFactor> = (0..100).map(|i| make_factor(i as f64 / 100.0)).collect();

        let stats = compute_cell_statistics(CellID(0), &factors, 5).unwrap();

        assert!(stats.min <= stats.percentile_25);
        assert!(stats.percentile_25 <= stats.median);
        assert!(stats.median <= stats.percentile_75);
        assert!(stats.percentile_75 <= stats.percentile_95);
        assert!(stats.percentile_95 <= stats.max);
    }

    #[test]
    fn test_cell_statistics_insufficient_data() {
        let factors = vec![make_factor(0.80), make_factor(0.85)];
        let result = compute_cell_statistics(CellID(0), &factors, 5);

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            MetricsError::InsufficientData {
                actual: 2,
                required: 5
            }
        ));
    }

    // -----------------------------------------------------------------------
    // detect_anomalies
    // -----------------------------------------------------------------------

    #[test]
    fn test_detect_anomalies_finds_outliers() {
        let mut factors: Vec<DelayFactor> = (0..20).map(|_| make_factor(0.80)).collect();
        factors.push(make_factor(0.10)); // Low outlier
        factors.push(make_factor(1.60)); // High outlier

        // Compute percentiles over the values
        let mut sorted: Vec<f64> = factors.iter().map(|f| f.value).collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
        let p25 = compute_percentile(&sorted, 0.25);
        let p75 = compute_percentile(&sorted, 0.75);

        let anomalies = detect_anomalies(&factors, p25, p75);
        assert!(
            anomalies.len() >= 2,
            "Should detect at least the 2 outliers"
        );
    }

    #[test]
    fn test_detect_anomalies_no_outliers() {
        let factors: Vec<DelayFactor> = (0..10).map(|_| make_factor(0.80)).collect();

        let anomalies = detect_anomalies(&factors, 0.80, 0.80);
        assert!(anomalies.is_empty());
    }

    // -----------------------------------------------------------------------
    // compute_all_statistics
    // -----------------------------------------------------------------------

    #[test]
    fn test_compute_all_statistics() {
        let config = MetricsConfig {
            m30_reference_speed: 90.0,
            urban_reference_speed: 50.0,
            min_observations: 2,
        };

        let mut index = crate::geospatial::s2_index::S2Index::new(15);

        // Insert urban points in the same area
        for i in 0..5 {
            index.insert(make_urban_point(
                3395 + i,
                30 + (i as i32) * 5,
                "2025-12-01 08:00:00",
            ));
        }

        let stats = compute_all_statistics(&index, &config);
        assert!(!stats.is_empty());

        // Check that the statistics are coherent
        for s in &stats {
            assert!(s.min <= s.median);
            assert!(s.median <= s.max);
            assert!(s.percentile_25 <= s.percentile_75);
        }
    }

    #[test]
    fn test_compute_all_statistics_empty() {
        let config = MetricsConfig {
            m30_reference_speed: 90.0,
            urban_reference_speed: 50.0,
            min_observations: 5,
        };

        let index = crate::geospatial::s2_index::S2Index::new(15);
        let stats = compute_all_statistics(&index, &config);
        assert!(stats.is_empty());
    }

    #[test]
    fn test_compute_all_statistics_sorted() {
        let config = MetricsConfig {
            m30_reference_speed: 90.0,
            urban_reference_speed: 50.0,
            min_observations: 2,
        };

        let mut index = crate::geospatial::s2_index::S2Index::new(15);

        // Zone 1: high load (congested)
        for i in 0..3 {
            index.insert(make_urban_point(3395 + i, 70, "2025-12-01 08:00:00"));
        }

        // Zone 2: low load (free flow) — different coordinates
        for i in 0..3 {
            let mut p = make_urban_point(3400 + i, 10, "2025-12-01 08:00:00");
            p.latitude = 40.50;
            p.longitude = -3.63;
            index.insert(p);
        }

        let stats = compute_all_statistics(&index, &config);

        // Verify ascending order by median
        for i in 1..stats.len() {
            assert!(
                stats[i - 1].median <= stats[i].median,
                "Statistics must be sorted by ascending median"
            );
        }
    }

    // -----------------------------------------------------------------------
    // CellStatistics methods
    // -----------------------------------------------------------------------

    #[test]
    fn test_anomaly_ratio() {
        let factors: Vec<DelayFactor> = (0..10).map(|_| make_factor(0.80)).collect();
        let stats = compute_cell_statistics(CellID(0), &factors, 1).unwrap();
        assert!((stats.anomaly_ratio() - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_is_congested() {
        // Mean/median of 0.40 → CongestionLevel::E → congested
        let factors: Vec<DelayFactor> = (0..10).map(|_| make_factor(0.40)).collect();
        let stats = compute_cell_statistics(CellID(0), &factors, 1).unwrap();
        assert!(stats.is_congested());

        // Mean/median of 0.90 → CongestionLevel::A → not congested
        let factors: Vec<DelayFactor> = (0..10).map(|_| make_factor(0.90)).collect();
        let stats = compute_cell_statistics(CellID(0), &factors, 1).unwrap();
        assert!(!stats.is_congested());
    }
}
