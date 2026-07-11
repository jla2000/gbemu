//! Always-on status sidebar: a compact, unconditional summary of CPU/PPU/
//! Timer/Joypad/Cartridge/APU state and run mode, rendered beside the
//! video area regardless of [`crate::app::App::debug_overlay`]. The
//! heavier panels (disassembly, memory, VRAM, log) stay behind that F12
//! toggle -- see [`crate::debug::overlay`] -- since they need more space
//! and aren't relevant every frame.
//!
//! Line-building is split into plain functions (testable without a
//! ratatui `Buffer`) from the widget that lays them out into bordered,
//! color-coded blocks.

use std::collections::HashMap;
use std::time::Instant;

use ratatui::buffer::Buffer;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use gb_core::apu::Apu;
use gb_core::cartridge::Cartridge;
use gb_core::cpu::{Cpu, Registers};
use gb_core::joypad::Button;
use gb_core::ppu::Ppu;
use gb_core::timer::Timer;

use crate::app::App;

/// Fixed order buttons are listed in on the sidebar's Joypad line --
/// D-pad first, then face/menu buttons, rather than `Button`'s
/// declaration order (which groups Right/Left before Up/Down).
const BUTTON_ORDER: [Button; 8] = [
    Button::Up,
    Button::Down,
    Button::Left,
    Button::Right,
    Button::A,
    Button::B,
    Button::Select,
    Button::Start,
];

fn button_name(b: Button) -> &'static str {
    match b {
        Button::Right => "Right",
        Button::Left => "Left",
        Button::Up => "Up",
        Button::Down => "Down",
        Button::A => "A",
        Button::B => "B",
        Button::Select => "Select",
        Button::Start => "Start",
    }
}

fn flag_char(set: bool) -> char {
    if set {
        '1'
    } else {
        '0'
    }
}

fn on_off(set: bool) -> &'static str {
    if set {
        "ON"
    } else {
        "OFF"
    }
}

/// Highlight style for a register that changed since the previous frame.
fn highlight_style() -> Style {
    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
}

/// CPU section: registers/flags/IME/HALT, with any register that changed
/// since `prev` (the previous frame's snapshot, see `App::prev_cpu_regs`)
/// rendered in [`highlight_style`] so execution activity is visible
/// without single-stepping.
pub fn cpu_lines(cpu: &Cpu, prev: &Registers) -> Vec<Line<'static>> {
    let r = &cpu.regs;
    let style_if = |changed: bool| if changed { highlight_style() } else { Style::default() };

    vec![
        Line::from(format!("AF: {:04X}  (A:{:02X} F:{:02X})", r.af(), r.a, r.f))
            .style(style_if(r.a != prev.a || r.f != prev.f)),
        Line::from(format!("BC: {:04X}  (B:{:02X} C:{:02X})", r.bc(), r.b, r.c))
            .style(style_if(r.b != prev.b || r.c != prev.c)),
        Line::from(format!("DE: {:04X}  (D:{:02X} E:{:02X})", r.de(), r.d, r.e))
            .style(style_if(r.d != prev.d || r.e != prev.e)),
        Line::from(format!("HL: {:04X}  (H:{:02X} L:{:02X})", r.hl(), r.h, r.l))
            .style(style_if(r.h != prev.h || r.l != prev.l)),
        Line::from(format!("SP: {:04X}", r.sp)).style(style_if(r.sp != prev.sp)),
        Line::from(format!("PC: {:04X}", r.pc)).style(style_if(r.pc != prev.pc)),
        Line::from(format!(
            "Flags: Z:{} N:{} H:{} C:{}",
            flag_char(r.zero()),
            flag_char(r.subtract()),
            flag_char(r.half_carry()),
            flag_char(r.carry()),
        ))
        .style(style_if(r.f != prev.f)),
        Line::from(format!(
            "IME:{} HALT:{} STOP:{}",
            if cpu.ime { 1 } else { 0 },
            if cpu.halted { 1 } else { 0 },
            if cpu.stopped { 1 } else { 0 },
        )),
    ]
}

