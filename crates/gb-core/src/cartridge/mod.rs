//! Cartridge loading, header parsing, and MBC0/1/2/3/5. Implemented in M3.

/// Placeholder cartridge. Header parsing + MBC implementations land in M3.
#[derive(Debug, Clone)]
pub struct Cartridge {
    _placeholder: (),
}

impl Cartridge {
    pub fn new() -> Self {
        Self { _placeholder: () }
    }
}
