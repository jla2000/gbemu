//! Keyboard input handling: crossterm key events -> joypad/debugger
//! actions. Full debugger keybind mapping (step/breakpoints/panel cycling)
//! lands in M6; this covers quit + the GB button mapping (arrows + Z/X/
//! Enter/RShift = D-pad/A/B/Start/Select, per `SPEC.md`).

use std::time::{Duration, Instant};

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, ModifierKeyCode};

use gb_core::joypad::Button;

use crate::app::App;

/// If a button hasn't seen a fresh press within this window, treat it as
/// released. Most terminals only ever report key-*press* events (even
/// OS-level auto-repeat while held arrives as repeated presses, not a
/// press/hold/release triple), so this is the practical way to detect
/// "released" without the Kitty keyboard protocol's release events. Long
/// enough that a held key's repeat cadence doesn't cause flicker, short
/// enough that letting go reads as an prompt release.
const AUTO_RELEASE_TIMEOUT: Duration = Duration::from_millis(150);

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

/// Drains all pending terminal events (non-blocking), then auto-releases
/// any button that's gone stale. Updates `app` in place.
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

fn release_stale_buttons(app: &mut App) {
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