/// Decodes `LCDC` into its 6 sub-fields alongside the raw hex byte, e.g.
/// `LCDC F3  LCD:ON BG:ON WIN:OFF OBJ:ON(8x8) MAP:9800 TILE:8000`.
pub fn decode_lcdc(lcdc: u8) -> String {
    let lcd = on_off(lcdc & 0x80 != 0);
    let bg = on_off(lcdc & 0x01 != 0);
    let win = on_off(lcdc & 0x20 != 0);
    let obj = if lcdc & 0x02 != 0 {
        let size = if lcdc & 0x04 != 0 { "8x16" } else { "8x8" };
        format!("ON({size})")
    } else {
        "OFF".to_string()
    };
    let map = if lcdc & 0x08 != 0 { "9C00" } else { "9800" };
    let tile = if lcdc & 0x10 != 0 { "8000" } else { "8800" };
    format!("LCDC {lcdc:02X}  LCD:{lcd} BG:{bg} WIN:{win} OBJ:{obj} MAP:{map} TILE:{tile}")
}

/// Decodes `STAT` into its PPU mode, LYC=LY flag, and enabled STAT
/// interrupt sources alongside the raw hex byte, e.g.
/// `STAT 87  Mode:3 LYC=LY:no  IntEn: LYC OAM VBL HBL`.
pub fn decode_stat(stat: u8) -> String {
    let mode = stat & 0b11;
    let lyc_eq = if stat & 0x04 != 0 { "yes" } else { "no" };
    let mut sources = Vec::new();
    if stat & 0x40 != 0 {
        sources.push("LYC");
    }
    if stat & 0x20 != 0 {
        sources.push("OAM");
    }
    if stat & 0x10 != 0 {
        sources.push("VBL");
    }
    if stat & 0x08 != 0 {
        sources.push("HBL");
    }
    let int_en = if sources.is_empty() { "-".to_string() } else { sources.join(" ") };
    format!("STAT {stat:02X}  Mode:{mode} LYC=LY:{lyc_eq}  IntEn: {int_en}")
}

/// PPU section: decoded LCDC/STAT plus the remaining scroll/window/
/// palette registers.
pub fn ppu_lines(ppu: &Ppu) -> Vec<String> {
    vec![
        decode_lcdc(ppu.read_lcdc()),
        decode_stat(ppu.read_stat()),
        format!("LY:{:02X} LYC:{:02X}", ppu.read_ly(), ppu.read_lyc()),
        format!(
            "SCX:{:02X} SCY:{:02X} WX:{:02X} WY:{:02X}",
            ppu.read_scx(),
            ppu.read_scy(),
            ppu.read_wx(),
            ppu.read_wy(),
        ),
        format!(
            "BGP:{:02X} OBP0:{:02X} OBP1:{:02X}",
            ppu.read_bgp(),
            ppu.read_obp0(),
            ppu.read_obp1(),
        ),
    ]
}

/// Timer section: DIV/TIMA/TMA plus TAC decoded into its run state and
/// input clock frequency.
pub fn timer_lines(timer: &Timer) -> Vec<String> {
    let tac = timer.read_tac();
    let running = if tac & 0x04 != 0 { "RUN" } else { "STOP" };
    let freq_hz = match tac & 0x03 {
        0 => 4096,
        1 => 262144,
        2 => 65536,
        _ => 16384,
    };
    vec![
        format!("DIV:{:02X} TIMA:{:02X} TMA:{:02X}", timer.read_div(), timer.read_tima(), timer.read_tma()),
        format!("TAC:{tac:02X} {running} {freq_hz}Hz"),
    ]
}

/// Joypad section: currently-held buttons, in D-pad-then-buttons order
/// rather than `Button`'s declaration order.
pub fn joypad_line(held: &HashMap<Button, Instant>) -> String {
    let names: Vec<&str> = BUTTON_ORDER
        .iter()
        .filter(|b| held.contains_key(b))
        .map(|b| button_name(*b))
        .collect();
    if names.is_empty() {
        "Held: -".to_string()
    } else {
        format!("Held: {}", names.join(" "))
    }
}

