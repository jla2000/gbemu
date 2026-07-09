//! CLI entry point: argument parsing, terminal setup/teardown, and the
//! top-level render loop.

mod app;
mod input;
mod log_ring;
mod render;

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};

use app::App;

/// Terminal Game Boy (DMG) emulator.
#[derive(Parser, Debug)]
#[command(name = "gbemu", version, about)]
struct Cli {
    /// Path to a Game Boy ROM file to load.
    rom: Option<PathBuf>,

    /// Run headless (no TUI) — used by test-ROM harnesses.
    #[arg(long)]
    headless: bool,

    /// Display palette to use for the DMG 4-shade framebuffer.
    #[arg(long, value_enum, default_value_t = Palette::Classic)]
    palette: Palette,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, clap::ValueEnum)]
enum Palette {
    /// Classic Game Boy green shades.
    Classic,
    /// Neutral grayscale shades.
    Grayscale,
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    log_ring::init_tracing();

    if cli.headless {
        tracing::info!("headless mode requested; TUI not started (rom: {:?})", cli.rom);
        // Headless test-harness execution lands alongside the Blargg/
        // Mealybug test infrastructure (see tests/blargg, tests/mealybug).
        return Ok(());
    }

    let mut terminal = init_terminal()?;
    let result = run(&mut terminal, cli.rom);
    restore_terminal(&mut terminal)?;
    result
}

fn init_terminal() -> Result<Terminal<CrosstermBackend<std::io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    rom: Option<PathBuf>,
) -> Result<()> {
    let mut app = App::new(rom);

    loop {
        terminal.draw(|frame| render::draw(frame, &app))?;

        input::handle_events(&mut app)?;

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
