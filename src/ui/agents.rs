//! Agents view - agent runs, tool calls with agent context, and LLM calls.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Row, Table},
    Frame,
};

use crate::app::App;
use super::format_duration;

/// Agent run info with start/end times.
#[derive(Debug, Clone)]
pub struct AgentRun {
    pub name: String,
    pub start_sec: f64,
    pub end_sec: Option<f64>,
}

/// Tool call with agent context.
#[derive(Debug, Clone)]
pub struct ToolWithAgent {
    pub name: String,
    pub start_sec: f64,
    pub duration_ms: f64,
    pub agent: String,
}

/// Extract agent runs with their time ranges.
pub fn extract_agent_runs(app: &App) -> Vec<AgentRun> {
    let mut runs: Vec<AgentRun> = Vec::new();

    for log in &app.analysis.logs {
        if log.message.contains("Executing agent run for") {
            if let Some(agent) = log.message.split("for ").last() {
                let agent_name = agent.trim().to_string();
                let ts = log.timestamp_sec();

                // Close previous run of different agent
                if let Some(last) = runs.last_mut() {
                    if last.end_sec.is_none() && last.name != agent_name {
                        last.end_sec = Some(ts);
                    }
                }

                // Check if this agent already has an open run
                let has_open_run = runs.iter().any(|r| r.name == agent_name && r.end_sec.is_none());

                if !has_open_run {
                    runs.push(AgentRun {
                        name: agent_name,
                        start_sec: ts,
                        end_sec: None,
                    });
                }
            }
        }
    }

    // Close any remaining open runs at session end
    let session_end = app.analysis.session_end;
    for run in &mut runs {
        if run.end_sec.is_none() {
            run.end_sec = Some(session_end);
        }
    }

    runs
}

/// Extract tools with their associated agent.
pub fn extract_tools_with_agent(app: &App) -> Vec<ToolWithAgent> {
    let agent_runs = extract_agent_runs(app);
    let mut tools: Vec<ToolWithAgent> = Vec::new();

    // Get tools from analysis
    for tool in &app.analysis.tool_calls {
        // Find which agent was active when tool was called
        let agent = agent_runs.iter()
            .filter(|r| r.start_sec <= tool.start && r.end_sec.map_or(true, |e| tool.start <= e))
            .max_by(|a, b| a.start_sec.partial_cmp(&b.start_sec).unwrap_or(std::cmp::Ordering::Equal))
            .map(|r| r.name.clone())
            .unwrap_or_else(|| "unknown".to_string());

        tools.push(ToolWithAgent {
            name: tool.name.clone(),
            start_sec: tool.start,
            duration_ms: tool.duration_ms,
            agent,
        });
    }

    // Also check function_call spans
    for span in &app.analysis.spans {
        if span.name == "function_call" || span.name == "tool_call" {
            let name = span.attributes
                .get("lk.function_name")
                .and_then(|v| v.as_str())
                .unwrap_or(&span.name)
                .to_string();

            let start = span.start_sec();

            // Skip duplicates
            if tools.iter().any(|t| (t.start_sec - start).abs() < 0.5 && t.name == name) {
                continue;
            }

            // Find agent
            let agent = agent_runs.iter()
                .filter(|r| r.start_sec <= start && r.end_sec.map_or(true, |e| start <= e))
                .max_by(|a, b| a.start_sec.partial_cmp(&b.start_sec).unwrap_or(std::cmp::Ordering::Equal))
                .map(|r| r.name.clone())
                .unwrap_or_else(|| "unknown".to_string());

            tools.push(ToolWithAgent {
                name,
                start_sec: start,
                duration_ms: span.duration_ms(),
                agent,
            });
        }
    }

    tools.sort_by(|a, b| a.start_sec.partial_cmp(&b.start_sec).unwrap_or(std::cmp::Ordering::Equal));
    tools
}

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    let agent_runs = extract_agent_runs(app);

    // Split vertically: summary on top, agents table below
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8), // Summary
            Constraint::Min(0),    // Agents table
        ])
        .split(area);

    render_summary(frame, app, &agent_runs, chunks[0]);
    render_agents_table(frame, app, &agent_runs, chunks[1]);
}

