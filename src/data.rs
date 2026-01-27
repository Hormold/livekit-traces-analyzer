//! Data structures for LiveKit call analysis.

use std::collections::HashMap;
use std::path::PathBuf;

/// Metrics for a single conversation turn.
#[derive(Debug, Clone, Default)]
pub struct TurnMetrics {
    pub started_speaking_at: Option<f64>,
    pub stopped_speaking_at: Option<f64>,
    pub llm_node_ttft: Option<f64>,      // LLM time to first token (seconds)
    pub tts_node_ttfb: Option<f64>,      // TTS time to first byte (seconds)
    pub e2e_latency: Option<f64>,        // End-to-end latency (seconds)
    pub transcript_confidence: Option<f64>,
}

impl TurnMetrics {
    pub fn speaking_duration(&self) -> Option<f64> {
        match (self.started_speaking_at, self.stopped_speaking_at) {
            (Some(start), Some(end)) => Some(end - start),
            _ => None,
        }
    }
}

/// A single turn in the conversation.
#[derive(Debug, Clone)]
pub struct ConversationTurn {
    pub id: String,
    pub turn_type: String,  // "message", "agent_handoff", "function_call", etc.
    pub role: Option<String>, // "user", "assistant"
    pub content: Vec<String>,
    pub interrupted: bool,
    pub created_at: f64,
    pub metrics: TurnMetrics,
    pub extra: HashMap<String, serde_json::Value>,
}

impl ConversationTurn {
    pub fn text(&self) -> String {
        self.content.join(" ")
    }
}

/// A single log entry from OTEL logs.
#[derive(Debug, Clone)]
pub struct LogEntry {
    pub timestamp_ns: i64,
    pub severity: String,
    pub message: String,
    pub logger_name: String,
}

impl LogEntry {
    pub fn timestamp_sec(&self) -> f64 {
        self.timestamp_ns as f64 / 1e9
    }
}

/// An OpenTelemetry span.
#[derive(Debug, Clone)]
pub struct Span {
    pub span_id: String,
    pub parent_span_id: Option<String>,
    pub name: String,
    pub start_time_ns: i64,
    pub end_time_ns: i64,
    pub attributes: HashMap<String, serde_json::Value>,
}

impl Span {
    pub fn duration_ms(&self) -> f64 {
        (self.end_time_ns - self.start_time_ns) as f64 / 1e6
    }

    pub fn start_sec(&self) -> f64 {
        self.start_time_ns as f64 / 1e9
    }

    pub fn end_sec(&self) -> f64 {
        self.end_time_ns as f64 / 1e9
    }
}

/// Context information for an LLM turn extracted from llm_node spans.
#[derive(Debug, Clone)]
pub struct LLMTurnContext {
    pub turn_index: usize,
    pub duration_ms: f64,
    pub context_messages: usize,
    pub context_chars: usize,
    pub context_tokens_est: usize,
    pub response_text: String,
    pub response_chars: usize,
    pub start_time: f64,
}

/// Tool call information.
#[derive(Debug, Clone)]
pub struct ToolCall {
    pub name: String,
    pub start: f64,
    pub duration_ms: f64,
}

/// Call diagnosis summary.
#[derive(Debug, Clone)]
pub struct CallDiagnosis {
    pub verdict: DiagnosisVerdict,
    pub primary_issue: Option<String>,
    pub primary_issue_detail: Option<String>,
    pub slow_turns_by_cause: HashMap<String, Vec<SlowTurnInfo>>,
    pub tts_retries: usize,
    pub tool_errors: usize,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DiagnosisVerdict {
    Healthy,
    NeedsAttention,
    Problematic,
}

#[derive(Debug, Clone)]
pub struct SlowTurnInfo {
    pub turn: usize,
    pub e2e_ms: f64,
    pub llm_ms: f64,
    pub tts_ms: f64,
    pub unexplained_ms: f64,
    pub text: String,
    pub tool_name: Option<String>,
}

/// Complete analysis of a call.
#[derive(Debug, Clone)]
pub struct CallAnalysis {
    pub folder_path: PathBuf,
    pub room_id: String,
    pub job_id: String,
    pub agent_name: String,
    pub room_name: String,
    pub participant_identity: String,

    // Timing
    pub session_start: f64,
    pub session_end: f64,

    // Data
    pub turns: Vec<ConversationTurn>,
    pub llm_turns: Vec<LLMTurnContext>,
    pub system_prompt: String,
    pub logs: Vec<LogEntry>,
    pub spans: Vec<Span>,

