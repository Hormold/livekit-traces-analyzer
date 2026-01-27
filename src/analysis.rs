//! Call diagnosis and analysis logic.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use regex::Regex;

use crate::data::{CallAnalysis, CallDiagnosis, DiagnosisVerdict, PipelineCycle, SlowTurnInfo, Span, ToolCall};
use crate::parser::{
    extract_system_prompt, load_json_file, parse_chat_history, parse_llm_turns_from_traces,
    parse_logs, parse_traces,
};

/// Load and analyze all data from an observability folder.
pub fn analyze_call(folder: &Path) -> Result<CallAnalysis> {
    let mut analysis = CallAnalysis::new(folder.to_path_buf());

    // Find JSON files
    let files: Vec<_> = fs::read_dir(folder)
        .with_context(|| format!("Failed to read directory: {}", folder.display()))?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().map_or(false, |ext| ext == "json"))
        .collect();

    let chat_file = files.iter().find(|f| f.file_name().to_string_lossy().contains("chat_history"));
    let logs_file = files.iter().find(|f| f.file_name().to_string_lossy().contains("logs"));
    let traces_file = files.iter().find(|f| f.file_name().to_string_lossy().contains("traces"));

    // Parse chat history
    if let Some(file) = chat_file {
        let data = load_json_file(&file.path())?;
        analysis.turns = parse_chat_history(&data);
    }

    // Parse logs
    if let Some(file) = logs_file {
        let data = load_json_file(&file.path())?;
        analysis.logs = parse_logs(&data);
        analysis.errors = analysis
            .logs
            .iter()
            .filter(|l| l.severity == "ERROR" || l.severity == "CRITICAL")
            .cloned()
            .collect();
        analysis.warnings = analysis
            .logs
            .iter()
            .filter(|l| l.severity == "WARN" || l.severity == "WARNING")
            .cloned()
            .collect();
    }

    // Parse traces
    if let Some(file) = traces_file {
        let data = load_json_file(&file.path())?;
        analysis.spans = parse_traces(&data);
        analysis.llm_turns = parse_llm_turns_from_traces(&data);
        analysis.system_prompt = extract_system_prompt(&data);
    }

    // Extract metadata from spans
    if let Some(session_span) = analysis.spans.iter().find(|s| s.name == "agent_session") {
        analysis.room_id = get_attr_string(&session_span.attributes, "room_id");
        analysis.job_id = get_attr_string(&session_span.attributes, "job_id");
        analysis.agent_name = get_attr_string(&session_span.attributes, "lk.agent_name");
        analysis.room_name = get_attr_string(&session_span.attributes, "lk.room_name");
        analysis.session_start = session_span.start_sec();
        analysis.session_end = session_span.end_sec();
    }

    // Extract participant from user turns
    if let Some(user_turn_span) = analysis.spans.iter().find(|s| s.name == "user_turn") {
        analysis.participant_identity =
            get_attr_string(&user_turn_span.attributes, "lk.participant_identity");
    }

    // Extract tool calls from spans
    for span in &analysis.spans {
        if span.name == "function_call" || span.name == "tool_call" {
            let name = get_attr_string(&span.attributes, "lk.function_name");
            analysis.tool_calls.push(ToolCall {
                name: if name.is_empty() { span.name.clone() } else { name },
                start: span.start_sec(),
                duration_ms: span.duration_ms(),
            });
        }
    }

    // Extract tool calls from logs - be more specific to avoid capturing agent runs
    let tool_pattern = Regex::new(r"tool=(\w+)").ok();
    let function_pattern = Regex::new(r"function[_\s]*call[:\s]+(\w+)").ok();

    for log in &analysis.logs {
        // Skip agent execution logs
        if log.message.contains("Executing agent run for") {
            continue;
        }

        // Look for actual tool/function traces
        if log.message.contains("TOOL-TRACE") {
            if let Some(ref re) = tool_pattern {
                if let Some(caps) = re.captures(&log.message) {
                    if let Some(m) = caps.get(1) {
                        let tool_name = m.as_str().to_string();
                        // Skip if it looks like an agent name
                        if !tool_name.contains("AI_CSR") && !tool_name.contains("ENTRY") && !tool_name.contains("MAIN") {
                            analysis.tool_calls.push(ToolCall {
                                name: tool_name,
                                start: log.timestamp_sec(),
                                duration_ms: 0.0,
                            });
                        }
                    }
                }
            }
        }
        // Look for function call execution logs
        else if log.message.contains("function_call") || log.message.contains("tool_call") {
            if let Some(ref re) = function_pattern {
                if let Some(caps) = re.captures(&log.message) {
                    if let Some(m) = caps.get(1) {
                        analysis.tool_calls.push(ToolCall {
                            name: m.as_str().to_string(),
                            start: log.timestamp_sec(),
                            duration_ms: 0.0,
                        });
                    }
                }
            }
        }
    }

    // Deduplicate tool calls that might appear in both spans and logs
    analysis.tool_calls.sort_by(|a, b| a.start.partial_cmp(&b.start).unwrap_or(std::cmp::Ordering::Equal));
    analysis.tool_calls.dedup_by(|a, b| (a.start - b.start).abs() < 0.5 && a.name == b.name);

    // Compute pipeline cycles (pass tool_calls for gap analysis)
    analysis.pipeline_cycles = compute_pipeline_cycles(&analysis.spans, &analysis.tool_calls);

    // Compute diagnosis
    analysis.diagnosis = Some(compute_diagnosis(&analysis));

    Ok(analysis)
}

