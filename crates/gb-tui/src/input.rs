//! Keyboard input handling: crossterm key events -> joypad/debugger
//! actions. Covers quit, the GB button mapping (arrows + Z/X/Enter/RShift
//! = D-pad/A/B/Start/Select), the debugger keybinds -- F12: toggle
//! overlay, Tab: cycle panel, Space/N: step one instruction, F: step one
//! frame, F5: run/pause, B: toggle breakpoint at the current PC, W:
//! toggle a watchpoint at the memory viewer's cursor, V: cycle the VRAM
//! panel's sub-tab -- and save states: F2 quicksaves, F3 quickloads, both
//! to `<rom>.state` (no-ops without a loaded ROM). All per `SPEC.md`.

use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, ModifierKeyCode};

use gb_core::joypad::Button;

use crate::app::{App, RunMode};

/// If a button hasn't seen a fresh press within this window, treat it as
/// released. Only used as a fallback on terminals that don't support the
/// Kitty keyboard protocol's real release events (`App::enhanced_keyboard`
/// unset) -- such terminals only ever report key-*press* events, and
/// OS-level auto-repeat while a key is held arrives as repeated presses at
/// the OS's repeat *cadence*, not starting immediately: there's a longer
/// initial delay (commonly 400-700ms, OS/terminal-configurable) between the
/// first press and the first repeat. This timeout has to bridge that gap,
/// not just the steady-state cadence between repeats -- a shorter value
/// (150ms was tried first) reads a still-held key as released during that
/// initial gap, then pressed again once repeats start, producing a
/// spurious short-tap-then-long-press blip instead of one continuous hold.
const AUTO_RELEASE_TIMEOUT: Duration = Duration::from_millis(700);

fn map_key(code: KeyCode) -> Option<Button> {
    match code {
        KeyCode::Up => Some(Button::Up),
        KeyCode::Down => Some(Button::Down),
        KeyCode::Left => Some(Button::Left),
        KeyCode::Right => Some(Button::Right),
        KeyCode::Char('z') | KeyCode::Char('Z') => Some(Button::A),
        KeyCode::Char('x') | KeyCode::Char('X') => Some(Button::B),
        KeyCode::Enter => Some(Button::Start),
        // Only reported as a distinct key by terminals speaking the Kitty
        // keyboard protocol's disambiguation extension (see `main.rs`'s
        // startup attempt to enable it) -- plain terminals fold Shift into
        // a modifier on other keys instead, with no way to see it alone.
        KeyCode::Modifier(ModifierKeyCode::RightShift) => Some(Button::Select),
        _ => None,
    }
}

/// Drains all pending terminal events (non-blocking), then -- on
/// terminals without real key-release events -- auto-releases any button
/// that's gone stale. Updates `app` in place.
pub fn handle_events(app: &mut App) -> Result<()> {
    while event::poll(Duration::ZERO)? {
        if let Event::Key(key) = event::read()? {
            handle_key(app, key.code, key.kind, key.modifiers);
        }
    }

    release_stale_buttons(app);
    Ok(())
}

fn handle_key(app: &mut App, code: KeyCode, kind: KeyEventKind, modifiers: event::KeyModifiers) {
    match code {
        KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
        KeyCode::Char('c') if modifiers.contains(event::KeyModifiers::CONTROL) => {
            app.should_quit = true;
        }
        _ => {}
    }

    if kind == KeyEventKind::Press {
        handle_debug_key(app, code);
    }

    let Some(button) = map_key(code) else { return };
    match kind {
        KeyEventKind::Release => {
            app.system.mmu.joypad.set_button(button, false);
            app.button_last_pressed.remove(&button);
        }
        KeyEventKind::Press | KeyEventKind::Repeat => {
            app.system.mmu.joypad.set_button(button, true);
            app.button_last_pressed.insert(button, Instant::now());
        }
    }
}

