//! Data models for the traffic metrics module
//!
//! Defines the structures that represent the delay_factor,
//! the congestion levels and the time slots for
//! urban mobility analysis
//!
//! ## delay_factor formula (adapted Speed Reduction Factor)
//!
//! The delay_factor is based on the Speed Reduction Factor
//! (SRF) concept of the Highway Capacity Manual / MAP-21:
//!
//! - **M-30** (speed available): `SRF = v_observed / v_reference`
//! - **Urban** (no speed): `SRF = 1.0 - (carga / 100.0)`
//!
//! Where `carga` (0–100) is the normalized degree of congestion from the
//! Madrid City Council. An SRF of 1.0 = free flow, 0.0 = collapse

use chrono::{Datelike, NaiveDateTime, Timelike, Weekday};
use s2::cellid::CellID;
use serde::{Deserialize, Serialize};

use crate::ingestion::model::RoadType;

// ---------------------------------------------------------------------------
// CongestionLevel — Congestion levels A–F (inspired by HCM LoS)
// ---------------------------------------------------------------------------

/// Congestion level based on the Speed Reduction Factor (SRF)
///
/// Classification inspired by the Highway Capacity Manual (HCM) Level
/// of Service, adapted to the MAP-21 thresholds for urban roads
/// and motorways
///
/// | Level | SRF          | Description               |
/// |-------|--------------|---------------------------|
/// | A     | ≥ 0.90       | Free flow                 |
/// | B     | 0.80 – 0.90  | Stable flow               |
/// | C     | 0.65 – 0.80  | Moderate restrictions     |
/// | D     | 0.50 – 0.65  | Unstable flow             |
/// | E     | 0.35 – 0.50  | Severe congestion         |
/// | F     | < 0.35       | Collapse                  |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CongestionLevel {
    /// Free flow (SRF ≥ 0.90)
    A,
    /// Stable flow (SRF 0.80–0.90)
    B,
    /// Moderate restrictions (SRF 0.65–0.80)
    C,
    /// Unstable flow (SRF 0.50–0.65)
    D,
    /// Severe congestion (SRF 0.35–0.50)
    E,
    /// Collapse (SRF < 0.35)
    F,
}

impl CongestionLevel {
    /// Determines the congestion level from an SRF value
    ///
    /// # Arguments
    /// * `srf` — Speed Reduction Factor (0.0–1.0+)
    ///
    /// # Example
    /// ```
    /// use tfg::metrics::model::CongestionLevel;
    ///
    /// assert_eq!(CongestionLevel::from_srf(0.95), CongestionLevel::A);
    /// assert_eq!(CongestionLevel::from_srf(0.70), CongestionLevel::C);
    /// assert_eq!(CongestionLevel::from_srf(0.20), CongestionLevel::F);
    /// ```
    pub fn from_srf(srf: f64) -> Self {
        match srf {
            s if s >= 0.90 => CongestionLevel::A,
            s if s >= 0.80 => CongestionLevel::B,
            s if s >= 0.65 => CongestionLevel::C,
            s if s >= 0.50 => CongestionLevel::D,
            s if s >= 0.35 => CongestionLevel::E,
            _ => CongestionLevel::F,
        }
    }

    /// Indicates whether the congestion level is severe (D, E or F)
    pub fn is_congested(&self) -> bool {
        matches!(
            self,
            CongestionLevel::D | CongestionLevel::E | CongestionLevel::F
        )
    }

    /// Returns the textual label of the congestion level
    pub fn label(&self) -> &'static str {
        match self {
            CongestionLevel::A => "Free flow",
            CongestionLevel::B => "Stable flow",
            CongestionLevel::C => "Moderate restrictions",
            CongestionLevel::D => "Unstable flow",
            CongestionLevel::E => "Severe congestion",
            CongestionLevel::F => "Collapse",
        }
    }
}

impl std::fmt::Display for CongestionLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} ({})", self, self.label())
    }
}

// ---------------------------------------------------------------------------
// TimeSlot — Time slots for temporal aggregation
// ---------------------------------------------------------------------------

