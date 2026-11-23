//! Polling scheduler for near-real-time mode
//!
//! Runs periodic ingestion → indexing → metric computation cycles
//! on a [`DataSource`](super::datasource::DataSource),
//! incrementally updating the S2 index
//!
//! ## Lifecycle of a tick
//!
//! ```text
//! ┌──────────────────────────────────────────────────┐
//! │ 1. fetch_batch()  → Vec<GpsPoint>                │
//! │ 2. insert_many()  → updated S2Index               │
//! │ 3. compute_all()  → Vec<CellDelayReport>          │
//! │ 4. TickReport     → cycle metrics                 │
//! └──────────────────────────────────────────────────┘
//!         ↓ sleep(poll_interval) ↓
//!       next tick...
//! ```
//!
//! ## Example
//!
//! ```rust,no_run
//! use tfg::ingestion::scheduler::PollingScheduler;
//! use tfg::ingestion::live_api::LiveApiSource;
//! use tfg::config::{MetricsConfig, GeospatialConfig};
//!
//! let source = LiveApiSource::new(
//!     "https://informo.madrid.es/informo/tmadrid/pm.xml",
//!     "data/samples/sample_locations.csv",
//! ).unwrap();
//!
//! let geo_config = GeospatialConfig {
//!     base_level: 15,
//!     subdivision_level: 17,
//!     delay_threshold: 1.5,
//!     min_gps_count: 50,
//! };
//!
//! let config = MetricsConfig {
//!     m30_reference_speed: 90.0,
//!     urban_reference_speed: 50.0,
//!     min_observations: 1,
//! };
//!
//! let mut scheduler = PollingScheduler::new(Box::new(source), geo_config, config);
//! let reports = scheduler.run(Some(3)).unwrap(); // 3 ticks
//! println!("Executed {} ticks", reports.len());
//! ```

use std::time::{Duration, Instant};

use chrono::NaiveDateTime;

use crate::config::{GeospatialConfig, MetricsConfig};
use crate::geospatial::s2_index::S2Index;
use crate::ingestion::datasource::DataSource;
use crate::ingestion::error::IngestionError;
use crate::metrics::anomaly::{AnomalyAlert, AnomalyDetector};
use crate::metrics::delay_factor::compute_all_cells;
use crate::metrics::model::CellDelayReport;

// ---------------------------------------------------------------------------
// TickReport — result of a polling cycle
// ---------------------------------------------------------------------------

/// Result of one polling scheduler cycle (tick)
///
/// Contains the cycle metrics: how many points were obtained,
/// how many cells are in the index, and the delay_factor reports
#[derive(Debug, Clone)]
pub struct TickReport {
    /// Tick number (1-indexed)
    pub tick_number: usize,
    /// Timestamp of the moment the tick ran
    pub timestamp: NaiveDateTime,
    /// GPS points obtained in this tick
    pub points_fetched: usize,
    /// Total points in the S2 index (cumulative)
    pub total_points_indexed: usize,
    /// Total number of S2 cells with data
    pub total_cells: usize,
    /// Per-cell delay_factor reports
    pub cell_reports: Vec<CellDelayReport>,
    /// Anomaly alerts detected in this tick
    /// Empty if no detector is configured or if there were no anomalies
    pub anomaly_alerts: Vec<AnomalyAlert>,
    /// Duration of the tick (fetch + index + compute)
    pub duration: Duration,
}

impl TickReport {
    /// Returns the number of congested cells (SRF < 0.5)
    pub fn congested_cells(&self) -> usize {
        self.cell_reports
            .iter()
            .filter(|r| r.avg_delay_factor < 0.5)
            .count()
    }
}

impl std::fmt::Display for TickReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Tick #{}: {} new points, {} total, {} cells, {} reports ({} congested) [{:.1}s]",
            self.tick_number,
            self.points_fetched,
            self.total_points_indexed,
            self.total_cells,
            self.cell_reports.len(),
            self.congested_cells(),
            self.duration.as_secs_f64()
        )
    }
}

// ---------------------------------------------------------------------------
// PollingScheduler
// ---------------------------------------------------------------------------

/// Scheduler that runs periodic ingestion and computation cycles
///
/// Keeps a persistent `S2Index` that is updated incrementally
/// with the data of each tick (new points are added without
/// rebuilding the index)
pub struct PollingScheduler {
    /// Data source (CSV or API)
    source: Box<dyn DataSource>,
    /// Persistent S2 index (grows with every tick)
    index: S2Index,
    /// Geospatial configuration (S2 levels)
    geospatial_config: GeospatialConfig,
    /// Metrics configuration
    metrics_config: MetricsConfig,
    /// Counter of executed ticks
    tick_count: usize,
    /// Optional anomaly detector
    anomaly_detector: Option<AnomalyDetector>,
}