fn handle_debug_key(app: &mut App, code: KeyCode) {
    match code {
        KeyCode::F(12) => app.debug_overlay = !app.debug_overlay,
        KeyCode::Tab if app.debug_overlay => app.cycle_debug_panel(),
        KeyCode::Char('v') | KeyCode::Char('V')
            if app.debug_overlay && app.debug_panel == crate::app::DebugPanel::Vram =>
        {
            app.cycle_vram_tab();
        }
        KeyCode::Char(' ') | KeyCode::Char('n') | KeyCode::Char('N') if app.run_mode == RunMode::Paused => {
            app.step_one_instruction();
        }
        KeyCode::Char('f') | KeyCode::Char('F') if app.run_mode == RunMode::Paused => {
            // Step one full frame (breakpoint-aware); run_mode stays
            // Paused either way since it only starts out Paused here and
            // the call only ever sets it back to Paused, never Running.
            app.run_frame_checking_breakpoints();
        }
        KeyCode::F(5) => {
            app.run_mode = match app.run_mode {
                RunMode::Running => RunMode::Paused,
                RunMode::Paused => RunMode::Running,
            };
        }
        KeyCode::Char('b') | KeyCode::Char('B') if app.debug_overlay => {
            let pc = app.system.cpu.regs.pc;
            app.breakpoints.toggle_pc(pc);
        }
        KeyCode::Char('w') | KeyCode::Char('W')
            if app.debug_overlay && app.debug_panel == crate::app::DebugPanel::Memory =>
        {
            use gb_core::cpu::Bus;
            let addr = app.mem_viewer_addr;
            let current = app.system.mmu.read(addr);
            app.breakpoints.toggle_watch(addr, current);
        }
        KeyCode::F(2) => {
            if let Some(rom_path) = app.rom_path.clone() {
                crate::save::quicksave(&app.system, &rom_path);
            }
        }
        KeyCode::F(3) => {
            if let Some(rom_path) = app.rom_path.clone() {
                crate::save::quickload(&mut app.system, &rom_path);
            }
        }
        _ => {}
    }
}

/// No-op on terminals speaking the Kitty protocol's event-type extension
/// (`App::enhanced_keyboard`): those send a real `KeyEventKind::Release`
/// the instant a key physically comes up, which `handle_key` already acts
/// on directly, so there's nothing stale to sweep. Without it, a
/// still-held key can go quiet for a while between OS auto-repeat
/// presses, so staleness is the only signal available.
fn release_stale_buttons(app: &mut App) {
    if app.enhanced_keyboard {
        return;
    }

    let now = Instant::now();
    let stale: Vec<Button> = app
        .button_last_pressed
        .iter()
        .filter(|&(_, &last)| now.duration_since(last) > AUTO_RELEASE_TIMEOUT)
        .map(|(&button, _)| button)
        .collect();
    for button in stale {
        app.system.mmu.joypad.set_button(button, false);
        app.button_last_pressed.remove(&button);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::app::App;
    use crate::palette::Palette;

    #[test]
    fn enhanced_keyboard_trusts_a_real_release_event_immediately() {
        let mut app = App::new(None, Palette::Classic);
        app.enhanced_keyboard = true;

        handle_key(&mut app, KeyCode::Up, KeyEventKind::Press, event::KeyModifiers::NONE);
        assert!(app.button_last_pressed.contains_key(&Button::Up));

        handle_key(&mut app, KeyCode::Up, KeyEventKind::Release, event::KeyModifiers::NONE);
        assert!(!app.button_last_pressed.contains_key(&Button::Up));
    }

    #[test]
    fn enhanced_keyboard_ignores_staleness_even_past_the_timeout() {
        // Regression test: with real release events available, a gap
        // between OS auto-repeat presses (e.g. the OS's initial
        // repeat-delay, commonly longer than AUTO_RELEASE_TIMEOUT) must
        // not read as a release -- only an explicit Release event should.
        let mut app = App::new(None, Palette::Classic);
        app.enhanced_keyboard = true;

        handle_key(&mut app, KeyCode::Up, KeyEventKind::Press, event::KeyModifiers::NONE);
        // Backdate the press past the fallback timeout to simulate a long
        // gap before the next OS auto-repeat event arrives.
        app.button_last_pressed
            .insert(Button::Up, Instant::now() - AUTO_RELEASE_TIMEOUT * 2);

        release_stale_buttons(&mut app);

        assert!(app.button_last_pressed.contains_key(&Button::Up));
    }

    #[test]
    fn plain_terminal_releases_a_button_once_it_goes_stale() {
        let mut app = App::new(None, Palette::Classic);
        assert!(!app.enhanced_keyboard);

        handle_key(&mut app, KeyCode::Up, KeyEventKind::Press, event::KeyModifiers::NONE);
        app.button_last_pressed
            .insert(Button::Up, Instant::now() - AUTO_RELEASE_TIMEOUT * 2);

        release_stale_buttons(&mut app);

        assert!(!app.button_last_pressed.contains_key(&Button::Up));
    }

    #[test]
    fn plain_terminal_keeps_a_freshly_pressed_button_held() {
        let mut app = App::new(None, Palette::Classic);
        handle_key(&mut app, KeyCode::Up, KeyEventKind::Press, event::KeyModifiers::NONE);

        release_stale_buttons(&mut app);

        assert!(app.button_last_pressed.contains_key(&Button::Up));
    }
}
