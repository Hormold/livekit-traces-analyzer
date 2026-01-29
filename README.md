# LiveKit Traces Analyzer

Interactive TUI for analyzing LiveKit voice agent call observability data. Helps diagnose latency issues, understand pipeline timing, and debug voice agent performance.

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

Export traces from your LiveKit agent using the observability API or download from the LiveKit Cloud dashboard.

## Features

- **Overview**: Pipeline timing breakdown, bottleneck identification
- **Transcript**: Full conversation with timestamps
- **Latency**: Per-turn E2E, LLM, TTS latency analysis
- **Charts**: Visual latency distribution (ASCII)
- **Agents**: Agent session and state transitions
- **Tools**: Function/tool call history and durations
- **Context**: LLM prompts and responses
- **Logs**: Errors and warnings
- **Spans**: OpenTelemetry span timeline

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

## License

MIT
