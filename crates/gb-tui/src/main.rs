//! CLI entry point: argument parsing, terminal setup/teardown, and the
//! top-level render loop.

mod app;
mod audio;
mod debug;
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

/// DMG refresh rate: `DOTS_PER_FRAME` dots/frame at 4.194304 MHz == ~59.7275
/// Hz. Fallback pacing when no audio output device is available (this
/// sandbox's usual case) — otherwise nothing drains the APU's ring
/// buffer, so the buffer-backpressure pacing below would just see it
/// permanently full instead of actually tracking real time.
fn frame_duration() -> Duration {
    Duration::from_secs_f64(gb_core::ppu::DOTS_PER_FRAME as f64 / 4_194_304.0)
}

/// How long to sleep between checks while waiting for the audio ring
/// buffer to drain, when pacing off backpressure. Short enough that input
/// stays responsive (quit, button presses) during the wait.
const AUDIO_BACKPRESSURE_POLL: Duration = Duration::from_millis(2);

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

    // Audio is the preferred pacing source (see SPEC.md's execution
    // model: pacing off buffer backpressure avoids the audio/video drift
    // a wall-clock timer alone is prone to). `take_consumer` only
    // succeeds once; `audio::start` itself degrades to `None` on any
    // failure (no device, unsupported config, ...), in which case the
    // wall-clock fallback below takes over instead.
    let audio_output = app
        .system
        .mmu
        .apu
        .take_consumer()
        .and_then(audio::start);
    if let Some(output) = &audio_output {
        app.system.mmu.apu.set_sample_rate(output.sample_rate);
    }

    let mut last_save_check = Instant::now();
    let frame_duration = frame_duration();

    loop {
        let frame_start = Instant::now();

        let buffer_has_room = audio_output.as_ref().is_none_or(|_| {
            app.system.mmu.apu.queued_frames() < app.system.mmu.apu.buffer_capacity_frames()
        });

        if app.run_mode == RunMode::Running && buffer_has_room {
            app.run_frame_checking_breakpoints();
        }

        terminal.draw(|frame| render::draw(frame, &mut app))?;

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

        if audio_output.is_some() {
            if !buffer_has_room {
                std::thread::sleep(AUDIO_BACKPRESSURE_POLL);
            }
        } else {
            let elapsed = frame_start.elapsed();
            if elapsed < frame_duration {
                std::thread::sleep(frame_duration - elapsed);
            }
        }
    }

    if let Some(rom_path) = &rom {
        save::persist(&mut app.system, rom_path);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    /// End-to-end smoke test: drives `render::draw` (the same function
    /// the real run loop calls every frame) through a `TestBackend`
    /// terminal, in both the plain and debug-overlay layouts, across
    /// every debug panel. This is the closest thing to interactively
    /// exercising the TUI available in a sandbox with no attached TTY --
    /// it can't verify the output *looks* right, but it does verify the
    /// whole render pipeline (video widget, every debug panel, status
    /// line) runs without panicking against a real ratatui `Buffer`.
    #[test]
    fn full_render_pipeline_does_not_panic_across_all_debug_panels() {
        let backend = TestBackend::new(260, 100);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(None, Palette::Classic);
        app.system.load_rom(&[0x00]);

        terminal.draw(|frame| render::draw(frame, &mut app)).unwrap();

        app.debug_overlay = true;
        for panel in [
            app::DebugPanel::Disassembly,
            app::DebugPanel::Memory,
            app::DebugPanel::Vram,
            app::DebugPanel::Log,
        ] {
            app.debug_panel = panel;
            for vram_tab in [app::VramTab::Tiles, app::VramTab::BgMap, app::VramTab::Oam] {
                app.vram_tab = vram_tab;
                terminal.draw(|frame| render::draw(frame, &mut app)).unwrap();
            }
        }
    }

    #[test]
    fn resize_prompt_renders_without_panicking_on_a_too_small_terminal() {
        let backend = TestBackend::new(20, 5); // smaller than the GB screen
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(None, Palette::Classic);
        terminal.draw(|frame| render::draw(frame, &mut app)).unwrap();
    }
}
