//! Progress bars and visual logging module for the CLI
//!
//! Provides helpers on top of `indicatif` to show consistent
//! progress across all pipeline phases:
//!
//! - [`pipeline_bar`] — Numeric progress bar for N-step phases
//! - [`spinner`] — Animated spinner for operations of unknown duration
//! - [`phase_header`] — Prints a phase header with its number and name
//! - [`phase_done`] — Marks a phase as completed with a summary
//! - [`tick_summary`] — Prints the summary of a live scheduler tick

use std::time::Duration;

use indicatif::{ProgressBar, ProgressStyle};

// ---------------------------------------------------------------------------
// Predefined styles
// ---------------------------------------------------------------------------

/// Style for numeric progress bars
const BAR_STYLE: &str =
    "  {spinner:.cyan} [{bar:40.cyan/blue}] {pos}/{len} {msg} [{elapsed_precise}]";

/// Style for spinners of unknown duration
const SPINNER_STYLE: &str = "  {spinner:.cyan} {msg}";

/// Spinner frame set (braille)
const SPINNER_FRAMES: &[&str] = &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

// ---------------------------------------------------------------------------
// Public functions
// ---------------------------------------------------------------------------

/// Creates a numeric progress bar with `total` steps
///
/// Ideal for operations with a known number of items
/// (e.g., loading N CSV records, indexing N points)
///
/// # Example
/// ```rust,no_run
/// use tfg::progress::pipeline_bar;
///
/// let bar = pipeline_bar(6163, "CSV records");
/// for _ in 0..6163 {
///     bar.inc(1);
/// }
/// bar.finish_with_message("loaded");
/// ```
pub fn pipeline_bar(total: u64, unit: &str) -> ProgressBar {
    let bar = ProgressBar::new(total);
    bar.set_style(
        ProgressStyle::with_template(BAR_STYLE)
            .unwrap()
            .progress_chars("=>-"),
    );
    bar.set_message(unit.to_string());
    bar.enable_steady_tick(Duration::from_millis(80));
    bar
}

/// Creates an animated spinner for operations of unknown duration
///
/// Ideal for HTTP requests, compilation, etc
///
/// # Example
/// ```rust,no_run
/// use tfg::progress::spinner;
///
/// let sp = spinner("Connecting to the Madrid API...");
/// // ... long operation ...
/// sp.finish_with_message("✓ 6021 points fetched");
/// ```
pub fn spinner(msg: &str) -> ProgressBar {
    let bar = ProgressBar::new_spinner();
    bar.set_style(
        ProgressStyle::with_template(SPINNER_STYLE)
            .unwrap()
            .tick_strings(SPINNER_FRAMES),
    );
    bar.set_message(msg.to_string());
    bar.enable_steady_tick(Duration::from_millis(80));
    bar
}

/// Prints the header of a pipeline phase
///
/// Format: `▶ Phase N [US-XX]: Phase name`
pub fn phase_header(number: u8, us: &str, name: &str) {
    println!();
    println!("▶ Phase {} [{}]: {}", number, us, name);
}

/// Prints the completion of a phase with a summary message
///
/// Format: `  ↳ message`
pub fn phase_done(summary: &str) {
    println!("  ↳ {}", summary);
}

/// Data used to show the summary of a live-mode tick
#[derive(Debug, Clone)]
pub struct TickSummaryData {
    /// Current tick number (1-based)
    pub tick: usize,
    /// Maximum number of planned ticks (None = unbounded)
    pub max: Option<usize>,
    /// GPS points obtained from the API in this tick
    pub points_fetched: usize,
    /// Cumulative total of points in the S2 index
    pub total_indexed: usize,
    /// Number of active S2 cells with data
    pub total_cells: usize,
    /// Cells with congestion level D, E or F
    pub congested: usize,
    /// Global mean SRF of this tick
    pub avg_srf: f64,
    /// Tick duration in milliseconds
    pub elapsed_ms: u128,
}

/// Prints the summary of a live-mode tick
pub fn tick_summary(data: &TickSummaryData) {
    let tick_label = match data.max {
        Some(m) => format!("{}/{}", data.tick, m),
        None => data.tick.to_string(),
    };

    println!();
    println!(
        "  ┌─ Tick {} ─────────────────────────────────────┐",
        tick_label
    );
    println!(
        "  │  Points fetched   : {:>6}                     │",
        data.points_fetched
    );
    println!(
        "  │  Total indexed    : {:>6}                     │",
        data.total_indexed
    );
    println!(
        "  │  Active cells     : {:>6}                     │",
        data.total_cells
    );
    println!(
        "  │  Congested cells  : {:>6}  (level D/E/F)      │",
        data.congested
    );
    println!(
        "  │  Mean SRF         : {:>6.3}                   │",
        data.avg_srf
    );
    println!(
        "  │  Tick duration    : {:>5}ms                   │",
        data.elapsed_ms
    );
    println!("  └───────────────────────────────────────────────┘");
}

/// Prints the wait between live-mode ticks
pub fn tick_waiting(secs: u64) {
    println!();
    println!("  ⏱  Waiting {}s until the next fetch...", secs);
}

/// Prints the live-mode header
pub fn live_header(endpoint: &str, warm_start: bool, max_ticks: Option<usize>) {
    println!();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  🚗  Urban Mobility Engine — Near-Real-Time Mode           ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  API : {:<54}║", truncate(endpoint, 54));
    println!(
        "║  Warm-start : {:<47}║",
        if warm_start {
            "Yes (historical data preloaded)"
        } else {
            "No"
        }
    );
    println!(
        "║  Ticks : {:<52}║",
        match max_ticks {
            Some(n) => format!("{} ticks planned", n),
            None => "Continuous mode (Ctrl+C to stop)".to_string(),
        }
    );
    println!("╚══════════════════════════════════════════════════════════════╝");
}

/// Prints the header of the historical pipeline
pub fn demo_header() {
    println!();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  🚗  Urban Mobility Engine — Madrid                        ║");
    println!("║  Pipeline: CSV → S2 → Metrics → JSON → Dashboard          ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
}

/// Prints the pipeline completion with the output path
pub fn pipeline_done(output_path: &str, bytes: usize) {
    println!();
    println!("╔══════════════════════════════════════════════════════════════╗");
    println!("║  ✅  Pipeline completed                                     ║");
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  Output : {:<51}║", truncate(output_path, 51));
    println!("║  Size   : {:<51}║", format!("{} bytes", bytes));
    println!("╠══════════════════════════════════════════════════════════════╣");
    println!("║  To view the visualization:                                 ║");
    println!("║  1. cd demo && python3 -m http.server 8080                 ║");
    println!("║  2. Open http://localhost:8080/dashboard.html               ║");
    println!("╚══════════════════════════════════════════════════════════════╝");
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}