impl PollingScheduler {
    /// Creates a new scheduler with a data source and S2 level
    ///
    /// # Arguments
    /// * `source` — Data source (implements `DataSource`)
    /// * `geospatial` — Geospatial configuration (S2 levels)
    /// * `metrics` — Metrics configuration
    pub fn new(
        source: Box<dyn DataSource>,
        geospatial_config: GeospatialConfig,
        metrics_config: MetricsConfig,
    ) -> Self {
        tracing::info!(
            "PollingScheduler created: source='{}', S2 base_level={}, min_obs={}",
            source.name(),
            geospatial_config.base_level,
            metrics_config.min_observations
        );

        PollingScheduler {
            source,
            index: S2Index::new(geospatial_config.base_level),
            geospatial_config,
            metrics_config,
            tick_count: 0,
            anomaly_detector: None,
        }
    }

    /// Attaches an anomaly detector to the scheduler
    ///
    /// The generated alerts will be included in the `anomaly_alerts` field
    /// of every `TickReport` produced
    pub fn with_anomaly_detector(mut self, detector: AnomalyDetector) -> Self {
        self.anomaly_detector = Some(detector);
        self
    }

    /// Runs one cycle: fetch → index → compute
    ///
    /// New points are inserted incrementally into the existing
    /// S2Index (it is not rebuilt). Metrics are recomputed
    /// over all the accumulated data
    ///
    /// # Returns
    /// `TickReport` with the cycle metrics
    pub fn tick(&mut self) -> Result<TickReport, IngestionError> {
        let start = Instant::now();
        self.tick_count += 1;
        let tick_num = self.tick_count;

        tracing::info!("--- Tick #{} started ---", tick_num);

        // 1. Fetch: obtain new points
        let points = self.source.fetch_batch()?;
        let points_fetched = points.len();

        tracing::info!(
            "Tick #{}: {} points obtained from '{}'",
            tick_num,
            points_fetched,
            self.source.name()
        );

        // 2. Index: insert incrementally into the S2Index
        self.index.insert_many(points);

        // 2.5. Adaptive subdivision
        let max_level = self.geospatial_config.subdivision_level;
        let mut subdivision_passes = 0;

        loop {
            let temp_reports = compute_all_cells(&self.index, &self.metrics_config);
            let congested: Vec<s2::cellid::CellID> = temp_reports
                .iter()
                .filter(|r| r.avg_delay_factor < 0.65 && r.cell_id.level() < max_level)
                .map(|r| r.cell_id)
                .collect();

            if congested.is_empty() {
                break;
            }
            self.index.subdivide_cells(&congested);
            subdivision_passes += 1;
        }

        tracing::info!(
            "Tick #{}: S2 index → {} total points, {} cells ({} subdivision passes)",
            tick_num,
            self.index.len(),
            self.index.cell_count(),
            subdivision_passes
        );

        // 3. Compute: recompute metrics over the whole index
        let cell_reports = compute_all_cells(&self.index, &self.metrics_config);

        // 4. Detect anomalies (if a detector is configured)
        let anomaly_alerts = if let Some(ref detector) = self.anomaly_detector {
            let alerts = detector.detect(&cell_reports);
            tracing::info!("Tick #{}: {} anomalies detected", tick_num, alerts.len());
            alerts
        } else {
            Vec::new()
        };

        let duration = start.elapsed();

        let report = TickReport {
            tick_number: tick_num,
            timestamp: chrono::Local::now().naive_local(),
            points_fetched,
            total_points_indexed: self.index.len(),
            total_cells: self.index.cell_count(),
            cell_reports,
            anomaly_alerts,
            duration,
        };

        tracing::info!("{}", report);

        Ok(report)
    }