/// Time slot of the day for temporal traffic aggregation
///
/// The slots are defined according to typical Madrid traffic
/// patterns, capturing the rush and off-peak hours
///
/// | Slot            | Hours        | Typical pattern      |
/// |-----------------|--------------|----------------------|
/// | EarlyMorning    | 00:00–06:59  | Minimal traffic      |
/// | MorningRush     | 07:00–09:59  | Morning peak         |
/// | Midday          | 10:00–13:59  | Daytime lull         |
/// | AfternoonRush   | 14:00–16:59  | Early-afternoon peak |
/// | EveningRush     | 17:00–20:59  | Evening peak         |
/// | Night           | 21:00–23:59  | Night-time decline   |
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TimeSlot {
    /// Early morning: 00:00–06:59 — Minimal traffic
    EarlyMorning,
    /// Morning rush hour: 07:00–09:59 — Commute to work
    MorningRush,
    /// Midday: 10:00–13:59 — Daytime lull
    Midday,
    /// Afternoon rush hour: 14:00–16:59 — Lunch break/return
    AfternoonRush,
    /// Evening rush hour: 17:00–20:59 — Commute home
    EveningRush,
    /// Night: 21:00–23:59 — Night-time decline
    Night,
}

impl TimeSlot {
    /// Determines the time slot from an hour of the day (0–23)
    ///
    /// # Panics
    /// Panics if `hour > 23`
    ///
    /// # Example
    /// ```
    /// use tfg::metrics::model::TimeSlot;
    ///
    /// assert_eq!(TimeSlot::from_hour(3), TimeSlot::EarlyMorning);
    /// assert_eq!(TimeSlot::from_hour(8), TimeSlot::MorningRush);
    /// assert_eq!(TimeSlot::from_hour(18), TimeSlot::EveningRush);
    /// ```
    pub fn from_hour(hour: u32) -> Self {
        assert!(hour <= 23, "Hour must be between 0 and 23, got: {}", hour);
        match hour {
            0..=6 => TimeSlot::EarlyMorning,
            7..=9 => TimeSlot::MorningRush,
            10..=13 => TimeSlot::Midday,
            14..=16 => TimeSlot::AfternoonRush,
            17..=20 => TimeSlot::EveningRush,
            21..=23 => TimeSlot::Night,
            _ => unreachable!(),
        }
    }

    /// Determines the time slot from a `NaiveDateTime`
    pub fn from_datetime(dt: &NaiveDateTime) -> Self {
        Self::from_hour(dt.hour())
    }

    /// Returns the descriptive label of the time slot
    pub fn label(&self) -> &'static str {
        match self {
            TimeSlot::EarlyMorning => "Early morning (00–07)",
            TimeSlot::MorningRush => "Morning rush (07–10)",
            TimeSlot::Midday => "Midday (10–14)",
            TimeSlot::AfternoonRush => "Afternoon rush (14–17)",
            TimeSlot::EveningRush => "Evening rush (17–21)",
            TimeSlot::Night => "Night (21–00)",
        }
    }

    /// Returns all time slots in chronological order
    pub fn all() -> [TimeSlot; 6] {
        [
            TimeSlot::EarlyMorning,
            TimeSlot::MorningRush,
            TimeSlot::Midday,
            TimeSlot::AfternoonRush,
            TimeSlot::EveningRush,
            TimeSlot::Night,
        ]
    }
}

impl std::fmt::Display for TimeSlot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.label())
    }
}

// ---------------------------------------------------------------------------
// DayType — Day type for traffic patterns
// ---------------------------------------------------------------------------

/// Day type to distinguish traffic patterns
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DayType {
    /// Weekday (Monday to Friday)
    Weekday,
    /// Weekend (Saturday and Sunday)
    Weekend,
}

impl DayType {
    /// Determines the day type from a `chrono::Weekday`
    pub fn from_weekday(day: Weekday) -> Self {
        match day {
            Weekday::Sat | Weekday::Sun => DayType::Weekend,
            _ => DayType::Weekday,
        }
    }

