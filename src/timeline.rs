//! Timeline report generator — BREAKDOWN.md-style chronological reconstruction.
//!
//! Produces a single Markdown file that interleaves logs, conversation turns,
//! tool calls, and handoffs in chronological order with inline latency
//! breakdowns. Designed for AI agent consumption.

use std::collections::HashSet;

use crate::data::{CallAnalysis, ConversationTurn, DiagnosisVerdict, LogEntry, PipelineSummary};
use crate::format::{format_duration, word_wrap};

// =============================================================================
// EFFECTIVE SESSION START — fallback when agent_session span is missing
// =============================================================================

/// Compute the effective session start time.
/// Falls back to earliest log or turn timestamp when `session_start == 0.0`.
fn effective_start(analysis: &CallAnalysis) -> f64 {
    if analysis.session_start > 0.0 {
        return analysis.session_start;
    }

    let earliest_log = analysis
        .logs
        .first()
        .map(|l| l.timestamp_sec())
        .unwrap_or(f64::MAX);

    let earliest_turn = analysis
        .turns
        .first()
        .map(|t| t.created_at)
        .filter(|&t| t > 0.0)
        .unwrap_or(f64::MAX);

    let earliest = earliest_log.min(earliest_turn);
    if earliest == f64::MAX {
        0.0
    } else {
        earliest
    }
}

/// Compute effective duration.
fn effective_duration(analysis: &CallAnalysis, start: f64) -> f64 {
    if analysis.session_end > 0.0 && analysis.session_start > 0.0 {
        return analysis.duration_sec();
    }

    let latest_log = analysis
        .logs
        .last()
        .map(|l| l.timestamp_sec())
        .unwrap_or(start);

    let latest_turn = analysis
        .turns
        .last()
        .map(|t| {
            // Use the end of speaking if available
            t.metrics.stopped_speaking_at.unwrap_or(t.created_at)
        })
        .filter(|&t| t > 0.0)
        .unwrap_or(start);

    latest_log.max(latest_turn) - start
}

/// Compute effective room ID — fallback to room_name or session folder.
fn effective_room_id(analysis: &CallAnalysis) -> String {
    if !analysis.room_id.is_empty() {
        return analysis.room_id.clone();
    }
    if !analysis.room_name.is_empty() {
        return analysis.room_name.clone();
    }
    // Try to extract from folder path
    let folder_name = analysis
        .folder_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    folder_name.to_string()
}

// =============================================================================
// TIMELINE EVENT — unified type for chronological sorting
// =============================================================================

