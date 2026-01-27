//! Transcript view - conversation with metrics.

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::app::App;
use super::latency_color;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();
    let scroll = app.transcript_scroll;

    // Calculate visible range
    let visible_height = area.height.saturating_sub(2) as usize;

    for (i, turn) in app.analysis.turns.iter().enumerate() {
        // Skip turns before scroll position (approximate)
        if i < scroll {
            continue;
        }

        // Handle agent handoff
        if turn.turn_type == "agent_handoff" {
            let new_agent = turn.extra
                .get("new_agent_id")
                .and_then(|v| v.as_str())
                .unwrap_or(&turn.id);

            lines.push(Line::from(vec![
                Span::styled(format!("  [{}] ", i + 1), Style::default().fg(Color::DarkGray)),
                Span::styled("[->] ", Style::default().fg(Color::Magenta)),
                Span::styled(format!("Agent handoff: {}", new_agent), Style::default().fg(Color::DarkGray)),
            ]));
            lines.push(Line::from(""));
            continue;
        }

        let (role_icon, role_color) = match turn.role.as_deref() {
            Some("user") => ("[U]", Color::Green),
            Some("assistant") => ("[A]", Color::Cyan),
            _ => ("[?]", Color::Gray),
        };

        let role_label = turn.role.as_deref().unwrap_or("unknown").to_uppercase();

        // Interrupted marker
        let interrupt_marker = if turn.interrupted {
            Span::styled(" [INTERRUPTED]", Style::default().fg(Color::Red))
        } else {
            Span::raw("")
        };

        // Turn header
        lines.push(Line::from(vec![
            Span::styled(format!("  [{}] ", i + 1), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{} ", role_icon), Style::default().fg(role_color)),
            Span::styled(role_label, Style::default().fg(role_color).add_modifier(Modifier::BOLD)),
            interrupt_marker,
        ]));

        // Metrics line
        let mut metrics_spans: Vec<Span> = vec![Span::raw("      ")];

        if let Some(e2e) = turn.metrics.e2e_latency {
            let ms = e2e * 1000.0;
            metrics_spans.push(Span::styled(
                format!("E2E:{:.0}ms", ms),
                Style::default().fg(latency_color(ms)),
            ));
            metrics_spans.push(Span::raw("  "));
        }

        if let Some(llm) = turn.metrics.llm_node_ttft {
            let ms = llm * 1000.0;
            metrics_spans.push(Span::styled(
                format!("LLM:{:.0}ms", ms),
                Style::default().fg(latency_color(ms)),
            ));
            metrics_spans.push(Span::raw("  "));
        }

        if let Some(tts) = turn.metrics.tts_node_ttfb {
            let ms = tts * 1000.0;
            metrics_spans.push(Span::styled(
                format!("TTS:{:.0}ms", ms),
                Style::default().fg(latency_color(ms)),
            ));
            metrics_spans.push(Span::raw("  "));
        }

        if let Some(dur) = turn.metrics.speaking_duration() {
            metrics_spans.push(Span::styled(
                format!("dur:{:.1}s", dur),
                Style::default().fg(Color::Gray),
            ));
            metrics_spans.push(Span::raw("  "));
        }

        if let Some(conf) = turn.metrics.transcript_confidence {
            let pct = conf * 100.0;
            let color = if pct > 95.0 {
                Color::Green
            } else if pct > 85.0 {
                Color::Yellow
            } else {
                Color::Red
            };
            metrics_spans.push(Span::styled(
                format!("conf:{:.0}%", pct),
                Style::default().fg(color),
            ));
        }

        if metrics_spans.len() > 1 {
            lines.push(Line::from(metrics_spans));
        }

        // Content
        let text = turn.text();
        if !text.is_empty() {
            // Word wrap content manually
            let max_width = area.width.saturating_sub(10) as usize;
            let words: Vec<&str> = text.split_whitespace().collect();
            let mut current_line = String::from("      ");

            for word in words {
                if current_line.len() + word.len() + 1 > max_width {
                    lines.push(Line::from(Span::raw(current_line)));
                    current_line = format!("      {}", word);
                } else {
                    if current_line.len() > 6 {
                        current_line.push(' ');
                    }
                    current_line.push_str(word);
                }
            }
            if current_line.len() > 6 {
                lines.push(Line::from(Span::raw(current_line)));
            }
        }

        lines.push(Line::from(""));

        // Stop if we have enough lines
        if lines.len() > visible_height + 10 {
            break;
        }
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" Transcript ({} turns) ", app.analysis.turns.len()));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}
