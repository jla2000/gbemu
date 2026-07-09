//! SM83 CPU core: registers, flags, match-based opcode dispatch, interrupt
//! dispatch (IE/IF/IME priority), and HALT/STOP + the HALT-bug quirk.
//!
//! Interrupt request/enable bits live on the bus (`IF` at 0xFF0F, `IE` at
//! 0xFFFF) — any [`Bus`] implementation is enough to drive dispatch, so this
//! stays testable against [`bus::FlatBus`] without a real MMU/PPU/Timer.

pub mod bus;
pub mod registers;

mod execute;

pub use bus::Bus;
pub use registers::{Registers, FLAG_C, FLAG_H, FLAG_N, FLAG_Z};

/// `IF`/`IE` address and interrupt vector table, priority bit0 (VBlank)
/// highest through bit4 (Joypad) lowest.
pub(crate) const IF_ADDR: u16 = 0xFF0F;
pub(crate) const IE_ADDR: u16 = 0xFFFF;
const INT_VECTORS: [u16; 5] = [0x0040, 0x0048, 0x0050, 0x0058, 0x0060];

/// True if any `IF & IE` bit is set, regardless of IME — used by HALT to
/// decide whether it actually halts or triggers the HALT bug.
pub(crate) fn interrupt_pending(bus: &mut impl Bus) -> bool {
    bus.read(IF_ADDR) & bus.read(IE_ADDR) & 0x1F != 0
}

/// SM83 CPU state: registers + IME/HALT/STOP flags.
#[derive(Debug, Default, Clone, Copy)]
pub struct Cpu {
    pub regs: Registers,
    /// Interrupt Master Enable.
    pub ime: bool,
    /// Set by HALT (0x76) when no interrupt is pending-and-disabled at the
    /// time it executes; cleared when an enabled-and-pending interrupt
    /// wakes the CPU (serviced or not, per hardware behavior).
    pub halted: bool,
    /// Set by STOP (0x10); real low-power/DIV-reset behavior lands with
    /// Timer/Joypad wiring (M4) — wake-on-button-press specifically.
    pub stopped: bool,
    /// EI (0xFB) schedules `ime = true` to take effect only after the
    /// *following* instruction has fully executed, not immediately — the
    /// well-known one-instruction EI delay. Sampled at the top of `step`
    /// and applied at the end, so the instruction right after EI still
    /// runs with the old IME.
    ime_pending: bool,
    /// Set when HALT executes with IME=0 and an interrupt already
    /// pending-and-enabled: the CPU does not halt, and the *next* opcode
    /// fetch fails to advance PC, causing that opcode to be fetched (and
    /// executed) twice. Consumed by the first `fetch_byte` call after.
    halt_bug: bool,
}

impl Cpu {
    pub fn new() -> Self {
        Self::default()
    }

    /// Fetch, decode, and execute exactly one instruction (or the halted
    /// no-op), servicing a pending interrupt first if IME allows it.
    /// Returns elapsed T-cycles (4 per M-cycle).
    pub fn step(&mut self, bus: &mut impl Bus) -> u8 {
        let apply_ime_enable = self.ime_pending;

        if let Some(cycles) = self.dispatch_interrupt(bus) {
            self.finish_ime_delay(apply_ime_enable);
            return cycles;
        }

        if self.halted {
            self.finish_ime_delay(apply_ime_enable);
            return 4;
        }

        let opcode = self.fetch_byte(bus);
        let cycles = execute::execute(self, bus, opcode);
        self.finish_ime_delay(apply_ime_enable);
        cycles
    }

    fn finish_ime_delay(&mut self, was_pending: bool) {
        if was_pending {
            self.ime = true;
            self.ime_pending = false;
        }
    }

    /// Set by `execute` when EI (0xFB) runs; the enable itself is deferred,
    /// see [`Cpu::ime_pending`].
    pub(super) fn schedule_ime_enable(&mut self) {
        self.ime_pending = true;
    }

    /// Set by `execute` when HALT (0x76) triggers the HALT bug.
    pub(super) fn trigger_halt_bug(&mut self) {
        self.halt_bug = true;
    }

    /// Checks `IF & IE` for a pending, enabled interrupt. Always wakes the
    /// CPU from HALT if one exists (even with IME=0 — HALT just stops
    /// fetching, it doesn't gate wake-up). Services (pushes PC, jumps to
    /// vector, clears the IF bit, clears IME) only if IME=1, returning the
    /// dispatch's elapsed cycles in that case.
    fn dispatch_interrupt(&mut self, bus: &mut impl Bus) -> Option<u8> {
        let pending = bus.read(IF_ADDR) & bus.read(IE_ADDR) & 0x1F;
        if pending == 0 {
            return None;
        }
        if self.halted {
            self.halted = false;
        }
        if !self.ime {
            return None;
        }
        let bit = pending.trailing_zeros() as usize;
        let cur_if = bus.read(IF_ADDR);
        bus.write(IF_ADDR, cur_if & !(1 << bit));
        self.ime = false;
        self.regs.sp = self.regs.sp.wrapping_sub(2);
        bus.write16(self.regs.sp, self.regs.pc);
        self.regs.pc = INT_VECTORS[bit];
        Some(20)
    }

