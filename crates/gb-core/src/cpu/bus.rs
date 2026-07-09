//! Bus trait: how the CPU reads/writes the 16-bit address space.
//!
//! Decouples CPU opcode dispatch from the MMU (a real bus with cartridge/
//! VRAM/WRAM/I/O routing lands in a later task) so the CPU can be unit
//! tested against a trivial flat-memory double.

/// Anything the CPU can fetch instructions from and read/write memory
/// through. `&mut self` on `read` allows implementations that need to model
/// read side effects (timer/PPU/OAM DMA ordering) later.
pub trait Bus {
    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, val: u8);

    fn read16(&mut self, addr: u16) -> u16 {
        let lo = self.read(addr) as u16;
        let hi = self.read(addr.wrapping_add(1)) as u16;
        (hi << 8) | lo
    }

    fn write16(&mut self, addr: u16, val: u16) {
        self.write(addr, val as u8);
        self.write(addr.wrapping_add(1), (val >> 8) as u8);
    }
}

/// Flat 64KB RAM bus. Used for CPU unit tests; a real MMU replaces this.
#[derive(Debug, Clone)]
pub struct FlatBus {
    pub mem: [u8; 0x10000],
}

impl Default for FlatBus {
    fn default() -> Self {
        Self { mem: [0; 0x10000] }
    }
}

impl FlatBus {
    pub fn new() -> Self {
        Self::default()
    }
}

impl Bus for FlatBus {
    fn read(&mut self, addr: u16) -> u8 {
        self.mem[addr as usize]
    }

    fn write(&mut self, addr: u16, val: u8) {
        self.mem[addr as usize] = val;
    }
}
