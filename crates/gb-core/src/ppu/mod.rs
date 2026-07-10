//! PPU registers, dot-accurate mode sequencing, and BG/window/sprite
//! rendering.
//!
//! `LCDC`/`STAT`/`SCY`/`SCX`/`LY`/`LYC`/`WY`/`WX`/`BGP`/`OBP0`/`OBP1`
//! register storage with the read/write masking real hardware applies
//! (wired onto the MMU bus, see `mmu/mod.rs`), and the Mode 2 (OAM scan,
//! 80 dots) -> Mode 3 (Drawing, variable) -> Mode 0 (HBlank, remainder to
//! 456 dots/line) sequence, repeated for 144 visible lines, followed by
//! Mode 1 (VBlank) for 10 lines (154 lines/frame, 70224 dots/frame total).
//!
//! VRAM (0x8000-0x9FFF) and OAM (0xFE00-0xFE9F) live here rather than in
//! `Mmu`'s flat array, same reasoning as the registers: the PPU is the
//! owner of that address range, and it's the component that actually needs
//! to fetch from them.
//!
//! Rendering is a per-scanline renderer, not a cycle-accurate pixel FIFO: a
//! full scanline (BG, then window, then sprites) is composited in one shot
//! at the Mode 3 -> Mode 0 (Drawing -> HBlank) boundary, using whatever
//! register/VRAM/OAM state is live at that instant. This gets tile
//! addressing, palettes, scrolling, and BG/window/sprite priority right,
//! but does not model mid-scanline register writes taking effect
//! part-way across a line (a real pixel FIFO would) or the sprite/window
//! Mode 3 length penalties — `mode3_len` only accounts for the SCX
//! fine-scroll penalty. `dmg-acid2`/Mealybug need that finer-grained
//! accuracy; this is a solid first cut pending those ROMs to test against.
//! STAT interrupt blocking (the "STAT IRQ bug" hardware quirk between mode
//! 2 and mode 3 on the same dot) is also not modeled — no currently
//! targeted test exercises it.

/// VBlank interrupt request bit (bit 0) in `IF`/`IE`.
pub const VBLANK_INT_BIT: u8 = 1 << 0;
/// STAT interrupt request bit (bit 1) in `IF`/`IE`.
pub const STAT_INT_BIT: u8 = 1 << 1;

/// Visible framebuffer dimensions.
pub const SCREEN_WIDTH: usize = 160;
pub const SCREEN_HEIGHT: usize = 144;

const VRAM_SIZE: usize = 0x2000;
const OAM_SIZE: usize = 0xA0;
/// First address of VRAM (0x8000-0x9FFF); callers pass full bus addresses
/// to [`Ppu::read_vram`]/[`Ppu::write_vram`].
pub const VRAM_BASE: u16 = 0x8000;
/// First address of OAM (0xFE00-0xFE9F); callers pass full bus addresses
/// to [`Ppu::read_oam`]/[`Ppu::write_oam`].
pub const OAM_BASE: u16 = 0xFE00;

const LCDC_ENABLE_BIT: u8 = 1 << 7;
const LCDC_WINDOW_TILE_MAP: u8 = 1 << 6;
const LCDC_WINDOW_ENABLE: u8 = 1 << 5;
const LCDC_TILE_DATA_SELECT: u8 = 1 << 4;
const LCDC_BG_TILE_MAP: u8 = 1 << 3;
const LCDC_OBJ_SIZE: u8 = 1 << 2;
const LCDC_OBJ_ENABLE: u8 = 1 << 1;
const LCDC_BG_WINDOW_ENABLE: u8 = 1 << 0;

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

/// Sprites (OBJ) per scanline real hardware selects, in OAM order, before
/// applying X-coordinate priority.
const MAX_SPRITES_PER_LINE: usize = 10;
const OAM_ENTRY_SIZE: usize = 4;
const OAM_ENTRY_COUNT: usize = OAM_SIZE / OAM_ENTRY_SIZE;

/// PPU mode, encoded to match `STAT` bits 0-1 (`Drawing` = 3, etc).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum Mode {
    #[default]
    HBlank = 0,
    VBlank = 1,
    OamScan = 2,
    Drawing = 3,
}

#[derive(Clone)]
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

    vram: [u8; VRAM_SIZE],
    oam: [u8; OAM_SIZE],
    /// Internal window line counter: increments only on scanlines where
    /// the window was actually drawn, resets every frame. This is what
    /// makes the window "remember" its position across an SCY-scrolled BG.
    window_line: u8,
    /// One shade index (0-3, post-palette) per pixel, row-major,
    /// `SCREEN_WIDTH` x `SCREEN_HEIGHT`. Filled one scanline at a time.
    framebuffer: [u8; SCREEN_WIDTH * SCREEN_HEIGHT],
}