    /// Runs the polling loop with the configured interval
    ///
    /// Stops after `max_ticks` cycles (if specified)
    /// or when the source has no more data (`has_more() == false`)
    ///
    /// Between ticks, sleeps for the interval the source indicates
    /// (`refresh_interval()`). If the source has no interval
    /// (e.g., CSV), it does not sleep
    ///
    /// # Arguments
    /// * `max_ticks` — Maximum number of ticks to run (`None` = unbounded)
    ///
    /// # Returns
    /// Vector of `TickReport`s of all executed ticks
    pub fn run(&mut self, max_ticks: Option<usize>) -> Result<Vec<TickReport>, IngestionError> {
        let mut reports = Vec::new();
        let interval = self.source.refresh_interval();

        tracing::info!(
            "PollingScheduler::run: source='{}', max_ticks={:?}, interval={:?}",
            self.source.name(),
            max_ticks,
            interval
        );

        loop {
            // Stop condition: max_ticks reached
            if let Some(max) = max_ticks {
                if self.tick_count >= max {
                    tracing::info!("Max ticks ({}) reached. Polling finished.", max);
                    break;
                }
            }

            // Stop condition: source exhausted (CSV)
            if !self.source.has_more() {
                tracing::info!(
                    "Source '{}' exhausted. Polling finished.",
                    self.source.name()
                );
                break;
            }

            // Run the tick
            let report = self.tick()?;
            reports.push(report);

            // Sleep between ticks (only if there is an interval and we will keep iterating)
            if let Some(interval) = interval {
                let should_continue =
                    self.source.has_more() && max_ticks.is_none_or(|max| self.tick_count < max);

                if should_continue {
                    tracing::info!("Waiting {}s until the next tick...", interval.as_secs());
                    std::thread::sleep(interval);
                }
            }
        }

        tracing::info!(
            "PollingScheduler: {} ticks completed, {} total points in index",
            reports.len(),
            self.index.len()
        );

        Ok(reports)
    }

    /// Returns a reference to the current S2Index
    pub fn index(&self) -> &S2Index {
        &self.index
    }

    /// Returns the number of executed ticks
    pub fn tick_count(&self) -> usize {
        self.tick_count
    }

