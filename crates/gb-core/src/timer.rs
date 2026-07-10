//! Timer: `DIV`/`TIMA`/`TMA`/`TAC` and the timer interrupt.
//!
//! Modeled the way real hardware works, not as a naive "every N cycles"
//! counter: `DIV` is the top byte of a free-running 16-bit counter that
//! increments every T-cycle and resets to 0 on any write to `DIV`. `TIMA`
//! increments on a *falling edge* of a TAC-selected bit of that 16-bit
//! counter. This matters because writing `DIV` or `TAC` can itself cause
//! that selected bit to fall (if it was already high), incrementing `TIMA`
//! immediately as a side effect — real test ROMs (and later, more advanced
//! Blargg timer tests) probe exactly this. On `TIMA` overflow it reloads
//! from `TMA` and requests the timer interrupt.
//!
//! Simplification: real hardware delays the overflow reload/interrupt by
//! one M-cycle (writes to `TIMA` during that window are dropped). Not
//! modeled yet — none of the currently targeted Blargg tests
//! (`cpu_instrs`, `instr_timing`, `mem_timing`(-2), `halt_bug`) exercise
//! that edge case.

/// Timer interrupt request bit (bit 2) in `IF`/`IE`.
pub const TIMER_INT_BIT: u8 = 1 << 2;

const TAC_ENABLE: u8 = 0b100;
/// Bit of the 16-bit `DIV` counter that feeds `TIMA`, indexed by `TAC &
/// 0b11` (the two clock-select bits): 4096 Hz, 262144 Hz, 65536 Hz, 16384
/// Hz.
const TAC_BITS: [u8; 4] = [9, 3, 5, 7];

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Timer {
    div_counter: u16,
    tima: u8,
    tma: u8,
    tac: u8,
    /// Set when `TIMA` overflows; consumed via [`Timer::take_interrupt`].
    interrupt_pending: bool,
}

impl Timer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read_div(&self) -> u8 {
        (self.div_counter >> 8) as u8
    }

    pub fn write_div(&mut self, _val: u8) {
        self.set_counter(0);
    }

    pub fn read_tima(&self) -> u8 {
        self.tima
    }

    pub fn write_tima(&mut self, val: u8) {
        self.tima = val;
    }

    pub fn read_tma(&self) -> u8 {
        self.tma
    }

    pub fn write_tma(&mut self, val: u8) {
        self.tma = val;
    }

    pub fn read_tac(&self) -> u8 {
        self.tac | 0b1111_1000
    }

    pub fn write_tac(&mut self, val: u8) {
        let was_high = self.selected_bit_high();
        self.tac = val & 0b111;
        if was_high && !self.selected_bit_high() {
            self.increment_tima();
        }
    }

    /// Advances the timer by `t_cycles` T-cycles (one CPU step's worth of
    /// elapsed time), one cycle at a time so the falling-edge check applies
    /// uniformly to natural counting.
    pub fn step(&mut self, t_cycles: u8) {
        for _ in 0..t_cycles {
            self.set_counter(self.div_counter.wrapping_add(1));
        }
    }

    /// Consumes and returns whether `TIMA` overflowed since the last call.
    pub fn take_interrupt(&mut self) -> bool {
        std::mem::take(&mut self.interrupt_pending)
    }

    fn selected_bit_high(&self) -> bool {
        self.tac & TAC_ENABLE != 0
            && (self.div_counter >> TAC_BITS[(self.tac & 0b11) as usize]) & 1 != 0
    }

    fn set_counter(&mut self, val: u16) {
        let was_high = self.selected_bit_high();
        self.div_counter = val;
        if was_high && !self.selected_bit_high() {
            self.increment_tima();
        }
    }

    fn increment_tima(&mut self) {
        let (new, overflow) = self.tima.overflowing_add(1);
        if overflow {
            self.tima = self.tma;
            self.interrupt_pending = true;
        } else {
            self.tima = new;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tima_increments_at_selected_rate() {
        let mut t = Timer::new();
        t.write_tac(0b101); // enable, select bit 3 (every 16 T-cycles)
        t.step(15);
        assert_eq!(t.read_tima(), 0);
        t.step(1);
        assert_eq!(t.read_tima(), 1);
    }

    #[test]
    fn tima_overflow_reloads_from_tma_and_requests_interrupt() {
        let mut t = Timer::new();
        t.write_tma(0x10);
        t.write_tac(0b101);
        t.write_tima(0xFF);
        t.step(16);
        assert_eq!(t.read_tima(), 0x10);
        assert!(t.take_interrupt());
        assert!(!t.take_interrupt());
    }

    #[test]
    fn writing_div_resets_counter_and_can_trigger_falling_edge() {
        let mut t = Timer::new();
        t.write_tac(0b101); // select bit 3
        t.step(8); // counter = 8, bit 3 set
        assert_eq!(t.read_tima(), 0);
        t.write_div(0); // counter -> 0: bit 3 falls 1->0
        assert_eq!(t.read_tima(), 1);
        assert_eq!(t.read_div(), 0);
    }

    #[test]
    fn disabled_timer_never_increments() {
        let mut t = Timer::new();
        t.write_tac(0b001); // select bit 3, but not enabled
        for _ in 0..4 {
            t.step(250);
        }
        assert_eq!(t.read_tima(), 0);
    }
}
