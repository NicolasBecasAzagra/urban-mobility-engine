use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

use super::app::{App, MENU_OPTIONS};

/// Renders the complete start menu
pub fn render_menu(frame: &mut Frame, area: Rect, app: &App) {
    // 1. Clear the background by painting a white block over the whole area
    frame.render_widget(
        Block::default().style(Style::default().bg(Color::White)),
        area,
    );

    // Vertical layout: header + menu + footer
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Header
            Constraint::Min(10),   // Menu
            Constraint::Length(3), // Footer
        ])
        .split(area);

    render_header(frame, chunks[0]);
    render_menu_items(frame, chunks[1], app);
    render_footer(frame, chunks[2], app);
}

/// Renders the header with the engine title
fn render_header(frame: &mut Frame, area: Rect) {
    let title = Paragraph::new(" Urban Mobility Engine — Madrid   v1.0.0")
        .style(
            Style::default()
                .fg(Color::Black) // Black text on a white background
                .add_modifier(Modifier::BOLD)
                .bg(Color::White),
        )
        .alignment(Alignment::Left)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray).bg(Color::White)),
        );
    frame.render_widget(title, area);
}

/// Renders the list of menu options with the selection highlighted
fn render_menu_items(frame: &mut Frame, area: Rect, app: &App) {
    let descriptions = [
        "Connects to the Madrid City Council API",
        "Demo/live_output.json — last exported tick",
        "Indexing and memory usage demonstration",
        "Close the application",
    ];

    let items: Vec<ListItem> = MENU_OPTIONS
        .iter()
        .enumerate()
        .map(|(i, &opt)| {
            let is_selected = i == app.menu_selection;

            // Selection prefix and item description
            let prefix = if is_selected { "❯ " } else { "  " };
            let desc = if i < descriptions.len() {
                format!("    {}", descriptions[i])
            } else {
                String::new()
            };
            // -----------------------------------------------------

            let main_line = if is_selected {
                Line::from(vec![
                    Span::styled(prefix, Style::default().fg(Color::Red)),
                    Span::styled(
                        opt,
                        Style::default()
                            .fg(Color::Black)
                            .add_modifier(Modifier::BOLD),
                    ),
                ])
            } else {
                Line::from(vec![
                    Span::raw(prefix),
                    Span::styled(opt, Style::default().fg(Color::Indexed(240))), // Dark gray
                ])
            };

            let desc_line = Line::from(Span::styled(
                desc,
                Style::default().fg(Color::Indexed(245)).bg(Color::White),
            ));

            ListItem::new(vec![main_line, desc_line]).style(Style::default().bg(Color::White))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .title(" Select the analysis mode ")
                .title_alignment(Alignment::Left)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue).bg(Color::White)),
        )
        // Very light gray background for the selection
        .highlight_style(Style::default().bg(Color::Indexed(254)));

    if let Some(ref msg) = app.status_message {
        let inner = Layout::default()
            .direction(Direction::Vertical)
            .constraints([Constraint::Min(5), Constraint::Length(3)])
            .split(area);

        frame.render_widget(list, inner[0]);

        let status = Paragraph::new(msg.as_str())
            .style(Style::default().fg(Color::Rgb(200, 0, 0)).bg(Color::White)) // Dark red
            .alignment(Alignment::Center)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Red).bg(Color::White)),
            );
        frame.render_widget(status, inner[1]);
    } else {
        frame.render_widget(list, area);
    }
}

/// Renders the footer with the keyboard controls
fn render_footer(frame: &mut Frame, area: Rect, _app: &App) {
    let footer = Paragraph::new(" [↑↓] Navigate   [Enter] Select   [q] Quit ")
        .style(Style::default().fg(Color::DarkGray).bg(Color::White))
        .alignment(Alignment::Center)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray).bg(Color::White)),
        );
    frame.render_widget(footer, area);
}
