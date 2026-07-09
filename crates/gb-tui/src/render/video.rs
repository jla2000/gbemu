//! Half-block video widget: renders the GB 160x144 framebuffer into the
//! terminal using half-block characters and truecolor RGB. Implemented in
//! M2 once the PPU produces real pixel data.

use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::widgets::Widget;

/// GB screen resolution in pixels.
pub const GB_WIDTH: u16 = 160;
pub const GB_HEIGHT: u16 = 144;

/// Terminal cell area required to render the GB screen at full resolution
/// (one cell = 2 vertical pixels via a half-block character).
pub const SCREEN_COLS: u16 = GB_WIDTH;
pub const SCREEN_ROWS: u16 = GB_HEIGHT / 2;

/// Placeholder video widget. Draws an empty bordered area until M2 wires up
/// the real PPU framebuffer.
pub struct VideoWidget;

impl Widget for VideoWidget {
    fn render(self, area: Rect, buf: &mut Buffer) {
        for y in area.y..area.y.saturating_add(area.height) {
            for x in area.x..area.x.saturating_add(area.width) {
                buf[(x, y)].set_char(' ').set_bg(Color::Black);
            }
        }
    }
}