    /// Determines the day type from a `NaiveDateTime`
    pub fn from_datetime(dt: &NaiveDateTime) -> Self {
        Self::from_weekday(dt.weekday())
    }
}

// ---------------------------------------------------------------------------
// DelayFactor — Main per-observation metric
// ---------------------------------------------------------------------------

/// Result of the delay_factor computation for a single observation
///
/// Represents the Speed Reduction Factor (SRF) adapted for a specific
/// GPS point. A value of 1.0 indicates free flow; lower
/// values indicate increasing congestion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelayFactor {
    /// SRF value (0.0–1.0+). Values > 1.0 indicate a speed
    /// above the reference
    pub value: f64,
    /// Congestion level derived from the SRF
    pub congestion: CongestionLevel,
    /// Road type where the metric was computed
    pub road_type: RoadType,
    /// Timestamp of the original observation
    pub timestamp: NaiveDateTime,
    /// ID of the source sensor
    pub sensor_id: u32,
}

impl DelayFactor {
    /// Creates a new `DelayFactor` from an SRF value
    ///
    /// The congestion level is computed automatically
    pub fn new(value: f64, road_type: RoadType, timestamp: NaiveDateTime, sensor_id: u32) -> Self {
        DelayFactor {
            value,
            congestion: CongestionLevel::from_srf(value),
            road_type,
            timestamp,
            sensor_id,
        }
    }

    /// Time slot of this observation
    pub fn time_slot(&self) -> TimeSlot {
        TimeSlot::from_datetime(&self.timestamp)
    }

    /// Day type of this observation
    pub fn day_type(&self) -> DayType {
        DayType::from_datetime(&self.timestamp)
    }

    /// Day of the week of this observation
    pub fn weekday(&self) -> Weekday {
        self.timestamp.weekday()
    }
}

// ---------------------------------------------------------------------------
// CellDelayReport — Per-S2-cell delay_factor report
// ---------------------------------------------------------------------------

/// Aggregated delay_factor report for an S2 cell
///
/// Summarizes the congestion metrics of all the GPS points
/// contained in a grid cell. It is the main output
/// of the metrics computation engine
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellDelayReport {
    /// Identifier of the S2 cell
    pub cell_id: CellID,
    /// Number of observations processed
    pub observation_count: usize,
    /// Mean delay_factor in the cell
    pub avg_delay_factor: f64,
    /// Minimum observed delay_factor
    pub min_delay_factor: f64,
    /// Maximum observed delay_factor
    pub max_delay_factor: f64,
    /// Standard deviation of the delay_factor
    pub std_delay_factor: f64,
    /// Average congestion level of the cell
    pub congestion_level: CongestionLevel,
    /// Dominant road type in the cell
    pub road_type_dominant: RoadType,
    /// Timestamp of the first observation
    pub timestamp_start: NaiveDateTime,
    /// Timestamp of the last observation
    pub timestamp_end: NaiveDateTime,
}

impl CellDelayReport {
    /// Builds a report from a vector of `DelayFactor`s
    ///
    /// # Arguments
    /// * `cell_id` — ID of the S2 cell
    /// * `factors` — Vector of computed delay_factors
    ///
    /// # Panics
    /// Panics if `factors` is empty
    pub fn from_factors(cell_id: CellID, factors: &[DelayFactor]) -> Self {
        assert!(
            !factors.is_empty(),
            "Cannot create a report without factors"
        );

        let count = factors.len();
        let sum: f64 = factors.iter().map(|f| f.value).sum();
        let avg = sum / count as f64;

        let min = factors
            .iter()
            .map(|f| f.value)
            .fold(f64::INFINITY, f64::min);
        let max = factors
            .iter()
            .map(|f| f.value)
            .fold(f64::NEG_INFINITY, f64::max);

        // Standard deviation
        let variance: f64 =
            factors.iter().map(|f| (f.value - avg).powi(2)).sum::<f64>() / count as f64;
        let std_dev = variance.sqrt();

        // Dominant road type (the most frequent)
        let m30_count = factors
            .iter()
            .filter(|f| f.road_type == RoadType::M30)
            .count();
        let road_type_dominant = if m30_count > count / 2 {
            RoadType::M30
        } else {
            RoadType::Urban
        };

        // Time range
        let timestamp_start = factors.iter().map(|f| f.timestamp).min().unwrap();
        let timestamp_end = factors.iter().map(|f| f.timestamp).max().unwrap();

        CellDelayReport {
            cell_id,
            observation_count: count,
            avg_delay_factor: avg,
            min_delay_factor: min,
            max_delay_factor: max,
            std_delay_factor: std_dev,
            congestion_level: CongestionLevel::from_srf(avg),
            road_type_dominant,
            timestamp_start,
            timestamp_end,
        }
    }

