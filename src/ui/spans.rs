//! Spans timeline view with detail panel.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span as TextSpan},
    widgets::{Block, Borders, Paragraph, Row, Table, Wrap},
    Frame,
};

use crate::app::App;
use super::{format_duration, latency_color};

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let filtered = app.filtered_spans();

    if filtered.is_empty() {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Spans ");
        let paragraph = Paragraph::new("  No spans matching current filter").block(block);
        frame.render_widget(paragraph, area);
        return;
    }

    // Split: table and detail view
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(45),  // Table
            Constraint::Min(0),          // Detail view
        ])
        .split(area);

    render_table(frame, app, &filtered, chunks[0]);
    render_detail(frame, app, &filtered, chunks[1]);
}

fn render_table(frame: &mut Frame, app: &App, filtered: &[&crate::data::Span], area: Rect) {
    let scroll = app.spans_scroll;
    let visible_height = area.height.saturating_sub(4) as usize;

    let header = Row::new(vec!["#", "Time", "Span Name", "Duration", "Parent"])
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .bottom_margin(1);

    let rows: Vec<Row> = filtered
        .iter()
        .enumerate()
        .skip(scroll.saturating_sub(2).min(scroll))
        .take(visible_height)
        .map(|(idx, span)| {
            let rel_time = span.start_sec() - app.analysis.session_start;
            let duration_ms = span.duration_ms();

            let name_color = match span.name.as_str() {
                "agent_turn" => Color::Cyan,
                "user_turn" => Color::Green,
                "llm_node" => Color::Yellow,
                "tts_node" | "tts_request" => Color::Magenta,
                "stt_request" => Color::Blue,
                "function_call" | "tool_call" => Color::Red,
                _ => Color::White,
            };

            // Mark selected row
            let style = if idx == scroll {
                Style::default().fg(Color::Black).bg(name_color)
            } else {
                Style::default().fg(name_color)
            };

            let has_parent = span.parent_span_id.is_some();

            Row::new(vec![
                format!("{}", idx + 1),
                format_duration(rel_time),
                span.name.clone(),
                format!("{:.0}ms", duration_ms),
                if has_parent { "yes" } else { "-" }.to_string(),
            ])
            .style(style)
        })
        .collect();

    let widths = [
        Constraint::Length(4),   // #
        Constraint::Length(10),  // Time
        Constraint::Min(20),     // Span name
        Constraint::Length(10),  // Duration
        Constraint::Length(6),   // Parent
    ];

    let title = format!(
        " Spans - {} ({} total) | Filter: {} (f) ",
        app.span_filter.label(),
        filtered.len(),
        app.span_filter.label()
    );

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title),
        );

    frame.render_widget(table, area);
}

fn render_detail(frame: &mut Frame, app: &App, filtered: &[&crate::data::Span], area: Rect) {
    let scroll = app.spans_scroll;

    if let Some(span) = filtered.get(scroll) {
        // All space for attributes with inline header
        render_detail_full(frame, app, span, area);
    } else {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Span Details ");
        let paragraph = Paragraph::new("  No span selected").block(block);
        frame.render_widget(paragraph, area);
    }
}

fn render_detail_full(frame: &mut Frame, app: &App, span: &crate::data::Span, area: Rect) {
    let rel_start = span.start_sec() - app.analysis.session_start;
    let rel_end = span.end_sec() - app.analysis.session_start;
    let duration_ms = span.duration_ms();
    let max_width = area.width.saturating_sub(4) as usize;

    let name_color = match span.name.as_str() {
        "agent_turn" => Color::Cyan,
        "user_turn" => Color::Green,
        "llm_node" => Color::Yellow,
        "tts_node" | "tts_request" => Color::Magenta,
        "stt_request" => Color::Blue,
        "function_call" | "tool_call" => Color::Red,
        _ => Color::White,
    };

    let mut lines = vec![
        // Compact header: all info in one line
        Line::from(vec![
            TextSpan::styled(&span.name, Style::default().fg(name_color).add_modifier(Modifier::BOLD)),
            TextSpan::styled(" | ", Style::default().fg(Color::DarkGray)),
            TextSpan::styled(format_duration(rel_start), Style::default().fg(Color::Cyan)),
            TextSpan::styled("→", Style::default().fg(Color::DarkGray)),
            TextSpan::styled(format_duration(rel_end), Style::default().fg(Color::Cyan)),
            TextSpan::styled(" | ", Style::default().fg(Color::DarkGray)),
            TextSpan::styled(format!("{:.0}ms", duration_ms), Style::default().fg(latency_color(duration_ms))),
            TextSpan::styled(" | ID: ", Style::default().fg(Color::DarkGray)),
            TextSpan::styled(&span.span_id[..span.span_id.len().min(8)], Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(""),
    ];

    // Attributes
    if span.attributes.is_empty() {
        lines.push(Line::from(TextSpan::styled(
            "  No attributes",
            Style::default().fg(Color::DarkGray)
        )));
    } else {
        let mut attrs: Vec<_> = span.attributes.iter().collect();
        attrs.sort_by(|a, b| a.0.cmp(b.0));

        for (key, value) in attrs {
            let value_str = format_json_value(value);
            let key_display = format!("  {}: ", key);

            // Try to fit key and value on one line if possible
            if key_display.len() + value_str.len() <= max_width {
                lines.push(Line::from(vec![
                    TextSpan::styled(key_display, Style::default().fg(Color::Cyan)),
                    TextSpan::styled(value_str, Style::default().fg(Color::White)),
                ]));
            } else {
                // Key on its own, then wrapped value
                lines.push(Line::from(vec![
                    TextSpan::styled(key_display, Style::default().fg(Color::Cyan)),
                ]));
                // Wrap long values
                let chars: Vec<char> = value_str.chars().collect();
                let indent = "    ";
                let wrap_width = max_width.saturating_sub(indent.len());
                for chunk in chars.chunks(wrap_width.max(20)) {
                    let line_text: String = chunk.iter().collect();
                    lines.push(Line::from(vec![
                        TextSpan::styled(format!("{}{}", indent, line_text), Style::default().fg(Color::White)),
                    ]));
                }
            }
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(name_color))
        .title(format!(" #{} {} ({} attrs) ", app.spans_scroll + 1, span.name, span.attributes.len()));

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

/// Format a JSON value for display
fn format_json_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Array(arr) => {
            let items: Vec<String> = arr.iter().map(format_json_value).collect();
            format!("[{}]", items.join(", "))
        }
        serde_json::Value::Object(obj) => {
            let items: Vec<String> = obj.iter()
                .map(|(k, v)| format!("{}: {}", k, format_json_value(v)))
                .collect();
            format!("{{{}}}", items.join(", "))
        }
    }
}
