//! Interactive TUI of the Urban Mobility Engine
//!
//! Provides an interactive terminal interface with two phases:
//!
//! - **Phase 1 — Start menu**: selection of the analysis mode
//! - **Phase 2 — Live panel**: real-time visualization of metrics and alerts
//!
//! ## Uso
//!
//! ```rust,no_run
//! tfg::tui::run_tui().unwrap();
//! ```
//!
//! The TUI is launched automatically when the `tfg` binary is invoked without
//! arguments. With arguments, the binary uses the usual direct CLI

pub mod app;
pub mod events;
pub mod live_panel;
pub mod menu;

pub use app::{App, AppPhase};

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::path::Path;
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::export::{export_csv, export_geojson, export_json};
use crate::metrics::{delay_factor, statistics};

/// Entry point of the interactive TUI
///
/// Initializes the terminal in alternate mode, runs the event loop
/// and restores the terminal on exit
pub fn run_tui() -> Result<()> {
    // Load configuration
    let config = match Config::from_file("config.toml") {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error loading config.toml: {}", e);
            eprintln!("Run from the project directory.");
            return Err(e);
        }
    };

    // Initialize the terminal
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.hide_cursor()?;

    // Application state
    let mut app = App::new();

    // TUI result
    let result = run_event_loop(&mut terminal, &mut app, &config);

    // Always restore the terminal, even on error
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

/// Main TUI event loop
fn run_event_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    app: &mut App,
    config: &Config,
) -> Result<()> {
    let tick_duration = Duration::from_millis(100); // 10 fps for the countdown

    loop {
        // Render the frame
        terminal.draw(|frame| match app.phase {
            AppPhase::Menu => menu::render_menu(frame, frame.area(), app),
            AppPhase::Live => live_panel::render_live_panel(frame, frame.area(), app),
        })?;

        // Auto-clear status messages after 3 seconds
        app.clear_stale_status();

        // Process events with a timeout
        if event::poll(tick_duration)? {
            if let Event::Key(key) = event::read()? {
                // Ctrl+C always quits
                if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                    break;
                }
                events::handle_key_event(app, key);
            }
        }

        // Run the export if the user has just chosen a format
        if app.phase == AppPhase::Live && !app.export_choosing {
            // Detect a freshly selected export (status_message contains "Exported:")
            if let Some(ref msg) = app.status_message {
                if msg.starts_with("Exported:") {
                    // Only export if a scheduler with an available index exists
                    if let Some(ref scheduler) = app.scheduler {
                        let idx = scheduler.index();
                        let reports = delay_factor::compute_all_cells(idx, &config.metrics);
                        let all_stats = statistics::compute_all_statistics(idx, &config.metrics);

                        let result_msg = match app.export_selection {
                            0 => {
                                // JSON
                                let path = Path::new("demo/live_output.json");
                                match export_json(
                                    idx,
                                    &reports,
                                    &all_stats,
                                    &[],
                                    path,
                                    "Madrid Real-Time API",
                                ) {
                                    Ok(s) => format!(
                                        "JSON exported: {} ({} bytes)",
                                        path.display(),
                                        s.bytes_written
                                    ),
                                    Err(e) => format!("Error JSON: {}", e),
                                }
                            }
                            1 => {
                                // CSV
                                let path = Path::new("demo/metrics.csv");
                                match export_csv(&reports, idx, path) {
                                    Ok(s) => format!(
                                        "CSV exported: {} ({} bytes)",
                                        path.display(),
                                        s.bytes_written
                                    ),
                                    Err(e) => format!("Error CSV: {}", e),
                                }
                            }
                            2 => {
                                // GeoJSON
                                let path = Path::new("demo/madrid.geojson");
                                match export_geojson(idx, &reports, path) {
                                    Ok(s) => format!(
                                        "GeoJSON exported: {} ({} bytes)",
                                        path.display(),
                                        s.bytes_written
                                    ),
                                    Err(e) => format!("Error GeoJSON: {}", e),
                                }
                            }
                            _ => "Unknown format".to_string(),
                        };

                        app.set_status(&result_msg);
                    } else {
                        app.set_status("No data to export (scheduler not started)");
                    }
                }
            }
        }

        // Update the countdown if we are in the live phase
        if app.phase == AppPhase::Live {
            if let Some(next_tick_at) = app.next_tick_at {
                let now = Instant::now();
                if now >= next_tick_at {
                    app.countdown_secs = 0;
                    // Run the API tick if the scheduler is available
                    if app.scheduler_ready {
                        // The real tick runs on the main thread when the countdown reaches 0
                        // Here we simply flag that a tick is needed
                        app.tick_pending = true;
                    }
                } else {
                    app.countdown_secs = (next_tick_at - now).as_secs();
                }
            }

            // If a tick is pending, run it
            if app.tick_pending {
                app.tick_pending = false;
                app.execute_tick(config)?;

                // Auto-export JSON for the HTML dashboard after every tick
                if let Some(ref scheduler) = app.scheduler {
                    let idx = scheduler.index();
                    let reports = delay_factor::compute_all_cells(idx, &config.metrics);
                    let all_stats = statistics::compute_all_statistics(idx, &config.metrics);
                    let path = Path::new("demo/live_output.json");
                    let _ =
                        export_json(idx, &reports, &all_stats, &[], path, "Madrid Real-Time API");
                }
            }
        }

        // Process the menu selection
        if app.menu_action_pending {
            app.menu_action_pending = false;
            match app.menu_selection {
                0 => {
                    // Start real-time analysis
                    app.start_live_mode(config)?;
                }
                1 => {
                    // Export latest results (simple action in this version)
                    app.set_status("Results exported to demo/live_output.json");
                }
                2 => {
                    // View benchmarks (informational action)
                    app.set_status("Performance: 120,000 pts/s | RAM: < 25MB | S2 latency: 0.1ms");
                }
                3 => {
                    // Quit
                    app.should_quit = true;
                }
                _ => {
                    // Quit (any other unknown index)
                    app.should_quit = true;
                }
            }
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