    // Computed
    pub errors: Vec<LogEntry>,
    pub warnings: Vec<LogEntry>,
    pub tool_calls: Vec<ToolCall>,
    pub diagnosis: Option<CallDiagnosis>,
    pub pipeline_cycles: Vec<PipelineCycle>,
}

impl CallAnalysis {
    pub fn new(folder_path: PathBuf) -> Self {
        Self {
            folder_path,
            room_id: String::new(),
            job_id: String::new(),
            agent_name: String::new(),
            room_name: String::new(),
            participant_identity: String::new(),
            session_start: 0.0,
            session_end: 0.0,
            turns: Vec::new(),
            llm_turns: Vec::new(),
            system_prompt: String::new(),
            logs: Vec::new(),
            spans: Vec::new(),
            errors: Vec::new(),
            warnings: Vec::new(),
            tool_calls: Vec::new(),
            diagnosis: None,
            pipeline_cycles: Vec::new(),
        }
    }

    pub fn duration_sec(&self) -> f64 {
        self.session_end - self.session_start
    }

    pub fn user_turns(&self) -> Vec<&ConversationTurn> {
        self.turns.iter()
            .filter(|t| t.role.as_deref() == Some("user"))
            .collect()
    }

    pub fn assistant_turns(&self) -> Vec<&ConversationTurn> {
        self.turns.iter()
            .filter(|t| t.role.as_deref() == Some("assistant"))
            .collect()
    }

    pub fn interrupted_turns(&self) -> Vec<&ConversationTurn> {
        self.turns.iter()
            .filter(|t| t.interrupted)
            .collect()
    }
}

/// A single user→agent pipeline cycle with timing breakdown.
#[derive(Debug, Clone)]
pub struct PipelineCycle {
    pub turn_number: usize,
    pub has_user_turn: bool,      // Whether this was triggered by user speech
    pub user_end: f64,            // When user stopped speaking (seconds)
    pub llm_start: f64,           // When LLM started processing (seconds)
    pub llm_end: f64,             // When LLM finished (seconds)
    pub llm_duration_ms: f64,
    pub tts_start: f64,
    pub tts_end: f64,
    pub tts_duration_ms: f64,
    pub agent_speaking_start: f64,
    pub total_duration_ms: f64,

    // Computed gaps
    pub user_to_llm_ms: f64,      // Gap 1: perception delay (EOL detection) - only valid if has_user_turn
    pub llm_tts_overlap_ms: f64,  // Positive = streaming benefit

    // Gap explanation
    pub gap_ms: f64,              // Unexplained time (total - llm - tts)
    pub gap_reason: Option<String>, // Explanation if we detected something (e.g., "tool: create_customer")
}

/// Severity level for pipeline metrics (used for coloring).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Severity {
    Good,
    Warning,
    Critical,
}

/// A detected delay in the pipeline.
#[derive(Debug, Clone)]
pub struct DetectedDelay {
    pub turn_number: usize,
    pub gap_ms: f64,
    pub reason: String,
    pub is_tool_related: bool,
}

/// Pre-computed pipeline summary for rendering.
#[derive(Debug, Clone)]
pub struct PipelineSummary {
    // Response time
    pub avg_total_ms: f64,
    pub max_total_ms: f64,
    pub total_verdict: &'static str,
    pub total_severity: Severity,

    // LLM breakdown
    pub avg_llm_ms: f64,
    pub llm_pct: f64,
    pub llm_verdict: &'static str,
    pub llm_severity: Severity,

    // TTS breakdown
    pub avg_tts_ms: f64,
    pub tts_pct: f64,
    pub tts_verdict: &'static str,
    pub tts_severity: Severity,

    // Perception delay (User→LLM)
    pub avg_user_to_llm_ms: f64,
    pub user_turn_count: usize,
    pub perception_verdict: &'static str,
    pub perception_severity: Severity,

    // System turns
    pub system_turn_count: usize,

    // Bottleneck
    pub bottleneck: String,
    pub bottleneck_severity: Severity,

    // Detected delays
    pub detected_delays: Vec<DetectedDelay>,
}