fn get_attr_string(attrs: &HashMap<String, serde_json::Value>, key: &str) -> String {
    attrs
        .get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string()
}

/// Find child spans by parent ID.
fn find_children<'a>(spans: &'a [Span], parent_id: &str) -> Vec<&'a Span> {
    spans
        .iter()
        .filter(|s| s.parent_span_id.as_deref() == Some(parent_id))
        .collect()
}

/// Find a span by name among a list of spans.
fn find_span_by_name<'a>(spans: &[&'a Span], name: &str) -> Option<&'a Span> {
    spans.iter().find(|s| s.name == name).copied()
}

/// Compute pipeline cycles from spans.
/// Each cycle represents one user→agent turn with timing breakdown.
pub fn compute_pipeline_cycles(spans: &[Span], tool_calls: &[ToolCall]) -> Vec<PipelineCycle> {
    let mut cycles = Vec::new();

    // Find all agent_turn spans (these contain the LLM/TTS processing)
    let agent_turns: Vec<&Span> = spans
        .iter()
        .filter(|s| s.name == "agent_turn")
        .collect();

    // Find all user_turn spans
    let user_turns: Vec<&Span> = spans
        .iter()
        .filter(|s| s.name == "user_turn")
        .collect();

    for (idx, agent_turn) in agent_turns.iter().enumerate() {
        // Find the preceding user_turn (by end time before agent_turn start)
        let agent_start = agent_turn.start_sec();
        let preceding_user_turn = user_turns
            .iter()
            .filter(|ut| {
                let ut_end = ut.end_sec();
                // User turn must end before or shortly after agent turn starts
                // and must be within 10 seconds (not an old unrelated turn)
                ut_end <= agent_start + 0.5 && (agent_start - ut_end) < 10.0
            })
            .max_by(|a, b| a.end_sec().partial_cmp(&b.end_sec()).unwrap_or(std::cmp::Ordering::Equal));

        let has_user_turn = preceding_user_turn.is_some();
        let user_end = preceding_user_turn
            .map(|ut| ut.end_sec())
            .unwrap_or(agent_start);

        // Find child spans of agent_turn
        let children = find_children(spans, &agent_turn.span_id);

        // Find LLM span (could be direct child or nested)
        let llm_span = find_span_by_name(&children, "llm_node")
            .or_else(|| {
                // Search recursively in children
                for child in &children {
                    let grandchildren = find_children(spans, &child.span_id);
                    if let Some(llm) = find_span_by_name(&grandchildren, "llm_node") {
                        return Some(llm);
                    }
                }
                None
            });

        // Find TTS span (could be tts_node or tts_request)
        let tts_span = find_span_by_name(&children, "tts_node")
            .or_else(|| find_span_by_name(&children, "tts_request"))
            .or_else(|| {
                // Search recursively
                for child in &children {
                    let grandchildren = find_children(spans, &child.span_id);
                    if let Some(tts) = find_span_by_name(&grandchildren, "tts_node")
                        .or_else(|| find_span_by_name(&grandchildren, "tts_request"))
                    {
                        return Some(tts);
                    }
                }
                None
            });

        // Find agent_speaking span
        let agent_speaking = find_span_by_name(&children, "agent_speaking")
            .or_else(|| {
                for child in &children {
                    let grandchildren = find_children(spans, &child.span_id);
                    if let Some(speaking) = find_span_by_name(&grandchildren, "agent_speaking") {
                        return Some(speaking);
                    }
                }
                None
            });

        // Extract timings
        let llm_start = llm_span.map(|s| s.start_sec()).unwrap_or(agent_start);
        let llm_end = llm_span.map(|s| s.end_sec()).unwrap_or(agent_start);
        let llm_duration_ms = llm_span.map(|s| s.duration_ms()).unwrap_or(0.0);

        let tts_start = tts_span.map(|s| s.start_sec()).unwrap_or(llm_end);
        let tts_end = tts_span.map(|s| s.end_sec()).unwrap_or(llm_end);
        let tts_duration_ms = tts_span.map(|s| s.duration_ms()).unwrap_or(0.0);

        let agent_speaking_start = agent_speaking.map(|s| s.start_sec()).unwrap_or(tts_end);

        let total_duration_ms = (agent_turn.end_sec() - user_end) * 1000.0;

        // Compute gaps - only valid if we have a user turn
        let user_to_llm_ms = if has_user_turn {
            (llm_start - user_end) * 1000.0
        } else {
            0.0 // No user turn, so no perception delay to measure
        };

        // LLM/TTS overlap (positive if TTS started before LLM finished - streaming benefit)
        let llm_tts_overlap_ms = if tts_start < llm_end {
            (llm_end - tts_start) * 1000.0
        } else {
            -((tts_start - llm_end) * 1000.0) // Negative = gap
        };

        // Calculate unexplained gap
        let gap_ms = (total_duration_ms - llm_duration_ms - tts_duration_ms).max(0.0);

        // Try to explain the gap by looking for tool calls during this turn
        let agent_turn_start = agent_turn.start_sec();
        let agent_turn_end = agent_turn.end_sec();

        let gap_reason = if gap_ms > 500.0 {
            // Find tool calls that happened during this agent_turn
            let tools_during_turn: Vec<&ToolCall> = tool_calls
                .iter()
                .filter(|t| t.start >= agent_turn_start && t.start <= agent_turn_end)
                .collect();

            if !tools_during_turn.is_empty() {
                let tool_names: Vec<&str> = tools_during_turn.iter().map(|t| t.name.as_str()).collect();
                Some(format!("tool call: {}", tool_names.join(", ")))
            } else if llm_tts_overlap_ms < -500.0 {
                // Large gap between LLM end and TTS start
                Some("LLM→TTS handoff delay".to_string())
            } else if user_to_llm_ms > 300.0 && has_user_turn {
                Some("VAD/EOL detection delay".to_string())
            } else {
                Some("processing overhead".to_string())
            }
        } else {
            None
        };

        cycles.push(PipelineCycle {
            turn_number: idx + 1,
            has_user_turn,
            user_end,
            llm_start,
            llm_end,
            llm_duration_ms,
            tts_start,
            tts_end,
            tts_duration_ms,
            agent_speaking_start,
            total_duration_ms,
            user_to_llm_ms,
            llm_tts_overlap_ms,
            gap_ms,
            gap_reason,
        });
    }

    cycles
}