fn render_summary(frame: &mut Frame, app: &App, agent_runs: &[AgentRun], area: Rect) {
    let tools = extract_tools_with_agent(app);
    let handoffs = if agent_runs.len() > 1 { agent_runs.len() - 1 } else { 0 };

    // Calculate total agent time
    let total_agent_time: f64 = agent_runs.iter()
        .filter_map(|r| r.end_sec.map(|e| e - r.start_sec))
        .sum();

    // Count tools per agent
    let mut tools_per_agent: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for tool in &tools {
        *tools_per_agent.entry(tool.agent.clone()).or_insert(0) += 1;
    }

    let lines = vec![
        Line::from(""),
        Line::from(vec![
            Span::styled("  Total Agents: ", Style::default().fg(Color::Gray)),
            Span::styled(format!("{}", agent_runs.len()), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::styled("  |  Handoffs: ", Style::default().fg(Color::Gray)),
            Span::styled(format!("{}", handoffs), Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
            Span::styled("  |  Total Time: ", Style::default().fg(Color::Gray)),
            Span::styled(format!("{:.1}s", total_agent_time), Style::default().fg(Color::Green)),
        ]),
        Line::from(vec![
            Span::styled("  Tool Calls: ", Style::default().fg(Color::Gray)),
            Span::styled(format!("{}", tools.len()), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::styled("  (", Style::default().fg(Color::DarkGray)),
            Span::raw(tools_per_agent.iter().map(|(k, v)| format!("{}: {}", k, v)).collect::<Vec<_>>().join(", ")),
            Span::styled(")", Style::default().fg(Color::DarkGray)),
        ]),
        Line::from(""),
        Line::from(Span::styled("  See [6] Tools tab for detailed tool call information", Style::default().fg(Color::DarkGray))),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Agent Summary ");

    let paragraph = Paragraph::new(lines).block(block);
    frame.render_widget(paragraph, area);
}

fn render_agents_table(frame: &mut Frame, app: &App, agent_runs: &[AgentRun], area: Rect) {
    let tools = extract_tools_with_agent(app);

    let header = Row::new(vec!["#", "Agent Name", "Started", "Ended", "Duration", "Tools"])
        .style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD))
        .bottom_margin(1);

    let visible_height = area.height.saturating_sub(4) as usize;

    let rows: Vec<Row> = agent_runs
        .iter()
        .enumerate()
        .take(visible_height)
        .map(|(idx, run)| {
            let start_rel = run.start_sec - app.analysis.session_start;
            let end_rel = run.end_sec.map(|e| e - app.analysis.session_start);
            let duration = run.end_sec.map(|e| e - run.start_sec).unwrap_or(0.0);

            // Count tools for this agent
            let tool_count = tools.iter().filter(|t| t.agent == run.name).count();

            let duration_color = if duration < 30.0 {
                Color::Green
            } else if duration < 120.0 {
                Color::Yellow
            } else {
                Color::Cyan
            };

            Row::new(vec![
                format!("{}", idx + 1),
                run.name.clone(),
                format_duration(start_rel),
                end_rel.map(|e| format_duration(e)).unwrap_or_else(|| "running...".to_string()),
                format!("{:.1}s ({:.0}%)", duration, (duration / app.analysis.duration_sec()) * 100.0),
                format!("{}", tool_count),
            ])
            .style(Style::default().fg(duration_color))
        })
        .collect();

    let widths = [
        Constraint::Length(3),   // #
        Constraint::Min(25),     // Agent name (full)
        Constraint::Length(12),  // Started
        Constraint::Length(12),  // Ended
        Constraint::Length(18),  // Duration with %
        Constraint::Length(6),   // Tools count
    ];

    let title = format!(
        " Agent Timeline ({:.0}s total) ",
        app.analysis.duration_sec(),
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

