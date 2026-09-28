use std::io::stdout;
use std::path::PathBuf;
use std::panic;
use clap::builder::styling::{AnsiColor, Effects, Styles};
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

const CLAP_STYLES: Styles = Styles::styled()
    .header(AnsiColor::Cyan.on_default().effects(Effects::BOLD))
    .usage(AnsiColor::Yellow.on_default().effects(Effects::BOLD))
    .literal(AnsiColor::Green.on_default().effects(Effects::BOLD))
    .placeholder(AnsiColor::Cyan.on_default())
    .valid(AnsiColor::BrightGreen.on_default())
    .invalid(AnsiColor::BrightRed.on_default());

const HELP_BANNER: &str = "\
\x1b[1;36m  ___ _ __   __ _  ___ ___ _ __ \x1b[0m
\x1b[1;36m / __| '_ \\ / _` |/ __/ _ \\ '__|\x1b[0m
\x1b[1;36m \\__ \\ |_) | (_| | (_|  __/ |   \x1b[0m
\x1b[1;36m |___/ .__/ \\__,_|\\___\\___|_|   \x1b[0m
\x1b[1;36m     |_|\x1b[0m  \x1b[1;32mv0.2.0\x1b[0m — \x1b[2mTerminal Storage Manager & Deduplicator in Rust\x1b[0m";

const HELP_EXAMPLES: &str = "\
\x1b[1;36mEXAMPLES:\x1b[0m
  \x1b[1;32mspacer\x1b[0m                         Launch interactive TUI in current directory
  \x1b[1;32mspacer ~\x1b[0m                       Scan and manage entire home directory
  \x1b[1;32mspacer clean --dry-run\x1b[0m         Inspect recoverable cache and build junk
  \x1b[1;32mspacer clean --path ~ --force\x1b[0m  Clean junk in home directory without prompts
  \x1b[1;32mspacer dupes ~ --hardlink\x1b[0m      Find duplicates and deduplicate with hardlinks (ln -f)
  \x1b[1;32mspacer top 20 --path /var\x1b[0m      List the 20 largest storage consumers in /var
  \x1b[1;32mspacer export --json out.json\x1b[0m  Export storage audit hierarchy and waste scores
  \x1b[1;32mspacer update\x1b[0m                  Check and update spacer to latest release

\x1b[1;36mKEYBOARD SHORTCUTS IN TUI:\x1b[0m
  \x1b[1;33mSpace\x1b[0m Stage/unstage   \x1b[1;33md\x1b[0m Move to Trash   \x1b[1;33mD\x1b[0m Permanent delete   \x1b[1;33mt\x1b[0m Treemap view
  \x1b[1;33mw\x1b[0m     Quick Wins      \x1b[1;33mF\x1b[0m Dupe finder     \x1b[1;33ms\x1b[0m Cycle sorting      \x1b[1;33mq\x1b[0m Quit";

#[derive(Parser, Debug)]
#[command(
    name = "spacer",
    author,
    version,
    about = "⚡ Fast terminal storage manager with waste prioritization, duplicate detection & treemaps",
    before_help = HELP_BANNER,
    after_help = HELP_EXAMPLES,
    styles = CLAP_STYLES,
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Directory path to analyze and manage in interactive TUI mode
    #[arg(default_value = ".")]
    path: PathBuf,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Check for updates and update spacer in-place to latest GitHub release
    Update,

    /// Scan and clean up safe-to-delete developer artifacts, logs, and caches
    Clean {
        /// Target path to scan (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Perform a dry run to inspect recoverable size without deleting
        #[arg(short, long)]
        dry_run: bool,

        /// Bypass interactive confirmation prompts
        #[arg(short, long)]
        force: bool,
    },

    /// Find duplicate files using 3-stage hashing and optionally deduplicate with hardlinks
    Dupes {
        /// Target path to scan (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Automatically replace all duplicate copies with hardlinks (ln -f)
        #[arg(short = 'H', long)]
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

    /// Export storage hierarchy and scorecards to JSON or CSV for reporting
    Export {
        /// Target path to scan (defaults to current directory)
        #[arg(default_value = ".")]
        path: PathBuf,

        /// Path to export structured JSON file
        #[arg(long)]
        json: Option<PathBuf>,

        /// Path to export tabular CSV file
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
                    eprintln!("\x1b[1;33m! Error:\x1b[0m Please specify \x1b[32m--json <file>\x1b[0m or \x1b[32m--csv <file>\x1b[0m to export.");
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
