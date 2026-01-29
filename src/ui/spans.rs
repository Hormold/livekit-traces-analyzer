//! Spans timeline view with pipeline analysis and detail panel.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span as TextSpan},
    widgets::{Block, Borders, Paragraph, Row, Table, Wrap},
    Frame,
};

use crate::app::App;
use crate::data::PipelineSummary;
use crate::thresholds;
use super::{format_duration, latency_color, severity_to_color};

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

    // Split: pipeline analysis, table, and detail view
    let has_pipeline = !app.analysis.pipeline_cycles.is_empty();

    let chunks = if has_pipeline {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(10), // Pipeline analysis
                Constraint::Percentage(35), // Table
                Constraint::Min(0),    // Detail view
            ])
            .split(area)
    } else {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Percentage(45), // Table
                Constraint::Min(0),         // Detail view
            ])
            .split(area)
    };

    if has_pipeline {
        render_pipeline_analysis(frame, app, chunks[0]);
        render_table(frame, app, &filtered, chunks[1]);
        render_detail(frame, app, &filtered, chunks[2]);
    } else {
        render_table(frame, app, &filtered, chunks[0]);
        render_detail(frame, app, &filtered, chunks[1]);
    }
}

/// Render the pipeline analysis panel showing per-turn timing breakdown.
fn render_pipeline_analysis(frame: &mut Frame, app: &App, area: Rect) {
    let cycles = &app.analysis.pipeline_cycles;

    if cycles.is_empty() {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Pipeline Analysis ");
        let paragraph = Paragraph::new("  No pipeline data").block(block);
        frame.render_widget(paragraph, area);
        return;
    }

    let summary = PipelineSummary::from_cycles(cycles);

    let mut lines: Vec<Line> = Vec::new();

    if let Some(ref s) = summary {
        // Line 1: Response time summary
        let total_color = severity_to_color(s.total_severity);
        lines.push(Line::from(vec![
            TextSpan::styled("Response: ", Style::default().fg(Color::White)),
            TextSpan::styled(format!("{:.0}ms avg", s.avg_total_ms), Style::default().fg(total_color).add_modifier(Modifier::BOLD)),
            TextSpan::styled(format!(" (max {:.0}ms)", s.max_total_ms), Style::default().fg(Color::DarkGray)),
            TextSpan::styled(format!(" - {}", s.total_verdict), Style::default().fg(total_color)),
        ]));

        // Line 2: LLM breakdown
        let llm_color = severity_to_color(s.llm_severity);
        lines.push(Line::from(vec![
            TextSpan::styled("  LLM: ", Style::default().fg(Color::Yellow)),
            TextSpan::styled(format!("{:.0}ms", s.avg_llm_ms), Style::default().fg(llm_color)),
            TextSpan::styled(format!(" ({:.0}%)", s.llm_pct), Style::default().fg(Color::DarkGray)),
            TextSpan::styled(format!(" - {}", s.llm_verdict), Style::default().fg(llm_color)),
        ]));

        // Line 3: TTS breakdown
        let tts_color = severity_to_color(s.tts_severity);
        lines.push(Line::from(vec![
            TextSpan::styled("  TTS: ", Style::default().fg(Color::Magenta)),
            TextSpan::styled(format!("{:.0}ms", s.avg_tts_ms), Style::default().fg(tts_color)),
            TextSpan::styled(format!(" ({:.0}%)", s.tts_pct), Style::default().fg(Color::DarkGray)),
            TextSpan::styled(format!(" - {}", s.tts_verdict), Style::default().fg(tts_color)),
        ]));

        // Line 4: Perception delay (only if we have user turns)
        if s.user_turn_count > 0 {
            let perception_color = severity_to_color(s.perception_severity);
            lines.push(Line::from(vec![
                TextSpan::styled("  VAD: ", Style::default().fg(Color::Cyan)),
                TextSpan::styled(format!("{:.0}ms", s.avg_user_to_llm_ms), Style::default().fg(perception_color)),
                TextSpan::styled(format!(" ({} user turns)", s.user_turn_count), Style::default().fg(Color::DarkGray)),
                TextSpan::styled(format!(" - {}", s.perception_verdict), Style::default().fg(perception_color)),
            ]));
        }

        // Line 5: Bottleneck
        let bottleneck_color = severity_to_color(s.bottleneck_severity);
        lines.push(Line::from(vec![
            TextSpan::styled("Bottleneck: ", Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
            TextSpan::styled(&s.bottleneck, Style::default().fg(bottleneck_color)),
        ]));

        // Line 6: Detected delays (if any)
        if !s.detected_delays.is_empty() {
            let delay_count = s.detected_delays.len().min(thresholds::MAX_DETECTED_DELAYS);
            let delays: Vec<String> = s.detected_delays.iter()
                .take(delay_count)
                .map(|d| format!("T{}:{:.0}ms({})", d.turn_number, d.gap_ms, d.reason))
                .collect();
            lines.push(Line::from(vec![
                TextSpan::styled("Delays: ", Style::default().fg(Color::Red)),
                TextSpan::styled(delays.join(" | "), Style::default().fg(Color::Yellow)),
            ]));
        }
    } else {
        lines.push(Line::from(TextSpan::styled(
            "No pipeline data available",
            Style::default().fg(Color::DarkGray)
        )));
    }

    let title = format!(" Pipeline Analysis ({} turns) ", cycles.len());
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(title);

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
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
