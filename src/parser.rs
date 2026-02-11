//! JSON parsing for OTEL formats.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde_json::Value;

use crate::data::{ConversationTurn, LLMTurnContext, LogEntry, Span, TurnMetrics};

/// Parse chat_history.json into ConversationTurn objects.
pub fn parse_chat_history(data: &Value) -> Vec<ConversationTurn> {
    let items = match data.get("items").and_then(|v| v.as_array()) {
        Some(items) => items,
        None => return Vec::new(),
    };

    items
        .iter()
        .filter_map(|item| parse_conversation_turn(item))
        .collect()
}

fn parse_conversation_turn(item: &Value) -> Option<ConversationTurn> {
    let id = item.get("id")?.as_str()?.to_string();
    let turn_type = item.get("type")?.as_str()?.to_string();

    let role = item.get("role").and_then(|v| v.as_str()).map(String::from);

    let content = match item.get("content") {
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str())
            .map(String::from)
            .collect(),
        Some(Value::String(s)) => vec![s.clone()],
        _ => Vec::new(),
    };

    let interrupted = item.get("interrupted").and_then(|v| v.as_bool()).unwrap_or(false);
    let created_at = item.get("created_at").and_then(|v| v.as_f64()).unwrap_or(0.0);

    let metrics = parse_turn_metrics(item);

    let mut extra = match item.get("extra") {
        Some(Value::Object(obj)) => obj
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect(),
        _ => HashMap::new(),
    };

    // Capture top-level fields used by function_call / function_call_output items
    for key in &["name", "arguments", "call_id", "output"] {
        if let Some(val) = item.get(*key) {
            extra.entry(key.to_string()).or_insert_with(|| val.clone());
        }
    }

    Some(ConversationTurn {
        id,
        turn_type,
        role,
        content,
        interrupted,
        created_at,
        metrics,
        extra,
    })
}

fn parse_turn_metrics(item: &Value) -> TurnMetrics {
    let metrics_data = item.get("metrics").cloned().unwrap_or(Value::Null);

    TurnMetrics {
        started_speaking_at: metrics_data.get("started_speaking_at").and_then(|v| v.as_f64()),
        stopped_speaking_at: metrics_data.get("stopped_speaking_at").and_then(|v| v.as_f64()),
        llm_node_ttft: metrics_data.get("llm_node_ttft").and_then(|v| v.as_f64()),
        tts_node_ttfb: metrics_data.get("tts_node_ttfb").and_then(|v| v.as_f64()),
        e2e_latency: metrics_data.get("e2e_latency").and_then(|v| v.as_f64()),
        transcript_confidence: item.get("transcript_confidence").and_then(|v| v.as_f64()),
    }
}

