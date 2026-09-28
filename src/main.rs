use std::io::stdout;
use std::path::PathBuf;
use std::panic;
use clap::{Parser, Subcommand};
use crossterm::{
    event::DisableMouseCapture,
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

mod analysis;
mod cli;
mod fs;
mod ui;
mod updater;

use ui::app::App;
use ui::events::handle_events;
use ui::render::render_ui;

#[derive(Parser, Debug)]
#[command(name = "spacer", author, version, about = "⚡ Terminal storage space manager with smart waste prioritization, quick wins cleaner, duplicate detection & treemap")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Directory path to analyze and manage
    #[arg(default_value = ".")]
    path: PathBuf,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Check for updates and update spacer to the latest release
    Update,

    /// Scan and clean up safe-to-delete developer artifacts and caches
    Clean {
        /// Target path to scan (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Perform a dry run without deleting anything
        #[arg(short, long)]
        dry_run: bool,

        /// Bypass interactive confirmation prompt
        #[arg(short, long)]
        force: bool,
    },

    /// Find duplicate files and optionally deduplicate with hardlinks
    Dupes {
        /// Target path to scan (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Automatically replace all duplicate copies with hardlinks
        #[arg(short, long)]
        hardlink: bool,
    },

    /// List top largest storage consumers and waste candidates
    Top {
        /// Target path to scan (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Number of top items to display
        #[arg(short, long, default_value_t = 20)]
        count: usize,
    },

    /// Export storage hierarchy and scorecards to JSON or CSV
    Export {
        /// Target path to scan (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Path to export JSON file
        #[arg(long)]
        json: Option<PathBuf>,

        /// Path to export CSV file
        #[arg(long)]
        csv: Option<PathBuf>,
    },
}

fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    // Check for CLI subcommands
    if let Some(cmd) = args.command {
        match cmd {
            Commands::Update => return updater::cli::run_cli_update(),
            Commands::Clean { path, dry_run, force } => {
                return cli::clean::run_clean_command(&path, dry_run, force);
            }
            Commands::Dupes { path, hardlink } => {
                return cli::dupes::run_dupes_command(&path, hardlink);
            }
            Commands::Top { path, count } => {
                return cli::top::run_top_command(&path, count);
            }
            Commands::Export { path, json, csv } => {
                if json.is_none() && csv.is_none() {
                    eprintln!("Please specify --json <file> or --csv <file> to export.");
                    return Ok(());
                }
                return cli::export::run_export_command(&path, json.as_deref(), csv.as_deref());
            }
        }
    }

    // Set up safe panic hook to restore terminal on unexpected errors
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), LeaveAlternateScreen, DisableMouseCapture);
        original_hook(panic_info);
    }));

    // Setup terminal
    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Create App instance
    let mut app = App::new(args.path);

    // Main event loop
    while !app.should_quit {
        // Poll scanner background updates & update checker
        app.process_scan_messages();

        // Render TUI
        terminal.draw(|frame| {
            render_ui(frame, &app);
        })?;

        // Process inputs
        if let Err(e) = handle_events(&mut app) {
            eprintln!("Error handling events: {}", e);
            break;
        }
    }

    // Restore terminal
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture)?;
    terminal.show_cursor()?;

    Ok(())
}
