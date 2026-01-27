//! Report generation for call analysis output.
//!
//! Provides text and JSON report formats similar to the Python call_analyzer.py.

use std::collections::HashMap;
use std::io::IsTerminal;

use serde::Serialize;

use crate::data::{CallAnalysis, DiagnosisVerdict, LatencyStats, PipelineSummary, Severity};

// =============================================================================
// ANSI COLOR SUPPORT
// =============================================================================

/// ANSI color codes for terminal output.
struct Colors;

impl Colors {
    const BLUE: &'static str = "\x1b[94m";
    const CYAN: &'static str = "\x1b[96m";
    const GREEN: &'static str = "\x1b[92m";
    const YELLOW: &'static str = "\x1b[93m";
    const RED: &'static str = "\x1b[91m";
    const BOLD: &'static str = "\x1b[1m";
    const DIM: &'static str = "\x1b[2m";
    const RESET: &'static str = "\x1b[0m";
}

/// Colorize text if colors are enabled.
fn colorize(text: &str, color: &str, use_color: bool) -> String {
    if use_color {
        format!("{}{}{}", color, text, Colors::RESET)
    } else {
        text.to_string()
    }
}

/// Combine multiple color codes.
fn colors(codes: &[&str]) -> String {
    codes.join("")
}

// =============================================================================
// FORMATTING HELPERS
// =============================================================================

/// Format duration as mm:ss.ms.
fn format_duration(seconds: f64) -> String {
    let mins = (seconds / 60.0).floor() as i64;
    let secs = seconds % 60.0;
    format!("{}:{:05.2}", mins, secs)
}

/// Format milliseconds value.
fn format_ms(value: Option<f64>) -> String {
    match value {
        Some(v) => format!("{:.0}ms", v * 1000.0),
        None => "-".to_string(),
    }
}

/// Get color based on latency value.
fn latency_color(ms: Option<f64>) -> &'static str {
    match ms {
        None => Colors::DIM,
        Some(v) => {
            let ms_val = v * 1000.0;
            if ms_val < 500.0 {
                Colors::GREEN
            } else if ms_val < 1500.0 {
                Colors::YELLOW
            } else {
                Colors::RED
            }
        }
    }
}

/// Convert Severity to ANSI color code.
fn severity_to_ansi(severity: Severity) -> &'static str {
    match severity {
        Severity::Good => Colors::GREEN,
        Severity::Warning => Colors::YELLOW,
        Severity::Critical => Colors::RED,
    }
}

/// Generate pipeline analysis text section using PipelineSummary.
fn generate_pipeline_analysis_text(summary: &PipelineSummary, use_color: bool) -> Vec<String> {
    let mut lines = Vec::new();
    let c = |text: &str, color: &str| colorize(text, color, use_color);

    // Response time summary
    lines.push(format!(
        "  Response time: {} (max {:.1}s) - {}",
        c(&format!("{:.1}s avg", summary.avg_total_ms / 1000.0), severity_to_ansi(summary.total_severity)),
        summary.max_total_ms / 1000.0,
        c(summary.total_verdict, severity_to_ansi(summary.total_severity))
    ));
    lines.push(String::new());

    // Breakdown
    lines.push("  Where time goes:".to_string());

    // LLM
    lines.push(format!(
        "    LLM: {} ({:.0}%) - {}",
        c(&format!("{:.1}s", summary.avg_llm_ms / 1000.0), severity_to_ansi(summary.llm_severity)),
        summary.llm_pct,
        summary.llm_verdict
    ));

    // TTS
    lines.push(format!(
        "    TTS: {} ({:.0}%) - {}",
        c(&format!("{:.1}s", summary.avg_tts_ms / 1000.0), severity_to_ansi(summary.tts_severity)),
        summary.tts_pct,
        summary.tts_verdict
    ));

    // User→LLM
    if summary.user_turn_count > 0 {
        lines.push(format!(
            "    Perception: {} ({} user turns) - {}",
            c(&format!("{:.0}ms", summary.avg_user_to_llm_ms), severity_to_ansi(summary.perception_severity)),
            summary.user_turn_count,
            summary.perception_verdict
        ));
    }

    // System turns
    if summary.system_turn_count > 0 {
        lines.push(c(
            &format!("    System-initiated: {} turns (greeting, tool responses)", summary.system_turn_count),
            Colors::DIM,
        ));
    }

    // Bottleneck identification
    lines.push(String::new());
    lines.push(format!(
        "  Bottleneck: {}",
        c(&summary.bottleneck, severity_to_ansi(summary.bottleneck_severity))
    ));

    // Detected delays
    if !summary.detected_delays.is_empty() {
        lines.push(String::new());
        lines.push(c("  Detected delays:", Colors::YELLOW));

        for delay in summary.detected_delays.iter().take(5) {
            lines.push(format!(
                "    Turn {}: +{:.1}s gap -> {}",
                delay.turn_number,
                delay.gap_ms / 1000.0,
                delay.reason
            ));
        }

        if summary.detected_delays.len() > 5 {
            lines.push(c(
                &format!("    ... and {} more turns with gaps", summary.detected_delays.len() - 5),
                Colors::DIM,
            ));
        }
    }

    lines
}

