//! Centralized threshold constants and severity functions for latency analysis.
//!
//! This module provides a single source of truth for all latency thresholds
//! used across the application (TUI, reports, analysis).
use crate::data::Severity;

// =============================================================================
// E2E LATENCY THRESHOLDS (End-to-End response time)
// =============================================================================

/// E2E latency below this is considered good (green).
pub const E2E_GOOD_MS: f64 = 500.0;
/// E2E latency below this is warning (yellow), above is critical (red).
pub const E2E_WARN_MS: f64 = 1500.0;
/// E2E latency above this is considered "slow" for diagnosis.
pub const E2E_SLOW_MS: f64 = 2000.0;

// =============================================================================
// LLM LATENCY THRESHOLDS (Time to first token)
// =============================================================================

/// LLM latency below this is fast (green).
pub const LLM_GOOD_MS: f64 = 1500.0;
/// LLM latency below this is warning (yellow), above is critical (red).
pub const LLM_WARN_MS: f64 = 3000.0;

// =============================================================================
// TTS LATENCY THRESHOLDS (Time to first byte)
// =============================================================================

/// TTS latency below this is fast (green).
pub const TTS_GOOD_MS: f64 = 2000.0;
/// TTS latency below this is warning (yellow), above is critical (red).
pub const TTS_WARN_MS: f64 = 4000.0;

// =============================================================================
// PERCEPTION DELAY THRESHOLDS (User→LLM, VAD/EOL detection)
// =============================================================================

/// Perception delay below this is instant (green).
pub const PERCEPTION_GOOD_MS: f64 = 100.0;
/// Perception delay below this is acceptable (yellow), above is slow (red).
pub const PERCEPTION_WARN_MS: f64 = 200.0;

// =============================================================================
// TOTAL PIPELINE THRESHOLDS (Full response cycle)
// =============================================================================

/// Total pipeline time below this is good for voice (green).
pub const TOTAL_GOOD_MS: f64 = 4000.0;
/// Total pipeline time below this is acceptable (yellow), above is slow (red).
pub const TOTAL_WARN_MS: f64 = 8000.0;

// =============================================================================
// TOOL CALL THRESHOLDS
// =============================================================================

#[allow(dead_code)]
pub const TOOL_GOOD_MS: f64 = 100.0;
#[allow(dead_code)]
pub const TOOL_WARN_MS: f64 = 500.0;

// =============================================================================
// GAP DETECTION THRESHOLDS
// =============================================================================

/// Gap duration above this is considered significant and worth explaining.
pub const GAP_SIGNIFICANT_MS: f64 = 500.0;

// =============================================================================
// CONFIDENCE THRESHOLDS (Transcript confidence)
// =============================================================================

#[allow(dead_code)]
pub const CONFIDENCE_GOOD_PCT: f64 = 95.0;
/// Confidence above this is acceptable (yellow), below is poor (red).
#[allow(dead_code)]
pub const CONFIDENCE_WARN_PCT: f64 = 85.0;

// =============================================================================
// DISPLAY LIMITS (for pagination/previews)
// =============================================================================

/// Max slow turns to show per cause in diagnosis.
pub const MAX_SLOW_TURNS_PER_CAUSE: usize = 3;
/// Max warnings to show in report.
pub const MAX_WARNINGS_DISPLAY: usize = 10;
/// Max tool calls to show in timeline.
pub const MAX_TOOL_CALLS_DISPLAY: usize = 15;
/// Max spans to show in timeline.
pub const MAX_SPANS_DISPLAY: usize = 30;
/// Max detected delays to show.
pub const MAX_DETECTED_DELAYS: usize = 5;
/// Text preview length (short).
pub const TEXT_PREVIEW_SHORT: usize = 50;
/// Text preview length (medium).
pub const TEXT_PREVIEW_MEDIUM: usize = 100;
/// Text preview length (long).
pub const TEXT_PREVIEW_LONG: usize = 200;

// =============================================================================
// SEVERITY FUNCTIONS
// =============================================================================

/// Get severity for E2E latency (in milliseconds).
pub fn e2e_severity(ms: f64) -> Severity {
    if ms < E2E_GOOD_MS {
        Severity::Good
    } else if ms < E2E_WARN_MS {
        Severity::Warning
    } else {
        Severity::Critical
    }
}

/// Get severity for LLM latency (in milliseconds).
pub fn llm_severity(ms: f64) -> Severity {
    if ms < LLM_GOOD_MS {
        Severity::Good
    } else if ms < LLM_WARN_MS {
        Severity::Warning
    } else {
        Severity::Critical
    }
}

