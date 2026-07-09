//! Memory management unit: bus, memory map, I/O register dispatch.
//! Full memory map wiring lands in M1 (stub bus for boot) and M3 (cartridge).

/// Placeholder MMU. A real 64KB-addressable bus with cartridge/VRAM/WRAM/
/// I/O routing lands in M1/M3.
#[derive(Debug, Clone)]
pub struct Mmu {
    _placeholder: (),
}

impl Default for Mmu {
    fn default() -> Self {
        Self { _placeholder: () }
    }
}

impl Mmu {
    pub fn new() -> Self {
        Self::default()
    }
}
