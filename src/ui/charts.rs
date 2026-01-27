//! Visualization charts - latency graphs, timelines, distributions.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{BarChart, Block, Borders, Paragraph, Sparkline},
    Frame,
};

use crate::app::App;
use super::latency_color;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10), // E2E latency over time
            Constraint::Length(10), // Bar charts row
            Constraint::Min(0),     // Span timeline
        ])
        .split(area);

    render_latency_sparkline(frame, app, chunks[0]);

    // Split middle row for two bar charts
    let bar_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    render_component_bars(frame, app, bar_chunks[0]);
    render_distribution(frame, app, bar_chunks[1]);

    render_span_timeline(frame, app, chunks[2]);
}

/// Render E2E latency sparkline over conversation turns.
fn render_latency_sparkline(frame: &mut Frame, app: &App, area: Rect) {
    let inner_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3), // Stats line
            Constraint::Min(0),    // Sparkline
        ])
        .margin(1)
        .split(area);

    // Collect E2E latencies for all assistant turns
    let latencies: Vec<u64> = app
        .analysis
        .assistant_turns()
        .iter()
        .map(|t| {
            t.metrics
                .e2e_latency
                .map(|e| (e * 1000.0) as u64)
                .unwrap_or(0)
        })
        .collect();

    if latencies.is_empty() {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" E2E Latency Over Time ");
        let paragraph = Paragraph::new("  No latency data available").block(block);
        frame.render_widget(paragraph, area);
        return;
    }

    let max_latency = *latencies.iter().max().unwrap_or(&0);
    let min_latency = *latencies.iter().filter(|&&x| x > 0).min().unwrap_or(&0);
    let avg_latency: u64 = if !latencies.is_empty() {
        latencies.iter().sum::<u64>() / latencies.len() as u64
    } else {
        0
    };

    // Stats line
    let stats_line = Line::from(vec![
        Span::styled("  Min: ", Style::default().fg(Color::Gray)),
        Span::styled(format!("{}ms", min_latency), Style::default().fg(Color::Green)),
        Span::raw("  |  "),
        Span::styled("Avg: ", Style::default().fg(Color::Gray)),
        Span::styled(format!("{}ms", avg_latency), Style::default().fg(latency_color(avg_latency as f64))),
        Span::raw("  |  "),
        Span::styled("Max: ", Style::default().fg(Color::Gray)),
        Span::styled(format!("{}ms", max_latency), Style::default().fg(latency_color(max_latency as f64))),
        Span::raw("  |  "),
        Span::styled("Turns: ", Style::default().fg(Color::Gray)),
        Span::raw(format!("{}", latencies.len())),
    ]);

    let stats_para = Paragraph::new(stats_line);
    frame.render_widget(stats_para, inner_chunks[0]);

    // Sparkline
    let sparkline = Sparkline::default()
        .data(&latencies)
        .max(max_latency.max(1))
        .style(Style::default().fg(Color::Cyan))
        .bar_set(symbols::bar::NINE_LEVELS);

    frame.render_widget(sparkline, inner_chunks[1]);

    // Border
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" E2E Latency Over Time (ms) ")
        .border_style(Style::default().fg(Color::Cyan));
    frame.render_widget(block, area);
}

/// Render component breakdown as horizontal bars.
fn render_component_bars(frame: &mut Frame, app: &App, area: Rect) {
    let assistant_turns = app.analysis.assistant_turns();

    // Calculate averages
    let llm_values: Vec<f64> = assistant_turns
        .iter()
        .filter_map(|t| t.metrics.llm_node_ttft)
        .collect();
    let tts_values: Vec<f64> = assistant_turns
        .iter()
        .filter_map(|t| t.metrics.tts_node_ttfb)
        .collect();
    let e2e_values: Vec<f64> = assistant_turns
        .iter()
        .filter_map(|t| t.metrics.e2e_latency)
        .collect();

    let avg_llm = if !llm_values.is_empty() {
        (llm_values.iter().sum::<f64>() / llm_values.len() as f64 * 1000.0) as u64
    } else {
        0
    };
    let avg_tts = if !tts_values.is_empty() {
        (tts_values.iter().sum::<f64>() / tts_values.len() as f64 * 1000.0) as u64
    } else {
        0
    };
    let avg_e2e = if !e2e_values.is_empty() {
        (e2e_values.iter().sum::<f64>() / e2e_values.len() as f64 * 1000.0) as u64
    } else {
        0
    };
    let avg_other = avg_e2e.saturating_sub(avg_llm + avg_tts);

    let data = vec![
        ("E2E", avg_e2e),
        ("LLM", avg_llm),
        ("TTS", avg_tts),
        ("Other", avg_other),
    ];

    let bar_chart = BarChart::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Average Latency by Component (ms) "),
        )
        .data(&data)
        .bar_width(8)
        .bar_gap(2)
        .bar_style(Style::default().fg(Color::Yellow))
        .value_style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
        .label_style(Style::default().fg(Color::Cyan));

    frame.render_widget(bar_chart, area);
}

