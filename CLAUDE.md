# LiveKit Traces Analyzer - Agent Documentation

## What This Tool Does

Analyzes LiveKit voice agent calls from multiple data sources:
- **Observability traces** (logs.json, spans.json) - Agent timing, errors, conversation
- **Network captures** (PCAP files) - SIP signaling, RTP quality, jitter, packet loss

Diagnoses latency issues, identifies bottlenecks, and provides actionable recommendations.

## Supported Input Formats

| Format | Description | Auto-detected |
|--------|-------------|---------------|
| `folder/` | Observability folder with logs.json, spans.json | Yes |
| `file.zip` | ZIP archive (auto-extracts) | Yes |
| `file.pcap` | Network capture (SIP/RTP analysis) | Yes |

You can combine multiple inputs:
```bash
# Traces + PCAP together for complete picture
livekit-analyzer ./traces.zip ./call.pcap --dump
```

## Quick Start

```bash
# Quick summary (RECOMMENDED for agents - easy to parse)
livekit-analyzer ./traces --summary

# Full text report (human-readable)
livekit-analyzer ./traces -r

# Everything combined (best for diagnosis)
livekit-analyzer ./traces --dump

# Chronological timeline (BREAKDOWN.md, best for agents)
livekit-analyzer ./traces --timeline

# PCAP network analysis only
livekit-analyzer ./call.pcap --pcap

# Combined: traces + network capture
livekit-analyzer ./traces.zip ./call.pcap --dump

# JSON output (structured data)
livekit-analyzer ./traces --json
```

## Summary Format (--summary)

Best format for agents. Returns key=value pairs, one per line:

```
verdict=HEALTHY
slow_turns=0
errors=0
warnings=2
duration_sec=45.3
total_turns=12
user_turns=6
assistant_turns=6
interrupted=0
tool_calls=3
e2e_avg_ms=1200
e2e_p95_ms=2100
e2e_max_ms=2500
llm_avg_ms=800
llm_p95_ms=1500
tts_avg_ms=400
tts_p95_ms=700
bottleneck=LLM_is_primary_bottleneck
llm_pct=65
tts_pct=30
perception_ms=80
room_id=RM_abc123
agent=voice-agent
```

**Key fields to check:**
- `verdict` - HEALTHY, NEEDS_ATTENTION, or PROBLEMATIC
- `slow_turns` - Number of turns >2s E2E
- `bottleneck` - What's causing delays
- `e2e_avg_ms` - Average response time
- `llm_pct` / `tts_pct` - Where time is spent

## Other Output Formats

### --timeline (Chronological BREAKDOWN.md — Best for Agents)

Generates a Markdown file that interleaves **all events** chronologically:
logs, conversation turns, tool calls, handoffs, and latency breakdowns.
Every assistant turn includes inline metrics, E2E breakdowns, and `[SLOW: ...]`
annotations. This is the format described in `docs/agent-friendly-debugging.md`.

```markdown
# Call Breakdown: RM_DRewYSCiKoEK

**Verdict: PROBLEMATIC — 28 slow turns, 2 errors, 69 warnings**

| Field | Value |
|---|---|
| Room ID | RM_DRewYSCiKoEK |
| Duration | 18:28.3 |
| Agent | onboarding-agent-production |
| ...   | ... |

## Pipeline

Response time: **2.2s avg** (max 4.7s) — slow

| Stage | Avg | % of total | Verdict |
|---|---|---|---|
| LLM (TTFT) | 876ms | 39% | normal |
| TTS (TTFB) | 380ms | 17% | fast |

Bottleneck: **Overhead/gaps dominate**

## Timeline

### 0.00s — LOG [WARN] dating-onboarding
> ENTRYPOINT V2 - 2026-02-16

### Turn 1 — HANDOFF to GreetingAgent

### Turn 2 — ASSISTANT
- E2E: 633ms | LLM: 633ms | TTS: 218ms

> Hey, you're here because swiping isn't cutting it anymore.

---

### Turn 3 — USER
> Ready.

---

### Turn 4 — TOOL: start_interview()
- Output: ok

---

### Turn 9 — ASSISTANT
- **E2E: 4121ms** | LLM: 1034ms | TTS: 445ms
- Breakdown: stt=225ms -> eol=751ms -> llm1=1839ms -> tool=1ms[record_answer] -> llm2=1034ms -> tts=445ms
- **[SLOW: overhead — llm1=1839ms deciding to call tool]**

> Mumbai and New York... that's a mix.

## Errors (2)
| Time | Source | Message |
|---:|---|---|
| 0.50s | webhook | Webhook client error: 404 |

## Warnings (69 total, grouped by pattern)
| Count | Pattern | Source | First | Last |
|---:|---|---|---|---|
| 19 | Section budget exhausted | dating-onboarding | 9:23.3 | 18:01.7 |

## Latency Stats
| Metric | Avg | Min | Max | P95 |
|---|---|---|---|---|
| E2E | 2229ms | 960ms | 4738ms | 4121ms |
```

