//! Top-level application state: run mode, loaded ROM, and the emulated
//! system. Full debugger-driven step/breakpoint transitions land in M6;
//! for now `run_mode` just gates whether the main loop advances the
//! emulator each frame.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use gb_core::joypad::Button;
use gb_core::System;

use crate::palette::Palette;

/// Coarse run mode. Debugger-driven transitions (breakpoints, step) land in
/// M6.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    Running,
    Paused,
}

pub struct App {
    pub rom_path: Option<PathBuf>,
    pub system: System,
    pub run_mode: RunMode,
    pub palette: Palette,
    pub should_quit: bool,
    /// When each currently-held button was last confirmed pressed —
    /// `input::handle_events` auto-releases a button once its entry goes
    /// stale, since most terminals never send a discrete key-release
    /// event. See that module for the timeout.
    pub button_last_pressed: HashMap<Button, Instant>,
}

impl App {
    pub fn new(rom_path: Option<PathBuf>, palette: Palette) -> Self {
        Self {
            rom_path,
            system: System::new(),
            run_mode: RunMode::Paused,
            palette,
            should_quit: false,
            button_last_pressed: HashMap::new(),
        }
    }
}
