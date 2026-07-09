//! Keyboard input handling: crossterm key events -> joypad/debugger actions.
//! Full GB-button + debugger keybind mapping lands in M4/M6. For M0 this
//! only wires quit and a non-blocking poll loop.

use std::time::Duration;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};

use crate::app::App;

/// Poll for and handle any pending terminal events. Updates `app` in place.
pub fn handle_events(app: &mut App) -> Result<()> {
    if !event::poll(Duration::from_millis(16))? {
        return Ok(());
    }

    if let Event::Key(key) = event::read()? {
        if key.kind != KeyEventKind::Press {
            return Ok(());
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => app.should_quit = true,
            KeyCode::Char('c') if key.modifiers.contains(event::KeyModifiers::CONTROL) => {
                app.should_quit = true;
            }
            _ => {}
        }
    }

    Ok(())
}