**Key features for agents:**
- Everything in one file, chronological order
- Slow turns marked with `**[SLOW: reason]**`
- Tool calls show inline with arguments and output
- Logs interleaved at their exact timestamp
- Breakdowns show the full pipeline: `stt -> eol -> llm1 -> tool -> llm2 -> tts`

### --transcript (Conversation Only)
```
# TRANSCRIPT (12 turns)
# Duration: 1:45.30

[1] USER
    Hi, I need help with my order

[2] ASSISTANT (e2e=1200ms, llm=800ms, tts=350ms)
    Hello! I'd be happy to help you with your order...
```

### --logs (All Logs)
```
# LOGS (156 total, 2 errors, 8 warnings)

   0.15s [INFO]  livekit.agents.worker | Agent connected to room
   1.23s [WARN]  livekit.agents.tts | TTS queue backpressure
  45.80s [ERROR] livekit.agents.llm | Request timeout after 5000ms
```

### --spans (All Spans with Timing)
```
# SPANS (234 total)

  START    DUR(ms)  NAME                            SPAN_ID
--------------------------------------------------------------------------------
   0.00s      45ms  agent_session                 * abc123...
   0.05s    1200ms  agent_turn                    * def456...
   0.06s     800ms  llm_node                      * ghi789...
```

### --dump (Everything Combined)
Best for comprehensive analysis. Includes:
- Summary (key=value)
- Transcript
- Tool calls
- Errors
- Warnings
- Key spans

```bash
# Save full dump for later analysis
livekit-analyzer ./traces --dump > analysis.txt
```

## Input Data Structure

The tool expects a folder containing:
- `logs.json` - Agent logs with timestamps, severity, messages
- `spans.json` - OpenTelemetry spans with timing data

These are exported from LiveKit Cloud or the agent's observability API.

## Output Interpretation Guide

### 1. VERDICT (Top of Report)

```
[OK] HEALTHY CALL - No major issues detected
[!] NEEDS ATTENTION - X slow turns
[!!] PROBLEMATIC CALL - X slow turns, Y errors
```

**Action by verdict:**
- `HEALTHY` - No action needed
- `NEEDS ATTENTION` - Review slow turns, may need optimization
- `PROBLEMATIC` - Investigate errors and slow turns urgently

### 2. PIPELINE ANALYSIS (Most Important Section)

Shows where time is spent in the voice pipeline:

```
Response time: 2.3s avg (max 4.1s) - acceptable

Where time goes:
  LLM: 1.2s (52%) - normal
  TTS: 0.8s (35%) - fast
  Perception: 120ms (3 user turns) - good VAD

Bottleneck: LLM is primary bottleneck
```

**Key metrics:**
| Metric | Good | Warning | Critical |
|--------|------|---------|----------|
| Response time (total) | <4s | 4-8s | >8s |
| LLM (time to first token) | <1.5s | 1.5-3s | >3s |
| TTS (time to first byte) | <2s | 2-4s | >4s |
| Perception (VAD/EOL delay) | <100ms | 100-200ms | >200ms |

**Common bottlenecks and fixes:**

| Bottleneck | Cause | Fix |
|------------|-------|-----|
| LLM >50% | Slow model or large context | Use faster model, trim context, reduce system prompt |
| TTS >50% | Slow TTS provider | Switch provider, use streaming TTS, shorter responses |
| Perception >200ms | VAD not tuned | Adjust VAD sensitivity, check audio quality |
| Unknown gaps | Network or processing | Check server location, optimize code paths |

### 3. AUTOMATIC DIAGNOSIS

Shows specific slow turns grouped by cause:

```
[LLM] LLM BOTTLENECK: 3 turns
  Turn 5: E2E=3200ms -> LLM=2800ms
    "Let me help you with..."
  Turn 8: E2E=4100ms -> LLM=3500ms
    "Based on my analysis..."
```

