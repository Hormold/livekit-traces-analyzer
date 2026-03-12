# LiveKit Traces Analyzer

Interactive TUI and CLI for analyzing LiveKit voice agent call observability data. Optimized for AI agents to quickly find where the problem is -- provides detailed E2E latency breakdowns showing exactly where time is spent (STT, VAD/EOL detection, LLM tool-decision calls, tool execution, LLM response generation, TTS).

## Installation

### One-liner (macOS/Linux)

```bash
curl -sSL https://raw.githubusercontent.com/Hormold/livekit-traces-analyzer/main/install.sh | bash
```

### Manual Download

Download the latest binary for your platform from [Releases](https://github.com/Hormold/livekit-traces-analyzer/releases):

| Platform | Download |
|----------|----------|
| macOS (Apple Silicon) | `livekit-analyzer-macos-arm64` |
| macOS (Intel) | `livekit-analyzer-macos-x86_64` |
| Linux (x86_64) | `livekit-analyzer-linux-x86_64` |
| Windows | `livekit-analyzer-windows-x86_64.exe` |

### From Source

```bash
cargo install --git https://github.com/Hormold/livekit-traces-analyzer
```

Or clone and build:

```bash
git clone https://github.com/Hormold/livekit-traces-analyzer
cd livekit-traces-analyzer
cargo build --release
./target/release/livekit-analyzer <path>
```

## Usage

```bash
# TUI mode (interactive)
livekit-analyzer /path/to/observability-folder

# Text report (for CI/scripts)
livekit-analyzer /path/to/observability-folder --format text

# JSON report
livekit-analyzer /path/to/observability-folder --format json
```

## Getting Observability Data

Export traces from your LiveKit agent using the observability API, download from the LiveKit Cloud dashboard, or use the built-in cloud integration:

```bash
# List your projects
livekit-analyzer cloud projects

# List recent sessions
livekit-analyzer cloud sessions

# Download session data (logs, traces, audio)
livekit-analyzer cloud download RM_bMvTTdAVKvmW -o ./session-data

# Analyze it
livekit-analyzer ./session-data --dump
```

Requires `lk cloud auth` first. See [Cloud Integration](#cloud-integration) below.

## Features

- **E2E Breakdown**: Per-turn latency waterfall showing STT -> EOL -> LLM1 (tool decision) -> Tool exec -> LLM2 (response) -> TTS
- **Overview**: Pipeline timing breakdown, bottleneck identification
- **Transcript**: Full conversation with per-turn metrics and inline breakdowns
- **Latency**: Per-turn E2E, LLM, TTS latency analysis with slow turn diagnosis
- **Charts**: Visual latency distribution (ASCII)
- **Agents**: Agent session and state transitions
- **Tools**: Function/tool call history and durations
- **Context**: LLM prompts and responses
- **Logs**: Errors and warnings
- **Spans**: OpenTelemetry span timeline
- **PCAP**: SIP signaling and RTP quality analysis
- **Cloud**: Fetch sessions and download data directly from LiveKit Cloud

## Keyboard Shortcuts

| Key | Action |
|-----|--------|
| `1-9` | Jump to tab |
| `Tab` | Next tab |
| `j/k` | Scroll down/up |
| `Ctrl+d/u` | Page down/up |
| `f` | Toggle filter (Logs/Spans) |
| `s` | Toggle sort (Latency) |
| `?` | Help |
| `q` | Quit |

## Cloud Integration

Fetch sessions and download observability data directly from LiveKit Cloud. Credentials are read from `~/.livekit/cli-config.yaml` (shared with the `lk` CLI).

### Setup

```bash
# 1. Install LiveKit CLI
brew install livekit-cli

# 2. Authenticate with LiveKit Cloud
lk cloud auth
```

### Commands

```bash
# List configured projects (* = default)
livekit-analyzer cloud projects

# List recent sessions (supports --limit, --page, --json)
livekit-analyzer cloud sessions
livekit-analyzer cloud sessions --project my-project --limit 10 --json

# Show session details
livekit-analyzer cloud info RM_bMvTTdAVKvmW

# Download observability data
livekit-analyzer cloud download RM_bMvTTdAVKvmW -o ./session-data
```

### Download Authentication

The `download` command requires a browser session token (the REST listing commands work automatically with API keys).

On first run, you'll be prompted to paste your token:

```
Session token required for downloading observability data.

To get it:
  1. Open https://cloud.livekit.io and log in
  2. Open DevTools (F12) → Application → Cookies → cloud.livekit.io
  3. Find `__Secure-authjs.browser-session-token`
  4. Double-click the Value column and copy it

Paste token: <paste here>
```

The token is saved to `~/.livekit/session-token` for reuse. If it expires, the tool re-prompts automatically.

You can also pass it explicitly:
```bash
# Via flag
livekit-analyzer cloud download RM_xxx --token <TOKEN>

# Via environment variable
export LK_CLOUD_TOKEN=<TOKEN>
livekit-analyzer cloud download RM_xxx
```

### Full Workflow Example

```bash
# Find a session
livekit-analyzer cloud sessions --limit 5

# Download it
livekit-analyzer cloud download RM_YVBwPvfznypc -o ./call-data

# Quick triage
livekit-analyzer ./call-data --summary

# Full analysis
livekit-analyzer ./call-data --dump
```

## License

MIT
