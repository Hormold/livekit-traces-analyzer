//! LLM context growth view.

use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Row, Table, Wrap},
    Frame,
};

use crate::app::App;
use super::latency_color;

pub fn render(frame: &mut Frame, app: &App, area: Rect) {
    if app.analysis.llm_turns.is_empty() {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" LLM Context ");
        let paragraph = Paragraph::new("  No LLM turn data available").block(block);
        frame.render_widget(paragraph, area);
        return;
    }

    // Split: table on top (60%), full text detail at bottom (40%)
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage(55),  // Table
            Constraint::Percentage(45),  // Detail view
        ])
        .split(area);

    render_table(frame, app, chunks[0]);
    render_detail(frame, app, chunks[1]);
}

fn render_table(frame: &mut Frame, app: &App, area: Rect) {
    let scroll = app.context_scroll;
    let visible_height = area.height.saturating_sub(4) as usize;

    // Calculate available width for preview (total - fixed columns - borders)
    let fixed_width = 3 + 4 + 7 + 5 + 6 + 5 + 10; // emoji + # + ms + msgs + in + out + padding
    let preview_width = area.width.saturating_sub(fixed_width) as usize;

    let header = Row::new(vec!["", "#", "ms", "Msg", "In", "Out", "Response Preview"])
        .style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
        .bottom_margin(1);

    let rows: Vec<Row> = app
        .analysis
        .llm_turns
        .iter()
        .enumerate()
        .skip(scroll)
        .take(visible_height)
        .map(|(idx, lt)| {
            let llm_color = latency_color(lt.duration_ms);
            // Mark selected row
            let style = if idx == scroll {
                Style::default().fg(Color::Black).bg(llm_color)
            } else {
                Style::default().fg(llm_color)
            };

            // Determine if this is a tool call response (empty response = likely tool)
            let role_emoji = if lt.response_chars == 0 {
                "🔧" // Tool call
            } else {
                "🤖" // Assistant response
            };

            // Wide preview of response - fill available space
            let preview = if lt.response_text.is_empty() {
                "(tool call - no text response)".to_string()
            } else {
                super::truncate(&lt.response_text.replace('\n', " "), preview_width.max(20))
            };

            Row::new(vec![
                role_emoji.to_string(),
                format!("{}", lt.turn_index),
                format!("{:.0}", lt.duration_ms),
                format!("{}", lt.context_messages),
                format!("{}", lt.context_chars),
                format!("{}", lt.response_chars),
                preview,
            ])
            .style(style)
        })
        .collect();

    let widths = [
        Constraint::Length(2),  // Emoji
        Constraint::Length(3),  // #
        Constraint::Length(6),  // ms
        Constraint::Length(4),  // Msgs
        Constraint::Length(5),  // In
        Constraint::Length(5),  // Out
        Constraint::Min(30),    // Preview - fills remaining space
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" LLM Turns ({}) 🤖=response 🔧=tool | scroll to see details below ", app.analysis.llm_turns.len())),
        );

    frame.render_widget(table, area);
}

fn render_detail(frame: &mut Frame, app: &App, area: Rect) {
    let scroll = app.context_scroll;

    if let Some(turn) = app.analysis.llm_turns.get(scroll) {
        // Determine role and emoji
        let (role_emoji, role_name, role_color) = if turn.response_chars == 0 {
            ("🔧", "Tool Call", Color::Yellow)
        } else {
            ("🤖", "Assistant", Color::Green)
        };

        // Try to find the preceding user message from conversation turns
        let user_context = find_preceding_user_message(app, turn.start_time);

        let mut lines = vec![
            Line::from(vec![
                Span::styled(format!("{} ", role_emoji), Style::default()),
                Span::styled(role_name, Style::default().fg(role_color).add_modifier(Modifier::BOLD)),
                Span::styled(" | Turn ", Style::default().fg(Color::Gray)),
                Span::styled(format!("#{}", turn.turn_index), Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                Span::styled(" | ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{:.0}ms", turn.duration_ms), Style::default().fg(latency_color(turn.duration_ms))),
                Span::styled(" | ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{} msgs, {} chars in context", turn.context_messages, turn.context_chars), Style::default().fg(Color::White)),
            ]),
            Line::from(""),
        ];

        // Show preceding user message if found
        if let Some(user_msg) = user_context {
            lines.push(Line::from(vec![
                Span::styled("👤 User said: ", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
            ]));
            let max_width = area.width.saturating_sub(6) as usize;
            let truncated_user = super::truncate(&user_msg.replace('\n', " "), max_width);
            lines.push(Line::from(vec![
                Span::styled(format!("   \"{}\"", truncated_user), Style::default().fg(Color::Blue)),
            ]));
            lines.push(Line::from(""));
        }

        // Response section
        if turn.response_chars == 0 {
            lines.push(Line::from(Span::styled("🔧 Tool Call Response:", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))));
            lines.push(Line::from(Span::styled("   (no text response - LLM called a tool/function)", Style::default().fg(Color::DarkGray))));
        } else {
            lines.push(Line::from(vec![
                Span::styled("🤖 Assistant Response:", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
                Span::styled(format!(" ({} chars)", turn.response_chars), Style::default().fg(Color::DarkGray)),
            ]));

            // Split response text into lines that fit the width
            let max_width = area.width.saturating_sub(6) as usize;
            let response_text = &turn.response_text;

            // Wrap text manually
            for paragraph in response_text.split('\n') {
                if paragraph.is_empty() {
                    lines.push(Line::from(""));
                } else {
                    let chars: Vec<char> = paragraph.chars().collect();
                    for chunk in chars.chunks(max_width.max(1)) {
                        let line_text: String = chunk.iter().collect();
                        lines.push(Line::from(Span::styled(format!("   {}", line_text), Style::default().fg(Color::White))));
                    }
                }
            }
        }

        let title_emoji = if turn.response_chars == 0 { "🔧" } else { "🤖" };
        let block = Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(role_color))
            .title(format!(" {} Turn #{} Details ", title_emoji, turn.turn_index));

        let paragraph = Paragraph::new(lines)
            .block(block)
            .wrap(Wrap { trim: false });

        frame.render_widget(paragraph, area);
    } else {
        let block = Block::default()
            .borders(Borders::ALL)
            .title(" Turn Details ");
        let paragraph = Paragraph::new("  No turn selected").block(block);
        frame.render_widget(paragraph, area);
    }
}

/// Find the most recent user message before the given timestamp
fn find_preceding_user_message(app: &App, llm_start_time: f64) -> Option<String> {
    app.analysis.turns
        .iter()
        .filter(|t| t.role.as_deref() == Some("user") && t.created_at < llm_start_time)
        .max_by(|a, b| a.created_at.partial_cmp(&b.created_at).unwrap_or(std::cmp::Ordering::Equal))
        .map(|t| t.text())
}
