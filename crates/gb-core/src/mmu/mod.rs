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
//! `SB`/`SC` (0xFF01/0xFF02) and `DIV`/`TIMA`/`TMA`/`TAC` (0xFF04-0xFF07)
//! are the exceptions: routed to a real [`Serial`] and [`Timer`]
//! respectively, since the Blargg harness needs serial output capture and
//! `instr_timing`/`mem_timing`(-2) self-time via a genuinely-running `TIMA`
//! (pulled forward from M4 — see `SPEC.md`). I/O-mapped components
//! naturally live behind the bus that owns their address range — the MMU,
//! not `System`, is where this and later PPU/APU/Joypad register wiring
//! belong. `IF` (0xFF0F) is likewise real: interrupt dispatch, the
//! serial-complete request, and the timer-overflow request all need a live
//! IF byte, and it's just flat memory until other interrupt sources
//! (PPU/Joypad) land.
//!
//! Implements [`crate::cpu::Bus`] so the CPU can drive it directly.

use crate::cpu::{Bus, IF_ADDR};
use crate::serial::{Serial, SERIAL_INT_BIT};
use crate::timer::{Timer, TIMER_INT_BIT};

const SB_ADDR: u16 = 0xFF01;
const SC_ADDR: u16 = 0xFF02;
const DIV_ADDR: u16 = 0xFF04;
const TIMA_ADDR: u16 = 0xFF05;
const TMA_ADDR: u16 = 0xFF06;
const TAC_ADDR: u16 = 0xFF07;

/// Flat 64KB-addressable memory, plus the real serial port and timer.
/// Every other address hits the same backing array for every address — no
/// banking, no region-specific behavior. Replaced by a real memory map as
/// cartridge/PPU/APU/Joypad land.
#[derive(Clone)]
pub struct Mmu {
    mem: [u8; 0x10000],
    pub serial: Serial,
    pub timer: Timer,
}

impl std::fmt::Debug for Mmu {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mmu")
            .field("mem", &"[u8; 65536]")
            .field("serial", &self.serial)
            .field("timer", &self.timer)
            .finish()
    }
}

impl Default for Mmu {
    fn default() -> Self {
        Self {
            mem: [0; 0x10000],
            serial: Serial::new(),
            timer: Timer::new(),
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

    /// Advances the timer by `t_cycles` T-cycles and folds a resulting
    /// overflow into `IF`. Called from `System::step` once per CPU
    /// instruction with its elapsed cycle count.
    pub fn step_timer(&mut self, t_cycles: u8) {
        self.timer.step(t_cycles);
        if self.timer.take_interrupt() {
            self.mem[IF_ADDR as usize] |= TIMER_INT_BIT;
        }
    }
}

impl Bus for Mmu {
    fn read(&mut self, addr: u16) -> u8 {
        match addr {
            SB_ADDR => self.serial.read_sb(),
            SC_ADDR => self.serial.read_sc(),
            DIV_ADDR => self.timer.read_div(),
            TIMA_ADDR => self.timer.read_tima(),
            TMA_ADDR => self.timer.read_tma(),
            TAC_ADDR => self.timer.read_tac(),
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
            DIV_ADDR => self.timer.write_div(val),
            TIMA_ADDR => self.timer.write_tima(val),
            TMA_ADDR => self.timer.write_tma(val),
            TAC_ADDR => self.timer.write_tac(val),
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
