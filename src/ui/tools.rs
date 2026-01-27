//! Tools detail view - detailed tool call information with arguments and results.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Row, Table, Wrap},
    Frame,
};

use crate::app::App;
use crate::ui::agents::{extract_tools_with_agent, ToolWithAgent};
use super::format_duration;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let tools = extract_tools_with_agent(app);

    if tools.is_empty() {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Tools ");
        let paragraph = Paragraph::new("  No tool calls found in this session").block(block);
        frame.render_widget(paragraph, area);
        return;
    }

    // Split: table on top (50%), full detail at bottom (50%)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(45),  // Table
            Constraint::Percentage(55),  // Detail view
        ])
        .split(area);

    render_table(frame, app, &tools, chunks[0]);
    render_detail(frame, app, &tools, chunks[1]);
}

fn render_table(frame: &mut Frame, app: &App, tools: &[ToolWithAgent], area: Rect) {
    let scroll = app.tools_scroll;
    let visible_height = area.height.saturating_sub(4) as usize;

    let header = Row::new(vec!["#", "Time", "Tool Name", "Duration", "Agent"])
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .bottom_margin(1);

    let rows: Vec<Row> = tools
        .iter()
        .enumerate()
        .skip(scroll.saturating_sub(2).min(scroll))
        .take(visible_height)
        .map(|(idx, tool)| {
            let rel_time = tool.start_sec - app.analysis.session_start;
            let time_str = format_duration(rel_time);

            // Color based on duration
            let duration_color = if tool.duration_ms < 100.0 {
                Color::Green
            } else if tool.duration_ms < 500.0 {
                Color::Yellow
            } else {
                Color::Red
            };

            // Mark selected row
            let style = if idx == scroll {
                Style::default().fg(Color::Black).bg(duration_color)
            } else {
                Style::default().fg(duration_color)
            };

            Row::new(vec![
                format!("{}", idx + 1),
                time_str,
                tool.name.clone(),
                if tool.duration_ms > 0.0 { format!("{:.0}ms", tool.duration_ms) } else { "-".to_string() },
                tool.agent.clone(),
            ])
            .style(style)
        })
        .collect();

    let widths = [
        Constraint::Length(3),   // #
        Constraint::Length(10),  // Time
        Constraint::Min(25),     // Tool name
        Constraint::Length(10),  // Duration
        Constraint::Length(20),  // Agent
    ];

    let total_duration: f64 = tools.iter().map(|t| t.duration_ms).sum();

    let title = format!(
        " Tools ({}) | {:.1}s total | scroll to see details below ",
        tools.len(),
        total_duration / 1000.0,
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

fn render_detail(frame: &mut Frame, app: &App, tools: &[ToolWithAgent], area: Rect) {
    let scroll = app.tools_scroll;

    if let Some(tool) = tools.get(scroll) {
        // Split detail area into header + input/output sections
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(5),  // Header info
                Constraint::Min(0),     // Input/Output
            ])
            .split(area);

        render_detail_header(frame, app, tool, chunks[0]);
        render_detail_body(frame, app, tool, chunks[1]);
    } else {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Tool Details ");
        let paragraph = Paragraph::new("  No tool selected").block(block);
        frame.render_widget(paragraph, area);
    }
}

