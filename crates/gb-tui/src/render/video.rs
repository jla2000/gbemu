//! Half-block video widget: renders the GB 160x144 framebuffer into the
//! terminal using half-block characters and truecolor RGB.
//!
//! One terminal cell covers 2 vertical GB pixels via `▀` (upper half
//! block): the character's foreground color is the top pixel, its
//! background color is the bottom pixel. Writes buffer cells directly
//! rather than going through `Canvas`, which is built around
//! braille/marker abstractions not needed for a fixed pixel grid.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::widgets::Widget;

use gb_core::ppu::{SCREEN_HEIGHT, SCREEN_WIDTH};

use crate::palette::Palette;

/// GB screen resolution in pixels.
pub const GB_WIDTH: u16 = SCREEN_WIDTH as u16;
pub const GB_HEIGHT: u16 = SCREEN_HEIGHT as u16;

/// Terminal cell area required to render the GB screen at full resolution
/// (one cell = 2 vertical pixels via a half-block character).
pub const SCREEN_COLS: u16 = GB_WIDTH;
pub const SCREEN_ROWS: u16 = GB_HEIGHT / 2;

pub(crate) const UPPER_HALF_BLOCK: char = '\u{2580}'; // '▀'

/// Renders a PPU framebuffer (shade indices 0-3, row-major, `SCREEN_WIDTH`
/// x `SCREEN_HEIGHT`) into the terminal via `palette`.
pub struct VideoWidget<'a> {
    framebuffer: &'a [u8; SCREEN_WIDTH * SCREEN_HEIGHT],
    palette: Palette,
}

impl<'a> VideoWidget<'a> {
    pub fn new(framebuffer: &'a [u8; SCREEN_WIDTH * SCREEN_HEIGHT], palette: Palette) -> Self {
        Self { framebuffer, palette }
    }

    fn shade_at(&self, x: usize, y: usize) -> u8 {
        self.framebuffer[y * SCREEN_WIDTH + x]
    }
}

impl<'a> Widget for VideoWidget<'a> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let cols = area.width.min(SCREEN_COLS);
        let rows = area.height.min(SCREEN_ROWS);
        for row in 0..rows {
            let top_y = (row * 2) as usize;
            let bottom_y = top_y + 1;
            for col in 0..cols {
                let x = col as usize;
                let top = self.palette.rgb(self.shade_at(x, top_y));
                let bottom = self.palette.rgb(self.shade_at(x, bottom_y));
                buf[(area.x + col, area.y + row)]
                    .set_char(UPPER_HALF_BLOCK)
                    .set_fg(top)
                    .set_bg(bottom);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    #[test]
    fn renders_top_and_bottom_pixel_as_fg_and_bg_of_one_cell() {
        let mut fb = [0u8; SCREEN_WIDTH * SCREEN_HEIGHT];
        fb[0] = 3; // (0,0): top-left pixel, darkest shade
        fb[SCREEN_WIDTH] = 0; // (0,1): the pixel directly below it, lightest shade

        let widget = VideoWidget::new(&fb, Palette::Classic);
        let area = Rect::new(0, 0, SCREEN_COLS, SCREEN_ROWS);
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf);

        let cell = &buf[(0, 0)];
        assert_eq!(cell.symbol(), "\u{2580}");
        assert_eq!(cell.fg, Palette::Classic.rgb(3));
        assert_eq!(cell.bg, Palette::Classic.rgb(0));
    }

    #[test]
    fn clips_to_the_given_area_without_panicking_on_a_smaller_rect() {
        let fb = [1u8; SCREEN_WIDTH * SCREEN_HEIGHT];
        let widget = VideoWidget::new(&fb, Palette::Grayscale);
        let area = Rect::new(0, 0, 10, 5); // smaller than the full screen
        let mut buf = Buffer::empty(area);
        widget.render(area, &mut buf); // must not panic / index out of bounds
        assert_eq!(buf[(0, 0)].symbol(), "\u{2580}");
    }
}
