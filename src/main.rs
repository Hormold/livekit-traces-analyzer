//! LiveKit Call Analyzer - Interactive TUI for analyzing call observability data.

mod analysis;
mod app;
mod cloud;
mod data;
mod events;
mod format;
mod input;
mod parser;
mod pcap;
mod report;
mod thresholds;
mod timeline;
mod ui;

use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use analysis::analyze_call;
use app::App;
use events::{handle_key_event, EventResult};
use report::{generate_json_report, generate_text_report, generate_text_report_no_color};

/// Command line options.
struct CliOptions {
    /// Input paths (folders, ZIPs, or PCAPs)
    inputs: Vec<PathBuf>,
    report_mode: ReportMode,
    output_file: Option<PathBuf>,
}

/// Report output mode.
#[derive(Debug, Clone, PartialEq)]
enum ReportMode {
    Tui,
    Text,
    Json,
    Summary,
    Logs,
    Spans,
    Transcript,
    Dump,
    Timeline,
    Pcap,
}

fn print_usage(program: &str) {
    eprintln!("Usage: {} [OPTIONS] <input> [<input>...]", program);
    eprintln!();
    eprintln!("Interactive TUI for analyzing LiveKit call observability data.");
    eprintln!();
    eprintln!("Supported Inputs:");
    eprintln!("  folder/             Observability folder (logs.json, spans.json)");
    eprintln!("  file.zip            ZIP archive (auto-extracts)");
    eprintln!("  file.pcap           Network capture (SIP/RTP analysis)");
    eprintln!();
    eprintln!("Output Formats:");
    eprintln!("  (default)           Interactive TUI");
    eprintln!("  -r, --report        Full text report (human-readable)");
    eprintln!("  --json              Structured JSON report");
    eprintln!("  --summary           Key metrics only (agent-friendly, key=value)");
    eprintln!("  --logs              All logs with timestamps");
    eprintln!("  --spans             All spans with timing");
    eprintln!("  --transcript        Conversation transcript only");
    eprintln!("  --dump              Everything: summary + transcript + logs + spans");
    eprintln!("  --timeline          Chronological BREAKDOWN.md (agent-optimized)");
    eprintln!("  --pcap              PCAP analysis only (SIP/RTP)");
    eprintln!();
    eprintln!("Other Options:");
    eprintln!("  -o, --output <file> Write report to file instead of stdout");
    eprintln!("  -h, --help          Show this help message");
    eprintln!();
    eprintln!("Cloud (experimental):");
    eprintln!("  cloud projects      List LiveKit Cloud projects");
    eprintln!("  cloud sessions      List recent sessions");
    eprintln!("  cloud download <ID> Download session observability data");
    eprintln!("  cloud --help        Full cloud help");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  {} ./observability-RM_xxx", program);
    eprintln!("  {} ./traces.zip", program);
    eprintln!("  {} ./traces.zip ./call.pcap", program);
    eprintln!("  {} --summary ./observability-RM_xxx", program);
    eprintln!("  {} --dump ./traces.zip", program);
}

fn parse_args() -> Result<CliOptions, String> {
    let args: Vec<String> = env::args().collect();
    let program = &args[0];

    let mut report_mode = ReportMode::Tui;
    let mut output_file: Option<PathBuf> = None;
    let mut inputs: Vec<PathBuf> = Vec::new();

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "-h" | "--help" => {
                print_usage(program);
                std::process::exit(0);
            }
            "-r" | "--report" => {
                report_mode = ReportMode::Text;
            }
            "--json" => {
                report_mode = ReportMode::Json;
            }
            "--summary" => {
                report_mode = ReportMode::Summary;
            }
            "--logs" => {
                report_mode = ReportMode::Logs;
            }
            "--spans" => {
                report_mode = ReportMode::Spans;
            }
            "--transcript" => {
                report_mode = ReportMode::Transcript;
            }
            "--dump" => {
                report_mode = ReportMode::Dump;
            }
            "--timeline" => {
                report_mode = ReportMode::Timeline;
            }
            "--pcap" => {
                report_mode = ReportMode::Pcap;
            }
            "-o" | "--output" => {
                i += 1;
                if i >= args.len() {
                    return Err("--output requires a file path argument".to_string());
                }
                output_file = Some(PathBuf::from(&args[i]));
            }
            _ if arg.starts_with('-') => {
                return Err(format!("Unknown option: {}", arg));
            }
            _ => {
                inputs.push(PathBuf::from(arg));
            }
        }
        i += 1;
    }

    if inputs.is_empty() {
        return Err("No input specified".to_string());
    }

    Ok(CliOptions {
        inputs,
        report_mode,
        output_file,
    })
}

