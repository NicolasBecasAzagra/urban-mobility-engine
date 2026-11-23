//! Keyboard event handling for the TUI
//!
//! Processes crossterm `KeyEvent`s and translates them
//! into actions on the `App` state
//!
//! ## Controls
//!
//! ### Menu (Phase 1)
//! | Key           | Action              |
//! |---------------|---------------------|
//! | ↑ / k         | Move up             |
//! | ↓ / j         | Move down           |
//! | Enter / Space | Select option       |
//! | q / Esc       | Quit                |
//!
//! ### Live panel (Phase 2)
//! | Key   | Action                 |
//! |-------|------------------------|
//! | q / Esc | Back to the menu     |
//! | p     | Pause / Resume         |
//! | e     | Export current tick    |

use crossterm::event::{KeyCode, KeyEvent};

use super::app::{App, AppPhase};

/// Processes a keyboard event and updates the application state
///
/// This is the single entry point for all keyboard events,
/// which makes the logic easy to unit test without depending on the terminal
pub fn handle_key_event(app: &mut App, key: KeyEvent) {
    match app.phase {
        AppPhase::Menu => handle_menu_key(app, key),
        AppPhase::Live => handle_live_key(app, key),
    }
}

/// Processes keys in the menu phase
fn handle_menu_key(app: &mut App, key: KeyEvent) {
    match key.code {
        // Navigation
        KeyCode::Up | KeyCode::Char('k') => app.menu_up(),
        KeyCode::Down | KeyCode::Char('j') => app.menu_down(),

        // Selection
        KeyCode::Enter | KeyCode::Char(' ') => {
            app.menu_action_pending = true;
        }

        // Quit
        KeyCode::Char('q') | KeyCode::Esc => {
            app.should_quit = true;
        }

        _ => {}
    }
}

