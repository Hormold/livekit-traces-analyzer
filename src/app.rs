//! Application state and business logic.

use std::path::Path;

use anyhow::Result;

use crate::analysis::analyze_call;
use crate::data::{CallAnalysis, LatencyStats, LogEntry, Span};
use crate::thresholds;

/// Available views in the TUI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum View {
    Overview,
    Transcript,
    Latency,
    Charts,
    Agents,
    Tools,
    Context,
    Logs,
    Spans,
}

impl View {
    pub fn all() -> &'static [View] {
        &[
            View::Overview,
            View::Transcript,
            View::Latency,
            View::Charts,
            View::Agents,
            View::Tools,
            View::Context,
            View::Logs,
            View::Spans,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            View::Overview => "Overview",
            View::Transcript => "Transcript",
            View::Latency => "Latency",
            View::Charts => "Charts",
            View::Agents => "Agents",
            View::Tools => "Tools",
            View::Context => "Context",
            View::Logs => "Logs",
            View::Spans => "Spans",
        }
    }

    pub fn hotkey(&self) -> char {
        match self {
            View::Overview => '1',
            View::Transcript => '2',
            View::Latency => '3',
            View::Charts => '4',
            View::Agents => '5',
            View::Tools => '6',
            View::Context => '7',
            View::Logs => '8',
            View::Spans => '9',
        }
    }

    pub fn from_hotkey(c: char) -> Option<View> {
        match c {
            '1' => Some(View::Overview),
            '2' => Some(View::Transcript),
            '3' => Some(View::Latency),
            '4' => Some(View::Charts),
            '5' => Some(View::Agents),
            '6' => Some(View::Tools),
            '7' => Some(View::Context),
            '8' => Some(View::Logs),
            '9' => Some(View::Spans),
            _ => None,
        }
    }

    pub fn next(&self) -> View {
        match self {
            View::Overview => View::Transcript,
            View::Transcript => View::Latency,
            View::Latency => View::Charts,
            View::Charts => View::Agents,
            View::Agents => View::Tools,
            View::Tools => View::Context,
            View::Context => View::Logs,
            View::Logs => View::Spans,
            View::Spans => View::Overview,
        }
    }

    pub fn prev(&self) -> View {
        match self {
            View::Overview => View::Spans,
            View::Transcript => View::Overview,
            View::Latency => View::Transcript,
            View::Charts => View::Latency,
            View::Agents => View::Charts,
            View::Tools => View::Agents,
            View::Context => View::Tools,
            View::Logs => View::Context,
            View::Spans => View::Logs,
        }
    }
}

/// Log severity filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogFilter {
    All,
    ErrorsOnly,
    WarningsOnly,
}

impl LogFilter {
    pub fn label(&self) -> &'static str {
        match self {
            LogFilter::All => "All",
            LogFilter::ErrorsOnly => "Errors",
            LogFilter::WarningsOnly => "Warnings",
        }
    }

    pub fn cycle(&self) -> LogFilter {
        match self {
            LogFilter::All => LogFilter::ErrorsOnly,
            LogFilter::ErrorsOnly => LogFilter::WarningsOnly,
            LogFilter::WarningsOnly => LogFilter::All,
        }
    }
}

/// Span filter for key spans.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpanFilter {
    KeySpans,
    AllSpans,
}

impl SpanFilter {
    pub fn label(&self) -> &'static str {
        match self {
            SpanFilter::KeySpans => "Key Spans",
            SpanFilter::AllSpans => "All Spans",
        }
    }

    pub fn toggle(&self) -> SpanFilter {
        match self {
            SpanFilter::KeySpans => SpanFilter::AllSpans,
            SpanFilter::AllSpans => SpanFilter::KeySpans,
        }
    }
}

/// Sort mode for latency view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LatencySortMode {
    ByLatency,
    ByTurn,
    ByLLM,
    ByTTS,
}

impl LatencySortMode {
    pub fn label(&self) -> &'static str {
        match self {
            LatencySortMode::ByLatency => "E2E",
            LatencySortMode::ByTurn => "Turn#",
            LatencySortMode::ByLLM => "LLM",
            LatencySortMode::ByTTS => "TTS",
        }
    }

    pub fn cycle(&self) -> LatencySortMode {
        match self {
            LatencySortMode::ByLatency => LatencySortMode::ByTurn,
            LatencySortMode::ByTurn => LatencySortMode::ByLLM,
            LatencySortMode::ByLLM => LatencySortMode::ByTTS,
            LatencySortMode::ByTTS => LatencySortMode::ByLatency,
        }
    }
}

/// Application state.
pub struct App {
    pub analysis: CallAnalysis,
    pub current_view: View,
    pub show_help: bool,

    // Scroll positions per view
    pub transcript_scroll: usize,
    pub logs_scroll: usize,
    pub spans_scroll: usize,
    pub context_scroll: usize,
    pub tools_scroll: usize,
    pub overview_scroll: usize,

    // Filters
    pub log_filter: LogFilter,
    pub span_filter: SpanFilter,
    pub latency_sort: LatencySortMode,

    // Cached computed data
    pub e2e_stats: Option<LatencyStats>,
    pub llm_stats: Option<LatencyStats>,
    pub tts_stats: Option<LatencyStats>,
}