/// Cartridge section: title, MBC type, and ROM/RAM bank counts from the
/// header, or a placeholder if no cartridge is loaded (e.g. a raw ROM
/// loaded via `load_rom` for tests, or no ROM at all).
pub fn cartridge_lines(cartridge: Option<&Cartridge>) -> Vec<String> {
    match cartridge {
        None => vec!["No cartridge loaded".to_string()],
        Some(c) => {
            let h = &c.header;
            let title = if h.title.is_empty() { "(untitled)" } else { h.title.as_str() };
            vec![
                title.to_string(),
                format!("MBC:{:?} ROM:{}banks RAM:{}banks", h.mbc, h.rom_banks, h.ram_banks),
                format!(
                    "Battery:{} RTC:{} Rumble:{}",
                    on_off(h.has_battery),
                    on_off(h.has_rtc),
                    on_off(h.has_rumble),
                ),
            ]
        }
    }
}

/// APU section: per-channel on/off decoded from `NR52`'s low 4 bits, plus
/// an `NR50`/`NR51` master-volume/panning summary.
pub fn apu_lines(apu: &Apu) -> Vec<String> {
    let nr52 = apu.read_nr52();
    let chans = format!(
        "CH1:{} CH2:{} CH3:{} CH4:{}",
        on_off(nr52 & 0x01 != 0),
        on_off(nr52 & 0x02 != 0),
        on_off(nr52 & 0x04 != 0),
        on_off(nr52 & 0x08 != 0),
    );
    let nr50 = apu.read_nr50();
    let left_vol = (nr50 >> 4) & 0x07;
    let right_vol = nr50 & 0x07;
    let nr51 = apu.read_nr51();
    vec![
        format!("Power:{} {chans}", on_off(nr52 & 0x80 != 0)),
        format!("Vol L:{left_vol} R:{right_vol}  Pan:{nr51:02X}"),
    ]
}

/// Sidebar header: run state (RUNNING/PAUSED) and breakpoint/watchpoint
/// counts -- moved up from the old status-line `status_summary` so it's
/// grouped with the rest of the always-on info.
pub fn run_state_line(app: &App) -> String {
    let run_state = match app.run_mode {
        crate::app::RunMode::Running => "RUNNING",
        crate::app::RunMode::Paused => "PAUSED",
    };
    let bp = app.breakpoints.pc_breakpoints().count();
    let wp = app.breakpoints.watchpoints().count();
    format!("[{run_state}]  breakpoints:{bp} watchpoints:{wp}")
}

/// Total sidebar width, including borders: wide enough for the longest
/// decoded line (the LCDC line, ~60 columns) plus a little breathing
/// room, since the spec's suggested "~32-34 cols" is too narrow once
/// decoded flags are drafted in.
pub const SIDEBAR_COLS: u16 = 64;

pub struct StatusSidebarWidget<'a> {
    app: &'a mut App,
}

impl<'a> StatusSidebarWidget<'a> {
    pub fn new(app: &'a mut App) -> Self {
        Self { app }
    }
}

fn section_block(title: &'static str, color: Color) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(color))
        .title(Line::styled(title, Style::default().fg(color).add_modifier(Modifier::BOLD)))
}