    /// Indicates whether the cell is congested (level D, E or F)
    pub fn is_congested(&self) -> bool {
        self.congestion_level.is_congested()
    }

    /// Indicates whether the cell should be subdivided due to high congestion
    ///
    /// A cell is subdivided if its mean delay_factor exceeds
    /// a configured threshold (typically 1.5 = 50% slower)
    pub fn should_subdivide(&self, delay_threshold: f64) -> bool {
        // Note: in our SRF model, low values = congestion
        // The original delay_threshold in the config is > 1.0, but
        // in SRF congestion is < 1.0. We convert:
        // An SRF of 0.66 is equivalent to a delay_factor of 1.5 (50% slower)
        let srf_threshold = 1.0 / delay_threshold;
        self.avg_delay_factor < srf_threshold
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDateTime;

    fn parse_dt(s: &str) -> NaiveDateTime {
        NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").unwrap()
    }

    // -----------------------------------------------------------------------
    // CongestionLevel tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_congestion_level_from_srf() {
        assert_eq!(CongestionLevel::from_srf(1.0), CongestionLevel::A);
        assert_eq!(CongestionLevel::from_srf(0.95), CongestionLevel::A);
        assert_eq!(CongestionLevel::from_srf(0.90), CongestionLevel::A);
        assert_eq!(CongestionLevel::from_srf(0.89), CongestionLevel::B);
        assert_eq!(CongestionLevel::from_srf(0.85), CongestionLevel::B);
        assert_eq!(CongestionLevel::from_srf(0.80), CongestionLevel::B);
        assert_eq!(CongestionLevel::from_srf(0.79), CongestionLevel::C);
        assert_eq!(CongestionLevel::from_srf(0.70), CongestionLevel::C);
        assert_eq!(CongestionLevel::from_srf(0.65), CongestionLevel::C);
        assert_eq!(CongestionLevel::from_srf(0.64), CongestionLevel::D);
        assert_eq!(CongestionLevel::from_srf(0.55), CongestionLevel::D);
        assert_eq!(CongestionLevel::from_srf(0.50), CongestionLevel::D);
        assert_eq!(CongestionLevel::from_srf(0.49), CongestionLevel::E);
        assert_eq!(CongestionLevel::from_srf(0.40), CongestionLevel::E);
        assert_eq!(CongestionLevel::from_srf(0.35), CongestionLevel::E);
        assert_eq!(CongestionLevel::from_srf(0.34), CongestionLevel::F);
        assert_eq!(CongestionLevel::from_srf(0.10), CongestionLevel::F);
        assert_eq!(CongestionLevel::from_srf(0.0), CongestionLevel::F);
    }

    #[test]
    fn test_congestion_level_above_one() {
        // Speeds above the reference → SRF > 1.0 → A
        assert_eq!(CongestionLevel::from_srf(1.2), CongestionLevel::A);
    }

    #[test]
    fn test_congestion_is_congested() {
        assert!(!CongestionLevel::A.is_congested());
        assert!(!CongestionLevel::B.is_congested());
        assert!(!CongestionLevel::C.is_congested());
        assert!(CongestionLevel::D.is_congested());
        assert!(CongestionLevel::E.is_congested());
        assert!(CongestionLevel::F.is_congested());
    }

    #[test]
    fn test_congestion_label() {
        assert_eq!(CongestionLevel::A.label(), "Free flow");
        assert_eq!(CongestionLevel::F.label(), "Collapse");
    }

