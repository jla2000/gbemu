//! Top-level application state: run mode, loaded ROM, the emulated
//! system, and the debugger's own state (which panel is focused, the
//! memory viewer's cursor, breakpoints). `run_mode` gates whether the
//! main loop advances the emulator each frame; breakpoint hits flip it
//! back to `Paused` (see `input::handle_events`' caller in `main.rs`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use gb_core::joypad::Button;
use gb_core::System;

use crate::debug::breakpoints::Breakpoints;
use crate::palette::Palette;

/// Coarse run mode. `Paused` is also what a hit breakpoint/watchpoint
/// transitions back to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunMode {
    Running,
    Paused,
}

/// Which debug panel `Tab` currently has focused, when the debug overlay
/// is visible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugPanel {
    Disassembly,
    Registers,
    Memory,
    Vram,
    Log,
}

const DEBUG_PANELS: [DebugPanel; 5] = [
    DebugPanel::Disassembly,
    DebugPanel::Registers,
    DebugPanel::Memory,
    DebugPanel::Vram,
    DebugPanel::Log,
];

/// Which sub-tab the VRAM panel shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VramTab {
    Tiles,
    BgMap,
    Oam,
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

    pub debug_overlay: bool,
    pub debug_panel: DebugPanel,
    pub vram_tab: VramTab,
    /// Cursor/jump-to-address for the memory viewer panel.
    pub mem_viewer_addr: u16,
    pub breakpoints: Breakpoints,
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
            debug_overlay: false,
            debug_panel: DebugPanel::Disassembly,
            vram_tab: VramTab::Tiles,
            mem_viewer_addr: 0,
            breakpoints: Breakpoints::new(),
        }
    }

    pub fn cycle_debug_panel(&mut self) {
        let idx = DEBUG_PANELS.iter().position(|&p| p == self.debug_panel).unwrap_or(0);
        self.debug_panel = DEBUG_PANELS[(idx + 1) % DEBUG_PANELS.len()];
    }

    pub fn cycle_vram_tab(&mut self) {
        self.vram_tab = match self.vram_tab {
            VramTab::Tiles => VramTab::BgMap,
            VramTab::BgMap => VramTab::Oam,
            VramTab::Oam => VramTab::Tiles,
        };
    }

    /// Executes exactly one CPU instruction, regardless of `run_mode` —
    /// the debugger's single-step control.
    pub fn step_one_instruction(&mut self) {
        self.system.step();
    }

    /// Runs one GB frame like `System::run_frame`, but stops early
    /// (returning `true` and setting `run_mode` back to `Paused`) if a PC
    /// breakpoint or watchpoint is hit. This duplicates `run_frame`'s
    /// VBlank-edge loop rather than adding breakpoint awareness to
    /// `gb-core` -- breakpoints are a debugger/frontend concept, not
    /// something the emulation core itself needs to know about.
    pub fn run_frame_checking_breakpoints(&mut self) -> bool {
        if !self.system.mmu.ppu.lcd_enabled() {
            return false;
        }
        loop {
            let pc = self.system.cpu.regs.pc;
            if self.breakpoints.should_break_at(pc) {
                self.run_mode = RunMode::Paused;
                return true;
            }

            let ly_before = self.system.mmu.ppu.read_ly();
            self.system.step();

            if self.breakpoints.check_watchpoints(&mut self.system.mmu) {
                self.run_mode = RunMode::Paused;
                return true;
            }

            let ly_after = self.system.mmu.ppu.read_ly();
            if ly_before != gb_core::ppu::VBLANK_START_LINE && ly_after == gb_core::ppu::VBLANK_START_LINE {
                return false;
            }
            if !self.system.mmu.ppu.lcd_enabled() {
                return false;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cycle_debug_panel_wraps_around() {
        let mut app = App::new(None, Palette::Classic);
        assert_eq!(app.debug_panel, DebugPanel::Disassembly);
        for expected in [
            DebugPanel::Registers,
            DebugPanel::Memory,
            DebugPanel::Vram,
            DebugPanel::Log,
            DebugPanel::Disassembly,
        ] {
            app.cycle_debug_panel();
            assert_eq!(app.debug_panel, expected);
        }
    }

    #[test]
    fn cycle_vram_tab_wraps_around() {
        let mut app = App::new(None, Palette::Classic);
        assert_eq!(app.vram_tab, VramTab::Tiles);
        app.cycle_vram_tab();
        assert_eq!(app.vram_tab, VramTab::BgMap);
        app.cycle_vram_tab();
        assert_eq!(app.vram_tab, VramTab::Oam);
        app.cycle_vram_tab();
        assert_eq!(app.vram_tab, VramTab::Tiles);
    }

    #[test]
    fn run_frame_checking_breakpoints_stops_at_a_pc_breakpoint() {
        let mut app = App::new(None, Palette::Classic);
        app.system.load_rom(&[0xC3, 0x00, 0x00]); // JP 0x0000 (infinite loop)
        app.system.mmu.ppu.write_lcdc(0x80); // LCD on
        app.breakpoints.toggle_pc(0x0000);
        app.run_mode = RunMode::Running;

        let hit = app.run_frame_checking_breakpoints();
        assert!(hit);
        assert_eq!(app.run_mode, RunMode::Paused);
        assert_eq!(app.system.cpu.regs.pc, 0x0000); // stopped before executing it
    }

    #[test]
    fn run_frame_checking_breakpoints_completes_a_full_frame_without_hits() {
        let mut app = App::new(None, Palette::Classic);
        app.system.load_rom(&[0xC3, 0x00, 0x00]); // JP 0x0000 (infinite loop)
        app.system.mmu.ppu.write_lcdc(0x80);

        let hit = app.run_frame_checking_breakpoints();
        assert!(!hit);
        assert_eq!(app.system.mmu.ppu.read_ly(), gb_core::ppu::VBLANK_START_LINE);
    }
}
