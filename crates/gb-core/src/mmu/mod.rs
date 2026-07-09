//! Memory management unit: bus, memory map, I/O register dispatch.
//!
//! This is still the M1 stub for the general address space: a flat 64KB
//! byte array addressable across the full `0x0000..=0xFFFF` range, enough
//! for the CPU to fetch/execute instructions and for Blargg test ROMs to
//! boot (no cartridge banking, no VRAM/OAM semantics yet). Real memory-map
//! routing for those regions — cartridge ROM/RAM banking, VRAM/WRAM/OAM/
//! HRAM regions, PPU/APU/Timer/Joypad register side effects — lands
//! incrementally in M2/M3/M4/M5.
//!
//! `SB`/`SC` (0xFF01/0xFF02) are the exception: routed to a real [`Serial`]
//! now (M1's "serial stub for test-output capture" task), since the
//! Blargg harness needs it and I/O-mapped components naturally live behind
//! the bus that owns their address range — the MMU, not `System`, is where
//! this and later PPU/APU/Timer/Joypad register wiring belong. `IF`
//! (0xFF0F) is likewise real: interrupt dispatch and the serial-complete
//! request both need a live IF byte, and it's just flat memory until other
//! interrupt sources (Timer/PPU/Joypad) land.
//!
//! Implements [`crate::cpu::Bus`] so the CPU can drive it directly.

use crate::cpu::{Bus, IF_ADDR};
use crate::serial::{Serial, SERIAL_INT_BIT};

const SB_ADDR: u16 = 0xFF01;
const SC_ADDR: u16 = 0xFF02;

/// Flat 64KB-addressable memory, plus the real serial port. Every other
/// address hits the same backing array for every address — no banking, no
/// region-specific behavior. Replaced by a real memory map as cartridge/
/// PPU/APU/Timer/Joypad land.
#[derive(Clone)]
pub struct Mmu {
    mem: [u8; 0x10000],
    pub serial: Serial,
}

impl std::fmt::Debug for Mmu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mmu")
            .field("mem", &"[u8; 65536]")
            .field("serial", &self.serial)
            .finish()
    }
}

impl Default for Mmu {
    fn default() -> Self {
        Self {
            mem: [0; 0x10000],
            serial: Serial::new(),
        }
    }
}

impl Mmu {
    pub fn new() -> Self {
        Self::default()
    }

    /// Load ROM bytes starting at address 0x0000, truncated to fit the
    /// address space. No banking — a real cartridge/MBC replaces this in
    /// M3; this exists only so Blargg test ROMs can boot against the M1
    /// CPU core.
    pub fn load_rom(&mut self, data: &[u8]) {
        let len = data.len().min(self.mem.len());
        self.mem[..len].copy_from_slice(&data[..len]);
    }
}

impl Bus for Mmu {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            SB_ADDR => self.serial.read_sb(),
            SC_ADDR => self.serial.read_sc(),
            _ => self.mem[addr as usize],
        }
    }

    fn write(&mut self, addr: u16, val: u8) {
        match addr {
            SB_ADDR => self.serial.write_sb(val),
            SC_ADDR => {
                self.serial.write_sc(val);
                if self.serial.take_interrupt() {
                    self.mem[IF_ADDR as usize] |= SERIAL_INT_BIT;
                }
            }
            _ => self.mem[addr as usize] = val,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_write_round_trip_across_full_address_space() {
        let mut mmu = Mmu::new();
        mmu.write(0x0000, 0x11);
        mmu.write(0x7FFF, 0x22);
        mmu.write(0x8000, 0x33);
        mmu.write(0xFFFF, 0x44);
        assert_eq!(mmu.read(0x0000), 0x11);
        assert_eq!(mmu.read(0x7FFF), 0x22);
        assert_eq!(mmu.read(0x8000), 0x33);
        assert_eq!(mmu.read(0xFFFF), 0x44);
    }

    #[test]
    fn load_rom_copies_bytes_from_zero() {
        let mut mmu = Mmu::new();
        mmu.load_rom(&[0xAA, 0xBB, 0xCC]);
        assert_eq!(mmu.read(0x0000), 0xAA);
        assert_eq!(mmu.read(0x0001), 0xBB);
        assert_eq!(mmu.read(0x0002), 0xCC);
        assert_eq!(mmu.read(0x0003), 0x00);
    }

    #[test]
    fn load_rom_truncates_oversized_data() {
        let mut mmu = Mmu::new();
        let data = vec![0x42u8; 0x20000]; // bigger than 64KB
        mmu.load_rom(&data);
        assert_eq!(mmu.read(0xFFFF), 0x42);
    }

    #[test]
    fn cpu_can_execute_through_mmu_bus() {
        use crate::cpu::Cpu;
        let mut mmu = Mmu::new();
        mmu.load_rom(&[0x3E, 0x2A]); // LD A, 0x2A
        let mut cpu = Cpu::new();
        let t = cpu.step(&mut mmu);
        assert_eq!(cpu.regs.a, 0x2A);
        assert_eq!(t, 8);
    }
}