    #[test]
    fn test_congestion_display() {
        let display = format!("{}", CongestionLevel::C);
        assert!(display.contains("C"));
        assert!(display.contains("Moderate restrictions"));
    }

    // -----------------------------------------------------------------------
    // TimeSlot tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_time_slot_from_hour() {
        // Early morning
        for h in 0..=6 {
            assert_eq!(TimeSlot::from_hour(h), TimeSlot::EarlyMorning);
        }
        // Morning rush
        for h in 7..=9 {
            assert_eq!(TimeSlot::from_hour(h), TimeSlot::MorningRush);
        }
        // Midday
        for h in 10..=13 {
            assert_eq!(TimeSlot::from_hour(h), TimeSlot::Midday);
        }
        // Afternoon rush
        for h in 14..=16 {
            assert_eq!(TimeSlot::from_hour(h), TimeSlot::AfternoonRush);
        }
        // Evening rush
        for h in 17..=20 {
            assert_eq!(TimeSlot::from_hour(h), TimeSlot::EveningRush);
        }
        // Night
        for h in 21..=23 {
            assert_eq!(TimeSlot::from_hour(h), TimeSlot::Night);
        }
    }

    #[test]
    fn test_time_slot_from_datetime() {
        let dt = parse_dt("2025-12-01 08:30:00");
        assert_eq!(TimeSlot::from_datetime(&dt), TimeSlot::MorningRush);

        let dt = parse_dt("2025-12-01 03:00:00");
        assert_eq!(TimeSlot::from_datetime(&dt), TimeSlot::EarlyMorning);

        let dt = parse_dt("2025-12-01 18:45:00");
        assert_eq!(TimeSlot::from_datetime(&dt), TimeSlot::EveningRush);
    }

    #[test]
    #[should_panic(expected = "Hour must be between 0 and 23")]
    fn test_time_slot_invalid_hour() {
        TimeSlot::from_hour(24);
    }

    #[test]
    fn test_time_slot_all() {
        let slots = TimeSlot::all();
        assert_eq!(slots.len(), 6);
        assert_eq!(slots[0], TimeSlot::EarlyMorning);
        assert_eq!(slots[5], TimeSlot::Night);
    }

    // -----------------------------------------------------------------------
    // DayType tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_day_type_weekday() {
        assert_eq!(DayType::from_weekday(Weekday::Mon), DayType::Weekday);
        assert_eq!(DayType::from_weekday(Weekday::Fri), DayType::Weekday);
    }

    #[test]
    fn test_day_type_weekend() {
        assert_eq!(DayType::from_weekday(Weekday::Sat), DayType::Weekend);
        assert_eq!(DayType::from_weekday(Weekday::Sun), DayType::Weekend);
    }

    #[test]
    fn test_day_type_from_datetime() {
        // 2025-12-01 is a Monday
        let dt = parse_dt("2025-12-01 08:00:00");
        assert_eq!(DayType::from_datetime(&dt), DayType::Weekday);

        // 2025-12-06 is a Saturday
        let dt = parse_dt("2025-12-06 10:00:00");
        assert_eq!(DayType::from_datetime(&dt), DayType::Weekend);
    }

    // -----------------------------------------------------------------------
    // DelayFactor tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_delay_factor_new() {
        let dt = parse_dt("2025-12-01 08:30:00");
        let df = DelayFactor::new(0.75, RoadType::M30, dt, 1006);

        assert!((df.value - 0.75).abs() < 0.001);
        assert_eq!(df.congestion, CongestionLevel::C);
        assert_eq!(df.road_type, RoadType::M30);
        assert_eq!(df.sensor_id, 1006);
    }

    #[test]
    fn test_delay_factor_time_slot() {
        let dt = parse_dt("2025-12-01 18:00:00");
        let df = DelayFactor::new(0.60, RoadType::Urban, dt, 3395);

        assert_eq!(df.time_slot(), TimeSlot::EveningRush);
    }

    #[test]
    fn test_delay_factor_day_type() {
        // Monday
        let dt = parse_dt("2025-12-01 08:00:00");
        let df = DelayFactor::new(0.80, RoadType::M30, dt, 1006);
        assert_eq!(df.day_type(), DayType::Weekday);

        // Saturday
        let dt = parse_dt("2025-12-06 08:00:00");
        let df = DelayFactor::new(0.90, RoadType::Urban, dt, 3395);
        assert_eq!(df.day_type(), DayType::Weekend);
    }

    // -----------------------------------------------------------------------
    // CellDelayReport tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_cell_delay_report_from_factors() {
        let dt1 = parse_dt("2025-12-01 08:00:00");
        let dt2 = parse_dt("2025-12-01 08:15:00");
        let dt3 = parse_dt("2025-12-01 08:30:00");

        // We use 0.85 + 0.75 + 0.95 = 2.55 / 3 = 0.85 → clearly level B
        let factors = vec![
            DelayFactor::new(0.85, RoadType::M30, dt1, 1006),
            DelayFactor::new(0.75, RoadType::M30, dt2, 1006),
            DelayFactor::new(0.95, RoadType::M30, dt3, 1006),
        ];

        let cell_id = CellID(0);
        let report = CellDelayReport::from_factors(cell_id, &factors);

        assert_eq!(report.observation_count, 3);
        assert!((report.avg_delay_factor - 0.85).abs() < 0.001);
        assert!((report.min_delay_factor - 0.75).abs() < 0.001);
        assert!((report.max_delay_factor - 0.95).abs() < 0.001);
        assert!(report.std_delay_factor > 0.0);
        assert_eq!(report.congestion_level, CongestionLevel::B);
        assert_eq!(report.road_type_dominant, RoadType::M30);
        assert_eq!(report.timestamp_start, dt1);
        assert_eq!(report.timestamp_end, dt3);
    }

    #[test]
    fn test_cell_delay_report_urban_dominant() {
        let dt = parse_dt("2025-12-01 08:00:00");
        let factors = vec![
            DelayFactor::new(0.60, RoadType::Urban, dt, 3395),
            DelayFactor::new(0.55, RoadType::Urban, dt, 3396),
            DelayFactor::new(0.65, RoadType::M30, dt, 1006),
        ];

        let report = CellDelayReport::from_factors(CellID(0), &factors);
        assert_eq!(report.road_type_dominant, RoadType::Urban);
    }

    #[test]
    fn test_cell_delay_report_is_congested() {
        let dt = parse_dt("2025-12-01 08:00:00");
        let factors = vec![
            DelayFactor::new(0.40, RoadType::Urban, dt, 3395),
            DelayFactor::new(0.45, RoadType::Urban, dt, 3396),
        ];

        let report = CellDelayReport::from_factors(CellID(0), &factors);
        assert!(report.is_congested());
    }

    #[test]
    fn test_cell_delay_report_should_subdivide() {
        let dt = parse_dt("2025-12-01 08:00:00");
        let factors = vec![
            DelayFactor::new(0.55, RoadType::Urban, dt, 3395),
            DelayFactor::new(0.50, RoadType::Urban, dt, 3396),
        ];

        let report = CellDelayReport::from_factors(CellID(0), &factors);
        // delay_threshold = 1.5 → srf_threshold = 1/1.5 ≈ 0.667
        // avg = 0.525 < 0.667 → should subdivide
        assert!(report.should_subdivide(1.5));
        // delay_threshold = 3.0 → srf_threshold = 0.333
        // avg = 0.525 > 0.333 → should NOT subdivide
        assert!(!report.should_subdivide(3.0));
    }

    #[test]
    fn test_cell_delay_report_std_deviation() {
        let dt = parse_dt("2025-12-01 08:00:00");
        // All equal → std = 0
        let factors = vec![
            DelayFactor::new(0.80, RoadType::M30, dt, 1006),
            DelayFactor::new(0.80, RoadType::M30, dt, 1006),
        ];

        let report = CellDelayReport::from_factors(CellID(0), &factors);
        assert!((report.std_delay_factor - 0.0).abs() < 0.001);
    }
}
