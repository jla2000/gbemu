//! Ties CPU/PPU/APU/MMU/Timer/Joypad/Serial together and drives execution.
//!
//! Real stepping logic (instruction-at-a-time CPU execution driving PPU/APU/
//! Timer by elapsed T-cycles) lands in M1 onward. For now this only
//! establishes the shape `gb-tui` builds against.

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

    /// Execute a single CPU instruction and advance other components by the
    /// elapsed T-cycles. No-op until M1 implements the CPU.
    pub fn step(&mut self) {
        // Implemented in M1.
    }

    /// Run until the PPU signals VBlank start, i.e. one full frame.
    /// No-op until M1/M2 implement CPU/PPU stepping.
    pub fn run_frame(&mut self) {
        // Implemented in M1/M2.
    }
}