/// Compute call diagnosis.
fn compute_diagnosis(analysis: &CallAnalysis) -> CallDiagnosis {
    let mut slow_turns_by_cause: HashMap<String, Vec<SlowTurnInfo>> = HashMap::new();
    slow_turns_by_cause.insert("LLM".to_string(), Vec::new());
    slow_turns_by_cause.insert("TTS".to_string(), Vec::new());
    slow_turns_by_cause.insert("STT".to_string(), Vec::new());
    slow_turns_by_cause.insert("TOOL".to_string(), Vec::new());
    slow_turns_by_cause.insert("OVERHEAD".to_string(), Vec::new());

    // Build turn number mapping (assistant turn index -> original conversation turn number)
    let mut assistant_turn_numbers: Vec<usize> = Vec::new();
    for (i, turn) in analysis.turns.iter().enumerate() {
        if turn.role.as_deref() == Some("assistant") {
            assistant_turn_numbers.push(i + 1);
        }
    }

    let assistant_turns = analysis.assistant_turns();

    for (idx, turn) in assistant_turns.iter().enumerate() {
        let e2e = match turn.metrics.e2e_latency {
            Some(e) if e >= 2.0 => e,
            _ => continue,
        };

        let i = assistant_turn_numbers.get(idx).copied().unwrap_or(idx + 1);
        let llm = turn.metrics.llm_node_ttft.unwrap_or(0.0);
        let tts = turn.metrics.tts_node_ttfb.unwrap_or(0.0);

        let e2e_ms = e2e * 1000.0;
        let llm_ms = llm * 1000.0;
        let tts_ms = tts * 1000.0;

        let explained_ms = llm_ms + tts_ms;
        let unexplained_ms = (e2e_ms - explained_ms).max(0.0);

        // Determine primary bottleneck
        let contributors = [("LLM", llm_ms), ("TTS", tts_ms), ("OTHER", unexplained_ms)];
        let (mut primary_cause, _) = contributors
            .iter()
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .unwrap();

        // Check if this turn had a tool call
        let turn_start = turn.metrics.started_speaking_at.unwrap_or(turn.created_at);
        let mut tool_name = None;

        for tool in &analysis.tool_calls {
            let tool_time = tool.start;
            if turn_start > 0.0 && (turn_start - tool_time) > 0.0 && (turn_start - tool_time) < 10.0
            {
                tool_name = Some(tool.name.clone());
                break;
            }
        }

        if tool_name.is_some() && (primary_cause == "OTHER" || unexplained_ms > 1000.0) {
            primary_cause = "TOOL";
        }

        let text = turn.text();
        let text_preview: String = text.chars().take(50).collect();

        let info = SlowTurnInfo {
            turn: i,
            e2e_ms,
            llm_ms,
            tts_ms,
            unexplained_ms,
            text: text_preview,
            tool_name,
        };

        let cause_key = if primary_cause == "OTHER" {
            "OVERHEAD"
        } else {
            primary_cause
        };

        slow_turns_by_cause
            .get_mut(cause_key)
            .unwrap()
            .push(info);
    }

    // Count TTS retries
    let tts_retries = analysis
        .warnings
        .iter()
        .filter(|w| w.message.to_lowercase().contains("failed to synthesize speech"))
        .count();

    // Count tool errors
    let tool_errors = analysis
        .errors
        .iter()
        .filter(|e| {
            e.message.to_lowercase().contains("tool") || e.message.contains("PROBOOK")
        })
        .count();

    let total_slow: usize = slow_turns_by_cause.values().map(|v| v.len()).sum();

    // Determine verdict
    let verdict = if total_slow == 0 && analysis.errors.is_empty() && tts_retries == 0 {
        DiagnosisVerdict::Healthy
    } else if total_slow > 5 || !analysis.errors.is_empty() {
        DiagnosisVerdict::Problematic
    } else {
        DiagnosisVerdict::NeedsAttention
    };

    // Determine primary issue
    let (primary_issue, primary_issue_detail) = determine_primary_issue(
        &slow_turns_by_cause,
        tts_retries,
        tool_errors,
        total_slow,
        analysis.errors.is_empty(),
    );

    CallDiagnosis {
        verdict,
        primary_issue,
        primary_issue_detail,
        slow_turns_by_cause,
        tts_retries,
        tool_errors,
    }
}

