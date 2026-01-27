//! LiveKit Call Analyzer - Interactive TUI for analyzing call observability data.

mod analysis;
mod app;
mod data;
mod events;
mod parser;
mod report;
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
    folder: PathBuf,
    report_mode: ReportMode,
    output_file: Option<PathBuf>,
}

/// Report output mode.
#[derive(Debug, Clone, PartialEq)]
enum ReportMode {
    Tui,
    Text,
    Json,
}

fn print_usage(program: &str) {
    eprintln!("Usage: {} [OPTIONS] <observability_folder>", program);
    eprintln!();
    eprintln!("Interactive TUI for analyzing LiveKit call observability data.");
    eprintln!();
    eprintln!("Options:");
    eprintln!("  -r, --report        Output text report to stdout (no TUI)");
    eprintln!("  --json              Output JSON report to stdout (no TUI)");
    eprintln!("  -o, --output <file> Write report to file instead of stdout");
    eprintln!("  -h, --help          Show this help message");
    eprintln!();
    eprintln!("Examples:");
    eprintln!("  {} ../observability-RM_3hhNSntAu8JG", program);
    eprintln!("  {} -r ../observability-RM_3hhNSntAu8JG", program);
    eprintln!("  {} --json -o report.json ../observability-RM_3hhNSntAu8JG", program);
}

fn parse_args() -> Result<CliOptions, String> {
    let args: Vec<String> = env::args().collect();
    let program = &args[0];

    let mut report_mode = ReportMode::Tui;
    let mut output_file: Option<PathBuf> = None;
    let mut folder: Option<PathBuf> = None;

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
                if folder.is_some() {
                    return Err("Multiple folders specified".to_string());
                }
                folder = Some(PathBuf::from(arg));
            }
        }
        i += 1;
    }

    let folder = folder.ok_or_else(|| "No observability folder specified".to_string())?;

    Ok(CliOptions {
        folder,
        report_mode,
        output_file,
    })
}

fn main() -> Result<()> {
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

    if !options.folder.exists() {
        eprintln!("Error: Folder not found: {}", options.folder.display());
        std::process::exit(1);
    }

    if !options.folder.is_dir() {
        eprintln!("Error: Path is not a directory: {}", options.folder.display());
        std::process::exit(1);
    }

    // Handle report mode vs TUI mode
    match options.report_mode {
        ReportMode::Tui => {
            // Load and analyze the data for TUI
            let app = App::load(&options.folder)
                .with_context(|| format!("Failed to analyze folder: {}", options.folder.display()))?;

            // Run the TUI
            run_tui(app)?;
        }
        ReportMode::Text | ReportMode::Json => {
            // Load and analyze the data
            let analysis = analyze_call(&options.folder)
                .with_context(|| format!("Failed to analyze folder: {}", options.folder.display()))?;

            // Generate report
            let report = match options.report_mode {
                ReportMode::Text => {
                    // Use no-color version if writing to file
                    if options.output_file.is_some() {
                        generate_text_report_no_color(&analysis)
                    } else {
                        generate_text_report(&analysis)
                    }
                }
                ReportMode::Json => generate_json_report(&analysis),
                _ => unreachable!(),
            };

            // Output report
            match options.output_file {
                Some(ref path) => {
                    fs::write(path, &report)
                        .with_context(|| format!("Failed to write report to: {}", path.display()))?;
                    eprintln!("Report written to: {}", path.display());
                }
                None => {
                    println!("{}", report);
                }
            }
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
