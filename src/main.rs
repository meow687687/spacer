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

mod fs;
mod ui;
mod updater;

use ui::app::App;
use ui::events::handle_events;
use ui::render::render_ui;

#[derive(Parser, Debug)]
#[command(name = "spacer", author, version, about = "⚡ Terminal storage space manager with smart waste prioritization & safe batch deletion")]
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
}

fn main() -> anyhow::Result<()> {
    let args = Cli::parse();

    // Check for CLI subcommands
    if let Some(Commands::Update) = args.command {
        return updater::cli::run_cli_update();
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