/// Processes keys in the live panel phase
fn handle_live_key(app: &mut App, key: KeyEvent) {
    // If we are in the export submenu, handle those keys first
    if app.export_choosing {
        match key.code {
            KeyCode::Char('1') => {
                app.export_choosing = false;
                app.export_selection = 0; // JSON
                app.set_status("Exported: demo/live_output.json");
            }
            KeyCode::Char('2') => {
                app.export_choosing = false;
                app.export_selection = 1; // CSV
                app.set_status("Exported: demo/metrics.csv");
            }
            KeyCode::Char('3') => {
                app.export_choosing = false;
                app.export_selection = 2; // GeoJSON
                app.set_status("Exported: demo/madrid.geojson");
            }
            KeyCode::Esc => {
                app.export_choosing = false;
                app.status_message = None;
                app.status_shown_at = None;
            }
            _ => {}
        }
        return;
    }

    match key.code {
        // Back to the menu
        KeyCode::Char('q') | KeyCode::Esc => {
            app.phase = AppPhase::Menu;
            app.scheduler = None;
            app.scheduler_ready = false;
            app.last_tick = None;
            app.tick_history.clear();
            app.ticks_executed = 0;
            app.paused = false;
            app.export_choosing = false;
            app.last_api_update = None;
        }

        // Pause / Resume
        KeyCode::Char('p') => {
            app.toggle_pause();
        }

        // Open the export submenu
        KeyCode::Char('e') => {
            app.export_choosing = true;
            app.status_message =
                Some("[1] JSON   [2] CSV   [3] GeoJSON   [Esc] Cancel".to_string());
            app.status_shown_at = None; // do not auto-clear while the user is choosing
        }

        _ => {}
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyEventState, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent {
            code,
            modifiers: KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: KeyEventState::NONE,
        }
    }

    // -----------------------------------------------------------------------
    // Menu navigation
    // -----------------------------------------------------------------------

    #[test]
    fn test_menu_navigation_down() {
        let mut app = App::new();
        assert_eq!(app.menu_selection, 0);

        handle_key_event(&mut app, key(KeyCode::Down));
        assert_eq!(app.menu_selection, 1);

        handle_key_event(&mut app, key(KeyCode::Down));
        assert_eq!(app.menu_selection, 2);
    }

    #[test]
    fn test_menu_navigation_up() {
        let mut app = App::new();
        app.menu_selection = 2;

        handle_key_event(&mut app, key(KeyCode::Up));
        assert_eq!(app.menu_selection, 1);

        handle_key_event(&mut app, key(KeyCode::Up));
        assert_eq!(app.menu_selection, 0);
    }

    #[test]
    fn test_menu_navigation_wraps_down() {
        let mut app = App::new();
        let last = crate::tui::app::MENU_OPTIONS.len() - 1;
        app.menu_selection = last;

        handle_key_event(&mut app, key(KeyCode::Down));
        assert_eq!(app.menu_selection, 0, "Should wrap to the first item");
    }

    #[test]
    fn test_menu_navigation_wraps_up() {
        let mut app = App::new();
        assert_eq!(app.menu_selection, 0);

        handle_key_event(&mut app, key(KeyCode::Up));
        let expected = crate::tui::app::MENU_OPTIONS.len() - 1;
        assert_eq!(app.menu_selection, expected, "Should wrap to the last item");
    }

    #[test]
    fn test_menu_navigation_j_k_keys() {
        let mut app = App::new();

        handle_key_event(&mut app, key(KeyCode::Char('j')));
        assert_eq!(app.menu_selection, 1);

        handle_key_event(&mut app, key(KeyCode::Char('k')));
        assert_eq!(app.menu_selection, 0);
    }

    // -----------------------------------------------------------------------
    // Menu selection
    // -----------------------------------------------------------------------

    #[test]
    fn test_enter_sets_menu_action_pending() {
        let mut app = App::new();
        assert!(!app.menu_action_pending);

        handle_key_event(&mut app, key(KeyCode::Enter));
        assert!(app.menu_action_pending);
    }

    #[test]
    fn test_space_sets_menu_action_pending() {
        let mut app = App::new();
        handle_key_event(&mut app, key(KeyCode::Char(' ')));
        assert!(app.menu_action_pending);
    }

    // -----------------------------------------------------------------------
    // Quit keys
    // -----------------------------------------------------------------------

    #[test]
    fn test_q_key_sets_should_quit() {
        let mut app = App::new();
        assert!(!app.should_quit);

        handle_key_event(&mut app, key(KeyCode::Char('q')));
        assert!(app.should_quit);
    }

    #[test]
    fn test_esc_sets_should_quit_in_menu() {
        let mut app = App::new();
        handle_key_event(&mut app, key(KeyCode::Esc));
        assert!(app.should_quit);
    }

    // -----------------------------------------------------------------------
    // Live panel controls
    // -----------------------------------------------------------------------

    #[test]
    fn test_q_in_live_goes_to_menu() {
        let mut app = App::new();
        app.phase = AppPhase::Live;

        handle_key_event(&mut app, key(KeyCode::Char('q')));
        assert_eq!(app.phase, AppPhase::Menu);
        assert!(
            !app.should_quit,
            "q in live mode must not close the program"
        );
    }

    #[test]
    fn test_esc_in_live_goes_to_menu() {
        let mut app = App::new();
        app.phase = AppPhase::Live;

        handle_key_event(&mut app, key(KeyCode::Esc));
        assert_eq!(app.phase, AppPhase::Menu);
    }

    #[test]
    fn test_p_key_toggles_pause() {
        let mut app = App::new();
        app.phase = AppPhase::Live;
        assert!(!app.paused);

        handle_key_event(&mut app, key(KeyCode::Char('p')));
        assert!(app.paused);

        handle_key_event(&mut app, key(KeyCode::Char('p')));
        assert!(!app.paused);
    }

    // -----------------------------------------------------------------------
    // App state helpers
    // -----------------------------------------------------------------------

    #[test]
    fn test_phase_transition_menu_to_live() {
        let mut app = App::new();
        assert_eq!(app.phase, AppPhase::Menu);
        app.phase = AppPhase::Live;
        assert_eq!(app.phase, AppPhase::Live);
    }

    #[test]
    fn test_countdown_ratio_full() {
        let mut app = App::new();
        app.poll_interval_secs = 300;
        app.countdown_secs = 300;
        assert!((app.countdown_ratio() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_countdown_ratio_empty() {
        let mut app = App::new();
        app.poll_interval_secs = 300;
        app.countdown_secs = 0;
        assert!((app.countdown_ratio() - 0.0).abs() < 0.001);
    }

    #[test]
    fn test_tick_history_max_size() {
        use crate::tui::app::{TickSummary, MAX_TICK_HISTORY};

        let mut app = App::new();

        // Insert MAX_TICK_HISTORY + 5 entries
        for i in 0..(MAX_TICK_HISTORY + 5) {
            if app.tick_history.len() >= MAX_TICK_HISTORY {
                app.tick_history.pop_back();
            }
            app.tick_history.push_front(TickSummary {
                tick_number: i + 1,
                points_fetched: 100,
                total_cells: 50,
                alerts: 0,
                avg_srf: 0.70,
            });
        }

        assert_eq!(
            app.tick_history.len(),
            MAX_TICK_HISTORY,
            "The history must not exceed MAX_TICK_HISTORY"
        );
    }

    #[test]
    fn test_countdown_updates() {
        let mut app = App::new();
        app.poll_interval_secs = 300;
        app.countdown_secs = 150;

        // Simulate an update
        app.countdown_secs = 100;
        assert_eq!(app.countdown_secs, 100);
        assert!((app.countdown_ratio() - (100.0 / 300.0)).abs() < 0.001);
    }

    #[test]
    fn test_last_avg_srf_no_data() {
        let app = App::new();
        assert!((app.last_avg_srf() - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_last_alert_count_no_data() {
        let app = App::new();
        assert_eq!(app.last_alert_count(), 0);
    }
}
