//! Registers/flags panel: dumps the CPU's 8/16-bit registers and Z/N/H/C
//! flags as text.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use gb_core::cpu::Cpu;

pub struct RegistersWidget<'a> {
    cpu: &'a Cpu,
}

impl<'a> RegistersWidget<'a> {
    pub fn new(cpu: &'a Cpu) -> Self {
        Self { cpu }
    }
}

/// Builds the display lines independent of any ratatui types, so this is
/// testable without constructing a `Buffer`.
pub fn lines(cpu: &Cpu) -> Vec<String> {
    let r = &cpu.regs;
    vec![
        format!("AF: {:04X}  (A:{:02X} F:{:02X})", r.af(), r.a, r.f),
        format!("BC: {:04X}  (B:{:02X} C:{:02X})", r.bc(), r.b, r.c),
        format!("DE: {:04X}  (D:{:02X} E:{:02X})", r.de(), r.d, r.e),
        format!("HL: {:04X}  (H:{:02X} L:{:02X})", r.hl(), r.h, r.l),
        format!("SP: {:04X}", r.sp),
        format!("PC: {:04X}", r.pc),
        String::new(),
        format!(
            "Flags: Z:{} N:{} H:{} C:{}",
            flag_char(r.zero()),
            flag_char(r.subtract()),
            flag_char(r.half_carry()),
            flag_char(r.carry()),
        ),
        format!("IME: {}", if cpu.ime { '1' } else { '0' }),
        format!("HALT: {}", if cpu.halted { '1' } else { '0' }),
    ]
}

fn flag_char(set: bool) -> char {
    if set {
        '1'
    } else {
        '0'
    }
}

impl<'a> Widget for RegistersWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default().borders(Borders::ALL).title("Registers");
        let text: Vec<Line> = lines(self.cpu).into_iter().map(Line::from).collect();
        Paragraph::new(text).block(block).render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_report_register_pairs_and_flags() {
        let mut cpu = Cpu::new();
        cpu.regs.set_af(0x1230); // F's low nibble is always masked to 0
        cpu.regs.pc = 0x0150;
        cpu.ime = true;
        let out = lines(&cpu);
        assert!(out[0].contains("1230"));
        assert!(out.iter().any(|l| l.contains("0150")));
        assert!(out.iter().any(|l| l.contains("IME: 1")));
    }

    #[test]
    fn flags_render_as_zero_or_one() {
        let mut cpu = Cpu::new();
        cpu.regs.set_af(0x00F0); // all flags set (Z N H C)
        let out = lines(&cpu);
        let flags_line = out.iter().find(|l| l.starts_with("Flags")).unwrap();
        assert_eq!(flags_line, "Flags: Z:1 N:1 H:1 C:1");
    }
}