/// Render span timeline visualization.
fn render_span_timeline(frame: &mut Frame, app: &App, area: Rect) {
    let session_start = app.analysis.session_start;
    let session_duration = app.analysis.duration_sec();

    if session_duration <= 0.0 {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Span Timeline ");
        let paragraph = Paragraph::new("  No timeline data available").block(block);
        frame.render_widget(paragraph, area);
        return;
    }

    // Get key spans grouped by type
    let key_spans: Vec<_> = app
        .analysis
        .spans
        .iter()
        .filter(|s| {
            matches!(
                s.name.as_str(),
                "agent_turn" | "user_turn" | "llm_node" | "tts_node" | "tts_request"
            )
        })
        .collect();

    let inner_area = Rect {
        x: area.x + 1,
        y: area.y + 2,
        width: area.width.saturating_sub(2),
        height: area.height.saturating_sub(3),
    };

    let timeline_width = inner_area.width.saturating_sub(12) as f64;

    let mut lines: Vec<Line> = Vec::new();

    // Header with time markers
    let mut header_spans = vec![Span::styled("            ", Style::default())];
    let time_markers = 5;
    let marker_interval = session_duration / time_markers as f64;
    for i in 0..=time_markers {
        let time = i as f64 * marker_interval;
        let label = format!("{:>5.1}s", time);
        // Simplified - just show markers at intervals
        if i < time_markers {
            let spacing = (timeline_width / time_markers as f64) as usize;
            header_spans.push(Span::styled(
                format!("{:<width$}", label, width = spacing),
                Style::default().fg(Color::DarkGray),
            ));
        } else {
            header_spans.push(Span::styled(label, Style::default().fg(Color::DarkGray)));
        }
    }
    lines.push(Line::from(header_spans));
    lines.push(Line::from(""));

    // Render each span type on its own row
    let span_types = [
        ("user_turn", "User     ", Color::Green),
        ("agent_turn", "Agent    ", Color::Cyan),
        ("llm_node", "LLM      ", Color::Yellow),
        ("tts_node", "TTS      ", Color::Magenta),
        ("tts_request", "TTS Req  ", Color::Blue),
    ];

    for (span_name, label, color) in &span_types {
        let type_spans: Vec<_> = key_spans
            .iter()
            .filter(|s| s.name == *span_name)
            .collect();

        if type_spans.is_empty() {
            continue;
        }

        // Build the timeline bar
        let mut bar = vec![' '; timeline_width as usize];

        for span in type_spans {
            let start_offset = span.start_sec() - session_start;
            let end_offset = span.end_sec() - session_start;

            let start_pos = ((start_offset / session_duration) * timeline_width).max(0.0) as usize;
            let end_pos = ((end_offset / session_duration) * timeline_width).min(timeline_width) as usize;

            for i in start_pos..end_pos.min(bar.len()) {
                bar[i] = '█';
            }
        }

        let bar_string: String = bar.into_iter().collect();

        lines.push(Line::from(vec![
            Span::styled(format!("{}", label), Style::default().fg(*color)),
            Span::styled(" │", Style::default().fg(Color::DarkGray)),
            Span::styled(bar_string, Style::default().fg(*color)),
            Span::styled("│", Style::default().fg(Color::DarkGray)),
        ]));
    }

    // Add legend
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  Legend: ", Style::default().fg(Color::Gray)),
        Span::styled("█ User ", Style::default().fg(Color::Green)),
        Span::styled("█ Agent ", Style::default().fg(Color::Cyan)),
        Span::styled("█ LLM ", Style::default().fg(Color::Yellow)),
        Span::styled("█ TTS ", Style::default().fg(Color::Magenta)),
    ]));

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Span Timeline ({:.1}s total) ", session_duration));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

/// Render latency distribution histogram.
fn render_distribution(frame: &mut Frame, app: &App, area: Rect) {
    let assistant_turns = app.analysis.assistant_turns();
    let latencies: Vec<f64> = assistant_turns
        .iter()
        .filter_map(|t| t.metrics.e2e_latency)
        .map(|e| e * 1000.0)
        .collect();

    if latencies.is_empty() {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Latency Distribution ");
        let paragraph = Paragraph::new("  No latency data").block(block);
        frame.render_widget(paragraph, area);
        return;
    }

    // Create histogram buckets
    let buckets = [
        (0.0, 500.0, "<500"),
        (500.0, 1000.0, "500-1k"),
        (1000.0, 1500.0, "1-1.5k"),
        (1500.0, 2000.0, "1.5-2k"),
        (2000.0, 3000.0, "2-3k"),
        (3000.0, f64::MAX, ">3k"),
    ];

    let counts: Vec<(&str, u64)> = buckets
        .iter()
        .map(|(min, max, label)| {
            let count = latencies
                .iter()
                .filter(|&&l| l >= *min && l < *max)
                .count() as u64;
            (*label, count)
        })
        .collect();

    let bar_chart = BarChart::default()
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" E2E Latency Distribution "),
        )
        .data(&counts)
        .bar_width(7)
        .bar_gap(1)
        .bar_style(Style::default().fg(Color::Cyan))
        .value_style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
        .label_style(Style::default().fg(Color::Gray));

    frame.render_widget(bar_chart, area);
}
