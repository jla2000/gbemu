//! PPU registers plus dot-accurate mode sequencing.
//!
//! `LCDC`/`STAT`/`SCY`/`SCX`/`LY`/`LYC`/`WY`/`WX`/`BGP`/`OBP0`/`OBP1`
//! register storage with the read/write masking real hardware applies
//! (wired onto the MMU bus, see `mmu/mod.rs`), and the Mode 2 (OAM scan,
//! 80 dots) -> Mode 3 (Drawing, variable) -> Mode 0 (HBlank, remainder to
//! 456 dots/line) sequence, repeated for 144 visible lines, followed by
//! Mode 1 (VBlank) for 10 lines (154 lines/frame, 70224 dots/frame total).
//!
//! Still missing (lands with the BG/window/sprite fetch checkbox): actual
//! pixel output, and the sprite/window Mode 3 length penalties — `mode3_len`
//! currently only accounts for the SCX fine-scroll penalty, so this is not
//! yet dot-accurate enough for `dmg-acid2`/Mealybug. STAT interrupt
//! blocking (the "STAT IRQ bug" hardware quirk between mode 2 and mode 3 on
//! the same dot) is also not modeled — no currently targeted test exercises
//! it.

/// VBlank interrupt request bit (bit 0) in `IF`/`IE`.
pub const VBLANK_INT_BIT: u8 = 1 << 0;
/// STAT interrupt request bit (bit 1) in `IF`/`IE`.
pub const STAT_INT_BIT: u8 = 1 << 1;

const LCDC_ENABLE_BIT: u8 = 1 << 7;

const STAT_HBLANK_INT_ENABLE: u8 = 1 << 3;
const STAT_VBLANK_INT_ENABLE: u8 = 1 << 4;
const STAT_OAM_INT_ENABLE: u8 = 1 << 5;
const STAT_LYC_INT_ENABLE: u8 = 1 << 6;
const STAT_WRITABLE_MASK: u8 =
    STAT_HBLANK_INT_ENABLE | STAT_VBLANK_INT_ENABLE | STAT_OAM_INT_ENABLE | STAT_LYC_INT_ENABLE;
const STAT_UNUSED_BIT: u8 = 0b1000_0000;
const STAT_COINCIDENCE_BIT: u8 = 0b0000_0100;

const MODE2_DOTS: u32 = 80;
const SCANLINE_DOTS: u32 = 456;
const LINES_PER_FRAME: u8 = 154;
/// `LY` value at which Mode 1 (VBlank) begins; a frame is "done" once `LY`
/// reaches this. Exposed for `System::run_frame`'s VBlank-edge loop.
pub const VBLANK_START_LINE: u8 = 144;

/// PPU mode, encoded to match `STAT` bits 0-1 (`Drawing` = 3, etc).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Mode {
    #[default]
    HBlank = 0,
    VBlank = 1,
    OamScan = 2,
    Drawing = 3,
}

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

    mode: Mode,
    /// Dots elapsed within the current scanline (0..SCANLINE_DOTS).
    dot: u32,
    /// Level of the STAT interrupt OR-line as of the last update, so a
    /// request is only raised on a 0->1 transition (real hardware is
    /// edge-triggered here, not level-triggered).
    stat_line: bool,
    vblank_interrupt_pending: bool,
    stat_interrupt_pending: bool,
}

impl Ppu {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read_lcdc(&self) -> u8 {
        self.lcdc
    }

    /// Turning the LCD off (bit 7 falling) freezes and resets `LY`/dot/mode
    /// to their power-on-equivalent state; turning it back on restarts the
    /// frame from line 0, dot 0, Mode 2 — matching real hardware.
    pub fn write_lcdc(&mut self, val: u8) {
        let was_enabled = self.enabled();
        self.lcdc = val;
        if self.enabled() != was_enabled {
            self.dot = 0;
            self.ly = 0;
            if self.enabled() {
                self.update_mode();
            } else {
                self.mode = Mode::HBlank;
                self.stat_line = false;
            }
        }
    }

    fn enabled(&self) -> bool {
        self.lcdc & LCDC_ENABLE_BIT != 0
    }

    /// Whether the LCD is currently on (`LCDC` bit 7). Exposed so
    /// `System::run_frame` can avoid spinning forever waiting for a VBlank
    /// that a disabled LCD will never signal.
    pub fn lcd_enabled(&self) -> bool {
        self.enabled()
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
        STAT_UNUSED_BIT | (self.stat & STAT_WRITABLE_MASK) | coincidence | (self.mode as u8)
    }

