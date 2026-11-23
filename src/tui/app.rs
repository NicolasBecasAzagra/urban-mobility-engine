//! Global state of the TUI application
//!
//! `App` is the central struct holding all TUI state:
//! the current phase, the tick history, the countdown to the next tick,
//! and the data of the last tick received

use std::collections::VecDeque;
use std::time::Instant;

use chrono::NaiveDateTime;

use anyhow::Result;

use crate::config::Config;
use crate::geospatial::s2_index::S2Index;
use crate::ingestion::csv_parser;
use crate::ingestion::live_api::LiveApiSource;
use crate::ingestion::scheduler::{PollingScheduler, TickReport};
use crate::metrics::{anomaly::AnomalyDetector, statistics};
use std::path::Path;

/// Interval in seconds between API ticks (for the countdown)
pub const DEFAULT_POLL_INTERVAL_SECS: u64 = 300; // 5 minutes

/// Maximum number of entries in the tick history
pub const MAX_TICK_HISTORY: usize = 10;

/// Main menu options
pub const MENU_OPTIONS: &[&str] = &[
    "Start real-time analysis",
    "Export latest results",
    "View performance metrics",
    "Quit",
];

// ---------------------------------------------------------------------------
// AppPhase
// ---------------------------------------------------------------------------

/// Current phase of the TUI application
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppPhase {
    /// Start menu (before connecting to the API)
    Menu,
    /// Live panel (during real-time analysis)
    Live,
}

/// Selected export format
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportChoice {
    Json,
    Csv,
    Geojson,
}

// ---------------------------------------------------------------------------
// TickSummary — compact summary of a tick for the history
// ---------------------------------------------------------------------------

/// Compact summary of a tick to show in the TUI history
#[derive(Debug, Clone)]
pub struct TickSummary {
    pub tick_number: usize,
    pub points_fetched: usize,
    pub total_cells: usize,
    pub alerts: usize,
    pub avg_srf: f64,
}

impl From<&TickReport> for TickSummary {
    fn from(r: &TickReport) -> Self {
        let avg_srf = if r.cell_reports.is_empty() {
            1.0
        } else {
            r.cell_reports
                .iter()
                .map(|c| c.avg_delay_factor)
                .sum::<f64>()
                / r.cell_reports.len() as f64
        };
        TickSummary {
            tick_number: r.tick_number,
            points_fetched: r.points_fetched,
            total_cells: r.total_cells,
            alerts: r.anomaly_alerts.len(),
            avg_srf,
        }
    }
}

// ---------------------------------------------------------------------------
// App — global state
// ---------------------------------------------------------------------------

/// Global state of the TUI application
pub struct App {
    // Phase and control
    /// Current phase: menu or live panel
    pub phase: AppPhase,
    /// Selected option in the menu (0-indexed)
    pub menu_selection: usize,
    /// The user has requested to quit
    pub should_quit: bool,
    /// A menu action is pending execution
    pub menu_action_pending: bool,
    /// Status message to show in the menu
    pub status_message: Option<String>,

    // Live data
    /// Last TickReport received from the scheduler
    pub last_tick: Option<TickReport>,
    /// History of the last MAX_TICK_HISTORY ticks
    pub tick_history: VecDeque<TickSummary>,
    /// Seconds until the next tick (countdown)
    pub countdown_secs: u64,
    /// Total duration of the polling interval in seconds
    pub poll_interval_secs: u64,
    /// Moment at which the next tick will run
    pub next_tick_at: Option<Instant>,
    /// The scheduler is initialized and ready
    pub scheduler_ready: bool,
    /// A tick is pending execution
    pub tick_pending: bool,
    /// The analysis is paused
    pub paused: bool,
    /// Timestamp of the last Madrid API update
    pub last_api_update: Option<NaiveDateTime>,
    /// The user is choosing an export format
    pub export_choosing: bool,
    /// Selected option in the export submenu (0=JSON, 1=CSV, 2=GeoJSON)
    pub export_selection: usize,
    /// Instant at which the last status_message was shown (to auto-clear it)
    pub status_shown_at: Option<Instant>,

    /// Internal scheduler (only available in the Live phase)
    pub(crate) scheduler: Option<PollingScheduler>,
    /// Number of ticks executed in the session
    pub ticks_executed: usize,
}

impl App {
    /// Creates a new `App` in its initial state (menu)
    pub fn new() -> Self {
        App {
            phase: AppPhase::Menu,
            menu_selection: 0,
            should_quit: false,
            menu_action_pending: false,
            status_message: None,
            last_tick: None,
            tick_history: VecDeque::with_capacity(MAX_TICK_HISTORY),
            countdown_secs: 0,
            poll_interval_secs: DEFAULT_POLL_INTERVAL_SECS,
            next_tick_at: None,
            scheduler_ready: false,
            tick_pending: false,
            paused: false,
            last_api_update: None,
            export_choosing: false,
            export_selection: 0,
            status_shown_at: None,
            scheduler: None,
            ticks_executed: 0,
        }
    }

    /// Shows a temporary message in the controls bar
    pub fn set_status(&mut self, msg: &str) {
        self.status_message = Some(msg.to_string());
        self.status_shown_at = Some(Instant::now());
    }

