//! Memory viewer: a scrollable hex dump, 16 bytes/row, jump-to-address via
//! `App::mem_viewer_addr`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use gb_core::cpu::Bus;
use gb_core::mmu::Mmu;

use crate::debug::breakpoints::Breakpoints;

const BYTES_PER_ROW: u16 = 16;

/// Builds `row_count` hex-dump rows of 16 bytes each, starting at
/// `start_addr` rounded down to a 16-byte boundary. Reads through the
/// live `Bus`, so this reflects exactly what the CPU would see (including
/// the OAM-DMA access gate). A byte with a watchpoint set (see
/// `Breakpoints`) is marked with a trailing `*` instead of a space.
pub fn lines(mmu: &mut Mmu, start_addr: u16, row_count: u16, breakpoints: &Breakpoints) -> Vec<String> {
    let aligned_start = start_addr - (start_addr % BYTES_PER_ROW);
    let mut out = Vec::with_capacity(row_count as usize);
    for row in 0..row_count {
        let row_addr = aligned_start.wrapping_add(row * BYTES_PER_ROW);
        let mut line = format!("{row_addr:04X}: ");
        let mut ascii = String::with_capacity(BYTES_PER_ROW as usize);
        for col in 0..BYTES_PER_ROW {
            // Wraps at the top of the address space rather than erroring —
            // an acceptable debug-view edge case, not a real access.
            let addr = row_addr.wrapping_add(col);
            let byte = mmu.read(addr);
            let sep = if breakpoints.has_watch(addr) { '*' } else { ' ' };
            line.push_str(&format!("{byte:02X}{sep}"));
            ascii.push(if (0x20..0x7F).contains(&byte) { byte as char } else { '.' });
        }
        line.push_str(" |");
        line.push_str(&ascii);
        line.push('|');
        out.push(line);
    }
    out
}

pub struct MemoryWidget<'a> {
    mmu: &'a mut Mmu,
    start_addr: u16,
    breakpoints: &'a Breakpoints,
}

impl<'a> MemoryWidget<'a> {
    pub fn new(mmu: &'a mut Mmu, start_addr: u16, breakpoints: &'a Breakpoints) -> Self {
        Self { mmu, start_addr, breakpoints }
    }
}

impl<'a> Widget for MemoryWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let rows = area.height.saturating_sub(2); // minus the block's borders
        let block = Block::default().borders(Borders::ALL).title("Memory");
        let text: Vec<Line> = lines(self.mmu, self.start_addr, rows, self.breakpoints)
            .into_iter()
            .map(Line::from)
            .collect();
        Paragraph::new(text).block(block).render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_are_16_byte_aligned_and_show_hex_plus_ascii() {
        let mut mmu = Mmu::new();
        mmu.write(0xC000, b'H');
        mmu.write(0xC001, b'i');
        mmu.write(0xC002, 0x00); // non-printable -> '.'
        let bp = Breakpoints::new();
        let out = lines(&mut mmu, 0xC000, 1, &bp);
        assert_eq!(out.len(), 1);
        assert!(out[0].starts_with("C000:"));
        assert!(out[0].contains("48 69 00"));
        assert!(out[0].contains("|Hi."));
    }

    #[test]
    fn start_address_rounds_down_to_row_boundary() {
        let mut mmu = Mmu::new();
        let bp = Breakpoints::new();
        let out = lines(&mut mmu, 0xC005, 1, &bp);
        assert!(out[0].starts_with("C000:")); // 0xC005 rounds down to 0xC000
    }

    #[test]
    fn row_count_controls_number_of_lines() {
        let mut mmu = Mmu::new();
        let bp = Breakpoints::new();
        let out = lines(&mut mmu, 0xC000, 4, &bp);
        assert_eq!(out.len(), 4);
        assert_eq!(out[1].split(':').next().unwrap(), "C010");
    }

    #[test]
    fn watched_byte_is_marked_with_an_asterisk() {
        let mut mmu = Mmu::new();
        let mut bp = Breakpoints::new();
        bp.toggle_watch(0xC001, mmu.read(0xC001));
        let out = lines(&mut mmu, 0xC000, 1, &bp);
        assert!(out[0].contains("00 00*00 ")); // byte at C001 marked, neighbors aren't
    }
}
