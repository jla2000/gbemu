//! CLI entry point: argument parsing, terminal setup/teardown, and the
//! top-level render loop.

mod app;
mod input;
mod log_ring;
mod palette;
mod render;
mod save;

use std::path::PathBuf;
use std::time::{Duration, Instant};

use anyhow::Result;
use clap::Parser;
use crossterm::{
    event::{
        DisableMouseCapture, EnableMouseCapture, KeyboardEnhancementFlags,
        PopKeyboardEnhancementFlags, PushKeyboardEnhancementFlags,
    },
    execute,
    terminal::{
        disable_raw_mode, enable_raw_mode, supports_keyboard_enhancement, EnterAlternateScreen,
        LeaveAlternateScreen,
    },
};
use ratatui::{backend::CrosstermBackend, Terminal};

use app::{App, RunMode};
use palette::Palette;

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

fn main() -> Result<()> {
    let cli = Cli::parse();

    log_ring::init_tracing();

    if cli.headless {
        tracing::info!("headless mode requested; TUI not started (rom: {:?})", cli.rom);
        // Headless test-harness execution lands alongside the Blargg/
        // Mealybug test infrastructure (see tests/blargg, tests/mealybug).
        return Ok(());
    }

    let (mut terminal, enhanced_keyboard) = init_terminal()?;
    let result = run(&mut terminal, cli.rom, cli.palette);
    restore_terminal(&mut terminal, enhanced_keyboard)?;
    result
}

/// Sets up raw mode + the alternate screen, and — on terminals that
/// support it — the Kitty keyboard protocol's disambiguation + event-type
/// reporting extensions, which is the only reliable way to see RShift as
/// its own key (for the Select button) and real key-release events (for
/// auto-releasing D-pad/A/B/Start without `input`'s stale-press timeout).
/// Returns whether that enhancement was actually enabled, so teardown
/// knows whether to pop it.
fn init_terminal() -> Result<(Terminal<CrosstermBackend<std::io::Stdout>>, bool)> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;

    let enhanced_keyboard = supports_keyboard_enhancement().unwrap_or(false);
    if enhanced_keyboard {
        execute!(
            stdout,
            PushKeyboardEnhancementFlags(
                KeyboardEnhancementFlags::DISAMBIGUATE_ESCAPE_CODES
                    | KeyboardEnhancementFlags::REPORT_EVENT_TYPES
            )
        )?;
    }

    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok((terminal, enhanced_keyboard))
}

fn restore_terminal(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    enhanced_keyboard: bool,
) -> Result<()> {
    disable_raw_mode()?;
    if enhanced_keyboard {
        execute!(terminal.backend_mut(), PopKeyboardEnhancementFlags)?;
    }
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )?;
    terminal.show_cursor()?;
    Ok(())
}

/// How often the run loop checks whether battery-backed cartridge RAM has
/// been dirtied and needs writing out, on top of the always-on
/// write-on-exit.
const SAVE_CHECK_INTERVAL: Duration = Duration::from_secs(5);

/// DMG refresh rate: 70224 dots/frame at 4.194304 MHz == ~59.7275 Hz.
/// Interim wall-clock pacing for M4 "first playable ROM" — `SPEC.md`'s
/// long-term design paces off audio-buffer backpressure instead once M5
/// lands real audio output, which avoids the audio/video drift a
/// wall-clock timer alone is prone to.
fn frame_duration() -> Duration {
    Duration::from_secs_f64(70224.0 / 4_194_304.0)
}

fn run(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    rom: Option<PathBuf>,
    palette: Palette,
) -> Result<()> {
    let mut app = App::new(rom.clone(), palette);

    if let Some(rom_path) = &rom {
        match std::fs::read(rom_path) {
            Ok(data) => {
                for warning in app.system.load_cartridge(&data) {
                    tracing::warn!("{warning}");
                }
                save::load(&mut app.system, rom_path);
                app.run_mode = RunMode::Running;
            }
            Err(e) => tracing::error!("failed to read ROM {}: {e}", rom_path.display()),
        }
    }

    let mut last_save_check = Instant::now();
    let frame_duration = frame_duration();

    loop {
        let frame_start = Instant::now();

        if app.run_mode == RunMode::Running {
            app.system.run_frame();
        }

        terminal.draw(|frame| render::draw(frame, &app))?;

        input::handle_events(&mut app)?;

        if let Some(rom_path) = &rom {
            if last_save_check.elapsed() >= SAVE_CHECK_INTERVAL {
                save::persist_if_dirty(&mut app.system, rom_path);
                last_save_check = Instant::now();
            }
        }

        if app.should_quit {
            break;
        }

        let elapsed = frame_start.elapsed();
        if elapsed < frame_duration {
            std::thread::sleep(frame_duration - elapsed);
        }
    }

    if let Some(rom_path) = &rom {
        save::persist(&mut app.system, rom_path);
    }

    Ok(())
}