/// Get severity for TTS latency (in milliseconds).
pub fn tts_severity(ms: f64) -> Severity {
    if ms < TTS_GOOD_MS {
        Severity::Good
    } else if ms < TTS_WARN_MS {
        Severity::Warning
    } else {
        Severity::Critical
    }
}

/// Get severity for perception delay (User→LLM, in milliseconds).
pub fn perception_severity(ms: f64) -> Severity {
    if ms < PERCEPTION_GOOD_MS {
        Severity::Good
    } else if ms < PERCEPTION_WARN_MS {
        Severity::Warning
    } else {
        Severity::Critical
    }
}

/// Get severity for total pipeline time (in milliseconds).
pub fn total_severity(ms: f64) -> Severity {
    if ms < TOTAL_GOOD_MS {
        Severity::Good
    } else if ms < TOTAL_WARN_MS {
        Severity::Warning
    } else {
        Severity::Critical
    }
}

/// Get severity for tool call duration (in milliseconds).
#[allow(dead_code)]
pub fn tool_severity(ms: f64) -> Severity {
    if ms < TOOL_GOOD_MS {
        Severity::Good
    } else if ms < TOOL_WARN_MS {
        Severity::Warning
    } else {
        Severity::Critical
    }
}

/// Get severity for transcript confidence (in percentage 0-100).
#[allow(dead_code)]
pub fn confidence_severity(pct: f64) -> Severity {
    if pct > CONFIDENCE_GOOD_PCT {
        Severity::Good
    } else if pct > CONFIDENCE_WARN_PCT {
        Severity::Warning
    } else {
        Severity::Critical
    }
}

// =============================================================================
// VERDICT FUNCTIONS (Human-readable descriptions)
// =============================================================================

/// Get verdict string for LLM latency.
pub fn llm_verdict(ms: f64) -> &'static str {
    if ms < 1000.0 {
        "fast"
    } else if ms < 2000.0 {
        "normal"
    } else if ms < 4000.0 {
        "slow - consider faster model or shorter prompts"
    } else {
        "very slow - check LLM provider or reduce context"
    }
}

/// Get verdict string for TTS latency.
pub fn tts_verdict(ms: f64) -> &'static str {
    if ms < 1500.0 {
        "fast"
    } else if ms < 3000.0 {
        "normal"
    } else if ms < 5000.0 {
        "slow - check TTS provider or voice settings"
    } else {
        "very slow - TTS is major bottleneck"
    }
}

/// Get verdict string for perception delay.
pub fn perception_verdict(ms: f64) -> &'static str {
    if ms < 100.0 {
        "instant"
    } else if ms < 200.0 {
        "good VAD"
    } else if ms < 500.0 {
        "noticeable delay after user stops"
    } else {
        "slow EOL detection - check VAD settings"
    }
}

/// Get verdict string for total pipeline time.
pub fn total_verdict(ms: f64) -> &'static str {
    if ms < 3000.0 {
        "good for voice"
    } else if ms < 5000.0 {
        "acceptable"
    } else if ms < 8000.0 {
        "slow, users will notice"
    } else {
        "very slow, poor UX"
    }
}

// =============================================================================
// KEY SPAN NAMES
// =============================================================================

/// Key span names used for filtering important spans in timeline/analysis.
pub const KEY_SPAN_NAMES: &[&str] = &[
    "agent_turn",
    "user_turn",
    "llm_node",
    "tts_request",
    "tts_node",
    "stt_request",
    "function_call",
    "tool_call",
];

/// Check if a span name is a key span.
pub fn is_key_span(name: &str) -> bool {
    KEY_SPAN_NAMES.contains(&name)
}

// =============================================================================
// CAUSE ICONS (for diagnosis display)
// =============================================================================

/// Icon/label pairs for slow turn causes.
pub const CAUSE_ICONS: &[(&str, &str)] = &[
    ("LLM", "[LLM]"),
    ("TTS", "[TTS]"),
    ("TOOL", "[TOOL]"),
    ("STT", "[STT]"),
    ("OVERHEAD", "[GAP]"),
];

/// Get the display label for a cause.
pub fn cause_label(cause: &str) -> &'static str {
    match cause {
        "OVERHEAD" => "PROCESSING GAPS",
        "LLM" => "LLM",
        "TTS" => "TTS",
        "TOOL" => "TOOL",
        "STT" => "STT",
        _ => "UNKNOWN",
    }
}

/// Get hint text explaining a cause.
pub fn cause_hint(cause: &str) -> &'static str {
    match cause {
        "OVERHEAD" => " (time between stages, streaming, network)",
        "LLM" => " (model inference)",
        "TTS" => " (speech synthesis)",
        "TOOL" => " (function execution)",
        _ => "",
    }
}