/// Parse logs.json (OpenTelemetry format) into LogEntry objects.
pub fn parse_logs(data: &Value) -> Vec<LogEntry> {
    let mut logs = Vec::new();

    let resource_logs = match data.get("resourceLogs").and_then(|v| v.as_array()) {
        Some(logs) => logs,
        None => return logs,
    };

    for resource_log in resource_logs {
        let scope_logs = match resource_log.get("scopeLogs").and_then(|v| v.as_array()) {
            Some(logs) => logs,
            None => continue,
        };

        for scope_log in scope_logs {
            let logger_name = scope_log
                .get("scope")
                .and_then(|s| s.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("unknown")
                .to_string();

            let records = match scope_log.get("logRecords").and_then(|v| v.as_array()) {
                Some(records) => records,
                None => continue,
            };

            for record in records {
                if let Some(log) = parse_log_record(record, &logger_name) {
                    logs.push(log);
                }
            }
        }
    }

    // Sort by timestamp
    logs.sort_by_key(|l| l.timestamp_ns);
    logs
}

fn parse_log_record(record: &Value, logger_name: &str) -> Option<LogEntry> {
    let timestamp_ns = parse_otel_timestamp(record.get("timeUnixNano"))?;

    let severity = record
        .get("severityText")
        .and_then(|v| v.as_str())
        .unwrap_or("INFO")
        .to_string();

    let message = record
        .get("body")
        .and_then(|b| b.get("stringValue"))
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    Some(LogEntry {
        timestamp_ns,
        severity,
        message,
        logger_name: logger_name.to_string(),
    })
}

/// Parse traces.json (OpenTelemetry format) into Span objects.
pub fn parse_traces(data: &Value) -> Vec<Span> {
    let mut spans = Vec::new();

    let resource_spans = match data.get("resourceSpans").and_then(|v| v.as_array()) {
        Some(spans) => spans,
        None => return spans,
    };

    for resource_span in resource_spans {
        let scope_spans = match resource_span.get("scopeSpans").and_then(|v| v.as_array()) {
            Some(spans) => spans,
            None => continue,
        };

        for scope_span in scope_spans {
            let span_data_list = match scope_span.get("spans").and_then(|v| v.as_array()) {
                Some(spans) => spans,
                None => continue,
            };

            for span_data in span_data_list {
                if let Some(span) = parse_span(span_data) {
                    spans.push(span);
                }
            }
        }
    }

    // Sort by start time
    spans.sort_by_key(|s| s.start_time_ns);
    spans
}

fn parse_span(span_data: &Value) -> Option<Span> {
    let span_id = span_data.get("spanId")?.as_str()?.to_string();
    let parent_span_id = span_data
        .get("parentSpanId")
        .and_then(|v| v.as_str())
        .map(String::from);
    let name = span_data.get("name")?.as_str()?.to_string();
    let start_time_ns = parse_otel_timestamp(span_data.get("startTimeUnixNano"))?;
    let end_time_ns = parse_otel_timestamp(span_data.get("endTimeUnixNano"))?;
    let attributes = parse_otel_attributes(span_data.get("attributes"));

    Some(Span {
        span_id,
        parent_span_id,
        name,
        start_time_ns,
        end_time_ns,
        attributes,
    })
}

fn parse_otel_timestamp(value: Option<&Value>) -> Option<i64> {
    match value {
        Some(Value::String(s)) => s.parse().ok(),
        Some(Value::Number(n)) => n.as_i64(),
        _ => None,
    }
}

fn parse_otel_attributes(attrs: Option<&Value>) -> HashMap<String, Value> {
    let mut result = HashMap::new();

    let attrs_array = match attrs.and_then(|v| v.as_array()) {
        Some(arr) => arr,
        None => return result,
    };

    for attr in attrs_array {
        let key = match attr.get("key").and_then(|k| k.as_str()) {
            Some(k) => k.to_string(),
            None => continue,
        };

        let value = match attr.get("value") {
            Some(v) => parse_otel_value(v),
            None => continue,
        };

        result.insert(key, value);
    }

    result
}

/// Parse OTEL value types (stringValue, intValue, doubleValue, kvlistValue, arrayValue).
fn parse_otel_value(value: &Value) -> Value {
    if let Some(s) = value.get("stringValue").and_then(|v| v.as_str()) {
        return Value::String(s.to_string());
    }
    if let Some(i) = value.get("intValue") {
        if let Some(s) = i.as_str() {
            if let Ok(n) = s.parse::<i64>() {
                return Value::Number(n.into());
            }
        }
        if let Some(n) = i.as_i64() {
            return Value::Number(n.into());
        }
    }
    if let Some(d) = value.get("doubleValue").and_then(|v| v.as_f64()) {
        return serde_json::json!(d);
    }
    if let Some(b) = value.get("boolValue").and_then(|v| v.as_bool()) {
        return Value::Bool(b);
    }
    if let Some(kv) = value.get("kvlistValue") {
        return parse_kvlist_value(kv);
    }
    if let Some(arr) = value.get("arrayValue") {
        return parse_array_value(arr);
    }
    Value::Null
}

fn parse_kvlist_value(kv: &Value) -> Value {
    let mut map = serde_json::Map::new();

    if let Some(values) = kv.get("values").and_then(|v| v.as_array()) {
        for item in values {
            if let Some(key) = item.get("key").and_then(|k| k.as_str()) {
                if let Some(val) = item.get("value") {
                    map.insert(key.to_string(), parse_otel_value(val));
                }
            }
        }
    }

    Value::Object(map)
}

fn parse_array_value(arr: &Value) -> Value {
    if let Some(values) = arr.get("values").and_then(|v| v.as_array()) {
        let parsed: Vec<Value> = values.iter().map(parse_otel_value).collect();
        Value::Array(parsed)
    } else {
        Value::Array(Vec::new())
    }
}

/// Extract LLM context info from llm_node spans in traces.
pub fn parse_llm_turns_from_traces(data: &Value) -> Vec<LLMTurnContext> {
    let mut llm_turns = Vec::new();
    let mut turn_index = 0;

    let resource_spans = match data.get("resourceSpans").and_then(|v| v.as_array()) {
        Some(spans) => spans,
        None => return llm_turns,
    };

    for rs in resource_spans {
        let scope_spans = match rs.get("scopeSpans").and_then(|v| v.as_array()) {
            Some(spans) => spans,
            None => continue,
        };

        for ss in scope_spans {
            let spans = match ss.get("spans").and_then(|v| v.as_array()) {
                Some(spans) => spans,
                None => continue,
            };

            for span in spans {
                if span.get("name").and_then(|n| n.as_str()) == Some("llm_node") {
                    turn_index += 1;

                    let start_ns = parse_otel_timestamp(span.get("startTimeUnixNano")).unwrap_or(0);
                    let end_ns = parse_otel_timestamp(span.get("endTimeUnixNano")).unwrap_or(0);
                    let duration_ms = (end_ns - start_ns) as f64 / 1e6;

                    let mut context_messages = 0;
                    let mut context_chars = 0;
                    let mut response_text = String::new();

                    if let Some(attrs) = span.get("attributes").and_then(|v| v.as_array()) {
                        for attr in attrs {
                            let key = attr.get("key").and_then(|k| k.as_str()).unwrap_or("");

                            if key == "lk.chat_ctx" {
                                if let Some(value) = attr.get("value") {
                                    let (msgs, chars) = parse_llm_chat_ctx(value);
                                    context_messages = msgs;
                                    context_chars = chars;
                                }
                            } else if key == "lk.response.text" {
                                response_text = attr
                                    .get("value")
                                    .and_then(|v| v.get("stringValue"))
                                    .and_then(|v| v.as_str())
                                    .unwrap_or("")
                                    .to_string();
                            }
                        }
                    }

                    llm_turns.push(LLMTurnContext {
                        turn_index,
                        duration_ms,
                        context_messages,
                        context_chars,
                        context_tokens_est: context_chars / 4,
                        response_text: response_text.clone(),
                        response_chars: response_text.len(),
                        start_time: start_ns as f64 / 1e9,
                    });
                }
            }
        }
    }

    llm_turns
}

/// Parse lk.chat_ctx and return (message_count, total_chars).
fn parse_llm_chat_ctx(value: &Value) -> (usize, usize) {
    let kv_list = match value.get("kvlistValue") {
        Some(kv) => kv,
        None => return (0, 0),
    };

    let mut message_count = 0;
    let mut total_chars = 0;

    if let Some(values) = kv_list.get("values").and_then(|v| v.as_array()) {
        for kv in values {
            if kv.get("key").and_then(|k| k.as_str()) == Some("items") {
                if let Some(items) = kv
                    .get("value")
                    .and_then(|v| v.get("arrayValue"))
                    .and_then(|v| v.get("values"))
                    .and_then(|v| v.as_array())
                {
                    for item in items {
                        if let Some(item_values) = item
                            .get("kvlistValue")
                            .and_then(|v| v.get("values"))
                            .and_then(|v| v.as_array())
                        {
                            for field in item_values {
                                let key = field.get("key").and_then(|k| k.as_str()).unwrap_or("");
                                if key == "role" {
                                    message_count += 1;
                                } else if key == "content" {
                                    if let Some(val) = field.get("value") {
                                        let content = extract_text_from_otel_value(val);
                                        total_chars += content.len();
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    (message_count, total_chars)
}

/// Recursively extract all text from OTEL value structure.
fn extract_text_from_otel_value(value: &Value) -> String {
    if let Some(s) = value.get("stringValue").and_then(|v| v.as_str()) {
        return s.to_string();
    }

    if let Some(arr) = value.get("arrayValue").and_then(|v| v.get("values")).and_then(|v| v.as_array()) {
        let texts: Vec<String> = arr.iter().map(extract_text_from_otel_value).filter(|s| !s.is_empty()).collect();
        return texts.join(" ");
    }

    if let Some(kvlist) = value.get("kvlistValue").and_then(|v| v.get("values")).and_then(|v| v.as_array()) {
        let texts: Vec<String> = kvlist
            .iter()
            .filter_map(|kv| kv.get("value"))
            .map(extract_text_from_otel_value)
            .filter(|s| !s.is_empty())
            .collect();
        return texts.join(" ");
    }

    String::new()
}

/// Extract system prompt from the first llm_node span.
pub fn extract_system_prompt(data: &Value) -> String {
    let resource_spans = match data.get("resourceSpans").and_then(|v| v.as_array()) {
        Some(spans) => spans,
        None => return String::new(),
    };

    for rs in resource_spans {
        let scope_spans = match rs.get("scopeSpans").and_then(|v| v.as_array()) {
            Some(spans) => spans,
            None => continue,
        };

        for ss in scope_spans {
            let spans = match ss.get("spans").and_then(|v| v.as_array()) {
                Some(spans) => spans,
                None => continue,
            };

            for span in spans {
                if span.get("name").and_then(|n| n.as_str()) != Some("llm_node") {
                    continue;
                }

                if let Some(attrs) = span.get("attributes").and_then(|v| v.as_array()) {
                    for attr in attrs {
                        if attr.get("key").and_then(|k| k.as_str()) != Some("lk.chat_ctx") {
                            continue;
                        }

                        if let Some(chat_ctx) = attr.get("value").and_then(|v| v.get("kvlistValue")) {
                            if let Some(values) = chat_ctx.get("values").and_then(|v| v.as_array()) {
                                for kv in values {
                                    if kv.get("key").and_then(|k| k.as_str()) != Some("items") {
                                        continue;
                                    }

                                    if let Some(items) = kv
                                        .get("value")
                                        .and_then(|v| v.get("arrayValue"))
                                        .and_then(|v| v.get("values"))
                                        .and_then(|v| v.as_array())
                                    {
                                        for item in items {
                                            if let Some(item_values) = item
                                                .get("kvlistValue")
                                                .and_then(|v| v.get("values"))
                                                .and_then(|v| v.as_array())
                                            {
                                                let mut role = String::new();
                                                let mut content = String::new();

                                                for field in item_values {
                                                    let key = field.get("key").and_then(|k| k.as_str()).unwrap_or("");
                                                    if key == "role" {
                                                        role = field
                                                            .get("value")
                                                            .and_then(|v| v.get("stringValue"))
                                                            .and_then(|v| v.as_str())
                                                            .unwrap_or("")
                                                            .to_string();
                                                    } else if key == "content" {
                                                        if let Some(val) = field.get("value") {
                                                            content = extract_text_from_otel_value(val);
                                                        }
                                                    }
                                                }

                                                if role == "system" {
                                                    return content;
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // No system message found in first llm_node
                return String::new();
            }
        }
    }

    String::new()
}

/// Load JSON file and parse it.
pub fn load_json_file(path: &Path) -> Result<Value> {
    let content = fs::read_to_string(path)
        .with_context(|| format!("Failed to read file: {}", path.display()))?;
    let data: Value = serde_json::from_str(&content)
        .with_context(|| format!("Failed to parse JSON: {}", path.display()))?;
    Ok(data)
}