fn main() -> Result<()> {
    // Check for `cloud` subcommand before normal parsing
    let args: Vec<String> = env::args().collect();
    if args.len() > 1 && args[1] == "cloud" {
        let cloud_args = &args[2..];
        let cloud_opts = match cloud::parse_cloud_args(cloud_args) {
            Ok(opts) => opts,
            Err(e) if e == "show_help" => {
                cloud::print_cloud_help();
                std::process::exit(0);
            }
            Err(e) => {
                eprintln!("Error: {}", e);
                eprintln!();
                cloud::print_cloud_help();
                std::process::exit(1);
            }
        };
        return cloud::run(cloud_opts);
    }

    // Parse command line arguments
    let options = match parse_args() {
        Ok(opts) => opts,
        Err(e) => {
            eprintln!("Error: {}", e);
            eprintln!();
            print_usage(&env::args().next().unwrap_or_else(|| "livekit-analyzer".to_string()));
            std::process::exit(1);
        }
    };

    // Prepare inputs (auto-detect type, extract ZIPs, etc.)
    let prepared = input::prepare_input(&options.inputs)
        .with_context(|| "Failed to prepare input")?;

    // Handle PCAP-only mode
    if options.report_mode == ReportMode::Pcap {
        if let Some(ref pcap_path) = prepared.pcap_file {
            let pcap_analysis = pcap::parse_pcap(pcap_path)
                .with_context(|| format!("Failed to parse PCAP: {}", pcap_path.display()))?;
            let report = pcap::generate_pcap_report(&pcap_analysis);
            output_report(&report, &options.output_file)?;
            return Ok(());
        } else {
            eprintln!("Error: --pcap requires a PCAP file input");
            std::process::exit(1);
        }
    }

    // We need traces for other modes
    let traces_folder = prepared.traces_folder.as_ref()
        .ok_or_else(|| anyhow::anyhow!("No traces folder found. Provide a folder or ZIP with logs.json/spans.json"))?;

    // Handle report mode vs TUI mode
    match options.report_mode {
        ReportMode::Tui => {
            // Load and analyze the data for TUI
            let app = App::load(traces_folder)
                .with_context(|| format!("Failed to analyze folder: {}", traces_folder.display()))?;

            // Run the TUI
            run_tui(app)?;
        }
        ReportMode::Text | ReportMode::Json | ReportMode::Summary
        | ReportMode::Logs | ReportMode::Spans | ReportMode::Transcript
        | ReportMode::Dump | ReportMode::Timeline => {
            // Load and analyze the data
            let analysis = analyze_call(traces_folder)
                .with_context(|| format!("Failed to analyze folder: {}", traces_folder.display()))?;

            // Generate report
            let mut report = match options.report_mode {
                ReportMode::Text => {
                    // Use no-color version if writing to file
                    if options.output_file.is_some() {
                        generate_text_report_no_color(&analysis)
                    } else {
                        generate_text_report(&analysis)
                    }
                }
                ReportMode::Json => generate_json_report(&analysis),
                ReportMode::Summary => report::generate_summary_report(&analysis),
                ReportMode::Logs => report::generate_logs_report(&analysis),
                ReportMode::Spans => report::generate_spans_report(&analysis),
                ReportMode::Transcript => report::generate_transcript_report(&analysis),
                ReportMode::Dump => report::generate_dump_report(&analysis),
                ReportMode::Timeline => timeline::generate_timeline_report(&analysis),
                _ => unreachable!(),
            };

            // If we also have a PCAP, append its analysis
            if let Some(ref pcap_path) = prepared.pcap_file {
                if let Ok(pcap_analysis) = pcap::parse_pcap(pcap_path) {
                    report.push_str("\n\n");
                    report.push_str(&"=".repeat(80));
                    report.push_str("\n");
                    report.push_str(&pcap::generate_pcap_report(&pcap_analysis));
                }
            }

            output_report(&report, &options.output_file)?;
        }
        ReportMode::Pcap => unreachable!(), // Handled above
    }

    Ok(())
}

/// Output a report to stdout or file.
fn output_report(report: &str, output_file: &Option<PathBuf>) -> Result<()> {
    match output_file {
        Some(ref path) => {
            fs::write(path, report)
                .with_context(|| format!("Failed to write report to: {}", path.display()))?;
            eprintln!("Report written to: {}", path.display());
        }
        None => {
            println!("{}", report);
        }
    }
    Ok(())
}

fn run_tui(mut app: App) -> Result<()> {
    // Setup terminal
    enable_raw_mode().context("Failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("Failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("Failed to create terminal")?;

    // Event loop
    let result = event_loop(&mut terminal, &mut app);

    // Restore terminal
    disable_raw_mode().context("Failed to disable raw mode")?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .context("Failed to leave alternate screen")?;
    terminal.show_cursor().context("Failed to show cursor")?;

    result
}

fn event_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    loop {
        // Draw UI
        terminal.draw(|frame| {
            ui::render(frame, app);
        })?;

        // Poll for events with timeout
        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                // Only handle key press events (not release)
                if key.kind == KeyEventKind::Press {
                    let viewport_height = terminal.size()?.height.saturating_sub(4) as usize;

                    match handle_key_event(app, key, viewport_height) {
                        EventResult::Quit => break,
                        EventResult::Continue => {}
                    }
                }
            }
        }
    }

    Ok(())
}
