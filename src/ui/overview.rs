//! Overview view - diagnosis, metadata, summary, pipeline analysis.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap},
    Frame,
};

use crate::app::App;
use crate::data::{DiagnosisVerdict, PipelineCycle, PipelineSummary, Severity};
use super::format_duration;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(12), // Top row: Metadata + Pipeline side by side
            Constraint::Min(0),      // Diagnosis details
        ])
        .split(area);

    // Split top row horizontally
    let top_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(55), // Metadata
            Constraint::Percentage(45), // Pipeline analysis
        ])
        .split(chunks[0]);

    render_metadata(frame, app, top_chunks[0]);
    render_pipeline_analysis(frame, app, top_chunks[1]);
    render_diagnosis(frame, app, chunks[1]);
}

fn render_metadata(frame: &mut Frame, app: &App, area: Rect) {
    let a = &app.analysis;
    let diagnosis = app.analysis.diagnosis.as_ref();

    // Verdict
    let (verdict_text, verdict_color) = match diagnosis.map(|d| &d.verdict) {
        Some(DiagnosisVerdict::Healthy) => ("✓ HEALTHY", Color::Green),
        Some(DiagnosisVerdict::NeedsAttention) => ("⚡ ATTENTION", Color::Yellow),
        Some(DiagnosisVerdict::Problematic) => ("⚠️ PROBLEMATIC", Color::Red),
        None => ("?", Color::Gray),
    };

    let lines = vec![
        Line::from(vec![
            Span::styled(format!(" {} ", verdict_text), Style::default().fg(verdict_color).add_modifier(Modifier::BOLD)),
            Span::styled("│ Duration: ", Style::default().fg(Color::DarkGray)),
            Span::styled(format_duration(a.duration_sec()), Style::default().fg(Color::White)),
        ]),
        Line::from(vec![
            Span::styled(" Room ID:     ", Style::default().fg(Color::Gray)),
            Span::raw(&a.room_id),
        ]),
        Line::from(vec![
            Span::styled(" Job ID:      ", Style::default().fg(Color::Gray)),
            Span::raw(&a.job_id),
        ]),
        Line::from(vec![
            Span::styled(" Agent:       ", Style::default().fg(Color::Gray)),
            Span::raw(&a.agent_name),
        ]),
        Line::from(vec![
            Span::styled(" Room Name:   ", Style::default().fg(Color::Gray)),
            Span::raw(&a.room_name),
        ]),
        Line::from(vec![
            Span::styled(" Participant: ", Style::default().fg(Color::Gray)),
            Span::raw(&a.participant_identity),
        ]),
        Line::from(vec![
            Span::styled(" Turns: ", Style::default().fg(Color::Gray)),
            Span::raw(format!("{}", a.turns.len())),
            Span::styled(" (👤", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{}", a.user_turns().len())),
            Span::styled(" 🤖", Style::default().fg(Color::DarkGray)),
            Span::raw(format!("{})", a.assistant_turns().len())),
            Span::styled(" │ Err:", Style::default().fg(Color::Gray)),
            Span::styled(format!("{}", a.errors.len()), if a.errors.is_empty() { Style::default().fg(Color::Green) } else { Style::default().fg(Color::Red) }),
            Span::styled(" Warn:", Style::default().fg(Color::Gray)),
            Span::styled(format!("{}", a.warnings.len()), if a.warnings.is_empty() { Style::default().fg(Color::Green) } else { Style::default().fg(Color::Yellow) }),
        ]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(verdict_color))
        .title(" Call Overview ");

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_diagnosis(frame: &mut Frame, app: &App, area: Rect) {
    let diagnosis = match &app.analysis.diagnosis {
        Some(d) => d,
        None => {
            let block = Block::default().borders(Borders::ALL).title(" Diagnosis ");
            let paragraph = Paragraph::new("  No diagnosis available").block(block);
            frame.render_widget(paragraph, area);
            return;
        }
    };

    let mut lines = vec![Line::from("")];

    // Pipeline Summary - human readable explanation
    let cycles = &app.analysis.pipeline_cycles;
    if !cycles.is_empty() {
        lines.extend(generate_pipeline_summary(cycles));
        lines.push(Line::from(""));
    }

    // Primary issue
    if let Some(ref issue) = diagnosis.primary_issue {
        lines.push(Line::from(vec![
            Span::styled("  PRIMARY ISSUE: ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled(issue.as_str(), Style::default().fg(Color::Yellow)),
        ]));
        if let Some(ref detail) = diagnosis.primary_issue_detail {
            lines.push(Line::from(vec![
                Span::raw("    -> "),
                Span::styled(detail.as_str(), Style::default().fg(Color::Gray)),
            ]));
        }
        lines.push(Line::from(""));
    }

    // Breakdown by cause
    let cause_icons = [
        ("LLM", "[LLM]"),
        ("TTS", "[TTS]"),
        ("TOOL", "[TOOL]"),
        ("STT", "[STT]"),
        ("OVERHEAD", "[GAP]"),
    ];

    for (cause, icon) in &cause_icons {
        if let Some(turns) = diagnosis.slow_turns_by_cause.get(*cause) {
            if turns.is_empty() {
                continue;
            }

            let color = match *cause {
                "LLM" | "TTS" | "TOOL" => Color::Red,
                "OVERHEAD" => Color::Yellow,
                _ => Color::Yellow,
            };

            let cause_label = match *cause {
                "OVERHEAD" => "PROCESSING GAPS",
                other => other,
            };

            let cause_hint = match *cause {
                "OVERHEAD" => " (time between stages, streaming, network)",
                "LLM" => " (model inference)",
                "TTS" => " (speech synthesis)",
                "TOOL" => " (function execution)",
                _ => "",
            };

            lines.push(Line::from(vec![
                Span::styled(format!("  {} {}: ", icon, cause_label), Style::default().fg(color).add_modifier(Modifier::BOLD)),
                Span::styled(format!("{} slow turns", turns.len()), Style::default().fg(color)),
                Span::styled(cause_hint, Style::default().fg(Color::DarkGray)),
            ]));

            for t in turns.iter().take(3) {
                let tool_info = t.tool_name.as_ref().map(|n| format!(" [tool: {}]", n)).unwrap_or_default();

                // Show breakdown: E2E = LLM + TTS + gaps
                let gap_ms = t.unexplained_ms;
                lines.push(Line::from(vec![
                    Span::styled(format!("    Turn {}: ", t.turn), Style::default().fg(color)),
                    Span::styled(format!("{:.1}s total", t.e2e_ms / 1000.0), Style::default().fg(Color::White)),
                    Span::styled(" = ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("LLM {:.1}s", t.llm_ms / 1000.0), Style::default().fg(if t.llm_ms > 2000.0 { Color::Red } else { Color::Gray })),
                    Span::styled(" + ", Style::default().fg(Color::DarkGray)),
                    Span::styled(format!("TTS {:.1}s", t.tts_ms / 1000.0), Style::default().fg(if t.tts_ms > 3000.0 { Color::Red } else { Color::Gray })),
                    Span::styled(format!(" + gaps {:.1}s", gap_ms / 1000.0), Style::default().fg(if gap_ms > 1000.0 { Color::Yellow } else { Color::DarkGray })),
                    Span::styled(tool_info, Style::default().fg(Color::Magenta)),
                ]));

                let preview = super::truncate(&t.text, 50);
                lines.push(Line::from(vec![
                    Span::styled(format!("      \"{}\"", preview), Style::default().fg(Color::DarkGray)),
                ]));
            }

            if turns.len() > 3 {
                lines.push(Line::from(vec![
                    Span::styled(format!("    ... and {} more {} slow turns", turns.len() - 3, cause), Style::default().fg(Color::DarkGray)),
                ]));
            }
            lines.push(Line::from(""));
        }
    }

    // TTS retries and tool errors
    if diagnosis.tts_retries > 0 {
        lines.push(Line::from(vec![
            Span::styled("  [TTS] TTS RETRIES: ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{}x synthesis failures", diagnosis.tts_retries), Style::default().fg(Color::Red)),
        ]));
        lines.push(Line::from(""));
    }

    if diagnosis.tool_errors > 0 {
        lines.push(Line::from(vec![
            Span::styled("  [TOOL] TOOL ERRORS: ", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::styled(format!("{} failures", diagnosis.tool_errors), Style::default().fg(Color::Red)),
        ]));
        lines.push(Line::from(""));
    }

    // Calculate scroll indicator
    let total_lines = lines.len();
    let scroll_pos = app.overview_scroll;
    let title = if total_lines > area.height as usize {
        format!(" Diagnosis Details (↓↑ to scroll, line {}/{}) ", scroll_pos + 1, total_lines)
    } else {
        " Diagnosis Details ".to_string()
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(title);

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false })
        .scroll((scroll_pos as u16, 0));
    frame.render_widget(paragraph, area);
}

/// Convert Severity to ratatui Color.
fn severity_color(severity: Severity) -> Color {
    match severity {
        Severity::Good => Color::Green,
        Severity::Warning => Color::Yellow,
        Severity::Critical => Color::Red,
    }
}

/// Generate human-readable pipeline summary using PipelineSummary.
fn generate_pipeline_summary(cycles: &[PipelineCycle]) -> Vec<Line<'static>> {
    let summary = match PipelineSummary::from_cycles(cycles) {
        Some(s) => s,
        None => return vec![],
    };

    let mut lines = Vec::new();

    // Header
    lines.push(Line::from(vec![
        Span::styled("  PIPELINE ANALYSIS", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
    ]));

    // Response time summary
    lines.push(Line::from(vec![
        Span::styled("  Response time: ", Style::default().fg(Color::White)),
        Span::styled(
            format!("{:.1}s avg", summary.avg_total_ms / 1000.0),
            Style::default().fg(severity_color(summary.total_severity)).add_modifier(Modifier::BOLD)
        ),
        Span::styled(format!(" (max {:.1}s) - ", summary.max_total_ms / 1000.0), Style::default().fg(Color::DarkGray)),
        Span::styled(summary.total_verdict, Style::default().fg(severity_color(summary.total_severity))),
    ]));

    // Breakdown with explanations
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  Where time goes:", Style::default().fg(Color::White)),
    ]));

    // LLM
    lines.push(Line::from(vec![
        Span::styled("    LLM: ", Style::default().fg(Color::Gray)),
        Span::styled(format!("{:.1}s", summary.avg_llm_ms / 1000.0), Style::default().fg(severity_color(summary.llm_severity))),
        Span::styled(format!(" ({:.0}%) - ", summary.llm_pct), Style::default().fg(Color::DarkGray)),
        Span::styled(summary.llm_verdict, Style::default().fg(severity_color(summary.llm_severity))),
    ]));

    // TTS
    lines.push(Line::from(vec![
        Span::styled("    TTS: ", Style::default().fg(Color::Gray)),
        Span::styled(format!("{:.1}s", summary.avg_tts_ms / 1000.0), Style::default().fg(severity_color(summary.tts_severity))),
        Span::styled(format!(" ({:.0}%) - ", summary.tts_pct), Style::default().fg(Color::DarkGray)),
        Span::styled(summary.tts_verdict, Style::default().fg(severity_color(summary.tts_severity))),
    ]));

    // User→LLM (only if we have user-initiated turns)
    if summary.user_turn_count > 0 {
        lines.push(Line::from(vec![
            Span::styled("    Perception: ", Style::default().fg(Color::Gray)),
            Span::styled(format!("{:.0}ms", summary.avg_user_to_llm_ms), Style::default().fg(severity_color(summary.perception_severity))),
            Span::styled(format!(" ({} user turns) - ", summary.user_turn_count), Style::default().fg(Color::DarkGray)),
            Span::styled(summary.perception_verdict, Style::default().fg(severity_color(summary.perception_severity))),
        ]));
    }

    // System turns info
    if summary.system_turn_count > 0 {
        lines.push(Line::from(vec![
            Span::styled(format!("    System-initiated: {} turns", summary.system_turn_count), Style::default().fg(Color::DarkGray)),
            Span::styled(" (greeting, tool responses)", Style::default().fg(Color::DarkGray)),
        ]));
    }

    // Detected delays
    if !summary.detected_delays.is_empty() {
        lines.push(Line::from(""));
        lines.push(Line::from(vec![
            Span::styled("  Detected delays:", Style::default().fg(Color::Yellow)),
        ]));

        for delay in summary.detected_delays.iter().take(3) {
            let reason_color = if delay.is_tool_related { Color::Magenta } else { Color::Yellow };
            lines.push(Line::from(vec![
                Span::styled(format!("    Turn {}: ", delay.turn_number), Style::default().fg(Color::Gray)),
                Span::styled(format!("+{:.1}s gap", delay.gap_ms / 1000.0), Style::default().fg(Color::Yellow)),
                Span::styled(" → ", Style::default().fg(Color::DarkGray)),
                Span::styled(delay.reason.clone(), Style::default().fg(reason_color)),
            ]));
        }

        if summary.detected_delays.len() > 3 {
            lines.push(Line::from(vec![
                Span::styled(format!("    ... and {} more", summary.detected_delays.len() - 3), Style::default().fg(Color::DarkGray)),
            ]));
        }
    }

    lines
}

/// Color for User→LLM delay (perception delay).
fn user_to_llm_color(ms: f64) -> Color {
    if ms < 100.0 {
        Color::Green
    } else if ms < 200.0 {
        Color::Yellow
    } else {
        Color::Red
    }
}

/// Color for LLM latency.
fn llm_color(ms: f64) -> Color {
    if ms < 1500.0 {
        Color::Green
    } else if ms < 3000.0 {
        Color::Yellow
    } else {
        Color::Red
    }
}

/// Color for TTS latency.
fn tts_color(ms: f64) -> Color {
    if ms < 2000.0 {
        Color::Green
    } else if ms < 4000.0 {
        Color::Yellow
    } else {
        Color::Red
    }
}

/// Color for total latency.
fn total_color(ms: f64) -> Color {
    if ms < 4000.0 {
        Color::Green
    } else if ms < 8000.0 {
        Color::Yellow
    } else {
        Color::Red
    }
}

/// Render the pipeline analysis section with per-turn breakdown.
fn render_pipeline_analysis(frame: &mut Frame, app: &App, area: Rect) {
    let cycles = &app.analysis.pipeline_cycles;

    let summary = match PipelineSummary::from_cycles(cycles) {
        Some(s) => s,
        None => {
            let block = Block::default()
                .borders(Borders::ALL)
                .title(" Pipeline Analysis ");
            let paragraph = Paragraph::new("  No pipeline cycles detected")
                .block(block);
            frame.render_widget(paragraph, area);
            return;
        }
    };

    // Build table rows - show last N turns that fit
    let max_rows = (area.height as usize).saturating_sub(4); // Header + borders + avg
    let display_cycles: Vec<&PipelineCycle> = cycles.iter().rev().take(max_rows).collect();

    let header = Row::new(vec![
        Cell::from("#").style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Cell::from("User→LLM").style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Cell::from("LLM").style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Cell::from("TTS").style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Cell::from("Total").style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
    ]).bottom_margin(0);

    let mut rows: Vec<Row> = display_cycles
        .iter()
        .rev()
        .map(|cycle| {
            // Show "-" for User→LLM if no user turn (system-initiated)
            let user_to_llm_cell = if cycle.has_user_turn {
                Cell::from(format!("{:.0}ms", cycle.user_to_llm_ms.max(0.0)))
                    .style(Style::default().fg(user_to_llm_color(cycle.user_to_llm_ms)))
            } else {
                Cell::from("-").style(Style::default().fg(Color::DarkGray))
            };

            Row::new(vec![
                Cell::from(format!("{}", cycle.turn_number)),
                user_to_llm_cell,
                Cell::from(format!("{:.0}ms", cycle.llm_duration_ms))
                    .style(Style::default().fg(llm_color(cycle.llm_duration_ms))),
                Cell::from(format!("{:.0}ms", cycle.tts_duration_ms))
                    .style(Style::default().fg(tts_color(cycle.tts_duration_ms))),
                Cell::from(format!("{:.0}ms", cycle.total_duration_ms))
                    .style(Style::default().fg(total_color(cycle.total_duration_ms))),
            ])
        })
        .collect();

    // Add average row using summary data
    rows.push(Row::new(vec![
        Cell::from("Avg").style(Style::default().add_modifier(Modifier::BOLD)),
        Cell::from(format!("{:.0}ms", summary.avg_user_to_llm_ms))
            .style(Style::default().fg(severity_color(summary.perception_severity)).add_modifier(Modifier::BOLD)),
        Cell::from(format!("{:.0}ms", summary.avg_llm_ms))
            .style(Style::default().fg(severity_color(summary.llm_severity)).add_modifier(Modifier::BOLD)),
        Cell::from(format!("{:.0}ms", summary.avg_tts_ms))
            .style(Style::default().fg(severity_color(summary.tts_severity)).add_modifier(Modifier::BOLD)),
        Cell::from(format!("{:.0}ms", summary.avg_total_ms))
            .style(Style::default().fg(severity_color(summary.total_severity)).add_modifier(Modifier::BOLD)),
    ]));

    let widths = [
        Constraint::Length(4),   // #
        Constraint::Length(10),  // User→LLM
        Constraint::Length(10),  // LLM
        Constraint::Length(10),  // TTS
        Constraint::Min(8),      // Total
    ];

    // Use bottleneck from summary
    let bottleneck_short = if summary.bottleneck.contains("TTS") {
        "TTS"
    } else if summary.bottleneck.contains("LLM") {
        "LLM"
    } else if summary.bottleneck.contains("Perception") {
        "VAD"
    } else {
        "OK"
    };
    let title = format!(" Pipeline ({}) → {} ", cycles.len(), bottleneck_short);

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(title),
        );

    frame.render_widget(table, area);
}
