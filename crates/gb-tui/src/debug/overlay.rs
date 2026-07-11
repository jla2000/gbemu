//! Composes the focused debug panel (per `App::debug_panel`) into a
//! widget, and the disassembly panel's "walk forward from PC" line
//! builder (needs live `Bus` access, unlike the pure `disasm` module).

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use gb_core::cpu::Bus;
use gb_core::mmu::Mmu;

use crate::app::{App, DebugPanel, VramTab};
use crate::debug::breakpoints::Breakpoints;
use crate::debug::{disasm, log_panel, memory, registers, vram};

/// Disassembles `count` instructions starting at `pc`, marking the first
/// (the CPU's current instruction) with `->` and any address with a
/// breakpoint set with `*`. True "centered on PC" would need to
/// disassemble backward from an arbitrary byte offset, which is ambiguous
/// for a variable-length instruction stream without walking forward from
/// a known-good alignment point first — showing PC and what comes after
/// it, which is unambiguous, is what's implemented here.
pub fn disassembly_lines(mmu: &mut Mmu, pc: u16, count: u16, breakpoints: &Breakpoints) -> Vec<String> {
    let mut out = Vec::with_capacity(count as usize);
    let mut addr = pc;
    for i in 0..count {
        let bytes = [mmu.read(addr), mmu.read(addr.wrapping_add(1)), mmu.read(addr.wrapping_add(2))];
        let instr = disasm::disassemble_one(&bytes, addr);
        let pc_marker = if i == 0 { "->" } else { "  " };
        let bp_marker = if breakpoints.has_pc(addr) { "*" } else { " " };
        out.push(format!("{pc_marker}{bp_marker}{addr:04X}: {}", instr.mnemonic));
        addr = addr.wrapping_add(instr.len.max(1));
    }
    out
}

pub struct DebugOverlayWidget<'a> {
    app: &'a mut App,
}

impl<'a> DebugOverlayWidget<'a> {
    pub fn new(app: &'a mut App) -> Self {
        Self { app }
    }
}

impl<'a> Widget for DebugOverlayWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        match self.app.debug_panel {
            DebugPanel::Disassembly => {
                let pc = self.app.system.cpu.regs.pc;
                let rows = area.height.saturating_sub(2);
                let block = Block::default().borders(Borders::ALL).title("Disassembly");
                let text: Vec<Line> =
                    disassembly_lines(&mut self.app.system.mmu, pc, rows, &self.app.breakpoints)
                        .into_iter()
                        .map(Line::from)
                        .collect();
                Paragraph::new(text).block(block).render(area, buf);
            }
            DebugPanel::Registers => {
                registers::RegistersWidget::new(&self.app.system.cpu).render(area, buf);
            }
            DebugPanel::Memory => {
                memory::MemoryWidget::new(
                    &mut self.app.system.mmu,
                    self.app.mem_viewer_addr,
                    &self.app.breakpoints,
                )
                .render(area, buf);
            }
            DebugPanel::Vram => match self.app.vram_tab {
                VramTab::Tiles => {
                    vram::TileGridWidget::new(&self.app.system.mmu.ppu, self.app.palette)
                        .render(area, buf);
                }
                VramTab::BgMap => {
                    let block = Block::default().borders(Borders::ALL).title("BG Map");
                    let rows = area.height.saturating_sub(2);
                    let map_base = if self.app.system.mmu.ppu.read_lcdc() & 0x08 != 0 { 0x9C00 } else { 0x9800 };
                    let text: Vec<Line> = vram::bg_map_lines(&self.app.system.mmu.ppu, map_base, rows)
                        .into_iter()
                        .map(Line::from)
                        .collect();
                    Paragraph::new(text).block(block).render(area, buf);
                }
                VramTab::Oam => {
                    vram::OamWidget::new(&self.app.system.mmu.ppu).render(area, buf);
                }
            },
            DebugPanel::Log => {
                log_panel::LogPanelWidget.render(area, buf);
            }
        }
    }
}

/// One-line summary shown regardless of which panel is focused: run
/// state and active breakpoints, so they're visible without switching
/// panels.
pub fn status_summary(app: &App) -> String {
    let run_state = match app.run_mode {
        crate::app::RunMode::Running => "RUNNING",
        crate::app::RunMode::Paused => "PAUSED",
    };
    let bp_count = app.breakpoints.pc_breakpoints().count();
    let watch_count = app.breakpoints.watchpoints().count();
    format!(
        "[{run_state}] breakpoints:{bp_count} watchpoints:{watch_count} | Tab: panel  Space: step  F: frame  F5: run/pause  B: breakpoint@PC  W: watch"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gb_core::System;

    use crate::palette::Palette;

    #[test]
    fn disassembly_lines_walks_forward_and_marks_pc() {
        let mut sys = System::new();
        sys.load_rom(&[0x00, 0x00, 0xC3, 0x00, 0x01]); // NOP; NOP; JP $0100
        let breakpoints = Breakpoints::new();
        let lines = disassembly_lines(&mut sys.mmu, 0, 3, &breakpoints);
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("->"));
        assert!(lines[0].contains("0000: NOP"));
        assert!(lines[1].contains("0001: NOP"));
        assert!(lines[2].contains("0002: JP $0100"));
    }

    #[test]
    fn disassembly_lines_marks_breakpoint_addresses() {
        let mut sys = System::new();
        sys.load_rom(&[0x00, 0x00]);
        let mut breakpoints = Breakpoints::new();
        breakpoints.toggle_pc(0x0001);
        let lines = disassembly_lines(&mut sys.mmu, 0, 2, &breakpoints);
        assert!(!lines[0].contains('*'));
        assert!(lines[1].contains('*'));
    }

    #[test]
    fn status_summary_reports_run_mode_and_breakpoint_counts() {
        let mut app = App::new(None, Palette::Classic);
        app.breakpoints.toggle_pc(0x100);
        let summary = status_summary(&app);
        assert!(summary.contains("PAUSED"));
        assert!(summary.contains("breakpoints:1"));
    }
}
