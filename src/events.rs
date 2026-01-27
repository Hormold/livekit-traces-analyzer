//! Keyboard event handling.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::app::{App, View};

/// Result of handling an event.
pub enum EventResult {
    /// Continue running the app.
    Continue,
    /// Quit the app.
    Quit,
}

/// Handle a keyboard event.
pub fn handle_key_event(app: &mut App, key: KeyEvent, viewport_height: usize) -> EventResult {
    // Help modal takes priority
    if app.show_help {
        match key.code {
            KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => {
                app.show_help = false;
            }
            _ => {}
        }
        return EventResult::Continue;
    }

    match key.code {
        // Quit
        KeyCode::Char('q') => return EventResult::Quit,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            return EventResult::Quit
        }

        // Help
        KeyCode::Char('?') => app.toggle_help(),

        // Tab navigation
        KeyCode::Tab => {
            if key.modifiers.contains(KeyModifiers::SHIFT) {
                app.prev_view();
            } else {
                app.next_view();
            }
        }
        KeyCode::BackTab => app.prev_view(),

        // Hotkey navigation
        KeyCode::Char(c @ '1'..='9') => {
            if let Some(view) = View::from_hotkey(c) {
                app.set_view(view);
            }
        }

        // Scrolling
        KeyCode::Down | KeyCode::Char('j') => app.scroll_down(1),
        KeyCode::Up | KeyCode::Char('k') => app.scroll_up(1),
        KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.page_down(viewport_height);
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.page_up(viewport_height);
        }
        KeyCode::PageDown => app.page_down(viewport_height),
        KeyCode::PageUp => app.page_up(viewport_height),
        KeyCode::Home | KeyCode::Char('g') => {
            // Go to top
            match app.current_view {
                View::Overview => app.overview_scroll = 0,
                View::Transcript => app.transcript_scroll = 0,
                View::Logs => app.logs_scroll = 0,
                View::Spans => app.spans_scroll = 0,
                View::Context => app.context_scroll = 0,
                _ => {}
            }
        }
        KeyCode::End | KeyCode::Char('G') => {
            // Go to bottom
            let max = app.max_scroll();
            match app.current_view {
                View::Overview => app.overview_scroll = max,
                View::Transcript => app.transcript_scroll = max,
                View::Logs => app.logs_scroll = max,
                View::Spans => app.spans_scroll = max,
                View::Context => app.context_scroll = max,
                _ => {}
            }
        }

        // View-specific controls
        KeyCode::Char('f') => {
            match app.current_view {
                View::Logs => app.cycle_log_filter(),
                View::Spans => app.toggle_span_filter(),
                _ => {}
            }
        }

        // Sort mode for latency view
        KeyCode::Char('s') => {
            if app.current_view == View::Latency {
                app.cycle_latency_sort();
            }
        }

        _ => {}
    }

    EventResult::Continue
}