impl PipelineSummary {
    /// Generate pipeline summary from pipeline cycles.
    pub fn from_cycles(cycles: &[PipelineCycle]) -> Option<Self> {
        if cycles.is_empty() {
            return None;
        }

        let count = cycles.len();
        let user_initiated: Vec<_> = cycles.iter().filter(|c| c.has_user_turn).collect();
        let user_count = user_initiated.len();

        // Calculate averages
        let avg_llm_ms: f64 = cycles.iter().map(|c| c.llm_duration_ms).sum::<f64>() / count as f64;
        let avg_tts_ms: f64 = cycles.iter().map(|c| c.tts_duration_ms).sum::<f64>() / count as f64;
        let avg_total_ms: f64 = cycles.iter().map(|c| c.total_duration_ms).sum::<f64>() / count as f64;
        let max_total_ms: f64 = cycles.iter().map(|c| c.total_duration_ms).fold(0.0, f64::max);

        let avg_user_to_llm_ms: f64 = if !user_initiated.is_empty() {
            user_initiated.iter().map(|c| c.user_to_llm_ms.max(0.0)).sum::<f64>() / user_count as f64
        } else {
            0.0
        };

        // Verdicts and severities
        let (total_verdict, total_severity) = if avg_total_ms < 3000.0 {
            ("good for voice", Severity::Good)
        } else if avg_total_ms < 5000.0 {
            ("acceptable", Severity::Good)
        } else if avg_total_ms < 8000.0 {
            ("slow, users will notice", Severity::Warning)
        } else {
            ("very slow, poor UX", Severity::Critical)
        };

        let llm_pct = if avg_total_ms > 0.0 { (avg_llm_ms / avg_total_ms) * 100.0 } else { 0.0 };
        let (llm_verdict, llm_severity) = if avg_llm_ms < 1000.0 {
            ("fast", Severity::Good)
        } else if avg_llm_ms < 2000.0 {
            ("normal", Severity::Good)
        } else if avg_llm_ms < 4000.0 {
            ("slow - consider faster model or shorter prompts", Severity::Warning)
        } else {
            ("very slow - check LLM provider or reduce context", Severity::Critical)
        };

        let tts_pct = if avg_total_ms > 0.0 { (avg_tts_ms / avg_total_ms) * 100.0 } else { 0.0 };
        let (tts_verdict, tts_severity) = if avg_tts_ms < 1500.0 {
            ("fast", Severity::Good)
        } else if avg_tts_ms < 3000.0 {
            ("normal", Severity::Good)
        } else if avg_tts_ms < 5000.0 {
            ("slow - check TTS provider or voice settings", Severity::Warning)
        } else {
            ("very slow - TTS is major bottleneck", Severity::Critical)
        };

        let (perception_verdict, perception_severity) = if avg_user_to_llm_ms < 100.0 {
            ("instant", Severity::Good)
        } else if avg_user_to_llm_ms < 200.0 {
            ("good VAD", Severity::Good)
        } else if avg_user_to_llm_ms < 500.0 {
            ("noticeable delay after user stops", Severity::Warning)
        } else {
            ("slow EOL detection - check VAD settings", Severity::Critical)
        };

        // Bottleneck identification
        let (bottleneck, bottleneck_severity) = if tts_pct > 50.0 {
            (format!("TTS is the main delay ({:.0}%)", tts_pct), Severity::Critical)
        } else if llm_pct > 50.0 {
            (format!("LLM is the main delay ({:.0}%)", llm_pct), Severity::Critical)
        } else if avg_user_to_llm_ms > 300.0 {
            ("Perception delay (VAD/EOL)".to_string(), Severity::Warning)
        } else {
            ("None dominant - balanced pipeline".to_string(), Severity::Good)
        };

        // Detected delays
        let detected_delays: Vec<DetectedDelay> = cycles.iter()
            .filter(|c| c.gap_ms > 500.0 && c.gap_reason.is_some())
            .map(|c| DetectedDelay {
                turn_number: c.turn_number,
                gap_ms: c.gap_ms,
                reason: c.gap_reason.clone().unwrap_or_default(),
                is_tool_related: c.gap_reason.as_ref().map(|r| r.starts_with("tool")).unwrap_or(false),
            })
            .collect();

        Some(Self {
            avg_total_ms,
            max_total_ms,
            total_verdict,
            total_severity,
            avg_llm_ms,
            llm_pct,
            llm_verdict,
            llm_severity,
            avg_tts_ms,
            tts_pct,
            tts_verdict,
            tts_severity,
            avg_user_to_llm_ms,
            user_turn_count: user_count,
            perception_verdict,
            perception_severity,
            system_turn_count: count - user_count,
            bottleneck,
            bottleneck_severity,
            detected_delays,
        })
    }

}

/// Latency statistics for a component.
#[derive(Debug, Clone, Default)]
pub struct LatencyStats {
    pub avg_ms: f64,
    pub min_ms: f64,
    pub max_ms: f64,
    pub p95_ms: f64,
    pub count: usize,
}

impl LatencyStats {
    pub fn from_values(values: &[f64]) -> Option<Self> {
        if values.is_empty() {
            return None;
        }

        let mut sorted: Vec<f64> = values.iter().map(|v| v * 1000.0).collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let sum: f64 = sorted.iter().sum();
        let count = sorted.len();
        let avg_ms = sum / count as f64;
        let min_ms = sorted.first().copied().unwrap_or(0.0);
        let max_ms = sorted.last().copied().unwrap_or(0.0);

        // P95 calculation
        let p95_idx = ((count as f64) * 0.95).ceil() as usize;
        let p95_ms = sorted.get(p95_idx.saturating_sub(1)).copied().unwrap_or(max_ms);

        Some(Self {
            avg_ms,
            min_ms,
            max_ms,
            p95_ms,
            count,
        })
    }
}
