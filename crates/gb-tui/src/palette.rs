//! DMG shade -> truecolor RGB palettes for the video widget.
//!
//! The PPU framebuffer (`gb_core::ppu::Ppu::framebuffer`) holds one shade
//! index per pixel (0 = lightest .. 3 = darkest), already resolved through
//! BGP/OBP0/OBP1. This module is the last step: mapping those four shade
//! indices to concrete RGB triples for the terminal.

use ratatui::style::Color;

/// Selects which four RGB triples the video widget maps DMG shade indices
/// (0-3) to. `clap::ValueEnum` lets this be set directly from `--palette`.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default, clap::ValueEnum)]
pub enum Palette {
    /// Classic Game Boy green shades.
    #[default]
    Classic,
    /// Neutral grayscale shades.
    Grayscale,
}

const CLASSIC: [Color; 4] = [
    Color::Rgb(0x9B, 0xBC, 0x0F),
    Color::Rgb(0x8B, 0xAC, 0x0F),
    Color::Rgb(0x30, 0x62, 0x30),
    Color::Rgb(0x0F, 0x38, 0x0F),
];

const GRAYSCALE: [Color; 4] = [
    Color::Rgb(0xFF, 0xFF, 0xFF),
    Color::Rgb(0xAA, 0xAA, 0xAA),
    Color::Rgb(0x55, 0x55, 0x55),
    Color::Rgb(0x00, 0x00, 0x00),
];

impl Palette {
    /// Maps a PPU shade index (0-3; other values wrap via `& 0b11`, since
    /// the framebuffer only ever contains 2-bit shades) to an RGB color.
    pub fn rgb(self, shade: u8) -> Color {
        let shades = match self {
            Palette::Classic => &CLASSIC,
            Palette::Grayscale => &GRAYSCALE,
        };
        shades[(shade & 0b11) as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classic_and_grayscale_map_all_four_shades_distinctly() {
        for palette in [Palette::Classic, Palette::Grayscale] {
            let colors: Vec<Color> = (0..4).map(|s| palette.rgb(s)).collect();
            for i in 0..colors.len() {
                for j in (i + 1)..colors.len() {
                    assert_ne!(colors[i], colors[j], "{palette:?} shades {i} and {j} collide");
                }
            }
        }
    }

    #[test]
    fn shade_index_wraps_to_two_bits() {
        assert_eq!(Palette::Classic.rgb(0), Palette::Classic.rgb(4));
        assert_eq!(Palette::Classic.rgb(3), Palette::Classic.rgb(7));
    }
}
