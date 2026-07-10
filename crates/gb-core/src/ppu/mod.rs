//! PPU registers: `LCDC`/`STAT`/`SCY`/`SCX`/`LY`/`LYC`/`WY`/`WX`/`BGP`/
//! `OBP0`/`OBP1`.
//!
//! This is still an M2 stub for rendering: register storage and the
//! read/write masking real hardware applies, wired onto the MMU bus (see
//! `mmu/mod.rs`), but no dot-accurate mode sequencing or BG/window/sprite
//! fetch yet. Consequences of that until mode sequencing lands:
//! - `LY` is fixed at 0 (writes are ignored, matching how the real register
//!   behaves once mode sequencing resets it every write anyway).
//! - `STAT`'s mode bits (0-1) always read as mode 0.
//! - The LYC=LY coincidence flag (`STAT` bit 2) is still computed live
//!   against the fixed `LY`, since that part of the comparator doesn't
//!   depend on scanline timing.

const STAT_WRITABLE_MASK: u8 = 0b0111_1000;
const STAT_UNUSED_BIT: u8 = 0b1000_0000;
const STAT_COINCIDENCE_BIT: u8 = 0b0000_0100;

#[derive(Debug, Default, Clone)]
pub struct Ppu {
    lcdc: u8,
    stat: u8,
    scy: u8,
    scx: u8,
    ly: u8,
    lyc: u8,
    wy: u8,
    wx: u8,
    bgp: u8,
    obp0: u8,
    obp1: u8,
}

impl Ppu {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read_lcdc(&self) -> u8 {
        self.lcdc
    }

    pub fn write_lcdc(&mut self, val: u8) {
        self.lcdc = val;
    }

    /// Bit 7 always reads 1 (unused). Bits 3-6 (interrupt-source enables)
    /// are the only CPU-writable bits; bits 0-2 (mode + LYC=LY coincidence)
    /// are hardware-controlled and computed here.
    pub fn read_stat(&self) -> u8 {
        let coincidence = if self.ly == self.lyc {
            STAT_COINCIDENCE_BIT
        } else {
            0
        };
        STAT_UNUSED_BIT | (self.stat & STAT_WRITABLE_MASK) | coincidence
    }

    pub fn write_stat(&mut self, val: u8) {
        self.stat = val & STAT_WRITABLE_MASK;
    }

    pub fn read_scy(&self) -> u8 {
        self.scy
    }

    pub fn write_scy(&mut self, val: u8) {
        self.scy = val;
    }

    pub fn read_scx(&self) -> u8 {
        self.scx
    }

    pub fn write_scx(&mut self, val: u8) {
        self.scx = val;
    }

    pub fn read_ly(&self) -> u8 {
        self.ly
    }

    /// No-op: `LY` is read-only from the CPU's perspective on real
    /// hardware too (writes are ignored once mode sequencing drives it).
    pub fn write_ly(&mut self, _val: u8) {}

    pub fn read_lyc(&self) -> u8 {
        self.lyc
    }

    pub fn write_lyc(&mut self, val: u8) {
        self.lyc = val;
    }

    pub fn read_wy(&self) -> u8 {
        self.wy
    }

    pub fn write_wy(&mut self, val: u8) {
        self.wy = val;
    }

    pub fn read_wx(&self) -> u8 {
        self.wx
    }

    pub fn write_wx(&mut self, val: u8) {
        self.wx = val;
    }

    pub fn read_bgp(&self) -> u8 {
        self.bgp
    }

    pub fn write_bgp(&mut self, val: u8) {
        self.bgp = val;
    }

    pub fn read_obp0(&self) -> u8 {
        self.obp0
    }

    pub fn write_obp0(&mut self, val: u8) {
        self.obp0 = val;
    }

    pub fn read_obp1(&self) -> u8 {
        self.obp1
    }

    pub fn write_obp1(&mut self, val: u8) {
        self.obp1 = val;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lcdc_round_trips_all_bits() {
        let mut p = Ppu::new();
        p.write_lcdc(0xA5);
        assert_eq!(p.read_lcdc(), 0xA5);
    }

    #[test]
    fn stat_masks_unwritable_bits_and_forces_unused_bit_high() {
        let mut p = Ppu::new();
        p.write_stat(0xFF);
        // Bit 7 forced 1, bits 3-6 kept, bits 0-2 hardware-controlled (mode
        // 0, and LYC=LY happens to be true here since both default to 0).
        assert_eq!(p.read_stat(), 0b1111_1100);
    }

    #[test]
    fn stat_coincidence_flag_tracks_ly_vs_lyc() {
        let mut p = Ppu::new();
        p.write_lyc(1);
        assert_eq!(p.read_stat() & STAT_COINCIDENCE_BIT, 0); // LY=0, LYC=1
        p.write_lyc(0);
        assert_eq!(p.read_stat() & STAT_COINCIDENCE_BIT, STAT_COINCIDENCE_BIT);
    }

    #[test]
    fn ly_is_read_only() {
        let mut p = Ppu::new();
        p.write_ly(99);
        assert_eq!(p.read_ly(), 0);
    }

    #[test]
    fn scroll_and_window_registers_round_trip() {
        let mut p = Ppu::new();
        p.write_scy(1);
        p.write_scx(2);
        p.write_wy(3);
        p.write_wx(4);
        assert_eq!((p.read_scy(), p.read_scx(), p.read_wy(), p.read_wx()), (1, 2, 3, 4));
    }

    #[test]
    fn palette_registers_round_trip() {
        let mut p = Ppu::new();
        p.write_bgp(0x1B);
        p.write_obp0(0x2C);
        p.write_obp1(0x3D);
        assert_eq!(
            (p.read_bgp(), p.read_obp0(), p.read_obp1()),
            (0x1B, 0x2C, 0x3D)
        );
    }
}