/// Word wrap text to a given width with prefix.
fn word_wrap(text: &str, max_width: usize, prefix: &str) -> Vec<String> {
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

// =============================================================================
// TEXT REPORT GENERATION
// =============================================================================

/// Generate a comprehensive text report.
pub fn generate_text_report(analysis: &CallAnalysis) -> String {
    let use_color = std::io::stdout().is_terminal();
    generate_text_report_impl(analysis, use_color)
}

/// Generate a text report with explicit color control.
pub fn generate_text_report_no_color(analysis: &CallAnalysis) -> String {
    generate_text_report_impl(analysis, false)
}

fn generate_text_report_impl(analysis: &CallAnalysis, use_color: bool) -> String {
    let mut lines: Vec<String> = Vec::new();
    let c = |text: &str, color: &str| colorize(text, color, use_color);

    // Compute latency stats
    let e2e_values: Vec<f64> = analysis
        .assistant_turns()
        .iter()
        .filter_map(|t| t.metrics.e2e_latency)
        .collect();
    let llm_values: Vec<f64> = analysis
        .assistant_turns()
        .iter()
        .filter_map(|t| t.metrics.llm_node_ttft)
        .collect();
    let tts_values: Vec<f64> = analysis
        .assistant_turns()
        .iter()
        .filter_map(|t| t.metrics.tts_node_ttfb)
        .collect();

    let e2e_stats = LatencyStats::from_values(&e2e_values);
    let llm_stats = LatencyStats::from_values(&llm_values);
    let tts_stats = LatencyStats::from_values(&tts_values);

    // Pre-calculate issues for header verdict
    let diagnosis = analysis.diagnosis.as_ref();
    let slow_turn_count: usize = diagnosis
        .map(|d| d.slow_turns_by_cause.values().map(|v| v.len()).sum())
        .unwrap_or(0);
    let _has_errors = !analysis.errors.is_empty();
    let _tts_retry_count = diagnosis.map(|d| d.tts_retries).unwrap_or(0);

    // Header with quick verdict
    lines.push(c(&"=".repeat(80), Colors::BOLD));
    lines.push(c(
        "  LIVEKIT CALL ANALYSIS REPORT",
        &colors(&[Colors::BOLD, Colors::CYAN]),
    ));
    lines.push(c(&"=".repeat(80), Colors::BOLD));

    // One-line verdict
    let verdict = match diagnosis.map(|d| &d.verdict) {
        Some(DiagnosisVerdict::Healthy) => c(
            "  [OK] HEALTHY CALL - No major issues detected",
            &colors(&[Colors::GREEN, Colors::BOLD]),
        ),
        Some(DiagnosisVerdict::Problematic) => c(
            &format!(
                "  [!!] PROBLEMATIC CALL - {} slow turns, {} errors",
                slow_turn_count,
                analysis.errors.len()
            ),
            &colors(&[Colors::RED, Colors::BOLD]),
        ),
        _ => c(
            &format!("  [!] NEEDS ATTENTION - {} slow turns", slow_turn_count),
            &colors(&[Colors::YELLOW, Colors::BOLD]),
        ),
    };
    lines.push(verdict);
    lines.push(c(&"=".repeat(80), Colors::BOLD));
    lines.push(String::new());

    // Metadata
    lines.push(c(
        "CALL METADATA",
        &colors(&[Colors::BOLD, Colors::BLUE]),
    ));
    lines.push(c(&"-".repeat(40), Colors::DIM));
    lines.push(format!("  Room ID:      {}", analysis.room_id));
    lines.push(format!("  Job ID:       {}", analysis.job_id));
    lines.push(format!("  Agent:        {}", analysis.agent_name));
    lines.push(format!("  Room Name:    {}", analysis.room_name));
    lines.push(format!("  Participant:  {}", analysis.participant_identity));
    lines.push(format!(
        "  Duration:     {}",
        format_duration(analysis.duration_sec())
    ));
    if analysis.session_start > 0.0 {
        let start_time = chrono_format_timestamp(analysis.session_start);
        lines.push(format!("  Start:        {}", start_time));
    }
    lines.push(String::new());

    // Summary Stats
    lines.push(c(
        "SUMMARY STATISTICS",
        &colors(&[Colors::BOLD, Colors::BLUE]),
    ));
    lines.push(c(&"-".repeat(40), Colors::DIM));

    let total_turns = analysis.turns.len();
    let user_turns = analysis.user_turns().len();
    let assistant_turns = analysis.assistant_turns().len();
    let interrupted = analysis.interrupted_turns().len();

    lines.push(format!("  Total turns:        {}", total_turns));
    lines.push(format!("  User turns:         {}", user_turns));
    lines.push(format!("  Assistant turns:    {}", assistant_turns));
    lines.push(format!("  Interrupted:        {}", interrupted));
    lines.push(format!("  Errors:             {}", analysis.errors.len()));
    lines.push(format!("  Warnings:           {}", analysis.warnings.len()));
    lines.push(String::new());

    // Pipeline Analysis
    if let Some(summary) = PipelineSummary::from_cycles(&analysis.pipeline_cycles) {
        lines.push(c(
            "PIPELINE ANALYSIS",
            &colors(&[Colors::BOLD, Colors::CYAN]),
        ));
        lines.push(c(&"-".repeat(80), Colors::DIM));
        lines.extend(generate_pipeline_analysis_text(&summary, use_color));
        lines.push(String::new());
    }

    // Automatic Diagnosis
    lines.push(c(
        "AUTOMATIC DIAGNOSIS",
        &colors(&[Colors::BOLD, Colors::RED]),
    ));
    lines.push(c(&"=".repeat(80), Colors::RED));

    if let Some(diag) = diagnosis {
        let total_slow: usize = diag.slow_turns_by_cause.values().map(|v| v.len()).sum();

        if total_slow == 0 && analysis.errors.is_empty() && diag.tts_retries == 0 {
            lines.push(c(
                "  [OK] NO MAJOR ISSUES DETECTED",
                &colors(&[Colors::GREEN, Colors::BOLD]),
            ));
            lines.push(c(
                "  Call performance is within acceptable limits.",
                Colors::GREEN,
            ));
        } else {
            lines.push(c(
                &format!("  [!] FOUND {} SLOW TURNS (>2s E2E)", total_slow),
                &colors(&[Colors::YELLOW, Colors::BOLD]),
            ));
            lines.push(String::new());

            // Show breakdown by cause
            let cause_icons = [
                ("LLM", "[LLM]"),
                ("TTS", "[TTS]"),
                ("STT", "[STT]"),
                ("TOOL", "[TOOL]"),
                ("OVERHEAD", "[GAP]"),
            ];

            for (cause, icon) in cause_icons {
                if let Some(turns) = diag.slow_turns_by_cause.get(cause) {
                    if turns.is_empty() {
                        continue;
                    }

                    let color = if cause == "LLM" || cause == "TTS" || cause == "TOOL" {
                        Colors::RED
                    } else {
                        Colors::YELLOW
                    };

                    lines.push(c(
                        &format!("  {} {} BOTTLENECK: {} turns", icon, cause, turns.len()),
                        &colors(&[color, Colors::BOLD]),
                    ));

                    for t in turns.iter().take(3) {
                        let cause_ms = match cause {
                            "LLM" => t.llm_ms,
                            "TTS" => t.tts_ms,
                            _ => t.unexplained_ms,
                        };
                        let tool_info = t
                            .tool_name
                            .as_ref()
                            .map(|n| format!(" (tool: {})", n))
                            .unwrap_or_default();
                        lines.push(c(
                            &format!(
                                "    Turn {}: E2E={:.0}ms -> {}={:.0}ms{}",
                                t.turn, t.e2e_ms, cause, cause_ms, tool_info
                            ),
                            color,
                        ));
                        lines.push(format!("      \"{}...\"", t.text));
                    }

                    if turns.len() > 3 {
                        lines.push(c(
                            &format!("    ... and {} more {}-slow turns", turns.len() - 3, cause),
                            Colors::DIM,
                        ));
                    }
                    lines.push(String::new());
                }
            }
        }

        // TTS Retries
        if diag.tts_retries > 0 {
            lines.push(c(
                &format!(
                    "  [TTS] TTS RETRIES: {}x synthesis failures",
                    diag.tts_retries
                ),
                &colors(&[Colors::RED, Colors::BOLD]),
            ));
            lines.push(String::new());
        }

        // Tool Errors
        if diag.tool_errors > 0 {
            lines.push(c(
                &format!("  [TOOL] TOOL ERRORS: {} failures", diag.tool_errors),
                &colors(&[Colors::RED, Colors::BOLD]),
            ));
            lines.push(String::new());
        }

        // Quick verdict
        lines.push(c(&format!("  {}", "-".repeat(76)), Colors::DIM));

        if let Some(ref issue) = diag.primary_issue {
            lines.push(c(
                &format!("  PRIMARY ISSUE: {}", issue),
                &colors(&[Colors::RED, Colors::BOLD]),
            ));
            if let Some(ref detail) = diag.primary_issue_detail {
                lines.push(format!("     -> {}", detail));
            }
        }
    }

    lines.push(String::new());
    lines.push(c(&"=".repeat(80), Colors::DIM));
    lines.push(String::new());

    // Latency Summary
    if let Some(ref stats) = e2e_stats {
        lines.push(c(
            "LATENCY SUMMARY",
            &colors(&[Colors::BOLD, Colors::BLUE]),
        ));
        lines.push(c(&"-".repeat(40), Colors::DIM));
        lines.push(format!(
            "  E2E Latency:    avg={:.0}ms  min={:.0}ms  max={:.0}ms  p95={:.0}ms",
            stats.avg_ms, stats.min_ms, stats.max_ms, stats.p95_ms
        ));
    }

    if let Some(ref stats) = llm_stats {
        lines.push(format!(
            "  LLM TTFT:       avg={:.0}ms  min={:.0}ms  max={:.0}ms  p95={:.0}ms",
            stats.avg_ms, stats.min_ms, stats.max_ms, stats.p95_ms
        ));
    }

    if let Some(ref stats) = tts_stats {
        lines.push(format!(
            "  TTS TTFB:       avg={:.0}ms  min={:.0}ms  max={:.0}ms  p95={:.0}ms",
            stats.avg_ms, stats.min_ms, stats.max_ms, stats.p95_ms
        ));
    }
    lines.push(String::new());

    // System Prompt
    if !analysis.system_prompt.is_empty() {
        lines.push(c(
            "SYSTEM PROMPT",
            &colors(&[Colors::BOLD, Colors::BLUE]),
        ));
        lines.push(c(&"-".repeat(80), Colors::DIM));
        lines.push(format!(
            "  Length: {} chars (~{} tokens)",
            analysis.system_prompt.len(),
            analysis.system_prompt.len() / 4
        ));
        lines.push(String::new());
        for line in word_wrap(&analysis.system_prompt, 78, "  ") {
            lines.push(line);
        }
        lines.push(String::new());
    }

    // LLM Context Per Turn
    if !analysis.llm_turns.is_empty() {
        lines.push(c(
            "LLM CONTEXT PER TURN",
            &colors(&[Colors::BOLD, Colors::BLUE]),
        ));
        lines.push(c(&"-".repeat(80), Colors::DIM));
        lines.push(c(
            "  Turn |  LLM ms | Msgs |  Chars | ~Tokens | Out chars | Response preview",
            Colors::DIM,
        ));
        lines.push(c(
            "  -----|---------|------|--------|---------|-----------|------------------",
            Colors::DIM,
        ));

        for lt in &analysis.llm_turns {
            let llm_color = if lt.duration_ms < 1000.0 {
                Colors::GREEN
            } else if lt.duration_ms < 2000.0 {
                Colors::YELLOW
            } else {
                Colors::RED
            };
            let llm_str = c(&format!("{:7.0}", lt.duration_ms), llm_color);
            let preview: String = lt
                .response_text
                .chars()
                .take(30)
                .collect::<String>()
                .replace('\n', " ");
            let preview = if lt.response_text.len() > 30 {
                format!("{}...", preview)
            } else {
                preview
            };

            lines.push(format!(
                "  {:4} | {} | {:4} | {:6} | {:7} | {:9} | {}",
                lt.turn_index,
                llm_str,
                lt.context_messages,
                lt.context_chars,
                lt.context_tokens_est,
                lt.response_chars,
                preview
            ));
        }
        lines.push(String::new());

        // Context growth summary
        if analysis.llm_turns.len() > 1 {
            let first = &analysis.llm_turns[0];
            let last = analysis.llm_turns.last().unwrap();
            lines.push(c("  Context Growth:", Colors::BOLD));
            lines.push(format!(
                "    Start: {} msgs, {} chars (~{} tokens)",
                first.context_messages, first.context_chars, first.context_tokens_est
            ));
            lines.push(format!(
                "    End:   {} msgs, {} chars (~{} tokens)",
                last.context_messages, last.context_chars, last.context_tokens_est
            ));
            lines.push(format!(
                "    Growth: +{} msgs, +{} chars",
                last.context_messages.saturating_sub(first.context_messages),
                last.context_chars.saturating_sub(first.context_chars)
            ));
        }
        lines.push(String::new());
    }

    // Conversation Transcript
    lines.push(c(
        "CONVERSATION TRANSCRIPT",
        &colors(&[Colors::BOLD, Colors::BLUE]),
    ));
    lines.push(c(&"-".repeat(80), Colors::DIM));

    for (i, turn) in analysis.turns.iter().enumerate() {
        let turn_num = i + 1;

        if turn.turn_type == "agent_handoff" {
            let new_agent = turn
                .extra
                .get("new_agent_id")
                .and_then(|v| v.as_str())
                .unwrap_or(&turn.id);
            lines.push(c(
                &format!("  [{}] -> Agent handoff: {}", turn_num, new_agent),
                Colors::DIM,
            ));
            continue;
        }

        let (role_icon, role_color) = match turn.role.as_deref() {
            Some("user") => ("[USER]", Colors::GREEN),
            Some("assistant") => ("[ASST]", Colors::CYAN),
            _ => ("[?]", Colors::DIM),
        };

        // Build metrics string
        let mut metrics_parts: Vec<String> = Vec::new();
        if let Some(e2e) = turn.metrics.e2e_latency {
            let color = latency_color(Some(e2e));
            metrics_parts.push(c(&format!("E2E:{}", format_ms(Some(e2e))), color));
        }
        if let Some(llm) = turn.metrics.llm_node_ttft {
            let color = latency_color(Some(llm));
            metrics_parts.push(c(&format!("LLM:{}", format_ms(Some(llm))), color));
        }
        if let Some(tts) = turn.metrics.tts_node_ttfb {
            let color = latency_color(Some(tts));
            metrics_parts.push(c(&format!("TTS:{}", format_ms(Some(tts))), color));
        }
        if let Some(dur) = turn.metrics.speaking_duration() {
            metrics_parts.push(format!("dur:{:.1}s", dur));
        }
        if let Some(conf) = turn.metrics.transcript_confidence {
            let conf_pct = conf * 100.0;
            let conf_color = if conf_pct > 95.0 {
                Colors::GREEN
            } else if conf_pct > 85.0 {
                Colors::YELLOW
            } else {
                Colors::RED
            };
            metrics_parts.push(c(&format!("conf:{:.0}%", conf_pct), conf_color));
        }

        let metrics_str = metrics_parts.join("  ");
        let interrupt_marker = if turn.interrupted {
            c(" [INTERRUPTED]", Colors::RED)
        } else {
            String::new()
        };

        // Format turn
        lines.push(String::new());
        let role_str = turn.role.as_deref().unwrap_or("unknown").to_uppercase();
        lines.push(c(
            &format!("  [{}] {} {}{}", turn_num, role_icon, role_str, interrupt_marker),
            &colors(&[role_color, Colors::BOLD]),
        ));
        if !metrics_str.is_empty() {
            lines.push(format!("      {}", metrics_str));
        }

        // Word-wrap content
        let text = turn.text();
        for line in word_wrap(&text, 76, "      ") {
            lines.push(line);
        }
    }

    lines.push(String::new());

    // Errors
    if !analysis.errors.is_empty() {
        lines.push(c("ERRORS", &colors(&[Colors::BOLD, Colors::RED])));
        lines.push(c(&"-".repeat(80), Colors::DIM));
        for log in &analysis.errors {
            let rel_time = log.timestamp_sec() - analysis.session_start;
            lines.push(c(
                &format!("  [{}] {}", format_duration(rel_time), log.logger_name),
                Colors::RED,
            ));
            let msg: String = log.message.chars().take(200).collect();
            lines.push(format!("    {}", msg));
        }
        lines.push(String::new());
    }

    // Warnings
    if !analysis.warnings.is_empty() {
        lines.push(c("WARNINGS", &colors(&[Colors::BOLD, Colors::YELLOW])));
        lines.push(c(&"-".repeat(80), Colors::DIM));
        for log in analysis.warnings.iter().take(10) {
            let rel_time = log.timestamp_sec() - analysis.session_start;
            lines.push(c(
                &format!("  [{}] {}", format_duration(rel_time), log.logger_name),
                Colors::YELLOW,
            ));
            let msg: String = log.message.chars().take(200).collect();
            lines.push(format!("    {}", msg));
        }
        if analysis.warnings.len() > 10 {
            lines.push(c(
                &format!("  ... and {} more warnings", analysis.warnings.len() - 10),
                Colors::DIM,
            ));
        }
        lines.push(String::new());
    }

    // Tool Calls
    if !analysis.tool_calls.is_empty() {
        lines.push(c("TOOL CALLS", &colors(&[Colors::BOLD, Colors::BLUE])));
        lines.push(c(&"-".repeat(80), Colors::DIM));

        // Group by unique tool names and show summary
        let mut tool_counts: HashMap<String, usize> = HashMap::new();
        for t in &analysis.tool_calls {
            *tool_counts.entry(t.name.clone()).or_insert(0) += 1;
        }

        lines.push(c(
            &format!(
                "  Summary: {} calls, {} unique tools",
                analysis.tool_calls.len(),
                tool_counts.len()
            ),
            Colors::DIM,
        ));

        let mut sorted_tools: Vec<_> = tool_counts.iter().collect();
        sorted_tools.sort_by(|a, b| b.1.cmp(a.1));
        for (name, count) in sorted_tools {
            lines.push(format!("    {}: {}x", name, count));
        }
        lines.push(String::new());

        // Show timeline
        lines.push(c("  Timeline:", Colors::DIM));
        for tool in analysis.tool_calls.iter().take(15) {
            let rel_time = tool.start - analysis.session_start;
            let dur_str = if tool.duration_ms > 0.0 {
                format!("({:.0}ms)", tool.duration_ms)
            } else {
                String::new()
            };
            lines.push(format!(
                "    [{}] {} {}",
                format_duration(rel_time),
                tool.name,
                dur_str
            ));
        }
        if analysis.tool_calls.len() > 15 {
            lines.push(c(
                &format!("    ... and {} more", analysis.tool_calls.len() - 15),
                Colors::DIM,
            ));
        }
        lines.push(String::new());
    }

    // High Latency Turns
    let assistant_turns = analysis.assistant_turns();
    let high_latency_turns: Vec<_> = assistant_turns
        .iter()
        .filter(|t| t.metrics.e2e_latency.map(|e| e > 3.0).unwrap_or(false))
        .collect();

    if !high_latency_turns.is_empty() {
        lines.push(c(
            "HIGH LATENCY TURNS (>3s E2E)",
            &colors(&[Colors::BOLD, Colors::RED]),
        ));
        lines.push(c(&"-".repeat(80), Colors::DIM));
        for turn in &high_latency_turns {
            lines.push(c(
                &format!(
                    "  E2E: {}  LLM: {}  TTS: {}",
                    format_ms(turn.metrics.e2e_latency),
                    format_ms(turn.metrics.llm_node_ttft),
                    format_ms(turn.metrics.tts_node_ttfb)
                ),
                Colors::RED,
            ));
            let text = turn.text();
            let preview: String = text.chars().take(100).collect();
            if text.len() > 100 {
                lines.push(format!("    \"{}...\"", preview));
            } else {
                lines.push(format!("    \"{}\"", preview));
            }
        }
        lines.push(String::new());
    }

    // Key Spans Timeline
    const KEY_SPAN_NAMES: &[&str] = &[
        "agent_turn",
        "user_turn",
        "llm_node",
        "tts_request",
        "stt_request",
        "function_call",
    ];
    let key_spans: Vec<_> = analysis
        .spans
        .iter()
        .filter(|s| KEY_SPAN_NAMES.contains(&s.name.as_str()))
        .collect();

    if !key_spans.is_empty() {
        lines.push(c(
            "KEY SPANS TIMELINE",
            &colors(&[Colors::BOLD, Colors::BLUE]),
        ));
        lines.push(c(&"-".repeat(80), Colors::DIM));
        for span in key_spans.iter().take(30) {
            let rel_start = span.start_sec() - analysis.session_start;
            lines.push(format!(
                "  [{}] {:20} ({:.0}ms)",
                format_duration(rel_start),
                span.name,
                span.duration_ms()
            ));
        }
        if key_spans.len() > 30 {
            lines.push(c(
                &format!("  ... and {} more spans", key_spans.len() - 30),
                Colors::DIM,
            ));
        }
        lines.push(String::new());
    }

    // Footer
    lines.push(c(&"=".repeat(80), Colors::BOLD));
    lines.push(c(
        &format!("  Report generated from: {}", analysis.folder_path.display()),
        Colors::DIM,
    ));
    lines.push(c(&"=".repeat(80), Colors::BOLD));

    lines.join("\n")
}

/// Simple timestamp formatting (without full chrono dependency).
fn chrono_format_timestamp(timestamp: f64) -> String {
    // Convert Unix timestamp to a simple readable format
    // This is a simplified version - for full formatting, add chrono crate
    let secs = timestamp as i64;
    let nanos = ((timestamp - secs as f64) * 1e9) as u32;

    // Simple ISO-8601 like format using std
    use std::time::{Duration, UNIX_EPOCH};
    let datetime = UNIX_EPOCH + Duration::new(secs as u64, nanos);

    // Format manually since we don't have chrono
    format!("{:?}", datetime)
}

// =============================================================================
// JSON REPORT GENERATION
// =============================================================================

/// JSON report structure for metadata.
#[derive(Debug, Serialize)]
pub struct JsonMetadata {
    pub room_id: String,
    pub job_id: String,
    pub agent_name: String,
    pub room_name: String,
    pub participant_identity: String,
    pub duration_sec: f64,
    pub session_start: f64,
    pub session_end: f64,
}

/// JSON report structure for summary.
#[derive(Debug, Serialize)]
pub struct JsonSummary {
    pub total_turns: usize,
    pub user_turns: usize,
    pub assistant_turns: usize,
    pub interrupted_turns: usize,
    pub errors: usize,
    pub warnings: usize,
    pub tool_calls: usize,
}

/// JSON report structure for latency stats.
#[derive(Debug, Serialize)]
pub struct JsonLatencyStats {
    pub avg_ms: Option<f64>,
    pub min_ms: Option<f64>,
    pub max_ms: Option<f64>,
    pub p95_ms: Option<f64>,
}

impl From<Option<&LatencyStats>> for JsonLatencyStats {
    fn from(stats: Option<&LatencyStats>) -> Self {
        match stats {
            Some(s) => JsonLatencyStats {
                avg_ms: Some(s.avg_ms),
                min_ms: Some(s.min_ms),
                max_ms: Some(s.max_ms),
                p95_ms: Some(s.p95_ms),
            },
            None => JsonLatencyStats {
                avg_ms: None,
                min_ms: None,
                max_ms: None,
                p95_ms: None,
            },
        }
    }
}

/// JSON report structure for latency.
#[derive(Debug, Serialize)]
pub struct JsonLatency {
    pub e2e: JsonLatencyStats,
    pub llm_ttft: JsonLatencyStats,
    pub tts_ttfb: JsonLatencyStats,
}

/// JSON report structure for turn metrics.
#[derive(Debug, Serialize)]
pub struct JsonTurnMetrics {
    pub e2e_latency_ms: Option<f64>,
    pub llm_ttft_ms: Option<f64>,
    pub tts_ttfb_ms: Option<f64>,
    pub speaking_duration_sec: Option<f64>,
    pub transcript_confidence: Option<f64>,
}

/// JSON report structure for a turn.
#[derive(Debug, Serialize)]
pub struct JsonTurn {
    pub index: usize,
    pub role: Option<String>,
    pub text: String,
    pub interrupted: bool,
    pub metrics: JsonTurnMetrics,
}

/// JSON report structure for high latency turn.
#[derive(Debug, Serialize)]
pub struct JsonHighLatencyTurn {
    pub index: usize,
    pub text: String,
    pub e2e_latency_ms: f64,
    pub llm_ttft_ms: Option<f64>,
    pub tts_ttfb_ms: Option<f64>,
}

/// JSON report structure for error.
#[derive(Debug, Serialize)]
pub struct JsonError {
    pub timestamp: f64,
    pub relative_time_sec: f64,
    pub logger: String,
    pub message: String,
}

/// JSON report structure for diagnosis.
#[derive(Debug, Serialize)]
pub struct JsonDiagnosis {
    pub verdict: String,
    pub primary_issue: Option<String>,
    pub primary_issue_detail: Option<String>,
    pub slow_turns_count: usize,
    pub tts_retries: usize,
    pub tool_errors: usize,
}

/// Complete JSON report structure.
#[derive(Debug, Serialize)]
pub struct JsonReport {
    pub metadata: JsonMetadata,
    pub summary: JsonSummary,
    pub diagnosis: Option<JsonDiagnosis>,
    pub latency: JsonLatency,
    pub turns: Vec<JsonTurn>,
    pub high_latency_turns: Vec<JsonHighLatencyTurn>,
    pub errors: Vec<JsonError>,
}

/// Generate a structured JSON report.
pub fn generate_json_report(analysis: &CallAnalysis) -> String {
    let report = build_json_report(analysis);
    serde_json::to_string_pretty(&report).unwrap_or_else(|_| "{}".to_string())
}

/// Build the JSON report structure.
fn build_json_report(analysis: &CallAnalysis) -> JsonReport {
    // Compute latency stats
    let e2e_values: Vec<f64> = analysis
        .assistant_turns()
        .iter()
        .filter_map(|t| t.metrics.e2e_latency)
        .collect();
    let llm_values: Vec<f64> = analysis
        .assistant_turns()
        .iter()
        .filter_map(|t| t.metrics.llm_node_ttft)
        .collect();
    let tts_values: Vec<f64> = analysis
        .assistant_turns()
        .iter()
        .filter_map(|t| t.metrics.tts_node_ttfb)
        .collect();

    let e2e_stats = LatencyStats::from_values(&e2e_values);
    let llm_stats = LatencyStats::from_values(&llm_values);
    let tts_stats = LatencyStats::from_values(&tts_values);

    // Build turns
    let turns: Vec<JsonTurn> = analysis
        .turns
        .iter()
        .enumerate()
        .filter(|(_, t)| t.turn_type == "message")
        .map(|(i, t)| JsonTurn {
            index: i + 1,
            role: t.role.clone(),
            text: t.text(),
            interrupted: t.interrupted,
            metrics: JsonTurnMetrics {
                e2e_latency_ms: t.metrics.e2e_latency.map(|v| v * 1000.0),
                llm_ttft_ms: t.metrics.llm_node_ttft.map(|v| v * 1000.0),
                tts_ttfb_ms: t.metrics.tts_node_ttfb.map(|v| v * 1000.0),
                speaking_duration_sec: t.metrics.speaking_duration(),
                transcript_confidence: t.metrics.transcript_confidence,
            },
        })
        .collect();

    // Build high latency turns
    let high_latency_turns: Vec<JsonHighLatencyTurn> = analysis
        .assistant_turns()
        .iter()
        .enumerate()
        .filter(|(_, t)| t.metrics.e2e_latency.map(|e| e > 3.0).unwrap_or(false))
        .map(|(i, t)| {
            let text: String = t.text().chars().take(100).collect();
            JsonHighLatencyTurn {
                index: i + 1,
                text,
                e2e_latency_ms: t.metrics.e2e_latency.unwrap_or(0.0) * 1000.0,
                llm_ttft_ms: t.metrics.llm_node_ttft.map(|v| v * 1000.0),
                tts_ttfb_ms: t.metrics.tts_node_ttfb.map(|v| v * 1000.0),
            }
        })
        .collect();

    // Build errors
    let errors: Vec<JsonError> = analysis
        .errors
        .iter()
        .map(|log| JsonError {
            timestamp: log.timestamp_sec(),
            relative_time_sec: log.timestamp_sec() - analysis.session_start,
            logger: log.logger_name.clone(),
            message: log.message.clone(),
        })
        .collect();

    // Build diagnosis
    let diagnosis = analysis.diagnosis.as_ref().map(|d| {
        let slow_turns_count: usize = d.slow_turns_by_cause.values().map(|v| v.len()).sum();
        JsonDiagnosis {
            verdict: match d.verdict {
                DiagnosisVerdict::Healthy => "healthy".to_string(),
                DiagnosisVerdict::NeedsAttention => "needs_attention".to_string(),
                DiagnosisVerdict::Problematic => "problematic".to_string(),
            },
            primary_issue: d.primary_issue.clone(),
            primary_issue_detail: d.primary_issue_detail.clone(),
            slow_turns_count,
            tts_retries: d.tts_retries,
            tool_errors: d.tool_errors,
        }
    });

    JsonReport {
        metadata: JsonMetadata {
            room_id: analysis.room_id.clone(),
            job_id: analysis.job_id.clone(),
            agent_name: analysis.agent_name.clone(),
            room_name: analysis.room_name.clone(),
            participant_identity: analysis.participant_identity.clone(),
            duration_sec: analysis.duration_sec(),
            session_start: analysis.session_start,
            session_end: analysis.session_end,
        },
        summary: JsonSummary {
            total_turns: analysis.turns.len(),
            user_turns: analysis.user_turns().len(),
            assistant_turns: analysis.assistant_turns().len(),
            interrupted_turns: analysis.interrupted_turns().len(),
            errors: analysis.errors.len(),
            warnings: analysis.warnings.len(),
            tool_calls: analysis.tool_calls.len(),
        },
        diagnosis,
        latency: JsonLatency {
            e2e: JsonLatencyStats::from(e2e_stats.as_ref()),
            llm_ttft: JsonLatencyStats::from(llm_stats.as_ref()),
            tts_ttfb: JsonLatencyStats::from(tts_stats.as_ref()),
        },
        turns,
        high_latency_turns,
        errors,
    }
}
