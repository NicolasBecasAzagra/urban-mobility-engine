//! Live analysis panel widget (Phase 2 of the TUI)
//!
//! Renders the two-column layout during real-time analysis:
//! - **Left**: Engine status (tick, points, SRF, countdown)
//! - **Right**: Active alerts and tick history
//!
//! ```text
//! ┌─── Engine Status ───────────────────┐ ┌─── Active Alerts ────────────────────┐
//! │  Tick: 3    Source: Madrid API      │ │  3 anomalies detected                │
//! │  Indexed points: 18,543  Cells:     │ │                                      │
//! │  Mean SRF: 0.71  Congested: 47      │ │  SEVERE — SRF 0.28 (normal 0.71)     │
//! │  Next tick in: 298s ████░░░░        │ │  MODERATE — SRF 0.41 (normal 0.68)   │
//! │                                     │ │                                      │
//! │  [q] Quit   [p] Pause   [e] Export  │ │  [Recent tick history]               │
//! └─────────────────────────────────────┘ └──────────────────────────────────────┘
//! ```
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph},
    Frame,
};

use super::app::App;

/// Renders the complete live panel with two columns
pub fn render_live_panel(frame: &mut Frame, area: Rect, app: &App) {
    // 1. Clear the background to white
    frame.render_widget(
        Block::default().style(Style::default().bg(Color::White)),
        area,
    );

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(55), // Engine status
            Constraint::Percentage(45), // Alerts
        ])
        .split(area);

    render_motor_status(frame, columns[0], app);
    render_alerts_panel(frame, columns[1], app);
}

/// Renders the engine status panel (left column)
fn render_motor_status(frame: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(8),    // Main info
            Constraint::Length(3), // Countdown bar
            Constraint::Length(3), // Controls
        ])
        .split(area);

    let tick_num = app.last_tick.as_ref().map(|t| t.tick_number).unwrap_or(0);
    let points = app
        .last_tick
        .as_ref()
        .map(|t| t.total_points_indexed)
        .unwrap_or(0);
    let cells = app.last_tick.as_ref().map(|t| t.total_cells).unwrap_or(0);
    let avg_srf = app.last_avg_srf();
    let congested = app.last_congested_cells();
    let alerts = app.last_alert_count();

    let pause_label = if app.paused { " [PAUSED]" } else { "" };
    let srf_color = srf_to_color(avg_srf);

    // Spanish date format (Madrid): DD/MM/YYYY HH:MM:SS
    let last_update_str = match app.last_api_update {
        Some(dt) => dt.format("%d/%m/%Y %H:%M:%S").to_string(),
        None => "Waiting...".to_string(),
    };

    let lines = vec![
        Line::from(vec![
            Span::styled("  Tick: ", Style::default().fg(Color::Indexed(240))),
            Span::styled(
                format!("{}", tick_num),
                Style::default()
                    .fg(Color::Black)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("   Source: ", Style::default().fg(Color::Indexed(240))),
            Span::styled(
                "Madrid API",
                Style::default()
                    .fg(Color::Blue)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                pause_label,
                Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("  Last update: ", Style::default().fg(Color::Indexed(240))),
            Span::styled(
                last_update_str,
                Style::default()
                    .fg(Color::Indexed(24))
                    .add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled(
                "  Indexed points: ",
                Style::default().fg(Color::Indexed(240)),
            ),
            Span::styled(format_number(points), Style::default().fg(Color::Black)),
        ]),
        Line::from(vec![
            Span::styled(
                "  Active S2 cells: ",
                Style::default().fg(Color::Indexed(240)),
            ),
            Span::styled(format_number(cells), Style::default().fg(Color::Black)),
        ]),
        Line::raw(""),
        Line::from(vec![
            Span::styled("  Mean SRF: ", Style::default().fg(Color::Indexed(240))),
            Span::styled(
                format!("{:.3}", avg_srf),
                Style::default().fg(srf_color).add_modifier(Modifier::BOLD),
            ),
            Span::styled("   Congested: ", Style::default().fg(Color::Indexed(240))),
            Span::styled(format!("{}", congested), Style::default().fg(Color::Red)),
        ]),
        Line::from(vec![
            Span::styled(
                "  Active alerts: ",
                Style::default().fg(Color::Indexed(240)),
            ),
            Span::styled(
                format!("{}", alerts),
                if alerts > 0 {
                    Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Rgb(0, 150, 0))
                },
            ),
        ]),
    ];

    let info = Paragraph::new(lines)
        .block(
            Block::default()
                .title(" Engine Status ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue).bg(Color::White)),
        )
        .style(Style::default().bg(Color::White));
    frame.render_widget(info, rows[0]);

    render_countdown_bar(frame, rows[1], app);

    let status_text = app
        .status_message
        .as_deref()
        .unwrap_or("[q] Quit   [p] Pause   [e] Export");
    let controls = Paragraph::new(status_text)
        .style(Style::default().fg(Color::Black).bg(Color::White))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray).bg(Color::White)),
        );
    frame.render_widget(controls, rows[2]);
}

