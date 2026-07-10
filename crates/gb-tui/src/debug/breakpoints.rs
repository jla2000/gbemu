//! Breakpoints: PC address breakpoints (exact, matching real debugger
//! semantics — break *before* executing the instruction at that address)
//! and memory watchpoints. Watchpoints here are value-change watches
//! (break when the watched byte's value differs from what it was after
//! the previous check), not true read/write-access traps — that would
//! need instrumenting every `Bus::read`/`write` call in `gb-core`, which
//! is a bigger architectural change than this debugger's first cut
//! warrants. Value-change watching still catches the common "did this
//! variable change" debugging use case.

use std::collections::{HashMap, HashSet};

use gb_core::cpu::Bus;
use gb_core::mmu::Mmu;

#[derive(Debug, Default)]
pub struct Breakpoints {
    pc_breakpoints: HashSet<u16>,
    /// Watched address -> last-seen value.
    watchpoints: HashMap<u16, u8>,
}

impl Breakpoints {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn toggle_pc(&mut self, addr: u16) {
        if !self.pc_breakpoints.remove(&addr) {
            self.pc_breakpoints.insert(addr);
        }
    }

    pub fn has_pc(&self, addr: u16) -> bool {
        self.pc_breakpoints.contains(&addr)
    }

    pub fn pc_breakpoints(&self) -> impl Iterator<Item = u16> + '_ {
        self.pc_breakpoints.iter().copied()
    }

    pub fn toggle_watch(&mut self, addr: u16, current_value: u8) {
        if self.watchpoints.remove(&addr).is_none() {
            self.watchpoints.insert(addr, current_value);
        }
    }

    pub fn has_watch(&self, addr: u16) -> bool {
        self.watchpoints.contains_key(&addr)
    }

    pub fn watchpoints(&self) -> impl Iterator<Item = u16> + '_ {
        self.watchpoints.keys().copied()
    }

    /// Whether `pc` is a breakpoint that should halt execution before this
    /// instruction runs.
    pub fn should_break_at(&self, pc: u16) -> bool {
        self.pc_breakpoints.contains(&pc)
    }

    /// Checks all watchpoints against `mmu`'s current contents, updates
    /// their stored values, and returns whether any changed since the
    /// last check.
    pub fn check_watchpoints(&mut self, mmu: &mut Mmu) -> bool {
        let mut hit = false;
        for (&addr, last) in self.watchpoints.iter_mut() {
            let current = mmu.read(addr);
            if current != *last {
                hit = true;
            }
            *last = current;
        }
        hit
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_pc_breakpoint_sets_then_clears() {
        let mut bp = Breakpoints::new();
        assert!(!bp.has_pc(0x100));
        bp.toggle_pc(0x100);
        assert!(bp.has_pc(0x100));
        bp.toggle_pc(0x100);
        assert!(!bp.has_pc(0x100));
    }

    #[test]
    fn should_break_at_matches_only_set_breakpoints() {
        let mut bp = Breakpoints::new();
        bp.toggle_pc(0x150);
        assert!(bp.should_break_at(0x150));
        assert!(!bp.should_break_at(0x151));
    }

    #[test]
    fn watchpoint_detects_value_change_across_checks() {
        let mut bp = Breakpoints::new();
        let mut mmu = Mmu::new();
        mmu.write(0xC000, 5);
        bp.toggle_watch(0xC000, mmu.read(0xC000));
        assert!(!bp.check_watchpoints(&mut mmu)); // unchanged since toggling
        mmu.write(0xC000, 6);
        assert!(bp.check_watchpoints(&mut mmu)); // changed
        assert!(!bp.check_watchpoints(&mut mmu)); // stable again after the check updates the stored value
    }

    #[test]
    fn toggle_watch_clears_an_existing_watchpoint() {
        let mut bp = Breakpoints::new();
        bp.toggle_watch(0xC000, 0);
        assert!(bp.has_watch(0xC000));
        bp.toggle_watch(0xC000, 0);
        assert!(!bp.has_watch(0xC000));
    }
}
