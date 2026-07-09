//! SM83 CPU core: registers, flags, match-based opcode dispatch.
//!
//! Interrupt dispatch (IE/IF/IME priority) and the HALT/STOP + HALT-bug
//! quirks are implemented in a later task; `halted`/`ime` exist here only so
//! opcodes that touch them (HALT, STOP, DI, EI) have somewhere to write.

pub mod bus;
pub mod registers;

mod execute;

pub use bus::Bus;
pub use registers::{Registers, FLAG_C, FLAG_H, FLAG_N, FLAG_Z};

/// SM83 CPU state: registers + IME/HALT/STOP flags.
#[derive(Debug, Default, Clone, Copy)]
pub struct Cpu {
    pub regs: Registers,
    /// Interrupt Master Enable. Full dispatch lands with interrupt handling.
    pub ime: bool,
    /// Set by HALT (0x76); real wake-on-interrupt lands with interrupt
    /// handling.
    pub halted: bool,
    /// Set by STOP (0x10); real low-power/DIV-reset behavior lands with
    /// interrupt handling.
    pub stopped: bool,
}

impl Cpu {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fetch, decode, and execute exactly one instruction (or the halted
    /// no-op). Returns elapsed T-cycles (4 per M-cycle).
    pub fn step(&mut self, bus: &mut impl Bus) -> u8 {
        if self.halted {
            return 4;
        }
        let opcode = self.fetch_byte(bus);
        execute::execute(self, bus, opcode)
    }

    pub(crate) fn fetch_byte(&mut self, bus: &mut impl Bus) -> u8 {
        let b = bus.read(self.regs.pc);
        self.regs.pc = self.regs.pc.wrapping_add(1);
        b
    }

    pub(crate) fn fetch_word(&mut self, bus: &mut impl Bus) -> u16 {
        let lo = self.fetch_byte(bus) as u16;
        let hi = self.fetch_byte(bus) as u16;
        (hi << 8) | lo
    }
}
