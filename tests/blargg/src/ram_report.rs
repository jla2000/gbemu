//! Cartridge-RAM result reporting, used by the Blargg suites built on
//! `shell.s` (`oam_bug`, `mem_timing-2`, `dmg_sound`, and — going by its
//! observed behavior with no bundled source — `halt_bug`) instead of the
//! serial-port protocol `instr_test.s`/`runtime.s`-based suites use
//! (`cpu_instrs`, `instr_timing`, `mem_timing`).
//!
//! `shell.s`'s `init_text_out` writes a `$80` "in progress" sentinel to
//! `$A000` (`final_result`) before running any tests, then `post_exit`
//! overwrites it with the real exit code (`0` = pass, `1` = fail, `255` =
//! internal error, anything else = "Failed #n") once done — and spins in an
//! infinite loop afterward (`forever: jr -`), since on real hardware/
//! devcart test rigs this is read out of band rather than reported back
//! to the console. A human-readable message is written alongside it as a
//! NUL-terminated ASCII string starting at `$A004` (`text_out_base`).

use gb_core::cpu::Bus;
use gb_core::System;

use crate::Outcome;

const FINAL_RESULT_ADDR: u16 = 0xA000;
const IN_PROGRESS: u8 = 0x80;
const TEXT_OUT_BASE: u16 = 0xA004;
const TEXT_OUT_MAX_LEN: usize = 512;

#[derive(Default)]
pub(crate) struct RamReport {
    /// Only trust a non-`$80` `final_result` once we've first observed the
    /// `$80` sentinel — otherwise a ROM that doesn't use this protocol at
    /// all (and so never touches `$A000`) would read back whatever
    /// cartridge RAM happens to default to, which could easily be `0`
    /// (read as an immediate false "Passed").
    saw_in_progress_marker: bool,
}

impl RamReport {
    /// Call once per step. Returns `Some(outcome)` once the ROM has
    /// reported a result through this protocol.
    pub(crate) fn poll(&mut self, sys: &mut System) -> Option<Outcome> {
        let result = sys.mmu.read(FINAL_RESULT_ADDR);
        if result == IN_PROGRESS {
            self.saw_in_progress_marker = true;
            return None;
        }
        if !self.saw_in_progress_marker {
            return None;
        }

        let message = read_text_out(sys);
        Some(if result == 0 {
            Outcome::Passed(message)
        } else {
            Outcome::Failed(message)
        })
    }
}

fn read_text_out(sys: &mut System) -> String {
    let mut out = String::new();
    for i in 0..TEXT_OUT_MAX_LEN as u16 {
        let byte = sys.mmu.read(TEXT_OUT_BASE + i);
        if byte == 0 {
            break;
        }
        out.push(byte as char);
    }
    out
}