    pub fn write_stat(&mut self, val: u8) {
        self.stat = val & STAT_WRITABLE_MASK;
        self.update_stat_interrupt_line();
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

    /// No-op: writes to `LY` are ignored on real hardware.
    pub fn write_ly(&mut self, _val: u8) {}

    pub fn read_lyc(&self) -> u8 {
        self.lyc
    }

    pub fn write_lyc(&mut self, val: u8) {
        self.lyc = val;
        self.update_stat_interrupt_line();
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

    /// Advances the mode sequencer by `t_cycles` T-cycles (== dots). No-op
    /// while the LCD is off. Called from `Mmu::step_ppu` once per CPU
    /// instruction with its elapsed cycle count.
    pub fn step(&mut self, t_cycles: u8) {
        if !self.enabled() {
            return;
        }
        for _ in 0..t_cycles {
            self.tick_dot();
        }
    }

    /// Consumes and returns whether VBlank (line 144) was entered since the
    /// last call.
    pub fn take_vblank_interrupt(&mut self) -> bool {
        std::mem::take(&mut self.vblank_interrupt_pending)
    }

    /// Consumes and returns whether the STAT interrupt line had a rising
    /// edge since the last call.
    pub fn take_stat_interrupt(&mut self) -> bool {
        std::mem::take(&mut self.stat_interrupt_pending)
    }

    fn tick_dot(&mut self) {
        self.dot += 1;
        if self.dot >= SCANLINE_DOTS {
            self.dot = 0;
            self.ly += 1;
            if self.ly >= LINES_PER_FRAME {
                self.ly = 0;
            }
            if self.ly == VBLANK_START_LINE {
                self.vblank_interrupt_pending = true;
            }
        }
        self.update_mode();
    }

    /// Mode 3's length in dots. Real hardware varies this with sprite
    /// fetches and a mid-scanline window trigger on top of the SCX
    /// fine-scroll penalty modeled here; those land with sprite/window
    /// fetch.
    fn mode3_len(&self) -> u32 {
        172 + (self.scx % 8) as u32
    }

    fn update_mode(&mut self) {
        let mode = if self.ly >= VBLANK_START_LINE {
            Mode::VBlank
        } else if self.dot < MODE2_DOTS {
            Mode::OamScan
        } else if self.dot < MODE2_DOTS + self.mode3_len() {
            Mode::Drawing
        } else {
            Mode::HBlank
        };
        self.mode = mode;
        self.update_stat_interrupt_line();
    }

    fn update_stat_interrupt_line(&mut self) {
        let coincidence = self.ly == self.lyc;
        let line = (self.mode == Mode::HBlank && self.stat & STAT_HBLANK_INT_ENABLE != 0)
            || (self.mode == Mode::VBlank && self.stat & STAT_VBLANK_INT_ENABLE != 0)
            || (self.mode == Mode::OamScan && self.stat & STAT_OAM_INT_ENABLE != 0)
            || (coincidence && self.stat & STAT_LYC_INT_ENABLE != 0);
        if line && !self.stat_line {
            self.stat_interrupt_pending = true;
        }
        self.stat_line = line;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled_ppu() -> Ppu {
        let mut p = Ppu::new();
        p.write_lcdc(LCDC_ENABLE_BIT);
        p
    }

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
        let mut p = enabled_ppu();
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

    #[test]
    fn mode_sequence_advances_oam_drawing_hblank_within_a_line() {
        let mut p = enabled_ppu();
        assert_eq!(p.read_stat() & 0b11, Mode::OamScan as u8);
        p.step(79);
        assert_eq!(p.read_stat() & 0b11, Mode::OamScan as u8);
        p.step(1); // dot 80
        assert_eq!(p.read_stat() & 0b11, Mode::Drawing as u8);
        p.step(171); // dot 251, mode3_len == 172 with scx=0
        assert_eq!(p.read_stat() & 0b11, Mode::Drawing as u8);
        p.step(1); // dot 252
        assert_eq!(p.read_stat() & 0b11, Mode::HBlank as u8);
        p.step(204); // dot 456 -> wraps to next line, dot 0
        assert_eq!(p.read_ly(), 1);
        assert_eq!(p.read_stat() & 0b11, Mode::OamScan as u8);
    }

    #[test]
    fn entering_vblank_at_line_144_requests_interrupt() {
        // step() takes u8, so drive it in per-scanline chunks.
        let mut p = enabled_ppu();
        for _ in 0..144 {
            p.step(255);
            p.step((SCANLINE_DOTS - 255) as u8);
        }
        assert_eq!(p.read_ly(), 144);
        assert_eq!(p.read_stat() & 0b11, Mode::VBlank as u8);
        assert!(p.take_vblank_interrupt());
        assert!(!p.take_vblank_interrupt());
    }

    #[test]
    fn frame_wraps_after_154_lines() {
        let mut p = enabled_ppu();
        for _ in 0..LINES_PER_FRAME as u32 {
            p.step(255);
            p.step((SCANLINE_DOTS - 255) as u8);
        }
        assert_eq!(p.read_ly(), 0);
        assert_eq!(p.read_stat() & 0b11, Mode::OamScan as u8);
    }

    #[test]
    fn lyc_interrupt_fires_once_on_coincidence_rising_edge() {
        let mut p = enabled_ppu();
        p.write_lyc(1); // LY=0 != LYC=1: no coincidence yet
        p.write_stat(STAT_LYC_INT_ENABLE); // enabling here is not itself an edge
        assert!(!p.take_stat_interrupt());
        p.step(255);
        p.step(200); // 455 dots total: not quite a full line yet
        assert!(!p.take_stat_interrupt());
        p.step(1); // 456th dot crosses into line 1 == LYC
        assert!(p.take_stat_interrupt());
        assert!(!p.take_stat_interrupt()); // consumed, no repeat mid-line
    }

    #[test]
    fn disabling_lcd_freezes_ly_and_resets_mode() {
        let mut p = enabled_ppu();
        p.step(100); // into Mode 3 on line 0
        p.write_lcdc(0); // disable
        assert_eq!(p.read_ly(), 0);
        assert_eq!(p.read_stat() & 0b11, Mode::HBlank as u8);
        p.step(255); // stepping while disabled is a no-op
        assert_eq!(p.read_ly(), 0);
    }
}
