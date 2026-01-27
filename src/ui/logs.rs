//! Logs view - errors and warnings.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::app::App;
use super::{format_duration, truncate};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let filtered = app.filtered_logs();
    let scroll = app.logs_scroll;
    let visible_height = area.height.saturating_sub(2) as usize;

    let mut lines: Vec<Line> = Vec::new();

    if filtered.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  No logs matching current filter",
            Style::default().fg(Color::Green),
        )));
    } else {
        for log in filtered.iter().skip(scroll).take(visible_height) {
            let rel_time = log.timestamp_sec() - app.analysis.session_start;

            let (severity_style, severity_label) = match log.severity.as_str() {
                "ERROR" | "CRITICAL" => (Style::default().fg(Color::Red).add_modifier(Modifier::BOLD), "ERR"),
                "WARN" | "WARNING" => (Style::default().fg(Color::Yellow), "WRN"),
                _ => (Style::default().fg(Color::Gray), "INF"),
            };

            // First line: timestamp, severity, logger
            lines.push(Line::from(vec![
                Span::styled(format!("  [{}] ", format_duration(rel_time)), Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{} ", severity_label), severity_style),
                Span::styled(truncate(&log.logger_name, 30), Style::default().fg(Color::Cyan)),
            ]));

            // Message (truncated to fit)
            let max_msg_len = area.width.saturating_sub(6) as usize;
            let msg = truncate(&log.message, max_msg_len);
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled(msg, Style::default().fg(Color::White)),
            ]));

            lines.push(Line::from(""));
        }
    }

    let title = format!(
        " Logs - {} ({} total) ",
        app.log_filter.label(),
        filtered.len()
    );

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title);

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}