fn render_detail_header(frame: &mut Frame, app: &App, tool: &ToolWithAgent, area: Rect) {
    let rel_time = tool.start_sec - app.analysis.session_start;

    // Check if this tool had an error
    let has_error = find_tool_error(app, tool).is_some();

    let duration_color = if tool.duration_ms < 100.0 {
        Color::Green
    } else if tool.duration_ms < 500.0 {
        Color::Yellow
    } else {
        Color::Red
    };

    let status_style = if has_error {
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
    };

    let lines = vec![
        Line::from(vec![
            Span::styled("Tool: ", Style::default().fg(Color::Gray)),
            Span::styled(&tool.name, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled("  Status: ", Style::default().fg(Color::Gray)),
            Span::styled(if has_error { "ERROR" } else { "OK" }, status_style),
        ]),
        Line::from(vec![
            Span::styled("Time: ", Style::default().fg(Color::Gray)),
            Span::styled(format_duration(rel_time), Style::default().fg(Color::Cyan)),
            Span::styled(" | Duration: ", Style::default().fg(Color::Gray)),
            Span::styled(
                if tool.duration_ms > 0.0 { format!("{:.0}ms", tool.duration_ms) } else { "-".to_string() },
                Style::default().fg(duration_color)
            ),
            Span::styled(" | Agent: ", Style::default().fg(Color::Gray)),
            Span::styled(&tool.agent, Style::default().fg(Color::Magenta)),
        ]),
    ];

    let border_color = if has_error { Color::Red } else { Color::Yellow };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(format!(" Tool #{} ", app.tools_scroll + 1));

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_detail_body(frame: &mut Frame, app: &App, tool: &ToolWithAgent, area: Rect) {
    // Split into Input and Output/Error sections
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(50), // Input (args)
            Constraint::Percentage(50), // Output/Error
        ])
        .split(area);

    render_tool_input(frame, app, tool, chunks[0]);
    render_tool_output(frame, app, tool, chunks[1]);
}

fn render_tool_input(frame: &mut Frame, app: &App, tool: &ToolWithAgent, area: Rect) {
    let max_width = area.width.saturating_sub(4) as usize;
    let mut lines = vec![];

    // Try to extract input args from logs
    if let Some(args) = extract_tool_args(app, tool) {
        for line in format_python_dict(&args, max_width) {
            lines.push(Line::from(Span::styled(format!("  {}", line), Style::default().fg(Color::White))));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "  No input arguments found",
            Style::default().fg(Color::DarkGray)
        )));
    }

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(" Input (Arguments) ");

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

fn render_tool_output(frame: &mut Frame, app: &App, tool: &ToolWithAgent, area: Rect) {
    let max_width = area.width.saturating_sub(4) as usize;
    let mut lines = vec![];

    // Check for error first
    if let Some(error) = find_tool_error(app, tool) {
        lines.push(Line::from(Span::styled(
            "  ERROR DETAILS:",
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)
        )));
        lines.push(Line::from(""));

        // Format error nicely
        for line in format_error(&error, max_width) {
            let style = if line.contains("TypeError") || line.contains("Error") {
                Style::default().fg(Color::Red)
            } else if line.starts_with("  File") || line.starts_with(">>") {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(Span::styled(format!("  {}", line), style)));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "  Tool executed successfully",
            Style::default().fg(Color::Green)
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  (No output/error details available)",
            Style::default().fg(Color::DarkGray)
        )));
    }

    let border_color = if find_tool_error(app, tool).is_some() { Color::Red } else { Color::Green };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(" Output / Error ");

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });

    frame.render_widget(paragraph, area);
}

/// Find error for a specific tool call
fn find_tool_error(app: &App, tool: &ToolWithAgent) -> Option<String> {
    // Look within a small time window after the tool start
    let tool_time = tool.start_sec;

    for log in &app.analysis.logs {
        let log_time = log.timestamp_sec();

        // Check if this error is close in time to the tool call
        if log_time >= tool_time && log_time <= tool_time + 5.0 {
            // Check PROBOOK_FAILURE logs
            if log.message.contains("PROBOOK_FAILURE") {
                // Match by tool name in the error
                if log.message.contains(&tool.name) || log.message.contains(&tool.name.replace("_tool", "")) {
                    return Some(log.message.clone());
                }
            }
        }
    }

    None
}