#[derive(Debug)]
enum TimelineEvent<'a> {
    Log(&'a LogEntry),
    Turn(usize, &'a ConversationTurn), // (1-based turn number, turn)
}

impl<'a> TimelineEvent<'a> {
    fn abs_timestamp(&self) -> f64 {
        match self {
            TimelineEvent::Log(log) => log.timestamp_sec(),
            TimelineEvent::Turn(_, turn) => turn.created_at,
        }
    }
}

// =============================================================================
// PUBLIC API
// =============================================================================

/// Generate a full timeline report in Markdown format (BREAKDOWN.md style).
pub fn generate_timeline_report(analysis: &CallAnalysis) -> String {
    let mut out = String::with_capacity(32_000);
    let start = effective_start(analysis);

    // === Header ===
    emit_header(&mut out, analysis, start);

    // === Pipeline Summary ===
    emit_pipeline(&mut out, analysis);

    // === Chronological Timeline ===
    emit_timeline(&mut out, analysis, start);

    // === Errors table ===
    emit_errors(&mut out, analysis, start);

    // === Warnings (grouped) ===
    emit_warnings(&mut out, analysis, start);

    // === Latency Stats ===
    emit_latency_stats(&mut out, analysis);

    // === LLM Context Growth ===
    emit_context_growth(&mut out, analysis);

    // === Key Spans ===
    emit_key_spans(&mut out, analysis, start);

    out
}

// =============================================================================
// HEADER
// =============================================================================

fn emit_header(out: &mut String, analysis: &CallAnalysis, start: f64) {
    let diagnosis = analysis.diagnosis.as_ref();
    let slow_count: usize = diagnosis
        .map(|d| d.slow_turns_by_cause.values().map(|v| v.len()).sum())
        .unwrap_or(0);

    let room_id = effective_room_id(analysis);
    out.push_str(&format!("# Call Breakdown: {}\n\n", room_id));

    // Verdict line
    let verdict_str = match diagnosis.map(|d| &d.verdict) {
        Some(DiagnosisVerdict::Healthy) => "HEALTHY".to_string(),
        Some(DiagnosisVerdict::Problematic) => format!(
            "PROBLEMATIC — {} slow turns, {} errors, {} warnings",
            slow_count,
            analysis.errors.len(),
            analysis.warnings.len()
        ),
        Some(DiagnosisVerdict::NeedsAttention) => format!(
            "NEEDS_ATTENTION — {} slow turns, {} warnings",
            slow_count,
            analysis.warnings.len()
        ),
        None => "UNKNOWN".to_string(),
    };
    out.push_str(&format!("**Verdict: {}**\n\n", verdict_str));

    // Metadata table
    let user_turns = analysis.user_turns().len();
    let assistant_turns = analysis.assistant_turns().len();
    let other_turns = analysis.turns.len() - user_turns - assistant_turns;
    let interrupted = analysis.interrupted_turns().len();
    let duration = effective_duration(analysis, start);

    out.push_str("| Field | Value |\n");
    out.push_str("|---|---|\n");
    out.push_str(&format!("| Room ID | {} |\n", room_id));
    out.push_str(&format!("| Duration | {} |\n", format_duration(duration)));
    if !analysis.agent_name.is_empty() {
        out.push_str(&format!("| Agent | {} |\n", analysis.agent_name));
    }
    if !analysis.participant_identity.is_empty() {
        out.push_str(&format!(
            "| Participant | {} |\n",
            analysis.participant_identity
        ));
    }
    if !analysis.span_metrics.llm_model.is_empty() {
        out.push_str(&format!(
            "| Model | {} |\n",
            analysis.span_metrics.llm_model
        ));
    }
    if !analysis.span_metrics.tts_provider.is_empty() {
        out.push_str(&format!(
            "| TTS Provider | {} |\n",
            analysis.span_metrics.tts_provider
        ));
    }
    out.push_str(&format!(
        "| Total turns | {} ({} user, {} assistant, {} tool/handoff) |\n",
        analysis.turns.len(),
        user_turns,
        assistant_turns,
        other_turns
    ));
    if interrupted > 0 {
        out.push_str(&format!("| Interrupted | {} |\n", interrupted));
    }
    if start > 0.0 {
        let ts = chrono_format_timestamp(start);
        out.push_str(&format!("| Start | {} |\n", ts));
    }
    out.push('\n');
}

// =============================================================================
// PIPELINE SUMMARY
// =============================================================================

fn emit_pipeline(out: &mut String, analysis: &CallAnalysis) {
    let summary = match PipelineSummary::from_cycles(&analysis.pipeline_cycles) {
        Some(s) => s,
        None => return,
    };

    out.push_str("## Pipeline\n\n");

    out.push_str(&format!(
        "Response time: **{:.1}s avg** (max {:.1}s) — {}\n\n",
        summary.avg_total_ms / 1000.0,
        summary.max_total_ms / 1000.0,
        summary.total_verdict
    ));

    out.push_str("| Stage | Avg | % of total | Verdict |\n");
    out.push_str("|---|---|---|---|\n");
    out.push_str(&format!(
        "| LLM (TTFT) | {:.0}ms | {:.0}% | {} |\n",
        summary.avg_llm_ms, summary.llm_pct, summary.llm_verdict
    ));
    out.push_str(&format!(
        "| TTS (TTFB) | {:.0}ms | {:.0}% | {} |\n",
        summary.avg_tts_ms, summary.tts_pct, summary.tts_verdict
    ));

    // Perception (STT + EOL from user turns)
    let user_turns_with_delays: Vec<_> = analysis
        .turns
        .iter()
        .filter(|t| {
            t.role.as_deref() == Some("user") && t.metrics.transcription_delay.is_some()
        })
        .collect();

    if !user_turns_with_delays.is_empty() {
        let count = user_turns_with_delays.len() as f64;
        let avg_stt: f64 = user_turns_with_delays
            .iter()
            .filter_map(|t| t.metrics.transcription_delay)
            .map(|v| v * 1000.0)
            .sum::<f64>()
            / count;
        let avg_eol: f64 = user_turns_with_delays
            .iter()
            .filter_map(|t| t.metrics.end_of_turn_delay)
            .map(|v| v * 1000.0)
            .sum::<f64>()
            / count;
        out.push_str(&format!(
            "| Perception | {:.0}ms STT + {:.0}ms EOL | — | {} |\n",
            avg_stt, avg_eol, summary.perception_verdict
        ));
    }

    out.push_str(&format!("\nBottleneck: **{}**\n", summary.bottleneck));

    // Detected delays
    if !summary.detected_delays.is_empty() {
        out.push_str("\nDetected delays:\n");
        for delay in summary.detected_delays.iter().take(10) {
            out.push_str(&format!(
                "- Turn {}: +{:.1}s gap — {}\n",
                delay.turn_number,
                delay.gap_ms / 1000.0,
                delay.reason
            ));
        }
        if summary.detected_delays.len() > 10 {
            out.push_str(&format!(
                "- ... and {} more\n",
                summary.detected_delays.len() - 10
            ));
        }
    }

    out.push('\n');
}

// =============================================================================
// CHRONOLOGICAL TIMELINE
// =============================================================================

fn emit_timeline(out: &mut String, analysis: &CallAnalysis, start: f64) {
    out.push_str("## Timeline\n\n");

    // Build a set of turn indices that are function_call_output — these will be
    // rendered inline after their corresponding function_call, not as separate
    // timeline events.  This prevents logs from splitting a tool call and its output.
    let output_indices: HashSet<usize> = analysis
        .turns
        .iter()
        .enumerate()
        .filter(|(_, t)| t.turn_type == "function_call_output")
        .map(|(i, _)| i)
        .collect();

    // For each function_call, find the matching function_call_output index.
    // We match by call_id if available, otherwise the next output turn.
    let mut fn_call_to_output: std::collections::HashMap<usize, usize> =
        std::collections::HashMap::new();
    for (i, turn) in analysis.turns.iter().enumerate() {
        if turn.turn_type == "function_call" {
            let call_id = turn
                .extra
                .get("call_id")
                .and_then(|v| v.as_str());
            // Look ahead for matching output
            for j in (i + 1)..analysis.turns.len() {
                if analysis.turns[j].turn_type == "function_call_output" {
                    let out_call_id = analysis.turns[j]
                        .extra
                        .get("call_id")
                        .and_then(|v| v.as_str());
                    if call_id == out_call_id || call_id.is_none() {
                        fn_call_to_output.insert(i, j);
                        break;
                    }
                }
            }
        }
    }

    // Build unified event list (excluding function_call_output — rendered inline)
    let mut events: Vec<TimelineEvent> = Vec::new();

    for log in &analysis.logs {
        events.push(TimelineEvent::Log(log));
    }

    for (i, turn) in analysis.turns.iter().enumerate() {
        if output_indices.contains(&i) {
            continue; // Skip — will be rendered inline with function_call
        }
        events.push(TimelineEvent::Turn(i + 1, turn));
    }

    // Sort by absolute timestamp
    events.sort_by(|a, b| {
        a.abs_timestamp()
            .partial_cmp(&b.abs_timestamp())
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Render events
    let mut last_was_turn = false;

    for event in &events {
        match event {
            TimelineEvent::Log(log) => {
                let rel = log.timestamp_sec() - start;
                let sev = match log.severity.as_str() {
                    "ERROR" | "CRITICAL" => "ERROR",
                    "WARN" | "WARNING" => "WARN",
                    "INFO" => "INFO",
                    "DEBUG" => "DEBUG",
                    other => other,
                };

                // Skip DEBUG logs in timeline (too noisy)
                if sev == "DEBUG" {
                    continue;
                }

                let msg = log.message.replace('\n', " | ");

                out.push_str(&format!(
                    "### {:.2}s — LOG [{}] {}\n",
                    rel, sev, log.logger_name
                ));
                out.push_str(&format!("> {}\n\n", msg));
                last_was_turn = false;
            }

            TimelineEvent::Turn(num, turn) => {
                let turn_idx = num - 1; // back to 0-based

                // Separator between adjacent turns
                if last_was_turn {
                    out.push_str("---\n\n");
                }

                // Render the turn itself
                emit_turn_in_timeline(out, *num, turn);

                // If this is a function_call, render its output inline
                if turn.turn_type == "function_call" {
                    if let Some(&out_idx) = fn_call_to_output.get(&turn_idx) {
                        emit_tool_output_inline(out, &analysis.turns[out_idx]);
                    }
                }

                last_was_turn = true;
            }
        }
    }
}

/// Render a single conversation turn in the timeline.
fn emit_turn_in_timeline(out: &mut String, num: usize, turn: &ConversationTurn) {
    // --- Agent Handoff ---
    if turn.turn_type == "agent_handoff" {
        let new_agent = turn
            .extra
            .get("new_agent_id")
            .and_then(|v| v.as_str())
            .unwrap_or(&turn.id);
        out.push_str(&format!("### Turn {} — HANDOFF to {}\n\n", num, new_agent));
        return;
    }

    // --- Function Call ---
    if turn.turn_type == "function_call" {
        let fn_name = turn
            .extra
            .get("name")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        let args_summary = turn
            .extra
            .get("arguments")
            .map(|v| {
                let s = if let Some(s) = v.as_str() {
                    s.to_string()
                } else {
                    v.to_string()
                };
                summarize_tool_args(&s)
            })
            .unwrap_or_default();

        if args_summary.is_empty() {
            out.push_str(&format!("### Turn {} — TOOL: {}()\n", num, fn_name));
        } else {
            out.push_str(&format!(
                "### Turn {} — TOOL: {}({})\n",
                num, fn_name, args_summary
            ));
        }
        return;
    }

    // --- Function Call Output (standalone, shouldn't normally happen) ---
    if turn.turn_type == "function_call_output" {
        emit_tool_output_inline(out, turn);
        return;
    }

    // --- User / Assistant message ---
    let role = turn.role.as_deref().unwrap_or("?").to_uppercase();

    let interrupted_tag = if turn.interrupted {
        " [INTERRUPTED]"
    } else {
        ""
    };

    out.push_str(&format!(
        "### Turn {} — {}{}\n",
        num, role, interrupted_tag
    ));

    // Metrics line
    let mut metrics = Vec::new();
    if let Some(e2e) = turn.metrics.e2e_latency {
        let e2e_ms = e2e * 1000.0;
        if e2e_ms > 2000.0 {
            metrics.push(format!("**E2E: {:.0}ms**", e2e_ms));
        } else {
            metrics.push(format!("E2E: {:.0}ms", e2e_ms));
        }
    }
    if let Some(llm) = turn.metrics.llm_node_ttft {
        metrics.push(format!("LLM: {:.0}ms", llm * 1000.0));
    }
    if let Some(tts) = turn.metrics.tts_node_ttfb {
        metrics.push(format!("TTS: {:.0}ms", tts * 1000.0));
    }
    if let Some(dur) = turn.metrics.speaking_duration() {
        metrics.push(format!("dur: {:.1}s", dur));
    }
    if !metrics.is_empty() {
        out.push_str(&format!("- {}\n", metrics.join(" | ")));
    }

    // E2E Breakdown line
    if let Some(ref bd) = turn.breakdown {
        let mut parts = Vec::new();
        if let Some(stt) = bd.stt_ms {
            parts.push(format!("stt={:.0}ms", stt));
        }
        if let Some(eol) = bd.eol_ms {
            parts.push(format!("eol={:.0}ms", eol));
        }
        if let Some(first) = bd.first_llm_ms {
            parts.push(format!("llm1={:.0}ms", first));
        }
        if let Some(tool) = bd.tool_ms {
            let names = if bd.tool_names.is_empty() {
                String::new()
            } else {
                format!("[{}]", bd.tool_names.join(","))
            };
            parts.push(format!("tool={:.0}ms{}", tool, names));
        }
        if let Some(llm) = bd.llm_ms {
            parts.push(format!("llm2={:.0}ms", llm));
        }
        if let Some(tts) = bd.tts_ms {
            parts.push(format!("tts={:.0}ms", tts));
        }
        if let Some(oh) = bd.overhead_ms {
            parts.push(format!("other={:.0}ms", oh));
        }
        if !parts.is_empty() {
            out.push_str(&format!("- Breakdown: {}\n", parts.join(" -> ")));
        }

        // Slow annotation
        if let Some(e2e) = turn.metrics.e2e_latency {
            let e2e_ms = e2e * 1000.0;
            if e2e_ms > 2000.0 {
                let reason = determine_slow_reason(bd);
                out.push_str(&format!("- **[SLOW: {}]**\n", reason));
            }
        }
    }

    // Content
    let text = turn.text();
    if !text.is_empty() {
        out.push('\n');
        for line in word_wrap(&text, 78, "> ") {
            out.push_str(&line);
            out.push('\n');
        }
    }

    out.push('\n');
}

/// Render a function_call_output inline (directly after its function_call).
fn emit_tool_output_inline(out: &mut String, turn: &ConversationTurn) {
    let output_raw = turn
        .extra
        .get("output")
        .and_then(|v| v.as_str())
        .unwrap_or("ok");
    let output_display =
        if output_raw.is_empty() || output_raw == "ok" || output_raw == "\"ok\"" {
            "ok".to_string()
        } else if output_raw.len() > 120 {
            format!("{}...", &output_raw[..117])
        } else {
            output_raw.to_string()
        };

    out.push_str(&format!("- Output: {}\n\n", output_display));
}

/// Determine the human-readable reason for a slow turn from its breakdown.
fn determine_slow_reason(bd: &crate::data::TurnBreakdown) -> String {
    let first_llm = bd.first_llm_ms.unwrap_or(0.0);
    let tool = bd.tool_ms.unwrap_or(0.0);
    let llm = bd.llm_ms.unwrap_or(0.0);
    let tts = bd.tts_ms.unwrap_or(0.0);
    let overhead = bd.overhead_ms.unwrap_or(0.0);

    let max_component = [
        ("overhead", overhead),
        ("llm1", first_llm),
        ("tool", tool),
        ("llm", llm),
        ("tts", tts),
    ]
    .iter()
    .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    .map(|(name, _)| *name)
    .unwrap_or("unknown");

    match max_component {
        "llm1" => format!(
            "overhead — llm1={:.0}ms deciding to call tool",
            first_llm
        ),
        "tool" => {
            let names = if bd.tool_names.is_empty() {
                "tool call".to_string()
            } else {
                bd.tool_names.join(", ")
            };
            format!("tool call dominated — {}={:.0}ms", names, tool)
        }
        "llm" => format!("LLM took {:.0}ms", llm),
        "tts" => format!("TTS took {:.0}ms", tts),
        "overhead" => format!("overhead — {:.0}ms unexplained gap", overhead),
        _ => "unknown".to_string(),
    }
}

// =============================================================================
// ERRORS
// =============================================================================

fn emit_errors(out: &mut String, analysis: &CallAnalysis, start: f64) {
    out.push_str(&format!("## Errors ({})\n\n", analysis.errors.len()));

    if analysis.errors.is_empty() {
        out.push_str("(no errors)\n\n");
        return;
    }

    out.push_str("| Time | Source | Message |\n");
    out.push_str("|---:|---|---|\n");
    for log in &analysis.errors {
        let rel = log.timestamp_sec() - start;
        let msg = log.message.replace('\n', " | ");
        let msg_short = if msg.len() > 120 {
            format!("{}...", &msg[..117])
        } else {
            msg
        };
        out.push_str(&format!(
            "| {:.2}s | {} | {} |\n",
            rel, log.logger_name, msg_short
        ));
    }
    out.push('\n');
}

// =============================================================================
// WARNINGS (GROUPED)
// =============================================================================

fn emit_warnings(out: &mut String, analysis: &CallAnalysis, start: f64) {
    if analysis.warnings.is_empty() {
        return;
    }

    out.push_str(&format!(
        "## Warnings ({} total, grouped by pattern)\n\n",
        analysis.warnings.len()
    ));

    let groups = group_warnings(&analysis.warnings, start);

    out.push_str("| Count | Pattern | Source | First | Last |\n");
    out.push_str("|---:|---|---|---|---|\n");
    for g in &groups {
        out.push_str(&format!(
            "| {} | {} | {} | {:.1}s | {:.1}s |\n",
            g.count, g.label, g.logger, g.first_time, g.last_time
        ));
    }
    out.push('\n');
}

struct WarningGroup {
    label: String,
    logger: String,
    count: usize,
    first_time: f64,
    last_time: f64,
}

fn group_warnings(warnings: &[LogEntry], session_start: f64) -> Vec<WarningGroup> {
    use std::collections::HashMap;

    let mut groups: Vec<WarningGroup> = Vec::new();
    let mut key_to_idx: HashMap<String, usize> = HashMap::new();

    for log in warnings {
        let rel = log.timestamp_sec() - session_start;
        let key = warning_group_key(&log.logger_name, &log.message);
        let full_msg = log.message.replace('\n', " | ");

        if let Some(&idx) = key_to_idx.get(&key) {
            let g = &mut groups[idx];
            g.count += 1;
            if rel < g.first_time {
                g.first_time = rel;
            }
            if rel > g.last_time {
                g.last_time = rel;
            }
        } else {
            let label = if full_msg.len() > 80 {
                format!("{}...", &full_msg[..77])
            } else {
                full_msg
            };
            let idx = groups.len();
            key_to_idx.insert(key, idx);
            groups.push(WarningGroup {
                label,
                logger: log.logger_name.clone(),
                count: 1,
                first_time: rel,
                last_time: rel,
            });
        }
    }

    groups.sort_by(|a, b| b.count.cmp(&a.count));
    groups
}

fn warning_group_key(logger: &str, message: &str) -> String {
    use regex::Regex;
    use std::sync::LazyLock;

    static RE_HEX: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"[0-9a-fA-F]{6,}").unwrap());
    static RE_NUM: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"\b\d+(\.\d+)?\b").unwrap());

    let short: String = message.chars().take(80).collect();
    let key = RE_HEX.replace_all(&short, "<ID>").to_string();
    let key = RE_NUM.replace_all(&key, "<N>").to_string();
    format!("{}::{}", logger, key)
}

