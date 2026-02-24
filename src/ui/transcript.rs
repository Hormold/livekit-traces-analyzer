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

        // Handle function_call: show tool name and summarized arguments
        if turn.turn_type == "function_call" {
            let fn_name = turn.extra
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            let args_summary = turn.extra
                .get("arguments")
                .map(|v| summarize_tool_args(v))
                .unwrap_or_default();

            lines.push(Line::from(vec![
                Span::styled(format!("  [{}] ", i + 1), Style::default().fg(Color::DarkGray)),
                Span::styled("[T] ", Style::default().fg(Color::Yellow)),
                Span::styled("TOOL: ", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
                Span::styled(fn_name.to_string(), Style::default().fg(Color::White).add_modifier(Modifier::BOLD)),
                Span::styled(format!("({})", args_summary), Style::default().fg(Color::DarkGray)),
            ]));
            lines.push(Line::from(""));
            continue;
        }

        // Handle function_call_output: show compactly
        if turn.turn_type == "function_call_output" {
            let fn_name = turn.extra
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            let output = turn.extra
                .get("output")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // Show compact output -- truncate if long
            let display_output = if output.is_empty() || output == "ok" || output == "\"ok\"" {
                "ok".to_string()
            } else if output.len() > 60 {
                format!("{}...", &output[..57])
            } else {
                output.to_string()
            };

            lines.push(Line::from(vec![
                Span::styled(format!("  [{}] ", i + 1), Style::default().fg(Color::DarkGray)),
                Span::styled("[T] ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{} -> ", fn_name), Style::default().fg(Color::DarkGray)),
                Span::styled(display_output, Style::default().fg(Color::DarkGray)),
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

        // Show E2E breakdown if available
        if let Some(ref bd) = turn.breakdown {
            if bd.has_tool_call || bd.overhead_ms.map(|v| v > 500.0).unwrap_or(false) {
                let mut bd_spans: Vec<Span> = vec![Span::raw("      ")];
                bd_spans.push(Span::styled("breakdown: ", Style::default().fg(Color::DarkGray)));

                let mut parts: Vec<Span> = Vec::new();
                if let Some(stt) = bd.stt_ms {
                    parts.push(Span::styled(format!("stt={:.0}ms", stt), Style::default().fg(Color::Gray)));
                }
                if let Some(eol) = bd.eol_ms {
                    parts.push(Span::styled(format!("eol={:.0}ms", eol), Style::default().fg(
                        if eol > 1000.0 { Color::Red } else if eol > 500.0 { Color::Yellow } else { Color::Gray }
                    )));
                }
                if let Some(fl) = bd.first_llm_ms {
                    parts.push(Span::styled(format!("llm1={:.0}ms", fl), Style::default().fg(latency_color(fl))));
                }
                if let Some(tool) = bd.tool_ms {
                    let tool_color = if tool > 500.0 { Color::Red } else if tool > 100.0 { Color::Yellow } else { Color::Gray };
                    parts.push(Span::styled(format!("tool={:.0}ms", tool), Style::default().fg(tool_color)));
                }
                if let Some(llm) = bd.llm_ms {
                    parts.push(Span::styled(format!("llm2={:.0}ms", llm), Style::default().fg(latency_color(llm))));
                }
                if let Some(tts) = bd.tts_ms {
                    parts.push(Span::styled(format!("tts={:.0}ms", tts), Style::default().fg(latency_color(tts))));
                }
                if let Some(oh) = bd.overhead_ms {
                    parts.push(Span::styled(format!("other={:.0}ms", oh), Style::default().fg(Color::Yellow)));
                }

                for (idx, part) in parts.into_iter().enumerate() {
                    if idx > 0 {
                        bd_spans.push(Span::styled(" -> ", Style::default().fg(Color::DarkGray)));
                    }
                    bd_spans.push(part);
                }

                lines.push(Line::from(bd_spans));
            }
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

/// Summarize tool call arguments into a compact string.
///
/// The arguments value can be either a JSON string (e.g. `"{ \"q\": \"q1\", ... }"`)
/// or a JSON object. We extract key-value pairs and produce a short summary like:
/// `q1, "answer text..."` -- truncating long values.
fn summarize_tool_args(value: &serde_json::Value) -> String {
    use serde_json::Value;

    // Parse the arguments -- could be a JSON string or already an object
    let obj = match value {
        Value::String(s) => {
            match serde_json::from_str::<Value>(s) {
                Ok(Value::Object(map)) => map,
                _ => return truncate_str(s, 80),
            }
        }
        Value::Object(map) => map.clone(),
        _ => return value.to_string(),
    };

    if obj.is_empty() {
        return String::new();
    }

    let mut parts: Vec<String> = Vec::new();
    let mut total_len = 0;
    let max_total = 80;

    for (key, val) in &obj {
        if total_len > max_total {
            parts.push("...".to_string());
            break;
        }

        let val_str = match val {
            Value::String(s) => {
                // Show short strings directly, truncate long ones
                if s.len() > 40 {
                    format!("\"{}...\"", &s[..37])
                } else {
                    format!("\"{}\"", s)
                }
            }
            Value::Bool(b) => b.to_string(),
            Value::Number(n) => n.to_string(),
            Value::Null => "null".to_string(),
            _ => {
                let s = val.to_string();
                if s.len() > 30 {
                    format!("{}...", &s[..27])
                } else {
                    s
                }
            }
        };

        let part = format!("{}={}", key, val_str);
        total_len += part.len() + 2;
        parts.push(part);
    }

    parts.join(", ")
}

/// Truncate a string to a maximum length, adding "..." if truncated.
fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}