impl App {
    /// Load and analyze a folder, creating a new App instance.
    pub fn load(folder: &Path) -> Result<Self> {
        let analysis = analyze_call(folder)?;

        // Compute latency stats using centralized method
        let (e2e_stats, llm_stats, tts_stats) = analysis.compute_latency_stats();

        Ok(Self {
            analysis,
            current_view: View::Overview,
            show_help: false,
            transcript_scroll: 0,
            logs_scroll: 0,
            spans_scroll: 0,
            context_scroll: 0,
            tools_scroll: 0,
            overview_scroll: 0,
            log_filter: LogFilter::All,
            span_filter: SpanFilter::KeySpans,
            latency_sort: LatencySortMode::ByLatency,
            e2e_stats,
            llm_stats,
            tts_stats,
        })
    }

    /// Get filtered logs based on current filter.
    pub fn filtered_logs(&self) -> Vec<&LogEntry> {
        match self.log_filter {
            LogFilter::All => self
                .analysis
                .logs
                .iter()
                .filter(|l| l.severity == "ERROR" || l.severity == "WARN" || l.severity == "WARNING" || l.severity == "CRITICAL")
                .collect(),
            LogFilter::ErrorsOnly => self.analysis.errors.iter().collect(),
            LogFilter::WarningsOnly => self.analysis.warnings.iter().collect(),
        }
    }

    /// Get filtered spans based on current filter.
    pub fn filtered_spans(&self) -> Vec<&Span> {
        match self.span_filter {
            SpanFilter::KeySpans => self
                .analysis
                .spans
                .iter()
                .filter(|s| thresholds::is_key_span(&s.name))
                .collect(),
            SpanFilter::AllSpans => self.analysis.spans.iter().collect(),
        }
    }

    /// Navigate to next view.
    pub fn next_view(&mut self) {
        self.current_view = self.current_view.next();
    }

    /// Navigate to previous view.
    pub fn prev_view(&mut self) {
        self.current_view = self.current_view.prev();
    }

    /// Set view by hotkey.
    pub fn set_view(&mut self, view: View) {
        self.current_view = view;
    }

    /// Toggle help modal.
    pub fn toggle_help(&mut self) {
        self.show_help = !self.show_help;
    }

    /// Get current scroll position for the active view.
    pub fn current_scroll(&self) -> usize {
        match self.current_view {
            View::Overview => self.overview_scroll,
            View::Transcript => self.transcript_scroll,
            View::Logs => self.logs_scroll,
            View::Spans => self.spans_scroll,
            View::Context => self.context_scroll,
            View::Tools => self.tools_scroll,
            _ => 0,
        }
    }

    /// Get max scroll for the active view.
    pub fn max_scroll(&self) -> usize {
        match self.current_view {
            View::Overview => {
                // Estimate diagnosis content lines
                let mut lines = 20; // Base for pipeline summary
                lines += self.analysis.pipeline_cycles.len() * 2; // Pipeline cycles + gap detection
                if let Some(ref d) = self.analysis.diagnosis {
                    for turns in d.slow_turns_by_cause.values() {
                        lines += 3 + turns.len().min(3) * 2; // Each cause section
                    }
                }
                lines
            }
            View::Transcript => self.analysis.turns.len().saturating_sub(1),
            View::Logs => self.filtered_logs().len().saturating_sub(1),
            View::Spans => self.filtered_spans().len().saturating_sub(1),
            View::Context => self.analysis.llm_turns.len().saturating_sub(1),
            View::Tools => self.analysis.tool_calls.len().saturating_sub(1),
            _ => 0,
        }
    }

    /// Scroll down by n lines.
    pub fn scroll_down(&mut self, n: usize) {
        let max = self.max_scroll();
        let scroll = match self.current_view {
            View::Overview => &mut self.overview_scroll,
            View::Transcript => &mut self.transcript_scroll,
            View::Logs => &mut self.logs_scroll,
            View::Spans => &mut self.spans_scroll,
            View::Context => &mut self.context_scroll,
            View::Tools => &mut self.tools_scroll,
            _ => return,
        };
        *scroll = (*scroll + n).min(max);
    }

    /// Scroll up by n lines.
    pub fn scroll_up(&mut self, n: usize) {
        let scroll = match self.current_view {
            View::Overview => &mut self.overview_scroll,
            View::Transcript => &mut self.transcript_scroll,
            View::Logs => &mut self.logs_scroll,
            View::Spans => &mut self.spans_scroll,
            View::Context => &mut self.context_scroll,
            View::Tools => &mut self.tools_scroll,
            _ => return,
        };
        *scroll = scroll.saturating_sub(n);
    }

    /// Page down (scroll by viewport height).
    pub fn page_down(&mut self, viewport_height: usize) {
        self.scroll_down(viewport_height.saturating_sub(2));
    }

    /// Page up (scroll by viewport height).
    pub fn page_up(&mut self, viewport_height: usize) {
        self.scroll_up(viewport_height.saturating_sub(2));
    }

    /// Cycle log filter.
    pub fn cycle_log_filter(&mut self) {
        self.log_filter = self.log_filter.cycle();
        self.logs_scroll = 0;
    }

    /// Toggle span filter.
    pub fn toggle_span_filter(&mut self) {
        self.span_filter = self.span_filter.toggle();
        self.spans_scroll = 0;
    }

    /// Cycle latency sort mode.
    pub fn cycle_latency_sort(&mut self) {
        self.latency_sort = self.latency_sort.cycle();
    }
}
