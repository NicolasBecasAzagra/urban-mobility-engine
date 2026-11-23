//! Temporal aggregations of the delay_factor per S2 cell
//!
//! Groups the computed `DelayFactor`s by time window
//! (hour, day of the week, time slot) and builds complete
//! time series per cell
//!
//! ## Example
//!
//! ```rust
//! use tfg::config::MetricsConfig;
//! use tfg::metrics::temporal::build_cell_time_series;
//! # use tfg::ingestion::model::{GpsPoint, RoadType, DataQuality};
//! # use chrono::NaiveDateTime;
//! # use s2::cellid::CellID;
//! #
//! # let config = MetricsConfig {
//! #     m30_reference_speed: 90.0,
//! #     urban_reference_speed: 50.0,
//! #     min_observations: 1,
//! # };
//! # let points = vec![GpsPoint {
//! #     id: 3395,
//! #     timestamp: NaiveDateTime::parse_from_str("2025-12-01 08:00:00", "%Y-%m-%d %H:%M:%S").unwrap(),
//! #     latitude: 40.45, longitude: -3.72,
//! #     road_type: RoadType::Urban, intensidad: 300, ocupacion: 15,
//! #     carga: 25, velocidad: None, quality: DataQuality::Normal,
//! # }];
//!
//! let series = build_cell_time_series(CellID(0), &points, &config);
//! println!("Hours with data: {}", series.hourly.len());
//! ```

use std::collections::HashMap;

use chrono::Weekday;
use s2::cellid::CellID;
use serde::{Deserialize, Serialize};

use crate::config::MetricsConfig;
use crate::ingestion::model::GpsPoint;
use crate::metrics::delay_factor::compute_delay_factor;
use crate::metrics::model::{CongestionLevel, DelayFactor, TimeSlot};

// ---------------------------------------------------------------------------
// TemporalAggregation — Result of grouping by time window
// ---------------------------------------------------------------------------

/// Result of aggregating delay_factors in a time window
///
/// Contains the congestion statistics for a specific
/// period (hour, day, slot) of an S2 cell
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemporalAggregation {
    /// Identifier of the S2 cell
    pub cell_id: CellID,
    /// Time slot of this aggregation
    pub time_slot: TimeSlot,
    /// Mean delay_factor in the time window
    pub avg_delay_factor: f64,
    /// Number of observations in the window
    pub observation_count: usize,
    /// Average congestion level
    pub congestion_level: CongestionLevel,
}

impl TemporalAggregation {
    /// Builds a temporal aggregation from a vector of `DelayFactor`s
    ///
    /// # Panics
    /// Panics if `factors` is empty
    pub fn from_factors(cell_id: CellID, time_slot: TimeSlot, factors: &[DelayFactor]) -> Self {
        assert!(!factors.is_empty(), "Cannot aggregate without factors");

        let count = factors.len();
        let avg: f64 = factors.iter().map(|f| f.value).sum::<f64>() / count as f64;

        TemporalAggregation {
            cell_id,
            time_slot,
            avg_delay_factor: avg,
            observation_count: count,
            congestion_level: CongestionLevel::from_srf(avg),
        }
    }

    /// Indicates whether this time window shows congestion (D, E or F)
    pub fn is_congested(&self) -> bool {
        self.congestion_level.is_congested()
    }
}

// ---------------------------------------------------------------------------
// CellTimeSeries — Complete time series of a cell
// ---------------------------------------------------------------------------

/// Complete delay_factor time series for an S2 cell
///
/// Groups the data along three time dimensions:
/// - By hour of the day (0–23)
/// - By day of the week (Mon–Sun)
/// - By time slot (EarlyMorning, MorningRush, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CellTimeSeries {
    /// Identifier of the S2 cell
    pub cell_id: CellID,
    /// Aggregation by hour of the day (key: 0–23)
    pub hourly: HashMap<u32, TemporalAggregation>,
    /// Aggregation by day of the week
    pub daily: HashMap<Weekday, TemporalAggregation>,
    /// Aggregation by time slot
    pub by_slot: HashMap<TimeSlot, TemporalAggregation>,
}

impl CellTimeSeries {
    /// Returns the hours of the day with congestion (level D, E or F)
    pub fn congested_hours(&self) -> Vec<u32> {
        let mut hours: Vec<u32> = self
            .hourly
            .iter()
            .filter(|(_, agg)| agg.is_congested())
            .map(|(h, _)| *h)
            .collect();
        hours.sort_unstable();
        hours
    }

