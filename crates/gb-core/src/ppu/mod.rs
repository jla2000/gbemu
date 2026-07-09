//! PPU (picture processing unit). Implemented in M2.

/// Placeholder PPU state. Dot-accurate mode sequencing lands in M2.
#[derive(Debug, Clone)]
pub struct Ppu {
    _placeholder: (),
}

impl Default for Ppu {
    fn default() -> Self {
        Self { _placeholder: () }
    }
}

impl Ppu {
    pub fn new() -> Self {
        Self::default()
    }
}