    /// Returns the name of the data source
    pub fn source_name(&self) -> &str {
        self.source.name()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ingestion::model::{DataQuality, GpsPoint, RoadType};

    // --- Mock DataSource for tests ---

    struct MockSource {
        batches: Vec<Vec<GpsPoint>>,
        cursor: usize,
    }

    impl MockSource {
        fn new(batches: Vec<Vec<GpsPoint>>) -> Self {
            MockSource { batches, cursor: 0 }
        }
    }

    impl DataSource for MockSource {
        fn name(&self) -> &str {
            "Mock Source"
        }

        fn fetch_batch(&mut self) -> Result<Vec<GpsPoint>, IngestionError> {
            if self.cursor < self.batches.len() {
                let batch = self.batches[self.cursor].clone();
                self.cursor += 1;
                Ok(batch)
            } else {
                Ok(vec![])
            }
        }

        fn has_more(&self) -> bool {
            self.cursor < self.batches.len()
        }

        fn refresh_interval(&self) -> Option<Duration> {
            None // No sleep in tests
        }
    }

    // --- Helpers ---

    fn make_point(id: u32, lat: f64, lon: f64, carga: i32) -> GpsPoint {
        GpsPoint {
            id,
            timestamp: chrono::NaiveDateTime::parse_from_str(
                "2025-12-01 08:00:00",
                "%Y-%m-%d %H:%M:%S",
            )
            .unwrap(),
            latitude: lat,
            longitude: lon,
            road_type: RoadType::Urban,
            intensidad: 300,
            ocupacion: 15,
            carga,
            velocidad: None,
            quality: DataQuality::Normal,
        }
    }

    fn test_config() -> MetricsConfig {
        MetricsConfig {
            m30_reference_speed: 90.0,
            urban_reference_speed: 50.0,
            min_observations: 1,
        }
    }

    fn test_geospatial_config() -> GeospatialConfig {
        GeospatialConfig {
            base_level: 15,
            subdivision_level: 17,
            delay_threshold: 1.5,
            min_gps_count: 50,
        }
    }

    fn make_batch(ids: &[(u32, i32)]) -> Vec<GpsPoint> {
        ids.iter()
            .map(|&(id, carga)| make_point(id, 40.42 + (id as f64) * 0.001, -3.70, carga))
            .collect()
    }

    // --- Tick tests ---

    #[test]
    fn test_scheduler_single_tick() {
        let batch = make_batch(&[(1, 30), (2, 50), (3, 70)]);
        let source = MockSource::new(vec![batch]);

        let mut scheduler =
            PollingScheduler::new(Box::new(source), test_geospatial_config(), test_config());

        let report = scheduler.tick().unwrap();
        assert_eq!(report.tick_number, 1);
        assert_eq!(report.points_fetched, 3);
        assert_eq!(report.total_points_indexed, 3);
        assert!(report.total_cells > 0);
    }

    #[test]
    fn test_scheduler_multiple_ticks() {
        let batch1 = make_batch(&[(1, 30), (2, 50)]);
        let batch2 = make_batch(&[(3, 70), (4, 90)]);
        let source = MockSource::new(vec![batch1, batch2]);

        let mut scheduler =
            PollingScheduler::new(Box::new(source), test_geospatial_config(), test_config());

        let r1 = scheduler.tick().unwrap();
        assert_eq!(r1.tick_number, 1);
        assert_eq!(r1.points_fetched, 2);
        assert_eq!(r1.total_points_indexed, 2);

        let r2 = scheduler.tick().unwrap();
        assert_eq!(r2.tick_number, 2);
        assert_eq!(r2.points_fetched, 2);
        assert_eq!(r2.total_points_indexed, 4); // Incremental: 2+2=4
    }

    #[test]
    fn test_scheduler_incremental_index() {
        let batch1 = make_batch(&[(1, 30)]);
        let batch2 = make_batch(&[(2, 50)]);
        let batch3 = make_batch(&[(3, 70)]);
        let source = MockSource::new(vec![batch1, batch2, batch3]);

        let mut scheduler =
            PollingScheduler::new(Box::new(source), test_geospatial_config(), test_config());

        scheduler.tick().unwrap();
        assert_eq!(scheduler.index().len(), 1);

        scheduler.tick().unwrap();
        assert_eq!(scheduler.index().len(), 2);

        scheduler.tick().unwrap();
        assert_eq!(scheduler.index().len(), 3);
    }

    // --- Run tests ---

    #[test]
    fn test_scheduler_run_with_max_ticks() {
        let batches: Vec<Vec<GpsPoint>> = (0..5)
            .map(|i| make_batch(&[(i as u32, 30 + i * 10)]))
            .collect();
        let source = MockSource::new(batches);

        let mut scheduler =
            PollingScheduler::new(Box::new(source), test_geospatial_config(), test_config());
        let reports = scheduler.run(Some(3)).unwrap();

        assert_eq!(reports.len(), 3);
        assert_eq!(scheduler.tick_count(), 3);
        assert_eq!(scheduler.index().len(), 3);
    }

    #[test]
    fn test_scheduler_run_exhausts_source() {
        let batch1 = make_batch(&[(1, 30)]);
        let batch2 = make_batch(&[(2, 50)]);
        let source = MockSource::new(vec![batch1, batch2]);

        let mut scheduler =
            PollingScheduler::new(Box::new(source), test_geospatial_config(), test_config());
        let reports = scheduler.run(None).unwrap(); // No limit → stops on exhaustion

        assert_eq!(reports.len(), 2);
        assert_eq!(scheduler.index().len(), 2);
    }

    #[test]
    fn test_scheduler_run_empty_source() {
        let source = MockSource::new(vec![]);

        let mut scheduler =
            PollingScheduler::new(Box::new(source), test_geospatial_config(), test_config());
        let reports = scheduler.run(None).unwrap();

        assert!(reports.is_empty());
        assert_eq!(scheduler.index().len(), 0);
    }

    // --- TickReport tests ---

    #[test]
    fn test_tick_report_display() {
        let batch = make_batch(&[(1, 30), (2, 50)]);
        let source = MockSource::new(vec![batch]);

        let mut scheduler =
            PollingScheduler::new(Box::new(source), test_geospatial_config(), test_config());
        let report = scheduler.tick().unwrap();

        let display = format!("{}", report);
        assert!(display.contains("Tick #1"));
        assert!(display.contains("2 new points"));
    }

    #[test]
    fn test_tick_report_congested_cells() {
        // High load (90) → low SRF → congested cell
        let batch = make_batch(&[(1, 90), (2, 90), (3, 90), (4, 90), (5, 90)]);
        let source = MockSource::new(vec![batch]);

        let mut scheduler =
            PollingScheduler::new(Box::new(source), test_geospatial_config(), test_config());
        let report = scheduler.tick().unwrap();

        // With load 90 → SRF = 1.0 - 0.9 = 0.1 < 0.5 → congested
        assert!(report.congested_cells() > 0 || report.cell_reports.is_empty());
    }

    // --- Accessor tests ---

    #[test]
    fn test_scheduler_accessors() {
        let source = MockSource::new(vec![make_batch(&[(1, 30)])]);
        let scheduler =
            PollingScheduler::new(Box::new(source), test_geospatial_config(), test_config());

        assert_eq!(scheduler.tick_count(), 0);
        assert_eq!(scheduler.source_name(), "Mock Source");
        assert!(scheduler.index().is_empty());
    }
}