    /// Returns the days of the week with congestion
    pub fn congested_days(&self) -> Vec<Weekday> {
        self.daily
            .iter()
            .filter(|(_, agg)| agg.is_congested())
            .map(|(d, _)| *d)
            .collect()
    }

    /// Returns the time slot with the highest congestion (lowest SRF)
    pub fn worst_time_slot(&self) -> Option<(&TimeSlot, &TemporalAggregation)> {
        self.by_slot.iter().min_by(|(_, a), (_, b)| {
            a.avg_delay_factor
                .partial_cmp(&b.avg_delay_factor)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
    }
}

// ---------------------------------------------------------------------------
// Temporal grouping functions
// ---------------------------------------------------------------------------

/// Groups GPS points by hour of the day and computes delay_factors
///
/// # Arguments
/// * `points` — GPS points to group
/// * `config` — Metrics configuration
///
/// # Returns
/// HashMap with key = hour (0–23) and value = vector of `DelayFactor`s
pub fn group_by_hour(
    points: &[GpsPoint],
    config: &MetricsConfig,
) -> HashMap<u32, Vec<DelayFactor>> {
    let mut groups: HashMap<u32, Vec<DelayFactor>> = HashMap::new();

    for point in points {
        if let Ok(df) = compute_delay_factor(point, config) {
            let hour = df
                .timestamp
                .format("%H")
                .to_string()
                .parse::<u32>()
                .unwrap();
            groups.entry(hour).or_default().push(df);
        }
    }

    groups
}

/// Groups GPS points by day of the week and computes delay_factors
///
/// # Arguments
/// * `points` — GPS points to group
/// * `config` — Metrics configuration
///
/// # Returns
/// HashMap with key = `Weekday` and value = vector of `DelayFactor`s
pub fn group_by_weekday(
    points: &[GpsPoint],
    config: &MetricsConfig,
) -> HashMap<Weekday, Vec<DelayFactor>> {
    let mut groups: HashMap<Weekday, Vec<DelayFactor>> = HashMap::new();

    for point in points {
        if let Ok(df) = compute_delay_factor(point, config) {
            groups.entry(df.weekday()).or_default().push(df);
        }
    }

    groups
}

/// Groups GPS points by time slot and computes delay_factors
///
/// # Arguments
/// * `points` — GPS points to group
/// * `config` — Metrics configuration
///
/// # Returns
/// HashMap with key = `TimeSlot` and value = vector of `DelayFactor`s
pub fn group_by_time_slot(
    points: &[GpsPoint],
    config: &MetricsConfig,
) -> HashMap<TimeSlot, Vec<DelayFactor>> {
    let mut groups: HashMap<TimeSlot, Vec<DelayFactor>> = HashMap::new();

    for point in points {
        if let Ok(df) = compute_delay_factor(point, config) {
            groups.entry(df.time_slot()).or_default().push(df);
        }
    }

    groups
}

/// Builds the complete time series of an S2 cell
///
/// Computes the delay_factors of all the points and groups them
/// by hour, day of the week and time slot, producing a
/// `CellTimeSeries` with all the time dimensions
///
/// # Arguments
/// * `cell_id` — ID of the S2 cell
/// * `points` — GPS points of the cell
/// * `config` — Metrics configuration
pub fn build_cell_time_series(
    cell_id: CellID,
    points: &[GpsPoint],
    config: &MetricsConfig,
) -> CellTimeSeries {
    // Group by hour
    let hourly_groups = group_by_hour(points, config);
    let hourly: HashMap<u32, TemporalAggregation> = hourly_groups
        .into_iter()
        .map(|(hour, factors)| {
            let slot = TimeSlot::from_hour(hour);
            (
                hour,
                TemporalAggregation::from_factors(cell_id, slot, &factors),
            )
        })
        .collect();

    // Group by day of the week
    let daily_groups = group_by_weekday(points, config);
    let daily: HashMap<Weekday, TemporalAggregation> = daily_groups
        .into_iter()
        .map(|(day, factors)| {
            // Use the most frequent time slot as the representative one
            let slot = most_frequent_slot(&factors);
            (
                day,
                TemporalAggregation::from_factors(cell_id, slot, &factors),
            )
        })
        .collect();

    // Group by time slot
    let slot_groups = group_by_time_slot(points, config);
    let by_slot: HashMap<TimeSlot, TemporalAggregation> = slot_groups
        .into_iter()
        .map(|(slot, factors)| {
            (
                slot,
                TemporalAggregation::from_factors(cell_id, slot, &factors),
            )
        })
        .collect();

    CellTimeSeries {
        cell_id,
        hourly,
        daily,
        by_slot,
    }
}

/// Determines the most frequent time slot in a set of factors
fn most_frequent_slot(factors: &[DelayFactor]) -> TimeSlot {
    let mut counts: HashMap<TimeSlot, usize> = HashMap::new();
    for f in factors {
        *counts.entry(f.time_slot()).or_insert(0) += 1;
    }
    counts
        .into_iter()
        .max_by_key(|(_, count)| *count)
        .map(|(slot, _)| slot)
        .unwrap_or(TimeSlot::EarlyMorning)
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
            min_observations: 1,
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

    /// Generates test points spread across several hours and days
    fn sample_points() -> Vec<GpsPoint> {
        vec![
            // Monday 08:00 — morning rush hour
            make_urban_point(3395, 40, "2025-12-01 08:00:00"),
            make_urban_point(3396, 50, "2025-12-01 08:30:00"),
            // Monday 12:00 — midday
            make_urban_point(3397, 15, "2025-12-01 12:00:00"),
            // Monday 18:00 — evening rush
            make_urban_point(3398, 60, "2025-12-01 18:00:00"),
            make_urban_point(3399, 55, "2025-12-01 18:30:00"),
            // Tuesday 08:00 — morning rush
            make_m30_point(1006, 70, 30, "2025-12-02 08:00:00"),
            // Saturday 03:00 — early morning
            make_urban_point(3400, 5, "2025-12-06 03:00:00"),
        ]
    }

    // -----------------------------------------------------------------------
    // group_by_hour
    // -----------------------------------------------------------------------

    #[test]
    fn test_group_by_hour() {
        let config = default_config();
        let points = sample_points();
        let groups = group_by_hour(&points, &config);

        // Hours present: 3, 8, 12, 18
        assert!(groups.contains_key(&8));
        assert!(groups.contains_key(&12));
        assert!(groups.contains_key(&18));
        assert!(groups.contains_key(&3));

        // Hour 8 has 3 points (2 urban on Monday + 1 M-30 on Tuesday)
        assert_eq!(groups[&8].len(), 3);

        // Hour 12 has 1 point
        assert_eq!(groups[&12].len(), 1);
    }

    #[test]
    fn test_group_by_hour_empty() {
        let config = default_config();
        let groups = group_by_hour(&[], &config);
        assert!(groups.is_empty());
    }

    // -----------------------------------------------------------------------
    // group_by_weekday
    // -----------------------------------------------------------------------

    #[test]
    fn test_group_by_weekday() {
        let config = default_config();
        let points = sample_points();
        let groups = group_by_weekday(&points, &config);

        // 2025-12-01 = Monday, 2025-12-02 = Tuesday, 2025-12-06 = Saturday
        assert!(groups.contains_key(&Weekday::Mon));
        assert!(groups.contains_key(&Weekday::Tue));
        assert!(groups.contains_key(&Weekday::Sat));

        // Monday: 5 points
        assert_eq!(groups[&Weekday::Mon].len(), 5);
        // Tuesday: 1 point
        assert_eq!(groups[&Weekday::Tue].len(), 1);
        // Saturday: 1 point
        assert_eq!(groups[&Weekday::Sat].len(), 1);
    }

    // -----------------------------------------------------------------------
    // group_by_time_slot
    // -----------------------------------------------------------------------

    #[test]
    fn test_group_by_time_slot() {
        let config = default_config();
        let points = sample_points();
        let groups = group_by_time_slot(&points, &config);

        assert!(groups.contains_key(&TimeSlot::MorningRush));
        assert!(groups.contains_key(&TimeSlot::Midday));
        assert!(groups.contains_key(&TimeSlot::EveningRush));
        assert!(groups.contains_key(&TimeSlot::EarlyMorning));

        // MorningRush: hour 8 → 3 points
        assert_eq!(groups[&TimeSlot::MorningRush].len(), 3);
        // EveningRush: hour 18 → 2 points
        assert_eq!(groups[&TimeSlot::EveningRush].len(), 2);
    }

    // -----------------------------------------------------------------------
    // build_cell_time_series
    // -----------------------------------------------------------------------

    #[test]
    fn test_build_time_series() {
        let config = default_config();
        let points = sample_points();
        let cell_id = CellID(0);

        let series = build_cell_time_series(cell_id, &points, &config);

        // Check the hours
        assert_eq!(series.hourly.len(), 4); // 3, 8, 12, 18
        assert!(series.hourly.contains_key(&8));
        assert!(series.hourly.contains_key(&18));

        // Check the days
        assert_eq!(series.daily.len(), 3); // Mon, Tue, Sat

        // Check the slots
        assert!(series.by_slot.len() >= 3); // MorningRush, Midday, EveningRush, EarlyMorning
    }

    #[test]
    fn test_time_series_congested_hours() {
        let config = default_config();
        // Points with high load at 8 and low at 3
        let points = vec![
            make_urban_point(3395, 70, "2025-12-01 08:00:00"), // SRF = 0.30 → F
            make_urban_point(3396, 65, "2025-12-01 08:30:00"), // SRF = 0.35 → E
            make_urban_point(3397, 5, "2025-12-01 03:00:00"),  // SRF = 0.95 → A
        ];

        let series = build_cell_time_series(CellID(0), &points, &config);
        let congested = series.congested_hours();

        assert!(congested.contains(&8), "Hour 8 must be congested");
        assert!(!congested.contains(&3), "Hour 3 must not be congested");
    }

    #[test]
    fn test_time_series_worst_slot() {
        let config = default_config();
        let points = vec![
            make_urban_point(3395, 10, "2025-12-01 12:00:00"), // SRF = 0.90 (good)
            make_urban_point(3396, 70, "2025-12-01 18:00:00"), // SRF = 0.30 (bad)
            make_urban_point(3397, 75, "2025-12-01 18:30:00"), // SRF = 0.25 (worst)
        ];

        let series = build_cell_time_series(CellID(0), &points, &config);
        let (worst_slot, worst_agg) = series.worst_time_slot().unwrap();

        assert_eq!(*worst_slot, TimeSlot::EveningRush);
        assert!(worst_agg.avg_delay_factor < 0.35);
    }

    // -----------------------------------------------------------------------
    // TemporalAggregation
    // -----------------------------------------------------------------------

    #[test]
    fn test_temporal_aggregation_from_factors() {
        let dt1 = parse_dt("2025-12-01 08:00:00");
        let dt2 = parse_dt("2025-12-01 08:15:00");
        let factors = vec![
            DelayFactor::new(0.80, RoadType::Urban, dt1, 3395),
            DelayFactor::new(0.60, RoadType::Urban, dt2, 3396),
        ];

        let agg = TemporalAggregation::from_factors(CellID(0), TimeSlot::MorningRush, &factors);

        assert_eq!(agg.observation_count, 2);
        assert!((agg.avg_delay_factor - 0.70).abs() < 0.001);
        assert_eq!(agg.congestion_level, CongestionLevel::C);
        assert_eq!(agg.time_slot, TimeSlot::MorningRush);
    }

    #[test]
    fn test_temporal_aggregation_is_congested() {
        let dt = parse_dt("2025-12-01 08:00:00");
        let factors = vec![
            DelayFactor::new(0.40, RoadType::Urban, dt, 3395),
            DelayFactor::new(0.30, RoadType::Urban, dt, 3396),
        ];

        let agg = TemporalAggregation::from_factors(CellID(0), TimeSlot::MorningRush, &factors);

        assert!(agg.is_congested());
    }

    // -----------------------------------------------------------------------
    // CellTimeSeries with empty data
    // -----------------------------------------------------------------------

    #[test]
    fn test_time_series_empty_points() {
        let config = default_config();
        let series = build_cell_time_series(CellID(0), &[], &config);

        assert!(series.hourly.is_empty());
        assert!(series.daily.is_empty());
        assert!(series.by_slot.is_empty());
    }

    #[test]
    fn test_time_series_congested_days() {
        let config = default_config();
        let points = vec![
            // Monday — congested
            make_urban_point(3395, 80, "2025-12-01 08:00:00"), // SRF = 0.20
            // Saturday — free
            make_urban_point(3396, 5, "2025-12-06 10:00:00"), // SRF = 0.95
        ];

        let series = build_cell_time_series(CellID(0), &points, &config);
        let congested_days = series.congested_days();

        assert!(congested_days.contains(&Weekday::Mon));
        assert!(!congested_days.contains(&Weekday::Sat));
    }
}