    /// Clears the status_message if it has been visible for more than 3 seconds
    pub fn clear_stale_status(&mut self) {
        if let Some(shown_at) = self.status_shown_at {
            if shown_at.elapsed() > std::time::Duration::from_secs(3) {
                self.status_message = None;
                self.status_shown_at = None;
            }
        }
    }

    /// Moves up in the menu
    pub fn menu_up(&mut self) {
        if self.menu_selection > 0 {
            self.menu_selection -= 1;
        } else {
            self.menu_selection = MENU_OPTIONS.len() - 1;
        }
    }

    /// Moves down in the menu
    pub fn menu_down(&mut self) {
        self.menu_selection = (self.menu_selection + 1) % MENU_OPTIONS.len();
    }

    /// Starts the transition to live mode
    ///
    /// Initializes the scheduler with the live source and the anomaly detector
    pub fn start_live_mode(&mut self, config: &Config) -> Result<()> {
        let live_config = config
            .live
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Missing [live] section in config.toml"))?;

        self.status_message = Some("Loading history and connecting to the API...".to_string());

        // Create LiveApiSource
        let source = LiveApiSource::new(&live_config.endpoint, &live_config.locations_path)
            .map_err(|e| anyhow::anyhow!("Error creating LiveApiSource: {}", e))?;

        // Build the base scheduler
        let scheduler_base = PollingScheduler::new(
            Box::new(source),
            config.geospatial.clone(),
            config.metrics.clone(),
        );

        // Try a silent warm-start if the file exists
        let mut scheduler = scheduler_base;
        let warm_path = Path::new("data/raw/06-2025.csv");
        if warm_path.exists() {
            if let Ok(records) = csv_parser::parse_traffic_csv(warm_path) {
                let loc_path = Path::new(&live_config.locations_path);
                if let Ok(locations) = csv_parser::load_locations_csv(loc_path) {
                    let points = csv_parser::build_gps_points(&records, &locations);
                    let idx = S2Index::build_from_iterator(points, config.geospatial.base_level);
                    let hist_stats = statistics::compute_all_statistics(&idx, &config.metrics);
                    let detector = AnomalyDetector::from_statistics(&hist_stats);
                    scheduler = scheduler.with_anomaly_detector(detector);
                }
            }
        }

        self.scheduler = Some(scheduler);
        self.poll_interval_secs = live_config.poll_interval_secs;
        self.scheduler_ready = true;
        self.phase = AppPhase::Live;
        self.status_message = None;

        // Run the first tick immediately
        self.tick_pending = true;

        Ok(())
    }

    /// Runs a scheduler tick and updates the TUI state
    pub fn execute_tick(&mut self, _config: &Config) -> Result<()> {
        if self.paused {
            return Ok(());
        }

        let scheduler = match self.scheduler.as_mut() {
            Some(s) => s,
            None => return Ok(()),
        };

        match scheduler.tick() {
            Ok(report) => {
                // Update the history
                let summary = TickSummary::from(&report);
                if self.tick_history.len() >= MAX_TICK_HISTORY {
                    self.tick_history.pop_back();
                }
                self.tick_history.push_front(summary);

                // Update the last tick
                self.last_api_update = Some(report.timestamp);
                self.last_tick = Some(report);
                self.ticks_executed += 1;

                // Schedule the next tick
                self.next_tick_at =
                    Some(Instant::now() + std::time::Duration::from_secs(self.poll_interval_secs));
                self.countdown_secs = self.poll_interval_secs;
            }
            Err(e) => {
                self.status_message = Some(format!("Tick error: {}", e));
            }
        }

        Ok(())
    }

    /// Computes the countdown percentage for the progress bar
    ///
    /// Returns a value between 0.0 (next tick) and 1.0 (just happened)
    pub fn countdown_ratio(&self) -> f64 {
        if self.poll_interval_secs == 0 {
            return 0.0;
        }
        self.countdown_secs as f64 / self.poll_interval_secs as f64
    }

    /// Returns the mean SRF of the last tick, or 1.0 if there is no data
    pub fn last_avg_srf(&self) -> f64 {
        self.last_tick
            .as_ref()
            .map(|r| {
                if r.cell_reports.is_empty() {
                    1.0
                } else {
                    r.cell_reports
                        .iter()
                        .map(|c| c.avg_delay_factor)
                        .sum::<f64>()
                        / r.cell_reports.len() as f64
                }
            })
            .unwrap_or(1.0)
    }

    /// Returns the number of alerts of the last tick
    pub fn last_alert_count(&self) -> usize {
        self.last_tick
            .as_ref()
            .map(|r| r.anomaly_alerts.len())
            .unwrap_or(0)
    }

    /// Returns the number of congested cells of the last tick
    pub fn last_congested_cells(&self) -> usize {
        self.last_tick
            .as_ref()
            .map(|r| r.congested_cells())
            .unwrap_or(0)
    }

    /// Toggles between pause and resume
    pub fn toggle_pause(&mut self) {
        self.paused = !self.paused;
    }
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}