// =============================================================================
// LATENCY STATS
// =============================================================================

fn emit_latency_stats(out: &mut String, analysis: &CallAnalysis) {
    let (e2e, llm, tts) = analysis.compute_latency_stats();

    // Skip if no latency data at all
    if e2e.is_none() && llm.is_none() && tts.is_none() {
        return;
    }

    out.push_str("## Latency Stats\n\n");
    out.push_str("| Metric | Avg | Min | Max | P95 |\n");
    out.push_str("|---|---|---|---|---|\n");

    if let Some(ref s) = e2e {
        out.push_str(&format!(
            "| E2E | {:.0}ms | {:.0}ms | {:.0}ms | {:.0}ms |\n",
            s.avg_ms, s.min_ms, s.max_ms, s.p95_ms
        ));
    }
    if let Some(ref s) = llm {
        out.push_str(&format!(
            "| LLM (TTFT) | {:.0}ms | {:.0}ms | {:.0}ms | {:.0}ms |\n",
            s.avg_ms, s.min_ms, s.max_ms, s.p95_ms
        ));
    }
    if let Some(ref s) = tts {
        out.push_str(&format!(
            "| TTS (TTFB) | {:.0}ms | {:.0}ms | {:.0}ms | {:.0}ms |\n",
            s.avg_ms, s.min_ms, s.max_ms, s.p95_ms
        ));
    }

    // STT/EOL averages from user turns
    let user_delays: Vec<_> = analysis
        .turns
        .iter()
        .filter(|t| {
            t.role.as_deref() == Some("user") && t.metrics.transcription_delay.is_some()
        })
        .collect();

    if !user_delays.is_empty() {
        let count = user_delays.len() as f64;
        let avg_stt: f64 = user_delays
            .iter()
            .filter_map(|t| t.metrics.transcription_delay)
            .map(|v| v * 1000.0)
            .sum::<f64>()
            / count;
        let avg_eol: f64 = user_delays
            .iter()
            .filter_map(|t| t.metrics.end_of_turn_delay)
            .map(|v| v * 1000.0)
            .sum::<f64>()
            / count;
        out.push_str(&format!("| STT | {:.0}ms | — | — | — |\n", avg_stt));
        out.push_str(&format!("| EOL | {:.0}ms | — | — | — |\n", avg_eol));
    }

    out.push('\n');
}

