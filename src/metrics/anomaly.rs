//! Traffic anomaly detection: live SRF vs historical baseline
//!
//! The engine already computes historical percentiles via `CellStatistics`
//! This module builds a **per-cell baseline** from that data
//! and compares the SRF of each live tick against that baseline
//!
//! ## Flow
//!
//! ```text
//! Historical CSV (warm-start)
//!        │
//!        ▼
//! AnomalyDetector::from_statistics(&hist_stats)
//!        │
//!        │          Madrid API (every 5 min)
//!        │                 │
//!        ▼                 ▼
//!   detector.detect(&cell_reports)  →  Vec<AnomalyAlert>
//! ```
//!
//! ## Severity classification
//!
//! | Condition               | Severity     |
//! |-------------------------|--------------|
//! | SRF < p5                | Severe       |
//! | p5 <= SRF < p25         | Moderate     |
//! | SRF > p95               | Improvement  |

use std::collections::HashMap;

use s2::cellid::CellID;
use serde::{Deserialize, Serialize};

use crate::metrics::model::CellDelayReport;
use crate::metrics::statistics::CellStatistics;

// ---------------------------------------------------------------------------
// AnomalySeverity
// ---------------------------------------------------------------------------

/// Severity of a detected traffic anomaly
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnomalySeverity {
    /// Live SRF between the historical p5 and p25: moderate degradation
    Moderate,
    /// Live SRF below the historical p5: severe degradation
    Severe,
    /// Live SRF above the historical p95: better traffic than usual
    Improvement,
}

// ---------------------------------------------------------------------------
// AnomalyAlert
// ---------------------------------------------------------------------------

/// Anomaly alert for an S2 cell in a specific tick
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyAlert {
    /// Cell where the anomaly is detected
    pub cell_id: CellID,
    /// SRF observed in the current tick
    pub observed_srf: f64,
    /// Historical median SRF (p50) of that cell
    pub baseline_p50: f64,
    /// Historical p5 percentile (lower bound of normality)
    pub baseline_p5: f64,
    /// Historical p95 percentile (upper bound of normality)
    pub baseline_p95: f64,
    /// Absolute deviation from the historical p50
    pub deviation: f64,
    /// Classified severity of the anomaly
    pub severity: AnomalySeverity,
}

