//! Top-level application state: run mode, loaded ROM, and the emulated
//! system. Full running/paused/stepping state machine + debugger wiring
//! lands in M6; this establishes the shape for M0.

use std::path::PathBuf;

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
}

impl App {
    pub fn new(rom_path: Option<PathBuf>, palette: Palette) -> Self {
        Self {
            rom_path,
            system: System::new(),
            run_mode: RunMode::Paused,
            palette,
            should_quit: false,
        }
    }
}