impl std::fmt::Debug for Ppu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Ppu")
            .field("lcdc", &self.lcdc)
            .field("stat", &self.stat)
            .field("scy", &self.scy)
            .field("scx", &self.scx)
            .field("ly", &self.ly)
            .field("lyc", &self.lyc)
            .field("wy", &self.wy)
            .field("wx", &self.wx)
            .field("bgp", &self.bgp)
            .field("obp0", &self.obp0)
            .field("obp1", &self.obp1)
            .field("mode", &self.mode)
            .field("dot", &self.dot)
            .field("vram", &"[u8; 8192]")
            .field("oam", &"[u8; 160]")
            .field("framebuffer", &"[u8; 23040]")
            .finish()
    }
}

impl Default for Ppu {
    fn default() -> Self {
        Self {
            lcdc: 0,
            stat: 0,
            scy: 0,
            scx: 0,
            ly: 0,
            lyc: 0,
            wy: 0,
            wx: 0,
            bgp: 0,
            obp0: 0,
            obp1: 0,
            mode: Mode::default(),
            dot: 0,
            stat_line: false,
            vblank_interrupt_pending: false,
            stat_interrupt_pending: false,
            vram: [0; VRAM_SIZE],
            oam: [0; OAM_SIZE],
            window_line: 0,
            framebuffer: [0; SCREEN_WIDTH * SCREEN_HEIGHT],
        }
    }
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
            self.window_line = 0;
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

    pub fn read_vram(&self, addr: u16) -> u8 {
        self.vram[(addr - VRAM_BASE) as usize]
    }

    pub fn write_vram(&mut self, addr: u16, val: u8) {
        self.vram[(addr - VRAM_BASE) as usize] = val;
    }

    pub fn read_oam(&self, addr: u16) -> u8 {
        self.oam[(addr - OAM_BASE) as usize]
    }

    pub fn write_oam(&mut self, addr: u16, val: u8) {
        self.oam[(addr - OAM_BASE) as usize] = val;
    }