/// Extract input args from logs for a tool call
fn extract_tool_args(app: &App, tool: &ToolWithAgent) -> Option<String> {
    let tool_time = tool.start_sec;

    for log in &app.analysis.logs {
        let log_time = log.timestamp_sec();

        // Check if this log is close in time to the tool call
        if log_time >= tool_time - 1.0 && log_time <= tool_time + 5.0 {
            if log.message.contains("args:") {
                // Extract args portion
                if let Some(args_start) = log.message.find("args:") {
                    let args_str = &log.message[args_start + 5..];
                    // Find the end of the args dict
                    if let Some(tb_pos) = args_str.find("Traceback") {
                        return Some(args_str[..tb_pos].trim().to_string());
                    } else {
                        // Take first 2000 chars if no traceback
                        let end = args_str.len().min(2000);
                        return Some(args_str[..end].trim().to_string());
                    }
                }
            }
        }
    }

    None
}

/// Format error message for display
fn format_error(error: &str, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();

    if error.contains("PROBOOK_FAILURE") {
        // Extract error type
        if let Some(start) = error.find("PROBOOK_FAILURE]") {
            let rest = &error[start + 16..];
            if let Some(pipe_pos) = rest.find('|') {
                let error_type = rest[..pipe_pos].trim();
                lines.push(format!("Type: {}", error_type));

                let after_pipe = &rest[pipe_pos + 1..];
                if let Some(args_pos) = after_pipe.find("args:") {
                    let error_msg = after_pipe[..args_pos].trim().trim_end_matches('|').trim();
                    lines.push(format!("Message: {}", error_msg));
                    lines.push(String::new());

                    // Extract traceback
                    if let Some(tb_start) = after_pipe.find("Traceback") {
                        lines.push("Traceback:".to_string());
                        let traceback = &after_pipe[tb_start..];
                        for tb_line in traceback.split(">>").filter(|s| !s.trim().is_empty()) {
                            let tb_line = tb_line.trim();
                            if tb_line.len() <= max_width {
                                lines.push(format!("  {}", tb_line));
                            } else {
                                lines.push(format!("  {}...", &tb_line[..max_width.saturating_sub(5).min(tb_line.len())]));
                            }
                        }
                    }
                }
            }
        }
    } else {
        // Generic error formatting
        let chars: Vec<char> = error.chars().collect();
        for chunk in chars.chunks(max_width) {
            lines.push(chunk.iter().collect());
        }
    }

    lines
}

/// Format a Python dict-like string for display
fn format_python_dict(s: &str, max_width: usize) -> Vec<String> {
    let mut lines = Vec::new();

    // Simple parsing: split by comma, but be careful of nested structures
    let s = s.trim().trim_start_matches('{').trim_end_matches('}');

    // Try to extract key-value pairs
    let mut current = String::new();
    let mut depth = 0;

    for c in s.chars() {
        match c {
            '{' | '[' | '(' => {
                depth += 1;
                current.push(c);
            }
            '}' | ']' | ')' => {
                depth -= 1;
                current.push(c);
            }
            ',' if depth == 0 => {
                let trimmed = current.trim();
                if !trimmed.is_empty() {
                    if trimmed.len() <= max_width {
                        lines.push(trimmed.to_string());
                    } else {
                        // Truncate long values
                        if let Some(colon_pos) = trimmed.find(':') {
                            let key = &trimmed[..colon_pos + 1];
                            let value = &trimmed[colon_pos + 1..].trim();
                            if value.len() > max_width - key.len() - 3 {
                                lines.push(format!("{} {}...", key, &value[..20.min(value.len())]));
                            } else {
                                lines.push(trimmed.to_string());
                            }
                        } else {
                            lines.push(format!("{}...", &trimmed[..max_width.saturating_sub(3)]));
                        }
                    }
                }
                current.clear();
            }
            _ => current.push(c),
        }
    }

    // Don't forget the last item
    let trimmed = current.trim();
    if !trimmed.is_empty() {
        if trimmed.len() <= max_width {
            lines.push(trimmed.to_string());
        } else if let Some(colon_pos) = trimmed.find(':') {
            let key = &trimmed[..colon_pos + 1];
            lines.push(format!("{} [truncated]", key));
        }
    }

    lines
}