pub fn render_countdown_bar(frame: &mut Frame, area: Rect, app: &App) {
    let ratio = app.countdown_ratio();
    let secs = app.countdown_secs;

    let label = if app.paused {
        "PAUSED".to_string()
    } else if secs == 0 && app.ticks_executed == 0 {
        "Connecting...".to_string()
    } else if secs > 0 && secs <= 30 {
        // With 30s or less remaining, the engine starts checking whether the API has refreshed
        format!("Checking whether the API has refreshed ({}s)", secs)
    } else {
        format!("Next tick in: {}s (retries every 30s)", secs)
    };

    let gauge = Gauge::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray).bg(Color::White)),
        )
        // Use a darker blue so the white text stands out inside
        .gauge_style(
            Style::default()
                .fg(Color::Rgb(0, 80, 200)) // Vivid blue
                .bg(Color::Indexed(252)) // Very light gray for the empty part
                .add_modifier(Modifier::BOLD),
        )
        .ratio(ratio)
        // IMPORTANT: Ratatui inverts the text color by default
        // when the bar passes underneath it
        .label(label);

    frame.render_widget(gauge, area);
}

fn render_alerts_panel(frame: &mut Frame, area: Rect, app: &App) {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    render_current_alerts(frame, rows[0], app);
    render_tick_history(frame, rows[1], app);
}

fn render_current_alerts(frame: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = match &app.last_tick {
        None => {
            vec![ListItem::new(Line::from(Span::styled(
                "  Waiting for the first tick...",
                Style::default().fg(Color::Indexed(245)),
            )))]
        }
        Some(tick) if tick.anomaly_alerts.is_empty() => {
            vec![ListItem::new(Line::from(Span::styled(
                "  No anomalies detected",
                Style::default().fg(Color::Rgb(0, 150, 0)),
            )))]
        }
        Some(tick) => {
            tick.anomaly_alerts
                .iter()
                .take(8)
                .map(|alert| {
                    use crate::metrics::anomaly::AnomalySeverity;
                    let (prefix, color) = match alert.severity {
                        AnomalySeverity::Severe => ("SEVERE", Color::Red),
                        AnomalySeverity::Moderate => ("MODERATE", Color::Indexed(166)), // Dark orange for contrast
                        AnomalySeverity::Improvement => ("IMPROVEMENT", Color::Rgb(0, 120, 0)),
                    };
                    let text = format!(
                        " [{prefix}] SRF {:.2} (ref {:.2})",
                        alert.observed_srf, alert.baseline_p50
                    );
                    ListItem::new(Line::from(Span::styled(text, Style::default().fg(color))))
                        .style(Style::default().bg(Color::White))
                })
                .collect()
        }
    };

    let alert_count = app
        .last_tick
        .as_ref()
        .map(|t| t.anomaly_alerts.len())
        .unwrap_or(0);
    let title = if alert_count > 0 {
        format!(" Active Alerts ({}) ", alert_count)
    } else {
        " Active Alerts ".to_string()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Red).bg(Color::White)),
        )
        .style(Style::default().bg(Color::White));

    frame.render_widget(list, area);
}

fn render_tick_history(frame: &mut Frame, area: Rect, app: &App) {
    let items: Vec<ListItem> = if app.tick_history.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "  No history yet",
            Style::default().fg(Color::Indexed(245)),
        )))]
    } else {
        app.tick_history
            .iter()
            .map(|s| {
                let alert_str = if s.alerts > 0 {
                    format!(" {} alerts", s.alerts)
                } else {
                    " ok".to_string()
                };
                let text = format!(
                    " Tick #{}: {} pts | SRF {:.2} |{}",
                    s.tick_number,
                    format_number(s.points_fetched),
                    s.avg_srf,
                    alert_str
                );
                // If there are alerts use orange/red, otherwise black/gray
                let color = if s.alerts > 0 {
                    Color::Indexed(166)
                } else {
                    Color::Black
                };
                ListItem::new(Line::from(Span::styled(text, Style::default().fg(color))))
                    .style(Style::default().bg(Color::White))
            })
            .collect()
    };

    let list = List::new(items)
        .block(
            Block::default()
                .title(" Tick History ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray).bg(Color::White)),
        )
        .style(Style::default().bg(Color::White));

    frame.render_widget(list, area);
}

// Helpers
fn srf_to_color(srf: f64) -> Color {
    match srf {
        s if s >= 0.80 => Color::Rgb(0, 150, 0), // Dark green
        s if s >= 0.65 => Color::Indexed(166),   // Dark orange/yellow
        s if s >= 0.50 => Color::Red,
        _ => Color::Indexed(124), // Dark red
    }
}

fn format_number(n: usize) -> String {
    let s = n.to_string();
    let mut result = String::new();
    for (i, c) in s.chars().rev().enumerate() {
        if i > 0 && i % 3 == 0 {
            result.push('.');
        }
        result.push(c);
    }
    result.chars().rev().collect()
}