// =============================================================================
// LLM CONTEXT GROWTH
// =============================================================================

fn emit_context_growth(out: &mut String, analysis: &CallAnalysis) {
    if analysis.llm_turns.is_empty() {
        return;
    }

    out.push_str("### LLM Context Growth\n\n");
    out.push_str("| Turn | LLM ms | Msgs | ~Tokens | Response preview |\n");
    out.push_str("|---|---|---|---|---|\n");

    // Sample: show ~15 representative turns (evenly spaced)
    let total = analysis.llm_turns.len();
    let step = if total > 15 { total / 15 } else { 1 };

    for (i, lt) in analysis.llm_turns.iter().enumerate() {
        if i % step != 0 && i != total - 1 {
            continue;
        }
        let preview: String = lt.response_text.chars().take(45).collect();
        let preview = preview.replace('\n', " ").replace('|', "/");
        out.push_str(&format!(
            "| {} | {:.0} | {} | {} | {}... |\n",
            lt.turn_index + 1,
            lt.duration_ms,
            lt.context_messages,
            lt.context_tokens_est,
            preview
        ));
    }

    out.push('\n');
}

// =============================================================================
// KEY SPANS
// =============================================================================

fn emit_key_spans(out: &mut String, analysis: &CallAnalysis, start: f64) {
    let key_spans: Vec<_> = analysis
        .spans
        .iter()
        .filter(|s| crate::thresholds::is_key_span(&s.name))
        .collect();

    if key_spans.is_empty() {
        return;
    }

    out.push_str("## Key Spans\n\n");
    out.push_str("| Start | Dur(ms) | Name |\n");
    out.push_str("|---:|---:|---|\n");

    for span in key_spans.iter().take(80) {
        let rel = span.start_sec() - start;
        out.push_str(&format!(
            "| {:.2}s | {:.0}ms | {} |\n",
            rel,
            span.duration_ms(),
            span.name
        ));
    }
    if key_spans.len() > 80 {
        out.push_str(&format!(
            "\n({} more spans not shown)\n",
            key_spans.len() - 80
        ));
    }

    out.push('\n');
}