    /// The rendered framebuffer: one shade index (0-3, post-palette) per
    /// pixel, row-major. Only rows up to the current `LY` reflect this
    /// frame's contents until a full frame has been rendered.
    pub fn framebuffer(&self) -> &[u8; SCREEN_WIDTH * SCREEN_HEIGHT] {
        &self.framebuffer
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
                self.window_line = 0;
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
        if self.mode == Mode::Drawing && mode == Mode::HBlank {
            self.render_scanline();
        }
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

    /// Tile data base address for a BG/window tile index, honoring
    /// `LCDC` bit 4: unsigned 0x8000-based addressing when set, signed
    /// 0x9000-based (`tile_index` as `i8`) when clear.
    fn bg_tile_data_addr(&self, tile_index: u8) -> u16 {
        if self.lcdc & LCDC_TILE_DATA_SELECT != 0 {
            VRAM_BASE + (tile_index as u16) * 16
        } else {
            (0x9000_i32 + (tile_index as i8 as i32) * 16) as u16
        }
    }

    fn tile_row_bytes(&self, tile_data_addr: u16, row_in_tile: u8) -> (u8, u8) {
        let addr = tile_data_addr + (row_in_tile as u16) * 2;
        (self.read_vram(addr), self.read_vram(addr + 1))
    }

    fn color_id_from_row(lo: u8, hi: u8, col_in_tile: u8) -> u8 {
        let bit = 7 - col_in_tile;
        (((hi >> bit) & 1) << 1) | ((lo >> bit) & 1)
    }

    fn apply_palette(palette: u8, color_id: u8) -> u8 {
        (palette >> (color_id * 2)) & 0b11
    }

    /// Composites one full scanline (BG, then window, then sprites) into
    /// `framebuffer[LY]`. Called once per line at the Mode 3 -> Mode 0
    /// boundary — see the module doc for the "not a pixel FIFO" caveat.
    fn render_scanline(&mut self) {
        let y = self.ly as usize;
        if y >= SCREEN_HEIGHT {
            return;
        }

        let mut bg_color_ids = [0u8; SCREEN_WIDTH];
        if self.lcdc & LCDC_BG_WINDOW_ENABLE != 0 {
            self.render_bg_row(&mut bg_color_ids);
            if self.lcdc & LCDC_WINDOW_ENABLE != 0 && self.wy <= self.ly {
                self.render_window_row(&mut bg_color_ids);
            }
        }

        let mut line_shades = [0u8; SCREEN_WIDTH];
        for (shade, &color_id) in line_shades.iter_mut().zip(bg_color_ids.iter()) {
            *shade = Self::apply_palette(self.bgp, color_id);
        }

        if self.lcdc & LCDC_OBJ_ENABLE != 0 {
            self.render_sprite_row(&bg_color_ids, &mut line_shades);
        }

        let row_start = y * SCREEN_WIDTH;
        self.framebuffer[row_start..row_start + SCREEN_WIDTH].copy_from_slice(&line_shades);
    }

    fn render_bg_row(&self, bg_color_ids: &mut [u8; SCREEN_WIDTH]) {
        let map_base: u16 = if self.lcdc & LCDC_BG_TILE_MAP != 0 { 0x9C00 } else { 0x9800 };
        let bg_y = self.ly.wrapping_add(self.scy);
        let tile_row = (bg_y / 8) as u16;
        let row_in_tile = bg_y % 8;
        for (x, id) in bg_color_ids.iter_mut().enumerate() {
            let bg_x = (x as u8).wrapping_add(self.scx);
            let tile_col = (bg_x / 8) as u16;
            let col_in_tile = bg_x % 8;
            let tile_index = self.read_vram(map_base + tile_row * 32 + tile_col);
            let data_addr = self.bg_tile_data_addr(tile_index);
            let (lo, hi) = self.tile_row_bytes(data_addr, row_in_tile);
            *id = Self::color_id_from_row(lo, hi, col_in_tile);
        }
    }

    /// Overwrites `bg_color_ids` with window pixels wherever the window is
    /// visible on this row (`x >= WX-7`). Advances `window_line` if any
    /// pixel was actually drawn.
    fn render_window_row(&mut self, bg_color_ids: &mut [u8; SCREEN_WIDTH]) {
        let map_base: u16 = if self.lcdc & LCDC_WINDOW_TILE_MAP != 0 { 0x9C00 } else { 0x9800 };
        let win_x_start = self.wx as i16 - 7;
        let tile_row = (self.window_line / 8) as u16;
        let row_in_tile = self.window_line % 8;
        let mut drawn = false;
        for (x, id) in bg_color_ids.iter_mut().enumerate() {
            let win_x = x as i16 - win_x_start;
            if win_x < 0 {
                continue;
            }
            let win_x = win_x as u16;
            let tile_col = win_x / 8;
            let col_in_tile = (win_x % 8) as u8;
            let tile_index = self.read_vram(map_base + tile_row * 32 + tile_col);
            let data_addr = self.bg_tile_data_addr(tile_index);
            let (lo, hi) = self.tile_row_bytes(data_addr, row_in_tile);
            *id = Self::color_id_from_row(lo, hi, col_in_tile);
            drawn = true;
        }
        if drawn {
            self.window_line += 1;
        }
    }

    /// Draws up to [`MAX_SPRITES_PER_LINE`] OBJs intersecting this
    /// scanline into `line_shades`, respecting X-coordinate + OAM-index
    /// priority (lower X wins; ties go to the lower OAM index) and each
    /// sprite's BG-over-OBJ attribute bit.
    fn render_sprite_row(&self, bg_color_ids: &[u8; SCREEN_WIDTH], line_shades: &mut [u8; SCREEN_WIDTH]) {
        let height: i16 = if self.lcdc & LCDC_OBJ_SIZE != 0 { 16 } else { 8 };
        let ly = self.ly as i16;

        // (OAM index, screen X) for sprites intersecting this line, in OAM
        // order, hardware-limited to the first 10 found.
        let mut visible: [(usize, i16); MAX_SPRITES_PER_LINE] = [(0, 0); MAX_SPRITES_PER_LINE];
        let mut visible_len = 0;
        for i in 0..OAM_ENTRY_COUNT {
            let oam_y = self.oam[i * OAM_ENTRY_SIZE] as i16 - 16;
            if ly >= oam_y && ly < oam_y + height {
                let oam_x = self.oam[i * OAM_ENTRY_SIZE + 1] as i16 - 8;
                visible[visible_len] = (i, oam_x);
                visible_len += 1;
                if visible_len == MAX_SPRITES_PER_LINE {
                    break;
                }
            }
        }
        let visible = &mut visible[..visible_len];
        // Stable sort: equal-X ties keep OAM order, giving the lower OAM
        // index priority as required.
        visible.sort_by_key(|&(_, x)| x);

        // Draw lowest priority first so the highest-priority sprite (index
        // 0 after sorting) ends up on top.
        for &(oam_index, sx) in visible.iter().rev() {
            let base = oam_index * OAM_ENTRY_SIZE;
            let oam_y = self.oam[base] as i16 - 16;
            let tile_index = self.oam[base + 2];
            let attrs = self.oam[base + 3];
            let y_flip = attrs & 0x40 != 0;
            let x_flip = attrs & 0x20 != 0;
            let bg_priority = attrs & 0x80 != 0;
            let palette = if attrs & 0x10 != 0 { self.obp1 } else { self.obp0 };

            let mut row_in_sprite = (ly - oam_y) as u8;
            if y_flip {
                row_in_sprite = height as u8 - 1 - row_in_sprite;
            }
            let tile_idx = if height == 16 {
                if row_in_sprite < 8 { tile_index & 0xFE } else { tile_index | 0x01 }
            } else {
                tile_index
            };
            let data_addr = VRAM_BASE + (tile_idx as u16) * 16;
            let (lo, hi) = self.tile_row_bytes(data_addr, row_in_sprite % 8);

            for col in 0..8i16 {
                let px = sx + col;
                if px < 0 || px as usize >= SCREEN_WIDTH {
                    continue;
                }
                let sample_col = if x_flip { 7 - col as u8 } else { col as u8 };
                let color_id = Self::color_id_from_row(lo, hi, sample_col);
                if color_id == 0 {
                    continue; // transparent
                }
                if bg_priority && bg_color_ids[px as usize] != 0 {
                    continue; // BG-over-OBJ: BG wins when non-zero
                }
                line_shades[px as usize] = Self::apply_palette(palette, color_id);
            }
        }
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

    /// LCD-enabled `Ppu` with additional `LCDC` bits set, for rendering
    /// tests.
    fn render_ppu(lcdc_extra: u8) -> Ppu {
        let mut p = Ppu::new();
        p.write_lcdc(LCDC_ENABLE_BIT | lcdc_extra);
        p
    }

    /// Steps `dots` T-cycles in <=255-sized chunks (`step` takes `u8`).
    fn step_dots(p: &mut Ppu, mut dots: u32) {
        while dots > 0 {
            let chunk = dots.min(255) as u8;
            p.step(chunk);
            dots -= chunk as u32;
        }
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

    #[test]
    fn bg_row_decodes_tile_pixels_with_unsigned_addressing_and_palette() {
        let mut p = render_ppu(LCDC_BG_WINDOW_ENABLE | LCDC_TILE_DATA_SELECT);
        // Tile 0 (map(0,0) defaults to index 0) row 0: color ids
        // 0,1,2,3,0,1,2,3 across the 8 columns (col0 = bit7).
        p.write_vram(0x8000, 0x55); // lo bits: 0,1,0,1,0,1,0,1
        p.write_vram(0x8001, 0x33); // hi bits: 0,0,1,1,0,0,1,1
        p.write_bgp(0xE4); // identity mapping: id N -> shade N
        step_dots(&mut p, SCANLINE_DOTS);
        assert_eq!(&p.framebuffer()[0..8], &[0, 1, 2, 3, 0, 1, 2, 3]);
    }

    #[test]
    fn bg_row_uses_signed_tile_addressing_when_lcdc_bit4_clear() {
        let mut p = render_ppu(LCDC_BG_WINDOW_ENABLE); // bit4 clear: signed mode
        p.write_vram(0x9800, 0xFF); // map(0,0) = tile index -1
        p.write_vram(0x8FF0, 0xFF); // 0x9000 + (-1 * 16) = 0x8FF0
        p.write_vram(0x8FF1, 0x00); // id = 1 for every column
        p.write_bgp(0xE4);
        step_dots(&mut p, SCANLINE_DOTS);
        assert_eq!(p.framebuffer()[0], 1);
    }

    #[test]
    fn bg_tile_map_select_bit_chooses_0x9c00() {
        let mut p = render_ppu(LCDC_BG_WINDOW_ENABLE | LCDC_TILE_DATA_SELECT | LCDC_BG_TILE_MAP);
        p.write_vram(0x9C00, 1); // map(0,0) = tile index 1 in the 0x9C00 map
        p.write_vram(0x8010, 0xFF);
        p.write_vram(0x8011, 0x00); // id = 1 for every column
        p.write_bgp(0xE4);
        step_dots(&mut p, SCANLINE_DOTS);
        assert_eq!(p.framebuffer()[0], 1);
    }

    #[test]
    fn window_row_overrides_bg_from_wx_minus_7_onward() {
        let mut p = render_ppu(
            LCDC_BG_WINDOW_ENABLE | LCDC_TILE_DATA_SELECT | LCDC_WINDOW_ENABLE | LCDC_WINDOW_TILE_MAP,
        );
        p.write_wy(0);
        p.write_wx(15); // window visible starting at screen x = 15 - 7 = 8
        p.write_bgp(0xE4);
        // BG: tile index 0 (default, all-zero VRAM) -> color id 0 everywhere.
        // Window: separate 0x9C00 map, tile index 1, all id 3.
        p.write_vram(0x9C00, 1);
        p.write_vram(0x8010, 0xFF);
        p.write_vram(0x8011, 0xFF);
        step_dots(&mut p, SCANLINE_DOTS);
        let fb = p.framebuffer();
        assert_eq!(fb[7], 0); // still BG, just before the window starts
        assert_eq!(fb[8], 3); // window pixel
    }

    #[test]
    fn sprite_x_coordinate_priority_lower_x_wins_overlap() {
        let mut p = render_ppu(LCDC_OBJ_ENABLE); // BG/window off: bg color ids all 0
        p.write_bgp(0xE4);
        p.write_obp0(0xE4);
        p.write_vram(0x8000, 0xFF);
        p.write_vram(0x8001, 0x00); // tile 0: id 1 everywhere
        p.write_vram(0x8010, 0x00);
        p.write_vram(0x8011, 0xFF); // tile 1: id 2 everywhere
        // Sprite A: screen X 0..7 (OAM X=8), tile 0.
        p.write_oam(0xFE00, 16);
        p.write_oam(0xFE01, 8);
        p.write_oam(0xFE02, 0);
        p.write_oam(0xFE03, 0);
        // Sprite B: screen X 4..11 (OAM X=12), tile 1, overlapping A at x=4..7.
        p.write_oam(0xFE04, 16);
        p.write_oam(0xFE05, 12);
        p.write_oam(0xFE06, 1);
        p.write_oam(0xFE07, 0);
        step_dots(&mut p, SCANLINE_DOTS);
        let fb = p.framebuffer();
        assert_eq!(fb[4], 1); // overlap: lower-X sprite A wins
        assert_eq!(fb[10], 2); // outside overlap: sprite B only
    }

    #[test]
    fn bg_over_obj_priority_flag_hides_sprite_when_bg_opaque() {
        let mut p = render_ppu(LCDC_BG_WINDOW_ENABLE | LCDC_TILE_DATA_SELECT | LCDC_OBJ_ENABLE);
        p.write_bgp(0xE4);
        p.write_obp0(0xE4);
        p.write_vram(0x8000, 0xFF);
        p.write_vram(0x8001, 0x00); // BG tile 0: id 1 everywhere (opaque)
        p.write_vram(0x8010, 0x00);
        p.write_vram(0x8011, 0xFF); // sprite tile 1: id 2 everywhere
        p.write_oam(0xFE00, 16);
        p.write_oam(0xFE01, 8);
        p.write_oam(0xFE02, 1);
        p.write_oam(0xFE03, 0x80); // BG-over-OBJ priority bit set
        step_dots(&mut p, SCANLINE_DOTS);
        assert_eq!(p.framebuffer()[0], 1); // BG (opaque, id 1) wins over the sprite
    }

    #[test]
    fn tall_sprite_selects_top_and_bottom_tile_by_row() {
        let mut p = render_ppu(LCDC_OBJ_ENABLE | LCDC_OBJ_SIZE); // 8x16 sprites
        p.write_obp0(0xE4);
        // Tile index 2 (even): top half. Bottom half is always tile 3
        // (index | 1), regardless of the low bit stored in OAM.
        p.write_vram(0x8000 + 2 * 16, 0xFF);
        p.write_vram(0x8000 + 2 * 16 + 1, 0x00); // top tile: id 1
        p.write_vram(0x8000 + 3 * 16, 0x00);
        p.write_vram(0x8000 + 3 * 16 + 1, 0xFF); // bottom tile: id 2
        p.write_oam(0xFE00, 16); // screen Y 0..15
        p.write_oam(0xFE01, 8);
        p.write_oam(0xFE02, 2);
        p.write_oam(0xFE03, 0);
        step_dots(&mut p, SCANLINE_DOTS); // renders line 0 (top tile)
        assert_eq!(p.framebuffer()[0], 1);
        for _ in 0..8 {
            step_dots(&mut p, SCANLINE_DOTS); // advances to line 8 (bottom tile)
        }
        assert_eq!(p.framebuffer()[8 * SCREEN_WIDTH], 2);
    }
}
