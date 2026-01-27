//! UI module - tab bar, layout, and views.

pub mod agents;
mod charts;
mod context;
mod help;
mod latency;
mod logs;
mod overview;
mod spans;
mod tools;
mod transcript;

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Tabs},
    Frame,
};

use crate::app::{App, View};

/// Render the entire UI.
pub fn render(frame: &mut Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Tab bar
            Constraint::Min(0),    // Content
            Constraint::Length(1), // Status bar
        ])
        .split(frame.area());

    render_tabs(frame, app, chunks[0]);
    render_content(frame, app, chunks[1]);
    render_status_bar(frame, app, chunks[2]);

    // Render help modal on top if active
    if app.show_help {
        help::render(frame, app);
    }
}

/// Render the tab bar.
fn render_tabs(frame: &mut Frame, app: &App, area: Rect) {
    let titles: Vec<Line> = View::all()
        .iter()
        .map(|v| {
            let style = if *v == app.current_view {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Gray)
            };
            Line::from(Span::styled(format!("[{}] {}", v.hotkey(), v.label()), style))
        })
        .collect();

    let tabs = Tabs::new(titles)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" LiveKit Call Analyzer "),
        )
        .select(View::all().iter().position(|v| *v == app.current_view).unwrap_or(0))
        .style(Style::default().fg(Color::White))
        .highlight_style(Style::default().fg(Color::Yellow));

    frame.render_widget(tabs, area);
}

/// Render the content area based on current view.
fn render_content(frame: &mut Frame, app: &App, area: Rect) {
    match app.current_view {
        View::Overview => overview::render(frame, app, area),
        View::Transcript => transcript::render(frame, app, area),
        View::Latency => latency::render(frame, app, area),
        View::Charts => charts::render(frame, app, area),
        View::Agents => agents::render(frame, app, area),
        View::Tools => tools::render(frame, app, area),
        View::Context => context::render(frame, app, area),
        View::Logs => logs::render(frame, app, area),
        View::Spans => spans::render(frame, app, area),
    }
}

/// Render the status bar.
fn render_status_bar(frame: &mut Frame, app: &App, area: Rect) {
    let scroll_info = match app.current_view {
        View::Transcript | View::Logs | View::Spans | View::Context | View::Tools => {
            format!(" [{}/{}]", app.current_scroll() + 1, app.max_scroll() + 1)
        }
        _ => String::new(),
    };

    let filter_info = match app.current_view {
        View::Logs => format!(" | Filter: {} (f)", app.log_filter.label()),
        View::Spans => format!(" | Filter: {} (f)", app.span_filter.label()),
        View::Latency => format!(" | Sort: {} (s)", app.latency_sort.label()),
        _ => String::new(),
    };

    let status = format!(
        " Tab/Shift+Tab: Navigate | j/k: Scroll | Ctrl+d/u: Page | ?: Help | q: Quit{}{}",
        scroll_info, filter_info
    );

    let paragraph = Paragraph::new(status)
        .style(Style::default().bg(Color::DarkGray).fg(Color::White));

    frame.render_widget(paragraph, area);
}

/// Color for latency values.
pub fn latency_color(ms: f64) -> Color {
    if ms < 500.0 {
        Color::Green
    } else if ms < 1500.0 {
        Color::Yellow
    } else {
        Color::Red
    }
}

/// Format duration as mm:ss.ms.
pub fn format_duration(seconds: f64) -> String {
    let mins = (seconds / 60.0) as u32;
    let secs = seconds % 60.0;
    format!("{}:{:05.2}", mins, secs)
}

/// Truncate text to max length with ellipsis (UTF-8 safe).
pub fn truncate(s: &str, max_len: usize) -> String {
    let char_count = s.chars().count();
    if char_count <= max_len {
        s.to_string()
    } else {
        let truncated: String = s.chars().take(max_len.saturating_sub(3)).collect();
        format!("{}...", truncated)
    }
}