fn determine_primary_issue(
    slow_turns_by_cause: &HashMap<String, Vec<SlowTurnInfo>>,
    tts_retries: usize,
    tool_errors: usize,
    total_slow: usize,
    no_errors: bool,
) -> (Option<String>, Option<String>) {
    let llm_turns = slow_turns_by_cause.get("LLM").map(|v| v.as_slice()).unwrap_or(&[]);
    let tts_turns = slow_turns_by_cause.get("TTS").map(|v| v.as_slice()).unwrap_or(&[]);
    let tool_turns = slow_turns_by_cause.get("TOOL").map(|v| v.as_slice()).unwrap_or(&[]);

    if !llm_turns.is_empty() {
        let avg_llm: f64 = llm_turns.iter().map(|t| t.llm_ms).sum::<f64>() / llm_turns.len() as f64;
        return (
            Some(format!("LLM latency (avg {:.0}ms in slow turns)", avg_llm)),
            Some("Consider: faster model, shorter prompts, or check LLM provider status".to_string()),
        );
    }

    if !tts_turns.is_empty() {
        let avg_tts: f64 = tts_turns.iter().map(|t| t.tts_ms).sum::<f64>() / tts_turns.len() as f64;
        return (
            Some(format!("TTS latency (avg {:.0}ms in slow turns)", avg_tts)),
            Some("Consider: TTS provider issues, voice model, or network".to_string()),
        );
    }

    if !tool_turns.is_empty() {
        return (
            Some("Tool execution delays".to_string()),
            Some("Consider: tool timeouts, API latency, or caching".to_string()),
        );
    }

    if tts_retries > 0 {
        return (
            Some("TTS synthesis failures causing retries".to_string()),
            Some("Consider: TTS provider quota, rate limits, or service issues".to_string()),
        );
    }

    if tool_errors > 0 {
        return (
            Some("Tool errors".to_string()),
            Some("Check tool implementation and error handling".to_string()),
        );
    }

    if total_slow == 0 && no_errors {
        return (
            Some("Call performed well!".to_string()),
            None,
        );
    }

    (None, None)
}
