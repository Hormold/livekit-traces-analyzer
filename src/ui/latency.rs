//! Latency statistics view.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Row, Table},
    Frame,
};

use crate::app::{App, LatencySortMode};
use crate::data::LatencyStats;
use super::latency_color;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(10), // Stats table
            Constraint::Min(0),     // Slow turns
        ])
        .split(area);

    render_stats_table(frame, app, chunks[0]);
    render_slow_turns(frame, app, chunks[1]);
}

fn render_stats_table(frame: &mut Frame, app: &App, area: Rect) {
    let header = Row::new(vec!["Component", "Avg", "Min", "Max", "P95", "Count"])
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .bottom_margin(1);

    let rows = vec![
        stats_row("E2E Latency", &app.e2e_stats),
        stats_row("LLM TTFT", &app.llm_stats),
        stats_row("TTS TTFB", &app.tts_stats),
    ];

    let widths = [
        Constraint::Length(15),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(10),
        Constraint::Length(8),
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" Latency Statistics "),
        );

    frame.render_widget(table, area);
}

fn stats_row<'a>(name: &'a str, stats: &Option<LatencyStats>) -> Row<'a> {
    match stats {
        Some(s) => {
            Row::new(vec![
                name.to_string(),
                format!("{:.0}ms", s.avg_ms),
                format!("{:.0}ms", s.min_ms),
                format!("{:.0}ms", s.max_ms),
                format!("{:.0}ms", s.p95_ms),
                format!("{}", s.count),
            ])
        }
        None => Row::new(vec![name.to_string(), "-".into(), "-".into(), "-".into(), "-".into(), "0".into()]),
    }
}

fn render_slow_turns(frame: &mut Frame, app: &App, area: Rect) {
    let assistant_turns = app.analysis.assistant_turns();
    let slow_turns: Vec<_> = assistant_turns
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            t.metrics.e2e_latency.filter(|&e| e > 2.0).map(|e| (i + 1, *t, e))
        })
        .collect();

    // Sort indicator
    let sort_label = app.latency_sort.label();
    let sort_arrow = match app.latency_sort {
        LatencySortMode::ByTurn => "^",
        _ => "v", // descending for latency values
    };

    let mut lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!("  {} turns with E2E > 2 seconds", slow_turns.len()),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::raw("  |  "),
            Span::styled("Sort: ", Style::default().fg(Color::Gray)),
            Span::styled(format!("{} {}", sort_label, sort_arrow), Style::default().fg(Color::Cyan)),
            Span::styled(" (s)", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(""),
    ];

    if slow_turns.is_empty() {
        lines.push(Line::from(Span::styled(
            "  All turns have acceptable latency!",
            Style::default().fg(Color::Green),
        )));
    } else {
        // Sort based on current mode
        let mut sorted = slow_turns;
        match app.latency_sort {
            LatencySortMode::ByLatency => {
                sorted.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
            }
            LatencySortMode::ByTurn => {
                sorted.sort_by_key(|(turn_num, _, _)| *turn_num);
            }
            LatencySortMode::ByLLM => {
                sorted.sort_by(|a, b| {
                    let a_llm = a.1.metrics.llm_node_ttft.unwrap_or(0.0);
                    let b_llm = b.1.metrics.llm_node_ttft.unwrap_or(0.0);
                    b_llm.partial_cmp(&a_llm).unwrap_or(std::cmp::Ordering::Equal)
                });
            }
            LatencySortMode::ByTTS => {
                sorted.sort_by(|a, b| {
                    let a_tts = a.1.metrics.tts_node_ttfb.unwrap_or(0.0);
                    let b_tts = b.1.metrics.tts_node_ttfb.unwrap_or(0.0);
                    b_tts.partial_cmp(&a_tts).unwrap_or(std::cmp::Ordering::Equal)
                });
            }
        }

        for (turn_num, turn, e2e) in sorted.iter().take(15) {
            let e2e_ms = e2e * 1000.0;
            let llm_ms = turn.metrics.llm_node_ttft.unwrap_or(0.0) * 1000.0;
            let tts_ms = turn.metrics.tts_node_ttfb.unwrap_or(0.0) * 1000.0;

            lines.push(Line::from(vec![
                Span::styled(format!("  Turn {:3}: ", turn_num), Style::default().fg(Color::Gray)),
                Span::styled(format!("E2E={:5.0}ms", e2e_ms), Style::default().fg(latency_color(e2e_ms))),
                Span::raw("  "),
                Span::styled(format!("LLM={:5.0}ms", llm_ms), Style::default().fg(latency_color(llm_ms))),
                Span::raw("  "),
                Span::styled(format!("TTS={:5.0}ms", tts_ms), Style::default().fg(latency_color(tts_ms))),
            ]));

            let text = turn.text();
            let preview = super::truncate(&text, 60);
            lines.push(Line::from(Span::styled(
                format!("           \"{}\"", preview),
                Style::default().fg(Color::DarkGray),
            )));
        }

        if sorted.len() > 15 {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                format!("  ... and {} more slow turns", sorted.len() - 15),
                Style::default().fg(Color::DarkGray),
            )));
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Slow Turns (>2s E2E) ");

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}