// =============================================================================
// HELPERS
// =============================================================================

/// Summarize tool arguments into a compact inline representation.
fn summarize_tool_args(args_str: &str) -> String {
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(args_str) {
        if let Some(obj) = val.as_object() {
            let parts: Vec<String> = obj
                .iter()
                .map(|(k, v)| {
                    let val_str = match v {
                        serde_json::Value::String(s) => {
                            if s.len() > 50 {
                                format!("\"{}...\"", &s[..47])
                            } else {
                                format!("\"{}\"", s)
                            }
                        }
                        other => {
                            let s = other.to_string();
                            if s.len() > 50 {
                                format!("{}...", &s[..47])
                            } else {
                                s
                            }
                        }
                    };
                    format!("{}={}", k, val_str)
                })
                .collect();
            return parts.join(", ");
        }
    }

    if args_str.len() > 80 {
        format!("{}...", &args_str[..77])
    } else {
        args_str.to_string()
    }
}

/// Format a Unix timestamp as a human-readable UTC datetime string.
fn chrono_format_timestamp(timestamp: f64) -> String {
    let secs = timestamp as i64;
    let total_secs = secs;
    let days_since_epoch = total_secs / 86400;
    let time_of_day = total_secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let mut year = 1970i64;
    let mut remaining_days = days_since_epoch;

    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }

    let days_in_months: [i64; 12] = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 0;
    for (i, &dm) in days_in_months.iter().enumerate() {
        if remaining_days < dm {
            month = i + 1;
            break;
        }
        remaining_days -= dm;
    }
    if month == 0 {
        month = 12;
    }
    let day = remaining_days + 1;

    format!(
        "{:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
        year, month, day, hours, minutes, seconds
    )
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}
