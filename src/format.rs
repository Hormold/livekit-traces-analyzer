//! Centralized formatting utilities.
//!
//! This module provides a single source of truth for all formatting functions
//! used across the application (TUI, reports).

/// Format duration as mm:ss.ms (e.g., "1:23.45").
pub fn format_duration(seconds: f64) -> String {
    let mins = (seconds / 60.0).floor() as u32;
    let secs = seconds % 60.0;
    format!("{}:{:05.2}", mins, secs)
}

/// Format milliseconds as "Xms" or "-" if None.
pub fn format_ms(value: Option<f64>) -> String {
    match value {
        Some(v) => format!("{:.0}ms", v * 1000.0),
        None => "-".to_string(),
    }
}

/// Truncate text to a maximum length, adding "..." if truncated.
pub fn truncate(text: &str, max_len: usize) -> String {
    if text.len() <= max_len {
        text.to_string()
    } else {
        format!("{}...", text.chars().take(max_len.saturating_sub(3)).collect::<String>())
    }
}

/// Word wrap text to a given width with prefix.
pub fn word_wrap(text: &str, max_width: usize, prefix: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let words: Vec<&str> = text.split_whitespace().collect();
    let mut current_line = prefix.to_string();

    for word in words {
        if current_line.len() + word.len() + 1 > max_width {
            if current_line.trim() != prefix.trim() {
                lines.push(current_line);
            }
            current_line = format!("{}{}", prefix, word);
        } else {
            if current_line.len() > prefix.len() {
                current_line.push(' ');
            }
            current_line.push_str(word);
        }
    }

    if current_line.trim().len() > prefix.trim().len() {
        lines.push(current_line);
    }

    lines
}

