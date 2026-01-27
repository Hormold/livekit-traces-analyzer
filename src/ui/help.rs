//! Help modal overlay.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::app::App;

pub fn render(frame: &mut Frame, _app: &App) {
    let area = centered_rect(60, 70, frame.area());

    // Clear the area first
    frame.render_widget(Clear, area);

    let help_text = vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Navigation",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  Tab / Shift+Tab    Switch views"),
        Line::from("  1-8                Jump to specific view"),
        Line::from(""),
        Line::from(Span::styled(
            "  Views",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  1 Overview    Health verdict, diagnosis"),
        Line::from("  2 Transcript  Conversation with metrics"),
        Line::from("  3 Latency     Statistics and slow turns"),
        Line::from("  4 Charts      Visual graphs & timeline"),
        Line::from("  5 Agents      Agent handoffs & timeline"),
        Line::from("  6 Context     LLM context growth"),
        Line::from("  7 Logs        Errors and warnings"),
        Line::from("  8 Spans       Span timeline list"),
        Line::from(""),
        Line::from(Span::styled(
            "  Scrolling",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  j / Down           Scroll down"),
        Line::from("  k / Up             Scroll up"),
        Line::from("  Ctrl+d / PageDown  Page down"),
        Line::from("  Ctrl+u / PageUp    Page up"),
        Line::from("  g / Home           Go to top"),
        Line::from("  G / End            Go to bottom"),
        Line::from(""),
        Line::from(Span::styled(
            "  View-Specific",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  f                  Toggle filter (Logs/Spans view)"),
        Line::from("  s                  Cycle sort mode (Latency view)"),
        Line::from(""),
        Line::from(Span::styled(
            "  General",
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from("  ?                  Toggle this help"),
        Line::from("  q / Ctrl+c         Quit"),
        Line::from(""),
        Line::from(Span::styled(
            "  Press any key to close",
            Style::default().fg(Color::DarkGray),
        )),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Help ");

    let paragraph = Paragraph::new(help_text).block(block);
    frame.render_widget(paragraph, area);
}

/// Create a centered rectangle.
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}