**Interpreting causes:**
- `LLM` - Model inference is slow. Check model choice, context size
- `TTS` - Speech synthesis is slow. Check TTS provider, voice settings
- `TOOL` - Function calls are slow. Optimize external API calls
- `OVERHEAD` - Gaps between stages. Network or processing issues

### 4. LATENCY SUMMARY

Statistical view of latencies:

```
E2E Latency:    avg=1850ms  min=800ms  max=4100ms  p95=3200ms
LLM TTFT:       avg=1200ms  min=400ms  max=2800ms  p95=2400ms
TTS TTFB:       avg=600ms   min=200ms  max=1500ms  p95=1200ms
```

**What to look for:**
- High p95 = occasional spikes (likely cold starts or heavy requests)
- High avg = consistent slowness (systemic issue)
- Large max-min spread = inconsistent performance

### 5. LLM CONTEXT PER TURN

Shows context growth over conversation:

```
Turn |  LLM ms | Msgs | Chars | ~Tokens | Out chars | Response preview
-----|---------|------|-------|---------|-----------|------------------
   1 |     450 |    2 |  1200 |     300 |       150 | Hello! How can I...
   5 |    1800 |   12 |  8500 |    2125 |       200 | Based on our conv...
  10 |    3200 |   22 | 15000 |    3750 |       180 | Let me summarize...
```

**What to look for:**
- LLM ms increasing with turn number = context growth issue
- High token counts = need context trimming
- Solution: Implement sliding window, summarization, or RAG

### 6. CONVERSATION TRANSCRIPT

Full conversation with per-turn metrics:

```
[1] [USER] USER
    E2E:N/A  conf:98%
    Hi, I need help with my order

[2] [ASST] ASSISTANT
    E2E:1200ms  LLM:800ms  TTS:350ms  dur:2.3s
    Hello! I'd be happy to help you with your order...
```

**Flags to watch:**
- `[INTERRUPTED]` - User interrupted the agent (may indicate slow response)
- Low `conf` (<90%) - Poor transcription quality, check audio
- Missing metrics - Data collection issue

### 7. ERRORS AND WARNINGS

```
ERRORS
  [0:45.32] livekit.agents.tts
    TTS synthesis failed: timeout after 5000ms

WARNINGS
  [1:12.45] livekit.agents.llm
    Context truncated: exceeded 8000 tokens
```

**Common errors and fixes:**

| Error Pattern | Likely Cause | Fix |
|---------------|--------------|-----|
| TTS timeout | Provider overloaded | Add retries, fallback provider |
| Context truncated | Too much history | Implement context management |
| Tool execution failed | External API issue | Add error handling, timeouts |
| STT low confidence | Audio quality | Check microphone, noise |

### 8. TOOL CALLS

```
Summary: 5 calls, 2 unique tools
  get_order_status: 3x
  update_order: 2x

Timeline:
  [0:15.20] get_order_status (150ms)
  [0:45.80] get_order_status (2300ms)  <- Slow!
```

**What to look for:**
- Tool calls >500ms are slow
- Multiple calls to same tool = possible optimization opportunity
- Tool calls during slow turns = likely cause of latency

## JSON Output Schema

For programmatic analysis, use `--format json`:

```json
{
  "metadata": {
    "room_id": "...",
    "duration_sec": 120.5,
    ...
  },
  "summary": {
    "total_turns": 15,
    "user_turns": 7,
    "errors": 2,
    ...
  },
  "diagnosis": {
    "verdict": "needs_attention",
    "primary_issue": "LLM latency",
    "slow_turns_count": 3,
    ...
  },
  "latency": {
    "e2e": { "avg_ms": 1850, "p95_ms": 3200, ... },
    "llm_ttft": { ... },
    "tts_ttfb": { ... }
  },
  "turns": [...],
  "high_latency_turns": [...],
  "errors": [...]
}
```

## LiveKit Cloud Integration (Experimental)

Fetch sessions and download observability data directly from LiveKit Cloud.
Credentials are read from `~/.livekit/cli-config.yaml` (shared with `lk` CLI).

### Prerequisites