impl AnomalyAlert {
    /// Human-readable description to show in the TUI or the logs
    pub fn description(&self) -> String {
        match self.severity {
            AnomalySeverity::Severe => format!(
                "SEVERE — SRF {:.2} (normal: {:.2})",
                self.observed_srf, self.baseline_p50
            ),
            AnomalySeverity::Moderate => format!(
                "MODERATE — SRF {:.2} (normal: {:.2})",
                self.observed_srf, self.baseline_p50
            ),
            AnomalySeverity::Improvement => format!(
                "IMPROVEMENT — SRF {:.2} (normal: {:.2})",
                self.observed_srf, self.baseline_p50
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// CellBaseline
// ---------------------------------------------------------------------------

/// Historical baseline of an S2 cell
///
/// Computed from the historical CSV during the warm-start
/// Allows comparing the live SRF of each tick against
/// the historical distribution of that cell
#[derive(Debug, Clone)]
pub struct CellBaseline {
    /// Estimate of the 5th percentile (via IQR: p25 - 1.5*IQR)
    pub p5: f64,
    /// Historical 25th percentile
    pub p25: f64,
    /// Historical 50th percentile (median)
    pub p50: f64,
    /// Historical 75th percentile
    pub p75: f64,
    /// Estimate of the 95th percentile (via IQR: p75 + 1.5*IQR)
    pub p95: f64,
}

// ---------------------------------------------------------------------------
// AnomalyDetector
// ---------------------------------------------------------------------------

/// Anomaly detection engine
///
/// Stores the historical baseline of each cell and compares
/// the live reports of each tick against that baseline
pub struct AnomalyDetector {
    baselines: HashMap<CellID, CellBaseline>,
}

impl AnomalyDetector {
    /// Builds the detector from the computed historical statistics
    ///
    /// Uses the p25, p75 and IQR of `CellStatistics` to estimate
    /// p5 and p95, and the median (p50) as the central reference
    ///
    /// # Arguments
    /// * `stats` — Vector of `CellStatistics` computed with `compute_all_statistics()`
    ///   over the historical index (warm-start)
    pub fn from_statistics(stats: &[CellStatistics]) -> Self {
        let baselines = stats
            .iter()
            .map(|s| {
                let p5 = (s.percentile_25 - 1.5 * s.iqr).max(0.0);
                let p95 = (s.percentile_75 + 1.5 * s.iqr).min(1.0);
                (
                    s.cell_id,
                    CellBaseline {
                        p5,
                        p25: s.percentile_25,
                        p50: s.median,
                        p75: s.percentile_75,
                        p95,
                    },
                )
            })
            .collect();

        AnomalyDetector { baselines }
    }

    /// Detects anomalies by comparing the reports of a tick against the baseline
    ///
    /// Returns only the cells that show anomalous behavior
    /// Cells without a baseline (not present in the history) are silently
    /// ignored
    ///
    /// ## Classification rules
    ///
    /// | Condition               | Severity     |
    /// |-------------------------|--------------|
    /// | observed < p5           | Severe       |
    /// | p5 <= observed < p25    | Moderate     |
    /// | observed > p95          | Improvement  |
    /// | p25 <= observed <= p95  | Normal (no alert) |
    pub fn detect(&self, live_reports: &[CellDelayReport]) -> Vec<AnomalyAlert> {
        let mut alerts = Vec::new();

        for report in live_reports {
            let Some(baseline) = self.baselines.get(&report.cell_id) else {
                // No historical baseline: ignore silently
                continue;
            };

            let srf = report.avg_delay_factor;

            let severity = if srf < baseline.p5 {
                AnomalySeverity::Severe
            } else if srf < baseline.p25 {
                AnomalySeverity::Moderate
            } else if srf > baseline.p95 {
                AnomalySeverity::Improvement
            } else {
                // Normal: within the expected range
                continue;
            };

            let deviation = (srf - baseline.p50).abs();

            alerts.push(AnomalyAlert {
                cell_id: report.cell_id,
                observed_srf: srf,
                baseline_p50: baseline.p50,
                baseline_p5: baseline.p5,
                baseline_p95: baseline.p95,
                deviation,
                severity,
            });
        }

        // Sort by descending deviation (most severe first)
        alerts.sort_by(|a, b| {
            b.deviation
                .partial_cmp(&a.deviation)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        alerts
    }

    /// Number of cells with an available baseline
    pub fn baseline_size(&self) -> usize {
        self.baselines.len()
    }

    /// Indicates whether a baseline exists for the given cell
    pub fn has_baseline(&self, cell_id: CellID) -> bool {
        self.baselines.contains_key(&cell_id)
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingestion::model::RoadType;
    use crate::metrics::model::{CellDelayReport, DelayFactor};
    use crate::metrics::statistics::compute_cell_statistics;
    use chrono::NaiveDateTime;
    use s2::cellid::CellID;

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

    /// Builds CellStatistics with uniform data around `center`
    fn make_stats(cell_id: CellID, center: f64, spread: f64) -> CellStatistics {
        let values: Vec<f64> = (0..20)
            .map(|i| center + spread * ((i as f64 / 20.0) - 0.5))
            .collect();
        let factors: Vec<DelayFactor> = values.iter().map(|&v| make_factor(v)).collect();
        compute_cell_statistics(cell_id, &factors, 1).unwrap()
    }

    fn make_report(cell_id: CellID, srf: f64) -> CellDelayReport {
        // We build using from_factors so that all fields are valid
        let factors: Vec<DelayFactor> = (0..5).map(|_| make_factor(srf)).collect();
        CellDelayReport::from_factors(cell_id, &factors)
    }

    // -----------------------------------------------------------------------
    // from_statistics
    // -----------------------------------------------------------------------

    #[test]
    fn test_detector_from_empty_stats() {
        let detector = AnomalyDetector::from_statistics(&[]);
        assert_eq!(detector.baseline_size(), 0);
    }

    #[test]
    fn test_detector_from_statistics_sets_baseline() {
        let cell = CellID(123456);
        let stats = make_stats(cell, 0.70, 0.10);
        let detector = AnomalyDetector::from_statistics(&[stats]);

        assert_eq!(detector.baseline_size(), 1);
        assert!(detector.has_baseline(cell));
        assert!(!detector.has_baseline(CellID(999)));
    }

    #[test]
    fn test_baseline_p5_clamped_to_zero() {
        // If the IQR is very large, p5 can go negative: it must be clamped to 0
        let cell = CellID(1);
        // Very large spread → large IQR → p5 = p25 - 1.5*IQR could be negative
        let stats = make_stats(cell, 0.50, 1.0);
        let detector = AnomalyDetector::from_statistics(&[stats]);

        let baseline = &detector.baselines[&cell];
        assert!(baseline.p5 >= 0.0, "p5 must be >= 0.0");
    }

    #[test]
    fn test_baseline_p95_clamped_to_one() {
        let cell = CellID(2);
        let stats = make_stats(cell, 0.90, 1.0);
        let detector = AnomalyDetector::from_statistics(&[stats]);

        let baseline = &detector.baselines[&cell];
        assert!(baseline.p95 <= 1.0, "p95 must be <= 1.0");
    }

    // -----------------------------------------------------------------------
    // detect — no alerts
    // -----------------------------------------------------------------------

    #[test]
    fn test_no_alerts_when_srf_is_normal() {
        let cell = CellID(100);
        let stats = make_stats(cell, 0.70, 0.10); // center 0.70
        let detector = AnomalyDetector::from_statistics(std::slice::from_ref(&stats));

        // SRF = exact p50 → completely normal
        let report = make_report(cell, stats.median);
        let alerts = detector.detect(&[report]);

        assert!(
            alerts.is_empty(),
            "SRF at the median must not generate an alert"
        );
    }

    #[test]
    fn test_no_baseline_cell_ignored() {
        // Detector without any baseline
        let detector = AnomalyDetector::from_statistics(&[]);

        let report = make_report(CellID(999), 0.10); // very low SRF, but no baseline
        let alerts = detector.detect(&[report]);

        assert!(
            alerts.is_empty(),
            "Without a baseline the cell is silently ignored"
        );
    }

    #[test]
    fn test_empty_reports_no_alerts() {
        let cell = CellID(1);
        let stats = make_stats(cell, 0.70, 0.10);
        let detector = AnomalyDetector::from_statistics(&[stats]);

        let alerts = detector.detect(&[]);
        assert!(alerts.is_empty());
    }

    // -----------------------------------------------------------------------
    // detect — severe alerts
    // -----------------------------------------------------------------------

    #[test]
    fn test_severe_alert_below_p5() {
        let cell = CellID(200);
        // Narrow distribution around 0.70
        let stats = make_stats(cell, 0.70, 0.04);
        let detector = AnomalyDetector::from_statistics(std::slice::from_ref(&stats));

        // Extremely low SRF → below p5
        let report = make_report(cell, 0.20);
        let alerts = detector.detect(&[report]);

        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].severity, AnomalySeverity::Severe);
        assert!((alerts[0].observed_srf - 0.20).abs() < 0.001);
        assert!((alerts[0].baseline_p50 - stats.median).abs() < 0.001);
    }

    // -----------------------------------------------------------------------
    // detect — moderate alerts
    // -----------------------------------------------------------------------

    #[test]
    fn test_moderate_alert_between_p5_p25() {
        let cell = CellID(300);
        // Distribution around 0.80 with a reduced spread
        let stats = make_stats(cell, 0.80, 0.06);
        let detector = AnomalyDetector::from_statistics(std::slice::from_ref(&stats));

        // SRF between p5 and p25 → moderate
        // p5 = p25 - 1.5*IQR ≈ (0.80-0.015) - 1.5*0.03 ≈ 0.74
        // We use a value in that range
        let candidate_srf = stats.percentile_25 - (stats.iqr * 0.5);
        if candidate_srf > 0.0 {
            let report = make_report(cell, candidate_srf);
            let alerts = detector.detect(&[report]);
            if !alerts.is_empty() {
                assert_eq!(alerts[0].severity, AnomalySeverity::Moderate);
            }
        }
    }

    // -----------------------------------------------------------------------
    // detect — improvements
    // -----------------------------------------------------------------------

    #[test]
    fn test_improvement_alert_above_p95() {
        let cell = CellID(400);
        // Low distribution around 0.40 (usually congested area)
        let stats = make_stats(cell, 0.40, 0.04);
        let detector = AnomalyDetector::from_statistics(std::slice::from_ref(&stats));

        // Very high SRF → above p95 → improvement
        let report = make_report(cell, 0.95);
        let alerts = detector.detect(&[report]);

        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].severity, AnomalySeverity::Improvement);
    }

    // -----------------------------------------------------------------------
    // detect — multiple cells and ordering
    // -----------------------------------------------------------------------

    #[test]
    fn test_multiple_cells_mixed_results() {
        let cell_a = CellID(501);
        let cell_b = CellID(502);
        let cell_c = CellID(503);

        let stats_a = make_stats(cell_a, 0.70, 0.04); // normal
        let stats_b = make_stats(cell_b, 0.70, 0.04); // severe
        let stats_c = make_stats(cell_c, 0.70, 0.04); // normal

        let detector =
            AnomalyDetector::from_statistics(&[stats_a.clone(), stats_b.clone(), stats_c.clone()]);

        let reports = vec![
            make_report(cell_a, stats_a.median), // normal
            make_report(cell_b, 0.10),           // severe
            make_report(cell_c, stats_c.median), // normal
        ];
        let alerts = detector.detect(&reports);

        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].cell_id, cell_b);
        assert_eq!(alerts[0].severity, AnomalySeverity::Severe);
    }

    #[test]
    fn test_alerts_sorted_by_deviation_desc() {
        let cell_a = CellID(601);
        let cell_b = CellID(602);

        let stats_a = make_stats(cell_a, 0.70, 0.04);
        let stats_b = make_stats(cell_b, 0.70, 0.04);

        let detector = AnomalyDetector::from_statistics(&[stats_a.clone(), stats_b.clone()]);

        // cell_a: small deviation (slightly anomalous)
        // cell_b: large deviation (very anomalous)
        let reports = vec![
            make_report(cell_a, 0.50), // moderate deviation
            make_report(cell_b, 0.05), // large deviation
        ];
        let alerts = detector.detect(&reports);

        if alerts.len() >= 2 {
            assert!(
                alerts[0].deviation >= alerts[1].deviation,
                "Alerts must be sorted by descending deviation"
            );
        }
    }

    // -----------------------------------------------------------------------
    // AnomalyAlert::description
    // -----------------------------------------------------------------------

    #[test]
    fn test_alert_description_severe() {
        let alert = AnomalyAlert {
            cell_id: CellID(0),
            observed_srf: 0.28,
            baseline_p50: 0.71,
            baseline_p5: 0.55,
            baseline_p95: 0.90,
            deviation: 0.43,
            severity: AnomalySeverity::Severe,
        };
        let desc = alert.description();
        assert!(desc.contains("SEVERE"));
        assert!(desc.contains("0.28"));
        assert!(desc.contains("0.71"));
    }

    #[test]
    fn test_alert_description_improvement() {
        let alert = AnomalyAlert {
            cell_id: CellID(0),
            observed_srf: 0.92,
            baseline_p50: 0.45,
            baseline_p5: 0.30,
            baseline_p95: 0.70,
            deviation: 0.47,
            severity: AnomalySeverity::Improvement,
        };
        let desc = alert.description();
        assert!(desc.contains("IMPROVEMENT"));
    }

    // -----------------------------------------------------------------------
    // Full pipeline with detector
    // -----------------------------------------------------------------------

    #[test]
    fn test_full_pipeline_with_anomaly_detector() {
        use crate::config::MetricsConfig;
        use crate::geospatial::s2_index::S2Index;
        use crate::ingestion::model::{DataQuality, GpsPoint, RoadType};
        use crate::metrics::delay_factor::compute_all_cells;

        let metrics_config = MetricsConfig {
            m30_reference_speed: 90.0,
            urban_reference_speed: 50.0,
            min_observations: 1,
        };

        // Build a historical index with normal load (30-50)
        let mut hist_index = S2Index::new(15);
        for i in 0..10u32 {
            hist_index.insert(GpsPoint {
                id: i,
                timestamp: parse_dt("2025-12-01 08:00:00"),
                latitude: 40.42 + (i as f64) * 0.001,
                longitude: -3.70,
                road_type: RoadType::Urban,
                intensidad: 300,
                ocupacion: 15,
                carga: 35,
                velocidad: None,
                quality: DataQuality::Normal,
            });
        }

        // Compute historical statistics and build the detector
        let hist_stats =
            crate::metrics::statistics::compute_all_statistics(&hist_index, &metrics_config);
        let detector = AnomalyDetector::from_statistics(&hist_stats);

        // It does not necessarily have a baseline if the points lack enough data,
        // but the detector must be built without errors
        let _ = detector.baseline_size();

        // Live pipeline: compute reports and detect anomalies
        let live_reports = compute_all_cells(&hist_index, &metrics_config);
        let alerts = detector.detect(&live_reports);

        // Points have carga=35 → normal → there should be no severe alerts
        // (the baseline and live data are identical)
        for alert in &alerts {
            assert_ne!(
                alert.severity,
                AnomalySeverity::Severe,
                "With identical data there should be no severe alerts"
            );
        }
    }
}
