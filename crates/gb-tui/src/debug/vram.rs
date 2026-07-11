//! VRAM viewer: tile data (as pixel art), the live BG tile map (hex grid),
//! and the OAM sprite list. Three tabs cycled independently of the main
//! debug-panel Tab key — see `VramTab` in `app.rs`.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Widget};

use gb_core::ppu::Ppu;

use crate::palette::Palette;
use crate::render::video::UPPER_HALF_BLOCK;

const TILE_COUNT: usize = 384; // all of 0x8000-0x97FF, 16 bytes/tile
const TILES_PER_ROW: usize = 16;

/// Decodes one pixel (0-3 color id, unsigned/0x8000-based addressing —
/// the convention for "just browse VRAM" tile viewers) of tile
/// `tile_index` at `(x, y)` within its 8x8 grid.
fn tile_pixel(ppu: &Ppu, tile_index: usize, x: u8, y: u8) -> u8 {
    let base = 0x8000u16 + (tile_index as u16) * 16;
    let addr = base + (y as u16) * 2;
    let lo = ppu.read_vram(addr);
    let hi = ppu.read_vram(addr + 1);
    let bit = 7 - x;
    (((hi >> bit) & 1) << 1) | ((lo >> bit) & 1)
}

/// Renders every tile in VRAM as an 8x8-pixel half-block grid,
/// `TILES_PER_ROW` tiles wide, into `area`.
pub struct TileGridWidget<'a> {
    ppu: &'a Ppu,
    palette: Palette,
}

impl<'a> TileGridWidget<'a> {
    pub fn new(ppu: &'a Ppu, palette: Palette) -> Self {
        Self { ppu, palette }
    }
}

impl<'a> Widget for TileGridWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default().borders(Borders::ALL).title("Tiles");
        let inner = block.inner(area);
        block.render(area, buf);

        let rows_of_tiles = TILE_COUNT.div_ceil(TILES_PER_ROW);
        let visible_tile_rows = (inner.height as usize).min(rows_of_tiles * 4); // 4 half-block rows/tile
        for cell_row in 0..visible_tile_rows.min(inner.height as usize) {
            let tile_row = cell_row / 4;
            let pixel_row_top = ((cell_row % 4) * 2) as u8;
            if tile_row >= rows_of_tiles {
                break;
            }
            for col in 0..(TILES_PER_ROW * 8).min(inner.width as usize) {
                let tile_col = col / 8;
                let px = (col % 8) as u8;
                let tile_index = tile_row * TILES_PER_ROW + tile_col;
                if tile_index >= TILE_COUNT {
                    continue;
                }
                let top = tile_pixel(self.ppu, tile_index, px, pixel_row_top);
                let bottom = tile_pixel(self.ppu, tile_index, px, pixel_row_top + 1);
                let x = inner.x + col as u16;
                let y = inner.y + cell_row as u16;
                if x < inner.x + inner.width && y < inner.y + inner.height {
                    buf[(x, y)]
                        .set_char(UPPER_HALF_BLOCK)
                        .set_fg(self.palette.rgb(top))
                        .set_bg(self.palette.rgb(bottom));
                }
            }
        }
    }
}

/// Text rows for the live BG tile map (`map_base` is `0x9800` or
/// `0x9C00`, per `LCDC` bit 3): each row is one map row's 32 tile indices
/// in hex.
pub fn bg_map_lines(ppu: &Ppu, map_base: u16, rows: u16) -> Vec<String> {
    let mut out = Vec::with_capacity(rows as usize);
    for row in 0..rows.min(32) {
        let mut line = String::new();
        for col in 0..32u16 {
            let addr = map_base + row * 32 + col;
            line.push_str(&format!("{:02X} ", ppu.read_vram(addr)));
        }
        out.push(line);
    }
    out
}

/// One OAM sprite entry, decoded for display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpriteEntry {
    pub index: usize,
    pub y: u8,
    pub x: u8,
    pub tile: u8,
    pub attrs: u8,
}

/// All 40 OAM entries, in OAM order.
pub fn oam_entries(ppu: &Ppu) -> Vec<SpriteEntry> {
    (0..40)
        .map(|i| {
            let base = gb_core::ppu::OAM_BASE + (i as u16) * 4;
            SpriteEntry {
                index: i,
                y: ppu.read_oam(base),
                x: ppu.read_oam(base + 1),
                tile: ppu.read_oam(base + 2),
                attrs: ppu.read_oam(base + 3),
            }
        })
        .collect()
}

pub fn oam_lines(ppu: &Ppu) -> Vec<String> {
    let mut out = vec!["## Y   X  Tile Attr".to_string()];
    out.extend(oam_entries(ppu).iter().map(|s| {
        format!("{:02} {:3} {:3} {:02X}   {:02X}", s.index, s.y, s.x, s.tile, s.attrs)
    }));
    out
}

pub struct OamWidget<'a> {
    ppu: &'a Ppu,
}

impl<'a> OamWidget<'a> {
    pub fn new(ppu: &'a Ppu) -> Self {
        Self { ppu }
    }
}

impl<'a> Widget for OamWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::default().borders(Borders::ALL).title("OAM");
        let text: Vec<Line> = oam_lines(self.ppu).into_iter().map(Line::from).collect();
        Paragraph::new(text).block(block).render(area, buf);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gb_core::ppu::OAM_BASE;

    #[test]
    fn bg_map_lines_reads_32_tile_indices_per_row() {
        let mut ppu = Ppu::new();
        ppu.write_vram(0x9800, 0x11);
        ppu.write_vram(0x9800 + 31, 0x22);
        let lines = bg_map_lines(&ppu, 0x9800, 1);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].starts_with("11 "));
        assert!(lines[0].trim_end().ends_with("22"));
    }

    #[test]
    fn oam_entries_decodes_all_40_sprites() {
        let mut ppu = Ppu::new();
        ppu.write_oam(OAM_BASE, 16);
        ppu.write_oam(OAM_BASE + 1, 8);
        ppu.write_oam(OAM_BASE + 2, 0x05);
        ppu.write_oam(OAM_BASE + 3, 0x80);
        let entries = oam_entries(&ppu);
        assert_eq!(entries.len(), 40);
        assert_eq!(entries[0], SpriteEntry { index: 0, y: 16, x: 8, tile: 0x05, attrs: 0x80 });
    }

    #[test]
    fn tile_pixel_decodes_known_pattern() {
        let mut ppu = Ppu::new();
        ppu.write_vram(0x8000, 0xFF); // row0 lo: all 1s
        ppu.write_vram(0x8001, 0x00); // row0 hi: all 0s -> color id 1 everywhere
        assert_eq!(tile_pixel(&ppu, 0, 0, 0), 1);
        assert_eq!(tile_pixel(&ppu, 0, 7, 0), 1);
    }
}
