//! Report generation for call analysis output.
//!
//! Provides text and JSON report formats similar to the Python call_analyzer.py.

use std::collections::HashMap;
use std::io::IsTerminal;

use serde::Serialize;

use crate::data::{CallAnalysis, DiagnosisVerdict, LatencyStats, PipelineSummary, Severity};
use crate::format::{format_duration, format_ms, word_wrap, truncate};
use crate::thresholds::{
    self, CAUSE_ICONS,
    MAX_SLOW_TURNS_PER_CAUSE, MAX_WARNINGS_DISPLAY, MAX_TOOL_CALLS_DISPLAY,
    MAX_SPANS_DISPLAY, MAX_DETECTED_DELAYS,
    TEXT_PREVIEW_MEDIUM, TEXT_PREVIEW_LONG,
    cause_label,
};

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

/// Get ANSI color based on latency value (seconds).
fn latency_color(ms: Option<f64>) -> &'static str {
    match ms {
        None => Colors::DIM,
        Some(v) => severity_to_ansi(thresholds::e2e_severity(v * 1000.0)),
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

        for delay in summary.detected_delays.iter().take(MAX_DETECTED_DELAYS) {
            lines.push(format!(
                "    Turn {}: +{:.1}s gap -> {}",
                delay.turn_number,
                delay.gap_ms / 1000.0,
                delay.reason
            ));
        }

        if summary.detected_delays.len() > MAX_DETECTED_DELAYS {
            lines.push(c(
                &format!("    ... and {} more turns with gaps", summary.detected_delays.len() - MAX_DETECTED_DELAYS),
                Colors::DIM,
            ));
        }
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

/// Generate a logs dump (all logs in simple format).
pub fn generate_logs_report(analysis: &CallAnalysis) -> String {
    let mut lines: Vec<String> = Vec::new();

    lines.push(format!("# LOGS ({} total, {} errors, {} warnings)",
        analysis.logs.len(), analysis.errors.len(), analysis.warnings.len()));
    lines.push(String::new());

    for log in &analysis.logs {
        let rel_time = log.timestamp_sec() - analysis.session_start;
        let severity = match log.severity.as_str() {
            "ERROR" | "CRITICAL" => "[ERROR]",
            "WARN" | "WARNING" => "[WARN] ",
            "INFO" => "[INFO] ",
            "DEBUG" => "[DEBUG]",
            _ => "[?????]",
        };

        lines.push(format!("{:>8.2}s {} {} | {}",
            rel_time,
            severity,
            truncate(&log.logger_name, 30),
            log.message.replace('\n', " | ")
        ));
    }

    lines.join("\n")
}

/// Generate a spans dump (all spans with timing).
pub fn generate_spans_report(analysis: &CallAnalysis) -> String {
    let mut lines: Vec<String> = Vec::new();

    lines.push(format!("# SPANS ({} total)", analysis.spans.len()));
    lines.push(String::new());
    lines.push(format!("{:>8}  {:>8}  {:30}  {}", "START", "DUR(ms)", "NAME", "TRACE_ID"));
    lines.push(format!("{}", "-".repeat(80)));

    for span in &analysis.spans {
        let rel_start = span.start_sec() - analysis.session_start;
        let dur_ms = span.duration_ms();

        // Mark key spans
        let marker = if thresholds::is_key_span(&span.name) { "*" } else { " " };

        lines.push(format!("{:>7.2}s  {:>7.0}ms  {:30}{} {}",
            rel_start,
            dur_ms,
            truncate(&span.name, 30),
            marker,
            truncate(&span.span_id, 16)
        ));
    }

    lines.push(String::new());
    lines.push("(* = key span)".to_string());

    lines.join("\n")
}

/// Generate transcript only (conversation).
pub fn generate_transcript_report(analysis: &CallAnalysis) -> String {
    let mut lines: Vec<String> = Vec::new();

    lines.push(format!("# TRANSCRIPT ({} turns)", analysis.turns.len()));
    lines.push(format!("# Duration: {}", format_duration(analysis.duration_sec())));
    lines.push(String::new());

    for (i, turn) in analysis.turns.iter().enumerate() {
        let turn_num = i + 1;

        if turn.turn_type == "agent_handoff" {
            let new_agent = turn
                .extra
                .get("new_agent_id")
                .and_then(|v| v.as_str())
                .unwrap_or(&turn.id);
            lines.push(format!("[{}] -- HANDOFF to {} --", turn_num, new_agent));
            continue;
        }

        if turn.turn_type == "function_call" {
            let fn_name = turn.extra.get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            let args_summary = turn.extra.get("arguments")
                .map(|v| {
                    let s = if let Some(s) = v.as_str() { s.to_string() } else { v.to_string() };
                    summarize_tool_args_text(&s)
                })
                .unwrap_or_default();
            if args_summary.is_empty() {
                lines.push(format!("[{}] TOOL: {}()", turn_num, fn_name));
            } else {
                lines.push(format!("[{}] TOOL: {}({})", turn_num, fn_name, args_summary));
            }
            continue;
        }

        if turn.turn_type == "function_call_output" {
            let fn_name = turn.extra.get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("tool");
            let output = turn.extra.get("output")
                .and_then(|v| v.as_str())
                .unwrap_or("ok");
            let output_display = if output.is_empty() || output == "ok" || output == "\"ok\"" {
                "ok".to_string()
            } else if output.len() > 60 {
                format!("{}...", &output[..57])
            } else {
                output.to_string()
            };
            lines.push(format!("[{}]   {} -> {}", turn_num, fn_name, output_display));
            continue;
        }

        let role = turn.role.as_deref().unwrap_or("?").to_uppercase();
        let text = turn.text();

        // Build metrics
        let mut metrics = Vec::new();
        if let Some(e2e) = turn.metrics.e2e_latency {
            metrics.push(format!("e2e={:.0}ms", e2e * 1000.0));
        }
        if let Some(llm) = turn.metrics.llm_node_ttft {
            metrics.push(format!("llm={:.0}ms", llm * 1000.0));
        }
        if let Some(tts) = turn.metrics.tts_node_ttfb {
            metrics.push(format!("tts={:.0}ms", tts * 1000.0));
        }
        if turn.interrupted {
            metrics.push("INTERRUPTED".to_string());
        }

        let metrics_str = if metrics.is_empty() {
            String::new()
        } else {
            format!(" ({})", metrics.join(", "))
        };

        lines.push(format!("[{}] {}{}", turn_num, role, metrics_str));

        // Show E2E breakdown if available
        if let Some(ref bd) = turn.breakdown {
            if bd.has_tool_call || bd.overhead_ms.map(|v| v > 500.0).unwrap_or(false) {
                let mut parts = Vec::new();
                if let Some(stt) = bd.stt_ms { parts.push(format!("stt={:.0}ms", stt)); }
                if let Some(eol) = bd.eol_ms { parts.push(format!("eol={:.0}ms", eol)); }
                if let Some(first) = bd.first_llm_ms { parts.push(format!("llm1={:.0}ms", first)); }
                if let Some(tool) = bd.tool_ms {
                    let names = if bd.tool_names.is_empty() {
                        String::new()
                    } else {
                        format!("[{}]", bd.tool_names.join(","))
                    };
                    parts.push(format!("tool={:.0}ms{}", tool, names));
                }
                if let Some(llm) = bd.llm_ms { parts.push(format!("llm2={:.0}ms", llm)); }
                if let Some(tts) = bd.tts_ms { parts.push(format!("tts={:.0}ms", tts)); }
                if let Some(oh) = bd.overhead_ms { parts.push(format!("other={:.0}ms", oh)); }
                if !parts.is_empty() {
                    lines.push(format!("    [{}]", parts.join(" -> ")));
                }
            }
        }

        // Word-wrap text
        for line in word_wrap(&text, 76, "    ") {
            lines.push(line);
        }
        lines.push(String::new());
    }

    lines.join("\n")
}

/// Generate full dump (everything in one output).
pub fn generate_dump_report(analysis: &CallAnalysis) -> String {
    let mut parts: Vec<String> = Vec::new();

    // Summary first
    parts.push("=".repeat(80));
    parts.push("SUMMARY".to_string());
    parts.push("=".repeat(80));
    parts.push(generate_summary_report(analysis));
    parts.push(String::new());

    // Transcript
    parts.push("=".repeat(80));
    parts.push(generate_transcript_report(analysis));
    parts.push(String::new());

    // Tool calls
    if !analysis.tool_calls.is_empty() {
        parts.push("=".repeat(80));
        parts.push(format!("# TOOL CALLS ({} total)", analysis.tool_calls.len()));
        parts.push(String::new());

        for tool in &analysis.tool_calls {
            let rel_time = tool.start - analysis.session_start;
            parts.push(format!("{:>7.2}s  {:>6.0}ms  {}",
                rel_time,
                tool.duration_ms,
                tool.name
            ));
        }
        parts.push(String::new());
    }

    // E2E Breakdowns for turns with tool calls or significant overhead
    let turns_with_breakdown: Vec<(usize, &crate::data::ConversationTurn)> = analysis.turns.iter()
        .enumerate()
        .filter(|(_, t)| t.breakdown.as_ref()
            .map(|b| b.has_tool_call || b.overhead_ms.map(|v| v > 500.0).unwrap_or(false))
            .unwrap_or(false))
        .collect();

    if !turns_with_breakdown.is_empty() {
        parts.push("=".repeat(80));
        parts.push(format!("# E2E BREAKDOWNS ({} turns with tool calls or significant overhead)",
            turns_with_breakdown.len()));
        parts.push(String::new());

        for (i, turn) in &turns_with_breakdown {
            let bd = turn.breakdown.as_ref().unwrap();
            let e2e = turn.metrics.e2e_latency.map(|v| v * 1000.0).unwrap_or(0.0);
            let text_preview: String = turn.text().chars().take(60).collect();

            parts.push(format!("  Turn {} (e2e={:.0}ms): \"{}...\"", i + 1, e2e, text_preview));

            if let Some(stt) = bd.stt_ms { parts.push(format!("    STT:        {:>6.0}ms  (transcription)", stt)); }
            if let Some(eol) = bd.eol_ms { parts.push(format!("    EOL:        {:>6.0}ms  (end-of-turn detection)", eol)); }
            if let Some(fl) = bd.first_llm_ms { parts.push(format!("    LLM (1st):  {:>6.0}ms  (tool decision)", fl)); }
            if let Some(tool) = bd.tool_ms {
                let names = bd.tool_names.join(", ");
                parts.push(format!("    Tool exec:  {:>6.0}ms  ({})", tool, if names.is_empty() { "tool call" } else { &names }));
            }
            if let Some(llm) = bd.llm_ms { parts.push(format!("    LLM (2nd):  {:>6.0}ms  (response generation)", llm)); }
            if let Some(tts) = bd.tts_ms { parts.push(format!("    TTS:        {:>6.0}ms  (speech synthesis)", tts)); }
            if let Some(oh) = bd.overhead_ms { parts.push(format!("    Overhead:   {:>6.0}ms  (processing/network)", oh)); }

            // Percentage bar
            if e2e > 0.0 {
                let pcts: Vec<String> = [
                    ("STT", bd.stt_ms),
                    ("EOL", bd.eol_ms),
                    ("LLM1", bd.first_llm_ms),
                    ("TOOL", bd.tool_ms),
                    ("LLM2", bd.llm_ms),
                    ("TTS", bd.tts_ms),
                    ("OTHER", bd.overhead_ms),
                ].iter()
                    .filter_map(|(name, val)| val.map(|v| format!("{}:{:.0}%", name, v / e2e * 100.0)))
                    .collect();
                parts.push(format!("    [{}]", pcts.join(" | ")));
            }

            parts.push(String::new());
        }
    }

    // Model & Provider Info
    let sm = &analysis.span_metrics;
    if sm.llm_request_count > 0 || sm.tts_request_count > 0 {
        parts.push("=".repeat(80));
        parts.push("# MODEL & PROVIDER INFO".to_string());
        parts.push(String::new());

        if !sm.llm_model.is_empty() {
            parts.push(format!("  LLM Model:    {}", sm.llm_model));
        }
        if !sm.tts_provider.is_empty() {
            parts.push(format!("  TTS Provider: {}", sm.tts_provider));
        }
        parts.push(String::new());

        if sm.llm_request_count > 0 {
            parts.push("  LLM Usage:".to_string());
            parts.push(format!("    Requests:       {}", sm.llm_request_count));
            parts.push(format!("    Prompt tokens:  {} ({} cached, {:.1}% hit rate)",
                sm.total_prompt_tokens, sm.total_cached_tokens, sm.cache_hit_pct));
            parts.push(format!("    Output tokens:  {}", sm.total_completion_tokens));
            if sm.avg_tokens_per_sec > 0.0 {
                parts.push(format!("    Speed:          {:.1} tok/s avg, {:.1} tok/s min",
                    sm.avg_tokens_per_sec, sm.min_tokens_per_sec));
            }
            if sm.cancelled_llm_count > 0 {
                parts.push(format!("    Cancelled:      {} requests", sm.cancelled_llm_count));
            }
            parts.push(String::new());
        }

        if sm.tts_request_count > 0 {
            parts.push("  TTS Usage:".to_string());
            parts.push(format!("    Requests:       {}", sm.tts_request_count));
            if sm.avg_tts_realtime_factor > 0.0 {
                parts.push(format!("    Realtime factor: {:.1}x (audio produced {:.1}x faster than realtime)",
                    sm.avg_tts_realtime_factor, sm.avg_tts_realtime_factor));
            }
            if sm.cancelled_tts_count > 0 {
                parts.push(format!("    Cancelled:      {} requests", sm.cancelled_tts_count));
            }
            parts.push(String::new());
        }

        if sm.eou_count > 0 {
            parts.push("  EOU (End-of-Utterance) Detection:".to_string());
            parts.push(format!("    Total detections:  {}", sm.eou_count));
            parts.push(format!("    High confidence:   {} (>50%)", sm.eou_high_confidence_count));
            parts.push(format!("    Low confidence:    {} (<10%)", sm.eou_low_confidence_count));
            parts.push(format!("    Avg probability:   {:.3}", sm.eou_avg_probability));
            if sm.eou_endpointing_delay > 0.0 {
                parts.push(format!("    Endpointing delay: {:.1}s", sm.eou_endpointing_delay));
            }
            parts.push(String::new());
        }
    }

    // Errors (always show)
    parts.push("=".repeat(80));
    parts.push(format!("# ERRORS ({} total)", analysis.errors.len()));
    parts.push(String::new());

    if analysis.errors.is_empty() {
        parts.push("(no errors)".to_string());
    } else {
        for log in &analysis.errors {
            let rel_time = log.timestamp_sec() - analysis.session_start;
            parts.push(format!("{:>7.2}s  {} | {}",
                rel_time,
                log.logger_name,
                log.message.replace('\n', " | ")
            ));
        }
    }
    parts.push(String::new());

    // Warnings
    if !analysis.warnings.is_empty() {
        parts.push("=".repeat(80));
        parts.push(format!("# WARNINGS ({} total)", analysis.warnings.len()));
        parts.push(String::new());

        for log in analysis.warnings.iter().take(20) {
            let rel_time = log.timestamp_sec() - analysis.session_start;
            parts.push(format!("{:>7.2}s  {} | {}",
                rel_time,
                log.logger_name,
                truncate(&log.message.replace('\n', " | "), 100)
            ));
        }
        if analysis.warnings.len() > 20 {
            parts.push(format!("... and {} more warnings", analysis.warnings.len() - 20));
        }
        parts.push(String::new());
    }

    // Key spans only (not all spans - too verbose)
    let key_spans: Vec<_> = analysis.spans.iter()
        .filter(|s| thresholds::is_key_span(&s.name))
        .collect();

    if !key_spans.is_empty() {
        parts.push("=".repeat(80));
        parts.push(format!("# KEY SPANS ({} of {} total)", key_spans.len(), analysis.spans.len()));
        parts.push(String::new());
        parts.push(format!("{:>8}  {:>8}  {}", "START", "DUR(ms)", "NAME"));

        for span in key_spans.iter().take(50) {
            let rel_start = span.start_sec() - analysis.session_start;
            parts.push(format!("{:>7.2}s  {:>7.0}ms  {}",
                rel_start,
                span.duration_ms(),
                span.name
            ));
        }
        if key_spans.len() > 50 {
            parts.push(format!("... and {} more spans", key_spans.len() - 50));
        }
    }

    parts.join("\n")
}

/// Generate a brief summary report (for agents/scripts).
/// Format: key=value pairs, one per line, easy to parse.
pub fn generate_summary_report(analysis: &CallAnalysis) -> String {
    let mut lines: Vec<String> = Vec::new();

    // Compute stats
    let (e2e_stats, llm_stats, tts_stats) = analysis.compute_latency_stats();
    let summary = PipelineSummary::from_cycles(&analysis.pipeline_cycles);
    let diagnosis = analysis.diagnosis.as_ref();

    // Verdict
    let verdict = match diagnosis.map(|d| &d.verdict) {
        Some(DiagnosisVerdict::Healthy) => "HEALTHY",
        Some(DiagnosisVerdict::Problematic) => "PROBLEMATIC",
        Some(DiagnosisVerdict::NeedsAttention) => "NEEDS_ATTENTION",
        None => "UNKNOWN",
    };
    lines.push(format!("verdict={}", verdict));

    // Quick counts
    let slow_turns: usize = diagnosis
        .map(|d| d.slow_turns_by_cause.values().map(|v| v.len()).sum())
        .unwrap_or(0);
    lines.push(format!("slow_turns={}", slow_turns));
    lines.push(format!("errors={}", analysis.errors.len()));
    lines.push(format!("warnings={}", analysis.warnings.len()));

    // Call info
    lines.push(format!("duration_sec={:.1}", analysis.duration_sec()));
    lines.push(format!("total_turns={}", analysis.turns.len()));
    lines.push(format!("user_turns={}", analysis.user_turns().len()));
    lines.push(format!("assistant_turns={}", analysis.assistant_turns().len()));
    lines.push(format!("interrupted={}", analysis.interrupted_turns().len()));
    lines.push(format!("tool_calls={}", analysis.tool_calls.len()));

    // Latency stats (in ms)
    if let Some(ref stats) = e2e_stats {
        lines.push(format!("e2e_avg_ms={:.0}", stats.avg_ms));
        lines.push(format!("e2e_p95_ms={:.0}", stats.p95_ms));
        lines.push(format!("e2e_max_ms={:.0}", stats.max_ms));
    }
    if let Some(ref stats) = llm_stats {
        lines.push(format!("llm_avg_ms={:.0}", stats.avg_ms));
        lines.push(format!("llm_p95_ms={:.0}", stats.p95_ms));
    }
    if let Some(ref stats) = tts_stats {
        lines.push(format!("tts_avg_ms={:.0}", stats.avg_ms));
        lines.push(format!("tts_p95_ms={:.0}", stats.p95_ms));
    }

    // Pipeline breakdown
    if let Some(ref s) = summary {
        lines.push(format!("bottleneck={}", s.bottleneck.replace(' ', "_")));
        lines.push(format!("llm_pct={:.0}", s.llm_pct));
        lines.push(format!("tts_pct={:.0}", s.tts_pct));
        lines.push(format!("perception_ms={:.0}", s.avg_user_to_llm_ms));
    }

    // Primary issue
    if let Some(diag) = diagnosis {
        if let Some(ref issue) = diag.primary_issue {
            lines.push(format!("primary_issue={}", issue.replace(' ', "_")));
        }
        if diag.tts_retries > 0 {
            lines.push(format!("tts_retries={}", diag.tts_retries));
        }
        if diag.tool_errors > 0 {
            lines.push(format!("tool_errors={}", diag.tool_errors));
        }
    }

    // Slow turn causes
    if let Some(diag) = diagnosis {
        for (cause, turns) in &diag.slow_turns_by_cause {
            if !turns.is_empty() {
                lines.push(format!("slow_{}_turns={}", cause.to_lowercase(), turns.len()));
            }
        }
    }

    // User-side delay averages
    let user_turns_with_delays: Vec<_> = analysis.turns.iter()
        .filter(|t| t.role.as_deref() == Some("user") && t.metrics.transcription_delay.is_some())
        .collect();

    if !user_turns_with_delays.is_empty() {
        let count = user_turns_with_delays.len() as f64;
        let avg_stt: f64 = user_turns_with_delays.iter()
            .filter_map(|t| t.metrics.transcription_delay)
            .map(|v| v * 1000.0)
            .sum::<f64>() / count;
        let avg_eol: f64 = user_turns_with_delays.iter()
            .filter_map(|t| t.metrics.end_of_turn_delay)
            .map(|v| v * 1000.0)
            .sum::<f64>() / count;
        lines.push(format!("stt_avg_ms={:.0}", avg_stt));
        lines.push(format!("eol_avg_ms={:.0}", avg_eol));
    }

    // Tool-call turn breakdown stats
    let tool_call_turns: Vec<_> = analysis.turns.iter()
        .filter(|t| t.breakdown.as_ref().map(|b| b.has_tool_call).unwrap_or(false))
        .collect();

    if !tool_call_turns.is_empty() {
        lines.push(format!("tool_call_turns={}", tool_call_turns.len()));

        let first_llm_vals: Vec<f64> = tool_call_turns.iter()
            .filter_map(|t| t.breakdown.as_ref().and_then(|b| b.first_llm_ms))
            .collect();
        if !first_llm_vals.is_empty() {
            let avg: f64 = first_llm_vals.iter().sum::<f64>() / first_llm_vals.len() as f64;
            lines.push(format!("first_llm_avg_ms={:.0}", avg));
        }

        let tool_exec_vals: Vec<f64> = tool_call_turns.iter()
            .filter_map(|t| t.breakdown.as_ref().and_then(|b| b.tool_ms))
            .collect();
        if !tool_exec_vals.is_empty() {
            let avg: f64 = tool_exec_vals.iter().sum::<f64>() / tool_exec_vals.len() as f64;
            lines.push(format!("tool_exec_avg_ms={:.0}", avg));
        }
    }

    // Room info
    lines.push(format!("room_id={}", analysis.room_id));
    lines.push(format!("agent={}", analysis.agent_name));

    // Span-derived metrics
    let sm = &analysis.span_metrics;
    if !sm.llm_model.is_empty() {
        lines.push(format!("llm_model={}", sm.llm_model));
    }
    if !sm.tts_provider.is_empty() {
        lines.push(format!("tts_provider={}", sm.tts_provider));
    }
    if sm.llm_request_count > 0 {
        lines.push(format!("llm_requests={}", sm.llm_request_count));
        lines.push(format!("total_prompt_tokens={}", sm.total_prompt_tokens));
        lines.push(format!("total_completion_tokens={}", sm.total_completion_tokens));
        lines.push(format!("cache_hit_pct={:.1}", sm.cache_hit_pct));
        if sm.avg_tokens_per_sec > 0.0 {
            lines.push(format!("avg_tokens_per_sec={:.1}", sm.avg_tokens_per_sec));
            lines.push(format!("min_tokens_per_sec={:.1}", sm.min_tokens_per_sec));
        }
        if sm.cancelled_llm_count > 0 {
            lines.push(format!("cancelled_llm_requests={}", sm.cancelled_llm_count));
        }
    }
    if sm.tts_request_count > 0 {
        lines.push(format!("tts_requests={}", sm.tts_request_count));
        if sm.avg_tts_realtime_factor > 0.0 {
            lines.push(format!("tts_realtime_factor={:.1}", sm.avg_tts_realtime_factor));
        }
        if sm.cancelled_tts_count > 0 {
            lines.push(format!("cancelled_tts_requests={}", sm.cancelled_tts_count));
        }
    }
    if sm.eou_count > 0 {
        lines.push(format!("eou_detections={}", sm.eou_count));
        lines.push(format!("eou_high_confidence={}", sm.eou_high_confidence_count));
        lines.push(format!("eou_low_confidence={}", sm.eou_low_confidence_count));
        lines.push(format!("eou_avg_probability={:.3}", sm.eou_avg_probability));
        if sm.eou_endpointing_delay > 0.0 {
            lines.push(format!("eou_endpointing_delay_sec={:.1}", sm.eou_endpointing_delay));
        }
    }

    lines.join("\n")
}

fn generate_text_report_impl(analysis: &CallAnalysis, use_color: bool) -> String {
    let mut lines: Vec<String> = Vec::new();
    let c = |text: &str, color: &str| colorize(text, color, use_color);

    // Compute latency stats using centralized method
    let (e2e_stats, llm_stats, tts_stats) = analysis.compute_latency_stats();

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

            // Show breakdown by cause (using centralized constants)
            for (cause, icon) in CAUSE_ICONS {
                if let Some(turns) = diag.slow_turns_by_cause.get(*cause) {
                    if turns.is_empty() {
                        continue;
                    }

                    let color = if *cause == "LLM" || *cause == "TTS" || *cause == "TOOL" {
                        Colors::RED
                    } else {
                        Colors::YELLOW
                    };

                    lines.push(c(
                        &format!("  {} {} BOTTLENECK: {} turns", icon, cause_label(cause), turns.len()),
                        &colors(&[color, Colors::BOLD]),
                    ));

                    for t in turns.iter().take(MAX_SLOW_TURNS_PER_CAUSE) {
                        let cause_ms = match *cause {
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

                        // Show inline breakdown for TOOL/OVERHEAD causes
                        if *cause == "TOOL" || *cause == "OVERHEAD" {
                            let mut bd_parts = Vec::new();
                            if let Some(stt) = t.stt_ms { bd_parts.push(format!("stt={:.0}ms", stt)); }
                            if let Some(eol) = t.eol_ms { bd_parts.push(format!("eol={:.0}ms", eol)); }
                            if let Some(fl) = t.first_llm_ms { bd_parts.push(format!("llm1={:.0}ms", fl)); }
                            if let Some(te) = t.tool_exec_ms { bd_parts.push(format!("tool={:.0}ms", te)); }
                            bd_parts.push(format!("llm2={:.0}ms", t.llm_ms));
                            bd_parts.push(format!("tts={:.0}ms", t.tts_ms));
                            if !bd_parts.is_empty() {
                                lines.push(c(
                                    &format!("      Breakdown: {}", bd_parts.join(" + ")),
                                    Colors::DIM,
                                ));
                            }
                        }

                        lines.push(format!("      \"{}...\"", truncate(&t.text, TEXT_PREVIEW_MEDIUM)));
                    }

                    if turns.len() > MAX_SLOW_TURNS_PER_CAUSE {
                        lines.push(c(
                            &format!("    ... and {} more {}-slow turns", turns.len() - MAX_SLOW_TURNS_PER_CAUSE, cause),
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

    // Model & Provider
    let sm = &analysis.span_metrics;
    if sm.llm_request_count > 0 || sm.tts_request_count > 0 {
        lines.push(c("MODEL & PROVIDER", &colors(&[Colors::BOLD, Colors::BLUE])));
        lines.push(c(&"-".repeat(40), Colors::DIM));
        if !sm.llm_model.is_empty() {
            lines.push(format!("  LLM:  {} ({} reqs, {:.1} tok/s avg)",
                sm.llm_model, sm.llm_request_count, sm.avg_tokens_per_sec));
        }
        if !sm.tts_provider.is_empty() {
            lines.push(format!("  TTS:  {} ({} reqs, {:.1}x realtime)",
                sm.tts_provider, sm.tts_request_count, sm.avg_tts_realtime_factor));
        }
        if sm.total_prompt_tokens > 0 {
            lines.push(format!("  Tokens: {}K prompt ({:.0}% cached), {}K completion",
                sm.total_prompt_tokens / 1000, sm.cache_hit_pct, sm.total_completion_tokens / 1000));
        }
        if sm.eou_count > 0 {
            lines.push(format!("  EOU:  {}/{} high confidence, avg prob={:.3}, delay={:.1}s",
                sm.eou_high_confidence_count, sm.eou_count,
                sm.eou_avg_probability, sm.eou_endpointing_delay));
        }
        lines.push(String::new());
    }

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
            lines.push(format!("    {}", truncate(&log.message, TEXT_PREVIEW_LONG)));
        }
        lines.push(String::new());
    }

    // Warnings
    if !analysis.warnings.is_empty() {
        lines.push(c("WARNINGS", &colors(&[Colors::BOLD, Colors::YELLOW])));
        lines.push(c(&"-".repeat(80), Colors::DIM));
        for log in analysis.warnings.iter().take(MAX_WARNINGS_DISPLAY) {
            let rel_time = log.timestamp_sec() - analysis.session_start;
            lines.push(c(
                &format!("  [{}] {}", format_duration(rel_time), log.logger_name),
                Colors::YELLOW,
            ));
            lines.push(format!("    {}", truncate(&log.message, TEXT_PREVIEW_LONG)));
        }
        if analysis.warnings.len() > MAX_WARNINGS_DISPLAY {
            lines.push(c(
                &format!("  ... and {} more warnings", analysis.warnings.len() - MAX_WARNINGS_DISPLAY),
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
        for tool in analysis.tool_calls.iter().take(MAX_TOOL_CALLS_DISPLAY) {
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
        if analysis.tool_calls.len() > MAX_TOOL_CALLS_DISPLAY {
            lines.push(c(
                &format!("    ... and {} more", analysis.tool_calls.len() - MAX_TOOL_CALLS_DISPLAY),
                Colors::DIM,
            ));
        }
        lines.push(String::new());
    }

    // High Latency Turns
    let assistant_turns = analysis.assistant_turns();
    let high_latency_threshold = thresholds::E2E_SLOW_MS / 1000.0 * 1.5; // 3s
    let high_latency_turns: Vec<_> = assistant_turns
        .iter()
        .filter(|t| t.metrics.e2e_latency.map(|e| e > high_latency_threshold).unwrap_or(false))
        .collect();

    if !high_latency_turns.is_empty() {
        lines.push(c(
            &format!("HIGH LATENCY TURNS (>{:.0}s E2E)", high_latency_threshold),
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
            lines.push(format!("    \"{}\"", truncate(&text, TEXT_PREVIEW_MEDIUM)));
        }
        lines.push(String::new());
    }

    // Key Spans Timeline (using centralized KEY_SPAN_NAMES)
    let key_spans: Vec<_> = analysis
        .spans
        .iter()
        .filter(|s| thresholds::is_key_span(&s.name))
        .collect();

    if !key_spans.is_empty() {
        lines.push(c(
            "KEY SPANS TIMELINE",
            &colors(&[Colors::BOLD, Colors::BLUE]),
        ));
        lines.push(c(&"-".repeat(80), Colors::DIM));
        for span in key_spans.iter().take(MAX_SPANS_DISPLAY) {
            let rel_start = span.start_sec() - analysis.session_start;
            lines.push(format!(
                "  [{}] {:20} ({:.0}ms)",
                format_duration(rel_start),
                span.name,
                span.duration_ms()
            ));
        }
        if key_spans.len() > MAX_SPANS_DISPLAY {
            lines.push(c(
                &format!("  ... and {} more spans", key_spans.len() - MAX_SPANS_DISPLAY),
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

/// JSON report structure for a turn's E2E breakdown.
#[derive(Debug, Serialize)]
pub struct JsonTurnBreakdown {
    pub stt_ms: Option<f64>,
    pub eol_ms: Option<f64>,
    pub first_llm_ms: Option<f64>,
    pub tool_ms: Option<f64>,
    pub tool_names: Vec<String>,
    pub llm_ms: Option<f64>,
    pub tts_ms: Option<f64>,
    pub overhead_ms: Option<f64>,
    pub has_tool_call: bool,
}

/// JSON report structure for turn metrics.
#[derive(Debug, Serialize)]
pub struct JsonTurnMetrics {
    pub e2e_latency_ms: Option<f64>,
    pub llm_ttft_ms: Option<f64>,
    pub tts_ttfb_ms: Option<f64>,
    pub speaking_duration_sec: Option<f64>,
    pub transcript_confidence: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub breakdown: Option<JsonTurnBreakdown>,
}

/// JSON report structure for a turn.
#[derive(Debug, Serialize)]
pub struct JsonTurn {
    pub index: usize,
    pub turn_type: String,
    pub role: Option<String>,
    pub text: String,
    pub interrupted: bool,
    pub created_at: f64,
    pub relative_time_sec: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_arguments: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gap_from_previous_sec: Option<f64>,
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

/// JSON report structure for span-derived metrics.
#[derive(Debug, Serialize)]
pub struct JsonSpanMetrics {
    pub llm_model: String,
    pub tts_provider: String,
    pub llm_requests: usize,
    pub total_prompt_tokens: u64,
    pub total_completion_tokens: u64,
    pub total_cached_tokens: u64,
    pub cache_hit_pct: f64,
    pub avg_tokens_per_sec: f64,
    pub min_tokens_per_sec: f64,
    pub cancelled_llm_requests: usize,
    pub tts_requests: usize,
    pub avg_tts_realtime_factor: f64,
    pub cancelled_tts_requests: usize,
    pub eou_detections: usize,
    pub eou_high_confidence: usize,
    pub eou_low_confidence: usize,
    pub eou_avg_probability: f64,
    pub eou_endpointing_delay_sec: f64,
}

/// Complete JSON report structure.
#[derive(Debug, Serialize)]
pub struct JsonReport {
    pub metadata: JsonMetadata,
    pub summary: JsonSummary,
    pub diagnosis: Option<JsonDiagnosis>,
    pub latency: JsonLatency,
    pub span_metrics: JsonSpanMetrics,
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
    // Compute latency stats using centralized method
    let (e2e_stats, llm_stats, tts_stats) = analysis.compute_latency_stats();

    // Build turns (include all turn types with timestamps)
    let mut prev_created_at: Option<f64> = None;
    let turns: Vec<JsonTurn> = analysis
        .turns
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let gap = prev_created_at
                .filter(|&prev| prev > 0.0 && t.created_at > 0.0)
                .map(|prev| t.created_at - prev);
            prev_created_at = Some(t.created_at);

            // Extract tool info for function_call / function_call_output turns
            let (tool_name, tool_arguments, tool_output) = match t.turn_type.as_str() {
                "function_call" => (
                    t.extra.get("name").and_then(|v| v.as_str()).map(String::from),
                    t.extra.get("arguments").map(|v| {
                        if let Some(s) = v.as_str() { s.to_string() } else { v.to_string() }
                    }),
                    None,
                ),
                "function_call_output" => (
                    t.extra.get("name").and_then(|v| v.as_str()).map(String::from),
                    None,
                    t.extra.get("output").and_then(|v| v.as_str()).map(String::from),
                ),
                _ => (None, None, None),
            };

            let text = match t.turn_type.as_str() {
                "function_call" | "function_call_output" | "agent_handoff" => String::new(),
                _ => t.text(),
            };

            JsonTurn {
                index: i + 1,
                turn_type: t.turn_type.clone(),
                role: t.role.clone(),
                text,
                interrupted: t.interrupted,
                created_at: t.created_at,
                relative_time_sec: if analysis.session_start > 0.0 && t.created_at > 0.0 {
                    t.created_at - analysis.session_start
                } else {
                    0.0
                },
                tool_name,
                tool_arguments,
                tool_output,
                gap_from_previous_sec: gap.filter(|&g| g > 0.5),
                metrics: JsonTurnMetrics {
                    e2e_latency_ms: t.metrics.e2e_latency.map(|v| v * 1000.0),
                    llm_ttft_ms: t.metrics.llm_node_ttft.map(|v| v * 1000.0),
                    tts_ttfb_ms: t.metrics.tts_node_ttfb.map(|v| v * 1000.0),
                    speaking_duration_sec: t.metrics.speaking_duration(),
                    transcript_confidence: t.metrics.transcript_confidence,
                    breakdown: t.breakdown.as_ref().map(|b| JsonTurnBreakdown {
                        stt_ms: b.stt_ms,
                        eol_ms: b.eol_ms,
                        first_llm_ms: b.first_llm_ms,
                        tool_ms: b.tool_ms,
                        tool_names: b.tool_names.clone(),
                        llm_ms: b.llm_ms,
                        tts_ms: b.tts_ms,
                        overhead_ms: b.overhead_ms,
                        has_tool_call: b.has_tool_call,
                    }),
                },
            }
        })
        .collect();

    // Build high latency turns
    let high_latency_threshold = thresholds::E2E_SLOW_MS / 1000.0 * 1.5; // 3s
    let high_latency_turns: Vec<JsonHighLatencyTurn> = analysis
        .assistant_turns()
        .iter()
        .enumerate()
        .filter(|(_, t)| t.metrics.e2e_latency.map(|e| e > high_latency_threshold).unwrap_or(false))
        .map(|(i, t)| {
            JsonHighLatencyTurn {
                index: i + 1,
                text: truncate(&t.text(), TEXT_PREVIEW_MEDIUM),
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

    let sm = &analysis.span_metrics;

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
        span_metrics: JsonSpanMetrics {
            llm_model: sm.llm_model.clone(),
            tts_provider: sm.tts_provider.clone(),
            llm_requests: sm.llm_request_count,
            total_prompt_tokens: sm.total_prompt_tokens,
            total_completion_tokens: sm.total_completion_tokens,
            total_cached_tokens: sm.total_cached_tokens,
            cache_hit_pct: sm.cache_hit_pct,
            avg_tokens_per_sec: sm.avg_tokens_per_sec,
            min_tokens_per_sec: sm.min_tokens_per_sec,
            cancelled_llm_requests: sm.cancelled_llm_count,
            tts_requests: sm.tts_request_count,
            avg_tts_realtime_factor: sm.avg_tts_realtime_factor,
            cancelled_tts_requests: sm.cancelled_tts_count,
            eou_detections: sm.eou_count,
            eou_high_confidence: sm.eou_high_confidence_count,
            eou_low_confidence: sm.eou_low_confidence_count,
            eou_avg_probability: sm.eou_avg_probability,
            eou_endpointing_delay_sec: sm.eou_endpointing_delay,
        },
        turns,
        high_latency_turns,
        errors,
    }
}

fn summarize_tool_args_text(args_str: &str) -> String {
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(args_str);
    let obj = match parsed {
        Ok(serde_json::Value::Object(map)) => map,
        _ => return String::new(),
    };

    let mut parts: Vec<String> = Vec::new();
    let mut total_len = 0usize;
    for (k, v) in &obj {
        let val_str = match v {
            serde_json::Value::String(s) => {
                if s.len() > 40 {
                    format!("\"{}...\"", &s[..37])
                } else {
                    format!("\"{}\"", s)
                }
            }
            serde_json::Value::Null => continue,
            other => other.to_string(),
        };
        let part = format!("{}={}", k, val_str);
        total_len += part.len() + 2;
        if total_len > 80 {
            parts.push("...".to_string());
            break;
        }
        parts.push(part);
    }
    parts.join(", ")
}
