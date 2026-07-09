//! Ties CPU/PPU/APU/MMU/Timer/Joypad/Serial together and drives execution.
//!
//! `step()` now runs the CPU against the MMU's flat 64KB bus (M1). PPU/APU/
//! Timer stepping by elapsed T-cycles, and `run_frame()`'s VBlank-driven
//! loop, land in M2 onward as those components stop being placeholders.

use crate::apu::Apu;
use crate::cartridge::Cartridge;
use crate::cpu::Cpu;
use crate::joypad::Joypad;
use crate::mmu::Mmu;
use crate::ppu::Ppu;
use crate::serial::Serial;
use crate::timer::Timer;

/// Top-level emulated system.
#[derive(Debug, Default)]
pub struct System {
    pub cpu: Cpu,
    pub ppu: Ppu,
    pub apu: Apu,
    pub mmu: Mmu,
    pub timer: Timer,
    pub joypad: Joypad,
    pub serial: Serial,
    pub cartridge: Option<Cartridge>,
}

impl System {
    pub fn new() -> Self {
        Self {
            cpu: Cpu::new(),
            ppu: Ppu::new(),
            apu: Apu::new(),
            mmu: Mmu::new(),
            timer: Timer::new(),
            joypad: Joypad::new(),
            serial: Serial::new(),
            cartridge: None,
        }
    }

    /// Load raw ROM bytes directly into the MMU's flat address space (no
    /// cartridge/MBC banking yet — lands in M3). Enough to boot Blargg test
    /// ROMs against the M1 CPU core.
    pub fn load_rom(&mut self, data: &[u8]) {
        self.mmu.load_rom(data);
    }

    /// Execute a single CPU instruction and advance other components by the
    /// elapsed T-cycles. Returns the elapsed T-cycles.
    ///
    /// PPU/APU/Timer are still placeholders (land in M2/M4/M5) so they are
    /// not yet advanced here; the CPU already runs against the MMU's flat
    /// 64KB bus.
    pub fn step(&mut self) -> u8 {
        self.cpu.step(&mut self.mmu)
    }

    /// Run until the PPU signals VBlank start, i.e. one full frame.
    /// No-op until M1/M2 implement CPU/PPU stepping.
    pub fn run_frame(&mut self) {
        // Implemented in M1/M2.
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn step_executes_one_instruction_via_mmu() {
        let mut sys = System::new();
        sys.load_rom(&[0x3E, 0x2A, 0x06, 0x10]); // LD A,0x2A; LD B,0x10
        let t1 = sys.step();
        assert_eq!(sys.cpu.regs.a, 0x2A);
        assert_eq!(t1, 8);
        let t2 = sys.step();
        assert_eq!(sys.cpu.regs.b, 0x10);
        assert_eq!(t2, 8);
        assert_eq!(sys.cpu.regs.pc, 4);
    }
}
