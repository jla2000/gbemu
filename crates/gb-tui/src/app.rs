//! Top-level application state: run mode, loaded ROM, the emulated
//! system, and the debugger's own state (which panel is focused, the
//! memory viewer's cursor, breakpoints). `run_mode` gates whether the
//! main loop advances the emulator each frame; breakpoint hits flip it
//! back to `Paused` (see `input::handle_events`' caller in `main.rs`).

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use gb_core::cpu::Registers;
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
/// is visible. `Registers` was retired in M8: that content now renders
/// unconditionally in the status sidebar (`debug::status`) instead of
/// living behind a togglable panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugPanel {
    Disassembly,
    Memory,
    Vram,
    Log,
}

const DEBUG_PANELS: [DebugPanel; 4] = [
    DebugPanel::Disassembly,
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
    /// event. See that module for the timeout. Unused when
    /// `enhanced_keyboard` is set: real release events are trusted
    /// instead.
    pub button_last_pressed: HashMap<Button, Instant>,
    /// Whether the terminal negotiated the Kitty keyboard protocol's
    /// event-type reporting extension (set once at startup from
    /// `main::init_terminal`'s probe). When true, `input::handle_events`
    /// trusts real press/release events instead of the staleness-timeout
    /// heuristic `button_last_pressed` exists for.
    pub enhanced_keyboard: bool,

    pub debug_overlay: bool,
    pub debug_panel: DebugPanel,
    pub vram_tab: VramTab,
    /// Cursor/jump-to-address for the memory viewer panel.
    pub mem_viewer_addr: u16,
    pub breakpoints: Breakpoints,
    /// The previous frame's CPU register snapshot, used by the status
    /// sidebar to highlight whichever registers changed since the last
    /// redraw (see `debug::status::cpu_lines`).
    pub prev_cpu_regs: Registers,
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
            enhanced_keyboard: false,
            debug_overlay: false,
            debug_panel: DebugPanel::Disassembly,
            vram_tab: VramTab::Tiles,
            mem_viewer_addr: 0,
            breakpoints: Breakpoints::new(),
            prev_cpu_regs: Registers::new(),
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

    /// Runs one GB frame's worth of T-cycles like `System::run_frame`
    /// (paced by a fixed dot budget, not a VBlank edge -- see that
    /// function's doc comment for why watching for an edge is unsafe when
    /// the LCD can be disabled mid-frame), but stops early (returning
    /// `true` and setting `run_mode` back to `Paused`) if a PC breakpoint
    /// or watchpoint is hit. This duplicates `run_frame`'s loop rather
    /// than adding breakpoint awareness to `gb-core` -- breakpoints are a
    /// debugger/frontend concept, not something the emulation core itself
    /// needs to know about.
    pub fn run_frame_checking_breakpoints(&mut self) -> bool {
        let mut dots_run: u32 = 0;
        while dots_run < gb_core::ppu::DOTS_PER_FRAME {
            let pc = self.system.cpu.regs.pc;
            if self.breakpoints.should_break_at(pc) {
                self.run_mode = RunMode::Paused;
                return true;
            }

            dots_run += self.system.step() as u32;

            if self.breakpoints.check_watchpoints(&mut self.system.mmu) {
                self.run_mode = RunMode::Paused;
                return true;
            }
        }
        false
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
        app.system.load_rom(&[0xC3, 0x00, 0x00]); // JP 0x0000 (infinite loop, 16 T-cycles/iter)
        app.system.mmu.ppu.write_lcdc(0x80);

        let hit = app.run_frame_checking_breakpoints();
        assert!(!hit);
        // DOTS_PER_FRAME is an exact multiple of this loop's 16
        // T-cycles/iteration, so it lands back at the start of the next
        // frame (LY=0) rather than stopping mid-VBlank.
        assert_eq!(app.system.mmu.ppu.read_ly(), 0);
    }

    #[test]
    fn run_frame_checking_breakpoints_keeps_stepping_while_the_lcd_is_off() {
        // Regression test: this used to bail out without stepping the CPU
        // at all whenever the LCD was off, freezing the debugger's
        // run/continue forever the instant a ROM disabled the LCD.
        let mut app = App::new(None, Palette::Classic);
        app.system.load_rom(&[0x3C, 0xC3, 0x00, 0x00]); // loop: INC A; JP 0x0000

        let hit = app.run_frame_checking_breakpoints();
        assert!(!hit);
        assert_ne!(app.system.cpu.regs.a, 0);
    }
}
