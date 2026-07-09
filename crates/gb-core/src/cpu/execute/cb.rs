//! CB-prefixed opcode dispatch: rotate/shift/bit ops on r8 (256 arms).
//!
//! Encoding: `op = (opcode>>6)&3` selects group for 0x00-0x3F rotate/shift
//! variants (8 sub-ops via bits 3-5), 0x40-0x7F = BIT b,r, 0x80-0xBF =
//! RES b,r, 0xC0-0xFF = SET b,r. `r = opcode & 7` selects the operand
//! (0=B 1=C 2=D 3=E 4=H 5=L 6=(HL) 7=A), matching `execute::read_r8`.

use super::super::{Bus, Cpu, FLAG_C, FLAG_H, FLAG_N, FLAG_Z};
use super::{read_r8, write_r8};

pub(in crate::cpu) fn execute_cb(cpu: &mut Cpu, bus: &mut impl Bus, opcode: u8) -> u8 {
    let r = opcode & 0x07;
    let is_hl = r == 6;
    match opcode {
        0x00..=0x3F => {
            let sub = (opcode >> 3) & 0x07;
            let v = read_r8(cpu, bus, r);
            let res = match sub {
                0 => rlc(cpu, v),
                1 => rrc(cpu, v),
                2 => rl(cpu, v),
                3 => rr(cpu, v),
                4 => sla(cpu, v),
                5 => sra(cpu, v),
                6 => swap(cpu, v),
                7 => srl(cpu, v),
                _ => unreachable!(),
            };
            write_r8(cpu, bus, r, res);
            if is_hl {
                16
            } else {
                8
            }
        }
        0x40..=0x7F => {
            let bit = (opcode >> 3) & 0x07;
            let v = read_r8(cpu, bus, r);
            bit_test(cpu, v, bit);
            if is_hl {
                12
            } else {
                8
            }
        }
        0x80..=0xBF => {
            let bit = (opcode >> 3) & 0x07;
            let v = read_r8(cpu, bus, r);
            write_r8(cpu, bus, r, v & !(1 << bit));
            if is_hl {
                16
            } else {
                8
            }
        }
        0xC0..=0xFF => {
            let bit = (opcode >> 3) & 0x07;
            let v = read_r8(cpu, bus, r);
            write_r8(cpu, bus, r, v | (1 << bit));
            if is_hl {
                16
            } else {
                8
            }
        }
    }
}

fn rlc(cpu: &mut Cpu, v: u8) -> u8 {
    let carry = v & 0x80 != 0;
    let r = v.rotate_left(1);
    set_shift_flags(cpu, r, carry);
    r
}

fn rrc(cpu: &mut Cpu, v: u8) -> u8 {
    let carry = v & 0x01 != 0;
    let r = v.rotate_right(1);
    set_shift_flags(cpu, r, carry);
    r
}

fn rl(cpu: &mut Cpu, v: u8) -> u8 {
    let old_c = if cpu.regs.carry() { 1u8 } else { 0 };
    let carry = v & 0x80 != 0;
    let r = (v << 1) | old_c;
    set_shift_flags(cpu, r, carry);
    r
}

fn rr(cpu: &mut Cpu, v: u8) -> u8 {
    let old_c = if cpu.regs.carry() { 0x80u8 } else { 0 };
    let carry = v & 0x01 != 0;
    let r = (v >> 1) | old_c;
    set_shift_flags(cpu, r, carry);
    r
}

fn sla(cpu: &mut Cpu, v: u8) -> u8 {
    let carry = v & 0x80 != 0;
    let r = v << 1;
    set_shift_flags(cpu, r, carry);
    r
}

fn sra(cpu: &mut Cpu, v: u8) -> u8 {
    let carry = v & 0x01 != 0;
    let r = ((v as i8) >> 1) as u8;
    set_shift_flags(cpu, r, carry);
    r
}

fn swap(cpu: &mut Cpu, v: u8) -> u8 {
    let r = (v << 4) | (v >> 4);
    cpu.regs.set_flag(FLAG_Z, r == 0);
    cpu.regs.set_flag(FLAG_N, false);
    cpu.regs.set_flag(FLAG_H, false);
    cpu.regs.set_flag(FLAG_C, false);
    r
}

fn srl(cpu: &mut Cpu, v: u8) -> u8 {
    let carry = v & 0x01 != 0;
    let r = v >> 1;
    set_shift_flags(cpu, r, carry);
    r
}

fn set_shift_flags(cpu: &mut Cpu, result: u8, carry: bool) {
    cpu.regs.set_flag(FLAG_Z, result == 0);
    cpu.regs.set_flag(FLAG_N, false);
    cpu.regs.set_flag(FLAG_H, false);
    cpu.regs.set_flag(FLAG_C, carry);
}

fn bit_test(cpu: &mut Cpu, v: u8, bit: u8) {
    let z = v & (1 << bit) == 0;
    cpu.regs.set_flag(FLAG_Z, z);
    cpu.regs.set_flag(FLAG_N, false);
    cpu.regs.set_flag(FLAG_H, true);
    // C unaffected.
}