    pub(crate) fn fetch_byte(&mut self, bus: &mut impl Bus) -> u8 {
        let b = bus.read(self.regs.pc);
        if self.halt_bug {
            // Consumed exactly once: PC fails to advance, so this same
            // opcode byte is fetched again (and executed again) next step.
            self.halt_bug = false;
        } else {
            self.regs.pc = self.regs.pc.wrapping_add(1);
        }
        b
    }

    pub(crate) fn fetch_word(&mut self, bus: &mut impl Bus) -> u16 {
        let lo = self.fetch_byte(bus) as u16;
        let hi = self.fetch_byte(bus) as u16;
        (hi << 8) | lo
    }
}

#[cfg(test)]
mod interrupt_tests {
    use super::*;
    use crate::cpu::bus::FlatBus;

    fn set_if(bus: &mut FlatBus, v: u8) {
        bus.mem[IF_ADDR as usize] = v;
    }
    fn set_ie(bus: &mut FlatBus, v: u8) {
        bus.mem[IE_ADDR as usize] = v;
    }

    #[test]
    fn services_highest_priority_pending_interrupt() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        cpu.ime = true;
        cpu.regs.sp = 0xFFFE;
        cpu.regs.pc = 0x1234;
        set_ie(&mut bus, 0x1F);
        set_if(&mut bus, 0b0000_0110); // STAT (bit1) + Timer (bit2) pending
        let t = cpu.step(&mut bus);
        assert_eq!(t, 20);
        assert_eq!(cpu.regs.pc, 0x0048); // STAT vector: bit1 wins over bit2
        assert!(!cpu.ime);
        assert_eq!(bus.mem[IF_ADDR as usize], 0b0000_0100); // bit1 cleared
        assert_eq!(bus.read16(cpu.regs.sp), 0x1234); // old PC pushed
    }

    #[test]
    fn disabled_interrupt_bit_in_ie_is_not_serviced() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        cpu.ime = true;
        cpu.regs.pc = 0x0000;
        set_ie(&mut bus, 0x00); // nothing enabled
        set_if(&mut bus, 0x01); // VBlank pending
        cpu.step(&mut bus); // executes NOP at 0x0000 instead
        assert_eq!(cpu.regs.pc, 1);
        assert!(cpu.ime);
    }

    #[test]
    fn ime_false_does_not_service_but_still_wakes_halt() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        cpu.ime = false;
        cpu.halted = true;
        cpu.regs.pc = 0x0000; // NOP at 0
        set_ie(&mut bus, 0x01);
        set_if(&mut bus, 0x01);
        let t = cpu.step(&mut bus);
        assert!(!cpu.halted); // woke up
        assert_eq!(cpu.regs.pc, 1); // executed the NOP instead of dispatch
        assert_eq!(t, 4);
        assert_eq!(bus.mem[IF_ADDR as usize], 0x01); // IF bit left untouched
    }

    #[test]
    fn ei_enable_is_delayed_by_one_instruction() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        // EI; NOP; NOP
        bus.mem[0] = 0xFB;
        bus.mem[1] = 0x00;
        bus.mem[2] = 0x00;
        set_ie(&mut bus, 0x01);
        set_if(&mut bus, 0x01); // VBlank pending throughout

        cpu.step(&mut bus); // EI executes; IME not yet true
        assert!(!cpu.ime);

        cpu.step(&mut bus); // instruction right after EI: still not serviced
        assert_eq!(cpu.regs.pc, 2);
        assert!(cpu.ime); // now enabled for the *next* check

        let t = cpu.step(&mut bus); // dispatch now preempts the second NOP
        assert_eq!(t, 20);
        assert_eq!(cpu.regs.pc, 0x0040);
    }

    #[test]
    fn halt_bug_executes_next_opcode_twice() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        // HALT; INC A; INC A (only one INC A should really be here, but the
        // bug re-fetches the same byte, so a single INC A runs twice).
        bus.mem[0] = 0x76; // HALT
        bus.mem[1] = 0x3C; // INC A
        bus.mem[2] = 0x00; // NOP (would run next on real hardware post-bug)
        cpu.ime = false;
        set_ie(&mut bus, 0x01);
        set_if(&mut bus, 0x01); // pending-and-disabled at HALT time -> bug

        cpu.step(&mut bus); // HALT: bug triggers, CPU does not actually halt
        assert!(!cpu.halted);
        assert_eq!(cpu.regs.pc, 1);

        cpu.step(&mut bus); // first fetch of INC A: PC does not advance
        assert_eq!(cpu.regs.a, 1);
        assert_eq!(cpu.regs.pc, 1);

        cpu.step(&mut bus); // second fetch of INC A: PC advances normally
        assert_eq!(cpu.regs.a, 2);
        assert_eq!(cpu.regs.pc, 2);
    }

    #[test]
    fn halt_without_pending_interrupt_halts_normally() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        bus.mem[0] = 0x76; // HALT
        cpu.ime = false;
        // No pending interrupt at all.
        cpu.step(&mut bus);
        assert!(cpu.halted);
        assert_eq!(cpu.regs.pc, 1);
    }
}