1. Install the LiveKit CLI: `brew install livekit-cli` (or see [docs](https://docs.livekit.io/home/cli/cli-setup/))
2. Authenticate: `lk cloud auth`
3. For **download**: a browser session token (see below)

### Commands

```bash
# List configured projects (* = default)
livekit-analyzer cloud projects

# List recent sessions
livekit-analyzer cloud sessions
livekit-analyzer cloud sessions --limit 10 --page 2
livekit-analyzer cloud sessions --json

# Show session details (participants, timing, region)
livekit-analyzer cloud info RM_bMvTTdAVKvmW

# Download observability data (logs, traces, audio, chat history)
livekit-analyzer cloud download RM_bMvTTdAVKvmW
livekit-analyzer cloud download RM_bMvTTdAVKvmW -o ./my-session
livekit-analyzer cloud download RM_bMvTTdAVKvmW --token <SESSION_TOKEN>

# Download + analyze in one pipeline
livekit-analyzer cloud download RM_xxx -o ./session && livekit-analyzer ./session --dump
```

### Options

| Flag | Description |
|------|-------------|
| `-p, --project <NAME>` | Project name (default: from config) |
| `-t, --token <TOKEN>` | Session token for download |
| `--limit <N>` | Max sessions to list (default: 20) |
| `--page <N>` | Page number (default: 0) |
| `--json` | JSON output for sessions list |
| `-o, --output <DIR>` | Output directory for download |

### Authentication

Two auth mechanisms are used:

| Command | Auth | Source |
|---------|------|--------|
| `projects`, `sessions`, `info` | JWT (automatic) | `~/.livekit/cli-config.yaml` |
| `download` | Session token | Browser cookie / `--token` / `LK_CLOUD_TOKEN` env |

**Getting the session token** (required for download):
1. Log in to https://cloud.livekit.io
2. Open DevTools (F12) → Application → Cookies → `cloud.livekit.io`
3. Find `__Secure-authjs.browser-session-token`
4. Copy the value

The token is saved to `~/.livekit/session-token` after first use. If it expires,
the tool automatically re-prompts. You can also set the `LK_CLOUD_TOKEN` env var.

### Download Output

The `download` command saves these files (renamed from ZIP for analyzer compatibility):

| File | Description |
|------|-------------|
| `metadata.json` | Session info (participants, timing, bandwidth) |
| `spans.json` | OpenTelemetry traces (renamed from `*_traces.json`) |
| `logs.json` | Agent logs (renamed from `*_logs.json`) |
| `audio.oga` | Session audio recording |
| `chat_history.json` | Conversation transcript |

### Agent Workflow (Autonomous)

For AI agents to autonomously analyze a LiveKit session:

```bash
# Step 1: Find the session
livekit-analyzer cloud sessions --json | jq '.[0].sessionId'

# Step 2: Download it
livekit-analyzer cloud download RM_xxx -o ./session-data

# Step 3: Analyze
livekit-analyzer ./session-data --summary

# Step 4: Deep dive if needed
livekit-analyzer ./session-data --dump
```

The `projects`, `sessions`, and `info` commands work fully automatically with
just `lk cloud auth`. Only `download` requires the one-time browser token setup.

## Analysis Workflow for Agents

### Step 1: Get Quick Assessment
```bash
livekit-analyzer ./traces --format text | head -50
```
Look at verdict and pipeline analysis.

### Step 2: Identify Primary Issue
Check AUTOMATIC DIAGNOSIS section for specific slow turns and causes.

### Step 3: Deep Dive
- If LLM slow: Check LLM CONTEXT PER TURN for context growth
- If TTS slow: Check for TTS errors in ERRORS section
- If tools slow: Check TOOL CALLS timeline

### Step 4: Check Conversation Quality
Review TRANSCRIPT for:
- Interrupted responses (UX issue)
- Low confidence transcriptions
- Unnatural conversation flow

### Step 5: Recommend Fixes
Based on findings, suggest:
- Model changes (faster/smaller)
- Context management (sliding window, summarization)
- Provider changes (different TTS/STT)
- Code optimizations (async tools, caching)

## Threshold Reference

### E2E Latency (User speaks → Agent speaks)
- Good: <500ms (feels instant)
- OK: 500-1500ms (noticeable but acceptable)
- Slow: 1500-2000ms (user notices delay)
- Bad: >2000ms (poor UX)

### LLM TTFT (Request → First token)
- Fast: <1500ms
- Normal: 1500-3000ms
- Slow: >3000ms

### TTS TTFB (Text → First audio byte)
- Fast: <2000ms
- Normal: 2000-4000ms
- Slow: >4000ms

### Perception Delay (User stops → LLM starts)
- Instant: <100ms
- Good: 100-200ms
- Slow: >200ms (VAD issue)

## Agent Diagnosis Workflow

When analyzing a call, follow this sequence:

### Step 1: Quick Triage
```bash
livekit-analyzer ./traces --summary
```

Check:
- `verdict` - HEALTHY/NEEDS_ATTENTION/PROBLEMATIC
- `slow_turns` - How many turns were slow
- `errors` - Any errors occurred
- `bottleneck` - What's the main issue

### Step 2: If PROBLEMATIC - Get Full Context
```bash
livekit-analyzer ./traces --dump
```

This shows everything. Look for:

1. **ERRORS section** - Full error messages with stack traces
2. **WARNINGS section** - TTS retries, connection issues
3. **TRANSCRIPT** - Which turns were slow (look for high e2e/llm/tts values)
4. **KEY SPANS** - Timing breakdown of each stage

### Step 3: Common Issue Patterns

| Pattern in Summary | Look in Dump For | Root Cause |
|-------------------|------------------|------------|
| `slow_llm_turns > 0` | Transcript turns with high `llm=` | LLM model/context issue |
| `tts_retries > 0` | Warnings about "failed to synthesize" | TTS provider issue |
| `tool_errors > 0` | ERRORS section with traceback | Bug in tool implementation |
| `bottleneck=TTS` | Spans showing long `tts_node` | TTS provider slow |
| High `perception_ms` | User turns timing | VAD configuration issue |
| Many `INTERRUPTED` | Transcript | Agent responding too slowly |

### Step 4: Specific Diagnosis Examples

**Tool Error Diagnosis:**
```
# In ERRORS section:
246.03s  probook-api-JCJFOR | [PROBOOK_FAILURE] AssertionError | Error creating new Service Titan account
```
→ Tool `create_new_customer_profile_tool` failed
→ Look at traceback: `AssertionError` in phone number validation
→ Fix: validate phone number format before calling tool

**TTS Retry Diagnosis:**
```
# In WARNINGS:
121.27s  livekit.agents | failed to synthesize speech, retrying in 0.1s
121.38s  livekit.agents | failed to synthesize speech, retrying in 2.0s
```
→ TTS provider is failing
→ Check TTS provider status, consider fallback

**LLM Latency Diagnosis:**
```
# In summary:
slow_llm_turns=6
llm_avg_ms=995
llm_p95_ms=1984
```
→ LLM is slow on some turns
→ Check transcript for which turns have high `llm=` values
→ Likely cause: context growing too large, or complex queries

### Step 5: Report Findings

Structure your findings as:
```
## Call Analysis: [room_id]

**Verdict**: [HEALTHY/PROBLEMATIC/NEEDS_ATTENTION]

**Issues Found**:
1. [Issue type]: [Description]
   - Evidence: [What you saw in the data]
   - Impact: [How it affected the call]
   - Recommendation: [How to fix]

**Metrics**:
- E2E latency: avg Xms, p95 Xms
- Slow turns: X of Y total
- Errors: X
```

## PCAP Network Analysis (--pcap)

Analyzes network captures for SIP/RTP quality metrics.

### Output Format
```
# PCAP NETWORK ANALYSIS
packets=2589
duration_sec=36.15
quality_score=97

## CALL SETUP
call_setup_ms=10178
call_setup_verdict=very slow
media_setup_ms=49

## RTP STREAMS
stream_0_direction=outgoing
stream_0_packets=1296
stream_0_duration_sec=25.9
stream_0_pps=50.0
stream_0_jitter_avg_ms=0.1
stream_0_jitter_max_ms=19.9
stream_0_loss_pct=0.00
stream_0_lost_packets=0
stream_0_codec=G.729 (20ms)

## ISSUES DETECTED
issue: Slow call setup: 10178ms (expected <3s)

## SIP SIGNALING TIMELINE
   0.00s  INVITE           +16462826210 -> +18482666247
   0.00s  100 Processing   ...
  10.18s  200 OK           ...
  36.15s  BYE              ...
```

### Key Network Metrics

| Metric | Good | Warning | Critical |
|--------|------|---------|----------|
| Call setup time | <2s | 2-5s | >5s |
| Media setup (after 200 OK) | <200ms | 200-500ms | >500ms |
| Jitter (avg) | <10ms | 10-30ms | >30ms |
| Jitter (max) | <50ms | 50-100ms | >100ms |
| Packet loss | <0.1% | 0.1-1% | >1% |
| Quality score | 90-100 | 70-90 | <70 |

### Network Issue Patterns

| Issue | Evidence | Root Cause | Fix |
|-------|----------|------------|-----|
| Slow call setup | Many 180 Ringing, >5s setup | Agent slow to answer | Check agent startup, reduce greeting latency |
| High jitter | jitter_avg >30ms | Network congestion | Check network path, use TURN server |
| Packet loss | loss_pct >1% | Network issues | Check connectivity, bandwidth |
| Out-of-order packets | out_of_order >0 | Network routing | Use better network path |
| No RTP after 200 OK | media_setup >500ms | ICE/DTLS issues | Check firewall, TURN config |

### SIP Error Codes

| Code | Meaning | Action |
|------|---------|--------|
| 180 Ringing | Call is ringing | Normal (but many = slow answer) |
| 200 OK | Call answered | Good |
| 408 Timeout | Request timeout | Check network, server availability |
| 486 Busy | Callee busy | Normal |
| 503 Service Unavailable | Server overloaded | Scale infrastructure |

## Combined Analysis (Traces + PCAP)

When you have both observability traces and PCAP:

```bash
livekit-analyzer ./traces.zip ./call.pcap --dump
```

This gives you:
1. **Agent performance** (from traces): LLM latency, TTS timing, errors
2. **Network quality** (from PCAP): SIP timing, RTP jitter, packet loss

### Correlation Examples

| Traces Show | PCAP Shows | Diagnosis |
|-------------|------------|-----------|
| High E2E latency | Good network | Agent-side issue (LLM/TTS) |
| Good E2E latency | High jitter | Network affecting audio quality |
| Interrupted responses | Packet loss | User couldn't hear agent |
| Slow first response | Slow call setup | Agent taking long to start |
| TTS errors | RTP stream stops | Network/server issue |

## Example Analysis Session

```
User: Analyze this call, it felt slow

Agent: Let me analyze the traces.

$ livekit-analyzer ./call-traces --format text

Reading the output:
- Verdict: NEEDS ATTENTION - 4 slow turns
- Pipeline: LLM is 58% of time (avg 2.1s)
- Diagnosis: LLM BOTTLENECK on turns 3, 5, 8, 12

The call has LLM latency issues. Looking at context growth:
- Turn 1: 300 tokens
- Turn 12: 4200 tokens

Root cause: Context growing too large over conversation.

Recommendations:
1. Implement sliding window context (keep last 10 messages)
2. Or use summarization for older messages
3. Consider a faster model for simple queries
```

## Complete Feature Reference

### Trace Analysis Features
- Conversation transcript with timing
- E2E, LLM, TTS latency breakdown
- Pipeline bottleneck identification
- Slow turn diagnosis by cause
- Tool call analysis
- Error and warning extraction
- Context growth tracking
- Interruption detection

### PCAP Analysis Features
- SIP signaling timeline
- Call setup time measurement
- RTP stream detection
- Jitter calculation (avg, max)
- Packet loss detection
- Out-of-order packet detection
- Codec identification
- Quality score (0-100)
- Media setup time

### Output Formats
| Format | Flag | Best For |
|--------|------|----------|
| Summary | `--summary` | Quick triage, scripts |
| Dump | `--dump` | Full diagnosis |
| Text | `-r` | Human reading |
| JSON | `--json` | Programmatic use |
| Transcript | `--transcript` | Conversation review |
| Timeline | `--timeline` | Agent-friendly BREAKDOWN.md |
| Logs | `--logs` | Error investigation |
| Spans | `--spans` | Timing deep-dive |
| PCAP | `--pcap` | Network-only analysis |

## Release Workflow

To release a new version:

1. **Bump version** in `Cargo.toml` (field `version`)
2. **Verify build**: `cargo build --release`
3. **Stage changed files**: `git add <files>` (include `Cargo.toml`)
4. **Commit**: `git commit -m "feat: <description>"`
5. **Tag**: `git tag -a vX.Y.Z -m "vX.Y.Z - <short description>"`
6. **Push with tag**: `git push origin main --tags`
7. **Create GitHub release**: `gh release create vX.Y.Z --generate-notes`

Version follows semver: bump **patch** for fixes, **minor** for features, **major** for breaking changes.