impl<'a> Widget for StatusSidebarWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let header = run_state_line(self.app);

        let cpu_lines = cpu_lines(&self.app.system.cpu, &self.app.prev_cpu_regs);
        self.app.prev_cpu_regs = self.app.system.cpu.regs;

        let ppu_lines = ppu_lines(&self.app.system.mmu.ppu);
        let timer_lines = timer_lines(&self.app.system.mmu.timer);
        let joypad_line = joypad_line(&self.app.button_last_pressed);
        let cartridge_lines = cartridge_lines(self.app.system.mmu.cartridge.as_ref());
        let apu_lines = apu_lines(&self.app.system.mmu.apu);

        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),                          // header (run state)
                Constraint::Length(cpu_lines.len() as u16 + 2),  // CPU
                Constraint::Length(ppu_lines.len() as u16 + 2),  // PPU
                Constraint::Length(timer_lines.len() as u16 + 2), // Timer
                Constraint::Length(3),                           // Joypad
                Constraint::Length(cartridge_lines.len() as u16 + 2), // Cartridge
                Constraint::Length(apu_lines.len() as u16 + 2),  // APU
                Constraint::Min(0),                              // unused remainder
            ])
            .split(area);

        Paragraph::new(Line::from(header)).render(chunks[0], buf);

        Paragraph::new(cpu_lines)
            .block(section_block("CPU", Color::Cyan))
            .render(chunks[1], buf);

        let ppu_text: Vec<Line> = ppu_lines.into_iter().map(Line::from).collect();
        Paragraph::new(ppu_text)
            .block(section_block("PPU", Color::Magenta))
            .render(chunks[2], buf);

        let timer_text: Vec<Line> = timer_lines.into_iter().map(Line::from).collect();
        Paragraph::new(timer_text)
            .block(section_block("Timer", Color::Yellow))
            .render(chunks[3], buf);

        Paragraph::new(Line::from(joypad_line))
            .block(section_block("Joypad", Color::Green))
            .render(chunks[4], buf);

        let cart_text: Vec<Line> = cartridge_lines.into_iter().map(Line::from).collect();
        Paragraph::new(cart_text)
            .block(section_block("Cartridge", Color::Blue))
            .render(chunks[5], buf);

        let apu_text: Vec<Line> = apu_lines.into_iter().map(Line::from).collect();
        Paragraph::new(apu_text)
            .block(section_block("APU", Color::Red))
            .render(chunks[6], buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decode_lcdc_reports_all_subfields() {
        // LCD on, BG on, window off, OBJ on 8x8, BG map 9800, tile data 8000
        assert_eq!(
            decode_lcdc(0x91),
            "LCDC 91  LCD:ON BG:ON WIN:OFF OBJ:OFF MAP:9800 TILE:8000"
        );
        // OBJ on, 8x16 size, window on, BG map 9C00, tile data 8800
        let lcdc = 0x80 | 0x02 | 0x04 | 0x20 | 0x08;
        assert_eq!(decode_lcdc(lcdc), "LCDC AE  LCD:ON BG:OFF WIN:ON OBJ:ON(8x16) MAP:9C00 TILE:8800");
    }

    #[test]
    fn decode_stat_reports_mode_lyc_and_enabled_interrupts() {
        // Mode 3, LYC=LY set, LYC + VBlank interrupts enabled
        let stat = 0b0101_0111;
        assert_eq!(decode_stat(stat), "STAT 57  Mode:3 LYC=LY:yes  IntEn: LYC VBL");
    }

    #[test]
    fn decode_stat_reports_no_enabled_interrupts_as_a_dash() {
        assert_eq!(decode_stat(0x00), "STAT 00  Mode:0 LYC=LY:no  IntEn: -");
    }

    #[test]
    fn cpu_lines_highlights_only_changed_registers() {
        let mut cpu = Cpu::new();
        cpu.regs.set_af(0x1200);
        cpu.regs.set_bc(0x0034);
        let prev = cpu.regs;

        cpu.regs.set_af(0x9900); // A/F changed
        let lines = cpu_lines(&cpu, &prev);

        assert_eq!(lines[0].style, highlight_style()); // AF line highlighted
        assert_eq!(lines[1].style, Style::default()); // BC line unchanged
    }

    #[test]
    fn joypad_line_lists_held_buttons_in_dpad_then_button_order() {
        let mut held = HashMap::new();
        held.insert(Button::Start, Instant::now());
        held.insert(Button::Up, Instant::now());
        assert_eq!(joypad_line(&held), "Held: Up Start");
    }

    #[test]
    fn joypad_line_reports_a_dash_when_nothing_is_held() {
        let held = HashMap::new();
        assert_eq!(joypad_line(&held), "Held: -");
    }

    #[test]
    fn cartridge_lines_reports_a_placeholder_with_no_cartridge() {
        let lines = cartridge_lines(None);
        assert_eq!(lines, vec!["No cartridge loaded".to_string()]);
    }

    #[test]
    fn apu_lines_decodes_channel_power_from_nr52() {
        let mut apu = Apu::new();
        apu.write_nr52(0x80); // power on, all channels off
        let lines = apu_lines(&apu);
        assert!(lines[0].contains("Power:ON"));
        assert!(lines[0].contains("CH1:OFF"));
    }
}
