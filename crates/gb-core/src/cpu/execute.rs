//! Match-based opcode dispatch: 256 base-opcode arms + 256 CB-prefixed arms.
//!
//! Cycle counts (T-cycles, 4 per M-cycle) and flag behavior follow the
//! standard SM83 opcode tables. Interrupt dispatch and the HALT-bug are
//! handled by the caller (`Cpu::step`) / a later task — this module only
//! implements straight-line instruction semantics.

use super::{Bus, Cpu, FLAG_C, FLAG_H, FLAG_N, FLAG_Z};

/// Execute one already-fetched base opcode. Returns elapsed T-cycles.
pub(super) fn execute(cpu: &mut Cpu, bus: &mut impl Bus, opcode: u8) -> u8 {
    match opcode {
        // --- 0x00..0x3F ---
        0x00 => 4, // NOP
        0x01 => {
            let v = cpu.fetch_word(bus);
            cpu.regs.set_bc(v);
            12
        }
        0x02 => {
            bus.write(cpu.regs.bc(), cpu.regs.a);
            8
        }
        0x03 => {
            cpu.regs.set_bc(cpu.regs.bc().wrapping_add(1));
            8
        }
        0x04 => {
            cpu.regs.b = inc8(cpu, cpu.regs.b);
            4
        }
        0x05 => {
            cpu.regs.b = dec8(cpu, cpu.regs.b);
            4
        }
        0x06 => {
            cpu.regs.b = cpu.fetch_byte(bus);
            8
        }
        0x07 => {
            rlca(cpu);
            4
        }
        0x08 => {
            let addr = cpu.fetch_word(bus);
            bus.write16(addr, cpu.regs.sp);
            20
        }
        0x09 => {
            add16(cpu, cpu.regs.bc());
            8
        }
        0x0A => {
            cpu.regs.a = bus.read(cpu.regs.bc());
            8
        }
        0x0B => {
            cpu.regs.set_bc(cpu.regs.bc().wrapping_sub(1));
            8
        }
        0x0C => {
            cpu.regs.c = inc8(cpu, cpu.regs.c);
            4
        }
        0x0D => {
            cpu.regs.c = dec8(cpu, cpu.regs.c);
            4
        }
        0x0E => {
            cpu.regs.c = cpu.fetch_byte(bus);
            8
        }
        0x0F => {
            rrca(cpu);
            4
        }

        0x10 => {
            // STOP: real low-power/DIV-reset behavior lands with interrupt
            // handling; consume the (ignored) operand byte per hardware
            // quirk and mark stopped.
            let _ = cpu.fetch_byte(bus);
            cpu.stopped = true;
            4
        }
        0x11 => {
            let v = cpu.fetch_word(bus);
            cpu.regs.set_de(v);
            12
        }
        0x12 => {
            bus.write(cpu.regs.de(), cpu.regs.a);
            8
        }
        0x13 => {
            cpu.regs.set_de(cpu.regs.de().wrapping_add(1));
            8
        }
        0x14 => {
            cpu.regs.d = inc8(cpu, cpu.regs.d);
            4
        }
        0x15 => {
            cpu.regs.d = dec8(cpu, cpu.regs.d);
            4
        }
        0x16 => {
            cpu.regs.d = cpu.fetch_byte(bus);
            8
        }
        0x17 => {
            rla(cpu);
            4
        }
        0x18 => {
            let off = cpu.fetch_byte(bus) as i8;
            cpu.regs.pc = cpu.regs.pc.wrapping_add(off as u16);
            12
        }
        0x19 => {
            add16(cpu, cpu.regs.de());
            8
        }
        0x1A => {
            cpu.regs.a = bus.read(cpu.regs.de());
            8
        }
        0x1B => {
            cpu.regs.set_de(cpu.regs.de().wrapping_sub(1));
            8
        }
        0x1C => {
            cpu.regs.e = inc8(cpu, cpu.regs.e);
            4
        }
        0x1D => {
            cpu.regs.e = dec8(cpu, cpu.regs.e);
            4
        }
        0x1E => {
            cpu.regs.e = cpu.fetch_byte(bus);
            8
        }
        0x1F => {
            rra(cpu);
            4
        }

        0x20 => jr_cc(cpu, bus, !cpu.regs.zero()),
        0x21 => {
            let v = cpu.fetch_word(bus);
            cpu.regs.set_hl(v);
            12
        }
        0x22 => {
            bus.write(cpu.regs.hl(), cpu.regs.a);
            cpu.regs.set_hl(cpu.regs.hl().wrapping_add(1));
            8
        }
        0x23 => {
            cpu.regs.set_hl(cpu.regs.hl().wrapping_add(1));
            8
        }
        0x24 => {
            cpu.regs.h = inc8(cpu, cpu.regs.h);
            4
        }
        0x25 => {
            cpu.regs.h = dec8(cpu, cpu.regs.h);
            4
        }
        0x26 => {
            cpu.regs.h = cpu.fetch_byte(bus);
            8
        }
        0x27 => {
            daa(cpu);
            4
        }
        0x28 => jr_cc(cpu, bus, cpu.regs.zero()),
        0x29 => {
            add16(cpu, cpu.regs.hl());
            8
        }
        0x2A => {
            cpu.regs.a = bus.read(cpu.regs.hl());
            cpu.regs.set_hl(cpu.regs.hl().wrapping_add(1));
            8
        }
        0x2B => {
            cpu.regs.set_hl(cpu.regs.hl().wrapping_sub(1));
            8
        }
        0x2C => {
            cpu.regs.l = inc8(cpu, cpu.regs.l);
            4
        }
        0x2D => {
            cpu.regs.l = dec8(cpu, cpu.regs.l);
            4
        }
        0x2E => {
            cpu.regs.l = cpu.fetch_byte(bus);
            8
        }
        0x2F => {
            cpu.regs.a = !cpu.regs.a;
            cpu.regs.set_flag(FLAG_N, true);
            cpu.regs.set_flag(FLAG_H, true);
            4
        }

        0x30 => jr_cc(cpu, bus, !cpu.regs.carry()),
        0x31 => {
            let v = cpu.fetch_word(bus);
            cpu.regs.sp = v;
            12
        }
        0x32 => {
            bus.write(cpu.regs.hl(), cpu.regs.a);
            cpu.regs.set_hl(cpu.regs.hl().wrapping_sub(1));
            8
        }
        0x33 => {
            cpu.regs.sp = cpu.regs.sp.wrapping_add(1);
            8
        }
        0x34 => {
            let v = bus.read(cpu.regs.hl());
            let r = inc8(cpu, v);
            bus.write(cpu.regs.hl(), r);
            12
        }
        0x35 => {
            let v = bus.read(cpu.regs.hl());
            let r = dec8(cpu, v);
            bus.write(cpu.regs.hl(), r);
            12
        }
        0x36 => {
            let v = cpu.fetch_byte(bus);
            bus.write(cpu.regs.hl(), v);
            12
        }
        0x37 => {
            cpu.regs.set_flag(FLAG_N, false);
            cpu.regs.set_flag(FLAG_H, false);
            cpu.regs.set_flag(FLAG_C, true);
            4
        }
        0x38 => jr_cc(cpu, bus, cpu.regs.carry()),
        0x39 => {
            add16(cpu, cpu.regs.sp);
            8
        }
        0x3A => {
            cpu.regs.a = bus.read(cpu.regs.hl());
            cpu.regs.set_hl(cpu.regs.hl().wrapping_sub(1));
            8
        }
        0x3B => {
            cpu.regs.sp = cpu.regs.sp.wrapping_sub(1);
            8
        }
        0x3C => {
            cpu.regs.a = inc8(cpu, cpu.regs.a);
            4
        }
        0x3D => {
            cpu.regs.a = dec8(cpu, cpu.regs.a);
            4
        }
        0x3E => {
            cpu.regs.a = cpu.fetch_byte(bus);
            8
        }
        0x3F => {
            let c = cpu.regs.carry();
            cpu.regs.set_flag(FLAG_N, false);
            cpu.regs.set_flag(FLAG_H, false);
            cpu.regs.set_flag(FLAG_C, !c);
            4
        }

        // --- 0x40..0x7F: 8-bit register/(HL) loads, HALT at 0x76 ---
        0x76 => {
            halt(cpu, bus);
            4
        }
        0x40..=0x7F => ld_r_r(cpu, bus, opcode),

        // --- 0x80..0xBF: ALU A,r ---
        0x80..=0xBF => alu_a_r(cpu, bus, opcode),

        // --- 0xC0..0xFF: control flow, stack, misc ---
        0xC0 => ret_cc(cpu, bus, !cpu.regs.zero()),
        0xC1 => {
            let v = pop(cpu, bus);
            cpu.regs.set_bc(v);
            12
        }
        0xC2 => jp_cc(cpu, bus, !cpu.regs.zero()),
        0xC3 => {
            cpu.regs.pc = cpu.fetch_word(bus);
            16
        }
        0xC4 => call_cc(cpu, bus, !cpu.regs.zero()),
        0xC5 => {
            push(cpu, bus, cpu.regs.bc());
            16
        }
        0xC6 => {
            let v = cpu.fetch_byte(bus);
            add8(cpu, v);
            8
        }
        0xC7 => {
            rst(cpu, bus, 0x00);
            16
        }
        0xC8 => ret_cc(cpu, bus, cpu.regs.zero()),
        0xC9 => {
            cpu.regs.pc = pop(cpu, bus);
            16
        }
        0xCA => jp_cc(cpu, bus, cpu.regs.zero()),
        0xCB => {
            let cb_op = cpu.fetch_byte(bus);
            cb::execute_cb(cpu, bus, cb_op)
        }
        0xCC => call_cc(cpu, bus, cpu.regs.zero()),
        0xCD => {
            let addr = cpu.fetch_word(bus);
            push(cpu, bus, cpu.regs.pc);
            cpu.regs.pc = addr;
            24
        }
        0xCE => {
            let v = cpu.fetch_byte(bus);
            adc8(cpu, v);
            8
        }
        0xCF => {
            rst(cpu, bus, 0x08);
            16
        }

        0xD0 => ret_cc(cpu, bus, !cpu.regs.carry()),
        0xD1 => {
            let v = pop(cpu, bus);
            cpu.regs.set_de(v);
            12
        }
        0xD2 => jp_cc(cpu, bus, !cpu.regs.carry()),
        0xD3 => 4, // unused opcode; treated as NOP-with-length
        0xD4 => call_cc(cpu, bus, !cpu.regs.carry()),
        0xD5 => {
            push(cpu, bus, cpu.regs.de());
            16
        }
        0xD6 => {
            let v = cpu.fetch_byte(bus);
            sub8(cpu, v);
            8
        }
        0xD7 => {
            rst(cpu, bus, 0x10);
            16
        }
        0xD8 => ret_cc(cpu, bus, cpu.regs.carry()),
        0xD9 => {
            cpu.regs.pc = pop(cpu, bus);
            cpu.ime = true;
            16
        }
        0xDA => jp_cc(cpu, bus, cpu.regs.carry()),
        0xDB => 4, // unused opcode
        0xDC => call_cc(cpu, bus, cpu.regs.carry()),
        0xDD => 4, // unused opcode
        0xDE => {
            let v = cpu.fetch_byte(bus);
            sbc8(cpu, v);
            8
        }
        0xDF => {
            rst(cpu, bus, 0x18);
            16
        }

        0xE0 => {
            let off = cpu.fetch_byte(bus) as u16;
            bus.write(0xFF00 + off, cpu.regs.a);
            12
        }
        0xE1 => {
            let v = pop(cpu, bus);
            cpu.regs.set_hl(v);
            12
        }
        0xE2 => {
            bus.write(0xFF00 + cpu.regs.c as u16, cpu.regs.a);
            8
        }
        0xE3 => 4, // unused opcode
        0xE4 => 4, // unused opcode
        0xE5 => {
            push(cpu, bus, cpu.regs.hl());
            16
        }
        0xE6 => {
            let v = cpu.fetch_byte(bus);
            and8(cpu, v);
            8
        }
        0xE7 => {
            rst(cpu, bus, 0x20);
            16
        }
        0xE8 => {
            add_sp_i8(cpu, bus);
            16
        }
        0xE9 => {
            cpu.regs.pc = cpu.regs.hl();
            4
        }
        0xEA => {
            let addr = cpu.fetch_word(bus);
            bus.write(addr, cpu.regs.a);
            16
        }
        0xEB => 4, // unused opcode
        0xEC => 4, // unused opcode
        0xED => 4, // unused opcode
        0xEE => {
            let v = cpu.fetch_byte(bus);
            xor8(cpu, v);
            8
        }
        0xEF => {
            rst(cpu, bus, 0x28);
            16
        }

        0xF0 => {
            let off = cpu.fetch_byte(bus) as u16;
            cpu.regs.a = bus.read(0xFF00 + off);
            12
        }
        0xF1 => {
            let v = pop(cpu, bus);
            cpu.regs.set_af(v);
            12
        }
        0xF2 => {
            cpu.regs.a = bus.read(0xFF00 + cpu.regs.c as u16);
            8
        }
        0xF3 => {
            cpu.ime = false;
            4
        }
        0xF4 => 4, // unused opcode
        0xF5 => {
            push(cpu, bus, cpu.regs.af());
            16
        }
        0xF6 => {
            let v = cpu.fetch_byte(bus);
            or8(cpu, v);
            8
        }
        0xF7 => {
            rst(cpu, bus, 0x30);
            16
        }
        0xF8 => {
            ld_hl_sp_i8(cpu, bus);
            12
        }
        0xF9 => {
            cpu.regs.sp = cpu.regs.hl();
            8
        }
        0xFA => {
            let addr = cpu.fetch_word(bus);
            cpu.regs.a = bus.read(addr);
            16
        }
        0xFB => {
            cpu.schedule_ime_enable();
            4
        }
        0xFC => 4, // unused opcode
        0xFD => 4, // unused opcode
        0xFE => {
            let v = cpu.fetch_byte(bus);
            cp8(cpu, v);
            8
        }
        0xFF => {
            rst(cpu, bus, 0x38);
            16
        }
    }
}

pub(super) mod cb;

// --- r8 helpers: standard SM83 encoding, 0=B 1=C 2=D 3=E 4=H 5=L 6=(HL) 7=A ---

fn read_r8(cpu: &mut Cpu, bus: &mut impl Bus, idx: u8) -> u8 {
    match idx {
        0 => cpu.regs.b,
        1 => cpu.regs.c,
        2 => cpu.regs.d,
        3 => cpu.regs.e,
        4 => cpu.regs.h,
        5 => cpu.regs.l,
        6 => bus.read(cpu.regs.hl()),
        7 => cpu.regs.a,
        _ => unreachable!(),
    }
}

fn write_r8(cpu: &mut Cpu, bus: &mut impl Bus, idx: u8, val: u8) {
    match idx {
        0 => cpu.regs.b = val,
        1 => cpu.regs.c = val,
        2 => cpu.regs.d = val,
        3 => cpu.regs.e = val,
        4 => cpu.regs.h = val,
        5 => cpu.regs.l = val,
        6 => bus.write(cpu.regs.hl(), val),
        7 => cpu.regs.a = val,
        _ => unreachable!(),
    }
}

/// 0x40..=0x7F (excluding 0x76 HALT, handled by caller): `LD r, r'`.
fn ld_r_r(cpu: &mut Cpu, bus: &mut impl Bus, opcode: u8) -> u8 {
    let dst = (opcode >> 3) & 0x07;
    let src = opcode & 0x07;
    let v = read_r8(cpu, bus, src);
    write_r8(cpu, bus, dst, v);
    if dst == 6 || src == 6 {
        8
    } else {
        4
    }
}

/// 0x80..=0xBF: `ALU A, r` (ADD/ADC/SUB/SBC/AND/XOR/OR/CP).
fn alu_a_r(cpu: &mut Cpu, bus: &mut impl Bus, opcode: u8) -> u8 {
    let op = (opcode >> 3) & 0x07;
    let src = opcode & 0x07;
    let v = read_r8(cpu, bus, src);
    match op {
        0 => add8(cpu, v),
        1 => adc8(cpu, v),
        2 => sub8(cpu, v),
        3 => sbc8(cpu, v),
        4 => and8(cpu, v),
        5 => xor8(cpu, v),
        6 => or8(cpu, v),
        7 => cp8(cpu, v),
        _ => unreachable!(),
    }
    if src == 6 {
        8
    } else {
        4
    }
}

// --- 8-bit ALU ---

fn add8(cpu: &mut Cpu, v: u8) {
    let a = cpu.regs.a;
    let (r, carry) = a.overflowing_add(v);
    let half = (a & 0x0F) + (v & 0x0F) > 0x0F;
    cpu.regs.a = r;
    cpu.regs.set_flag(FLAG_Z, r == 0);
    cpu.regs.set_flag(FLAG_N, false);
    cpu.regs.set_flag(FLAG_H, half);
    cpu.regs.set_flag(FLAG_C, carry);
}

fn adc8(cpu: &mut Cpu, v: u8) {
    let a = cpu.regs.a;
    let c = if cpu.regs.carry() { 1u8 } else { 0 };
    let r = a.wrapping_add(v).wrapping_add(c);
    let carry = (a as u16) + (v as u16) + (c as u16) > 0xFF;
    let half = (a & 0x0F) + (v & 0x0F) + c > 0x0F;
    cpu.regs.a = r;
    cpu.regs.set_flag(FLAG_Z, r == 0);
    cpu.regs.set_flag(FLAG_N, false);
    cpu.regs.set_flag(FLAG_H, half);
    cpu.regs.set_flag(FLAG_C, carry);
}

fn sub8(cpu: &mut Cpu, v: u8) {
    let a = cpu.regs.a;
    let (r, carry) = a.overflowing_sub(v);
    let half = (a & 0x0F) < (v & 0x0F);
    cpu.regs.a = r;
    cpu.regs.set_flag(FLAG_Z, r == 0);
    cpu.regs.set_flag(FLAG_N, true);
    cpu.regs.set_flag(FLAG_H, half);
    cpu.regs.set_flag(FLAG_C, carry);
}

fn sbc8(cpu: &mut Cpu, v: u8) {
    let a = cpu.regs.a;
    let c = if cpu.regs.carry() { 1u8 } else { 0 };
    let r = a.wrapping_sub(v).wrapping_sub(c);
    let carry = (a as i16) - (v as i16) - (c as i16) < 0;
    let half = (a as i16 & 0x0F) - (v as i16 & 0x0F) - (c as i16) < 0;
    cpu.regs.a = r;
    cpu.regs.set_flag(FLAG_Z, r == 0);
    cpu.regs.set_flag(FLAG_N, true);
    cpu.regs.set_flag(FLAG_H, half);
    cpu.regs.set_flag(FLAG_C, carry);
}

fn and8(cpu: &mut Cpu, v: u8) {
    cpu.regs.a &= v;
    cpu.regs.set_flag(FLAG_Z, cpu.regs.a == 0);
    cpu.regs.set_flag(FLAG_N, false);
    cpu.regs.set_flag(FLAG_H, true);
    cpu.regs.set_flag(FLAG_C, false);
}

fn xor8(cpu: &mut Cpu, v: u8) {
    cpu.regs.a ^= v;
    cpu.regs.set_flag(FLAG_Z, cpu.regs.a == 0);
    cpu.regs.set_flag(FLAG_N, false);
    cpu.regs.set_flag(FLAG_H, false);
    cpu.regs.set_flag(FLAG_C, false);
}

fn or8(cpu: &mut Cpu, v: u8) {
    cpu.regs.a |= v;
    cpu.regs.set_flag(FLAG_Z, cpu.regs.a == 0);
    cpu.regs.set_flag(FLAG_N, false);
    cpu.regs.set_flag(FLAG_H, false);
    cpu.regs.set_flag(FLAG_C, false);
}

fn cp8(cpu: &mut Cpu, v: u8) {
    let a = cpu.regs.a;
    sub8(cpu, v);
    cpu.regs.a = a; // CP doesn't store the result
}

fn inc8(cpu: &mut Cpu, v: u8) -> u8 {
    let r = v.wrapping_add(1);
    cpu.regs.set_flag(FLAG_Z, r == 0);
    cpu.regs.set_flag(FLAG_N, false);
    cpu.regs.set_flag(FLAG_H, (v & 0x0F) == 0x0F);
    r
}

fn dec8(cpu: &mut Cpu, v: u8) -> u8 {
    let r = v.wrapping_sub(1);
    cpu.regs.set_flag(FLAG_Z, r == 0);
    cpu.regs.set_flag(FLAG_N, true);
    cpu.regs.set_flag(FLAG_H, (v & 0x0F) == 0);
    r
}

fn daa(cpu: &mut Cpu) {
    let mut a = cpu.regs.a;
    let mut carry = cpu.regs.carry();
    if !cpu.regs.subtract() {
        if cpu.regs.carry() || a > 0x99 {
            a = a.wrapping_add(0x60);
            carry = true;
        }
        if cpu.regs.half_carry() || (a & 0x0F) > 0x09 {
            a = a.wrapping_add(0x06);
        }
    } else {
        if cpu.regs.carry() {
            a = a.wrapping_sub(0x60);
        }
        if cpu.regs.half_carry() {
            a = a.wrapping_sub(0x06);
        }
    }
    cpu.regs.a = a;
    cpu.regs.set_flag(FLAG_Z, a == 0);
    cpu.regs.set_flag(FLAG_H, false);
    cpu.regs.set_flag(FLAG_C, carry);
}

// --- 16-bit arithmetic ---

fn add16(cpu: &mut Cpu, v: u16) {
    let hl = cpu.regs.hl();
    let (r, carry) = hl.overflowing_add(v);
    let half = (hl & 0x0FFF) + (v & 0x0FFF) > 0x0FFF;
    cpu.regs.set_hl(r);
    cpu.regs.set_flag(FLAG_N, false);
    cpu.regs.set_flag(FLAG_H, half);
    cpu.regs.set_flag(FLAG_C, carry);
}

fn add_sp_i8(cpu: &mut Cpu, bus: &mut impl Bus) {
    let off = cpu.fetch_byte(bus) as i8;
    let sp = cpu.regs.sp;
    let off16 = off as i16 as u16;
    let r = sp.wrapping_add(off16);
    let half = (sp & 0x0F) + (off16 & 0x0F) > 0x0F;
    let carry = (sp & 0xFF) + (off16 & 0xFF) > 0xFF;
    cpu.regs.sp = r;
    cpu.regs.set_flag(FLAG_Z, false);
    cpu.regs.set_flag(FLAG_N, false);
    cpu.regs.set_flag(FLAG_H, half);
    cpu.regs.set_flag(FLAG_C, carry);
}

/// `LD HL,SP+e8` (0xF8): same offset-add flag semantics as `ADD SP,e8`
/// (`add_sp_i8`), but the result lands in `HL` — `SP` itself is
/// unaffected.
fn ld_hl_sp_i8(cpu: &mut Cpu, bus: &mut impl Bus) {
    let off = cpu.fetch_byte(bus) as i8;
    let sp = cpu.regs.sp;
    let off16 = off as i16 as u16;
    let r = sp.wrapping_add(off16);
    let half = (sp & 0x0F) + (off16 & 0x0F) > 0x0F;
    let carry = (sp & 0xFF) + (off16 & 0xFF) > 0xFF;
    cpu.regs.set_hl(r);
    cpu.regs.set_flag(FLAG_Z, false);
    cpu.regs.set_flag(FLAG_N, false);
    cpu.regs.set_flag(FLAG_H, half);
    cpu.regs.set_flag(FLAG_C, carry);
}

// --- rotates (accumulator forms: RLCA/RRCA/RLA/RRA) ---

fn rlca(cpu: &mut Cpu) {
    let a = cpu.regs.a;
    let carry = a & 0x80 != 0;
    cpu.regs.a = a.rotate_left(1);
    cpu.regs.set_flag(FLAG_Z, false);
    cpu.regs.set_flag(FLAG_N, false);
    cpu.regs.set_flag(FLAG_H, false);
    cpu.regs.set_flag(FLAG_C, carry);
}

fn rrca(cpu: &mut Cpu) {
    let a = cpu.regs.a;
    let carry = a & 0x01 != 0;
    cpu.regs.a = a.rotate_right(1);
    cpu.regs.set_flag(FLAG_Z, false);
    cpu.regs.set_flag(FLAG_N, false);
    cpu.regs.set_flag(FLAG_H, false);
    cpu.regs.set_flag(FLAG_C, carry);
}

fn rla(cpu: &mut Cpu) {
    let a = cpu.regs.a;
    let old_c = if cpu.regs.carry() { 1u8 } else { 0 };
    let carry = a & 0x80 != 0;
    cpu.regs.a = (a << 1) | old_c;
    cpu.regs.set_flag(FLAG_Z, false);
    cpu.regs.set_flag(FLAG_N, false);
    cpu.regs.set_flag(FLAG_H, false);
    cpu.regs.set_flag(FLAG_C, carry);
}

fn rra(cpu: &mut Cpu) {
    let a = cpu.regs.a;
    let old_c = if cpu.regs.carry() { 0x80u8 } else { 0 };
    let carry = a & 0x01 != 0;
    cpu.regs.a = (a >> 1) | old_c;
    cpu.regs.set_flag(FLAG_Z, false);
    cpu.regs.set_flag(FLAG_N, false);
    cpu.regs.set_flag(FLAG_H, false);
    cpu.regs.set_flag(FLAG_C, carry);
}

// --- stack helpers ---

fn push(cpu: &mut Cpu, bus: &mut impl Bus, v: u16) {
    cpu.regs.sp = cpu.regs.sp.wrapping_sub(2);
    bus.write16(cpu.regs.sp, v);
}

fn pop(cpu: &mut Cpu, bus: &mut impl Bus) -> u16 {
    let v = bus.read16(cpu.regs.sp);
    cpu.regs.sp = cpu.regs.sp.wrapping_add(2);
    v
}

fn rst(cpu: &mut Cpu, bus: &mut impl Bus, addr: u16) {
    push(cpu, bus, cpu.regs.pc);
    cpu.regs.pc = addr;
}

// --- HALT + HALT bug ---

/// HALT (0x76): halts CPU fetch/execute until an enabled interrupt wakes it
/// — *unless* IME=0 and an interrupt is already pending-and-enabled at the
/// moment HALT executes, in which case real hardware does not halt at all
/// and instead triggers the HALT bug (next opcode byte fetched twice).
fn halt(cpu: &mut Cpu, bus: &mut impl Bus) {
    if !cpu.ime && super::interrupt_pending(bus) {
        cpu.trigger_halt_bug();
    } else {
        cpu.halted = true;
    }
}

// --- control flow ---

fn jr_cc(cpu: &mut Cpu, bus: &mut impl Bus, cond: bool) -> u8 {
    let off = cpu.fetch_byte(bus) as i8;
    if cond {
        cpu.regs.pc = cpu.regs.pc.wrapping_add(off as u16);
        12
    } else {
        8
    }
}

fn jp_cc(cpu: &mut Cpu, bus: &mut impl Bus, cond: bool) -> u8 {
    let addr = cpu.fetch_word(bus);
    if cond {
        cpu.regs.pc = addr;
        16
    } else {
        12
    }
}

fn call_cc(cpu: &mut Cpu, bus: &mut impl Bus, cond: bool) -> u8 {
    let addr = cpu.fetch_word(bus);
    if cond {
        push(cpu, bus, cpu.regs.pc);
        cpu.regs.pc = addr;
        24
    } else {
        12
    }
}

fn ret_cc(cpu: &mut Cpu, bus: &mut impl Bus, cond: bool) -> u8 {
    if cond {
        cpu.regs.pc = pop(cpu, bus);
        20
    } else {
        8
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cpu::bus::FlatBus;

    fn run(prog: &[u8]) -> (Cpu, FlatBus) {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        bus.mem[..prog.len()].copy_from_slice(prog);
        cpu.regs.pc = 0;
        cpu.step(&mut bus);
        (cpu, bus)
    }

    #[test]
    fn nop_advances_pc_and_takes_4_cycles() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        let t = cpu.step(&mut bus);
        assert_eq!(t, 4);
        assert_eq!(cpu.regs.pc, 1);
    }

    #[test]
    fn ld_bc_d16() {
        let (cpu, _) = run(&[0x01, 0xEF, 0xBE]); // LD BC, 0xBEEF
        assert_eq!(cpu.regs.bc(), 0xBEEF);
        assert_eq!(cpu.regs.pc, 3);
    }

    #[test]
    fn ld_a_b_register_to_register() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        cpu.regs.b = 0x42;
        bus.mem[0] = 0x78; // LD A, B
        let t = cpu.step(&mut bus);
        assert_eq!(cpu.regs.a, 0x42);
        assert_eq!(t, 4);
    }

    #[test]
    fn ld_a_hl_indirect_costs_8() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        cpu.regs.set_hl(0x1000);
        bus.mem[0x1000] = 0x99;
        bus.mem[0] = 0x7E; // LD A, (HL)
        let t = cpu.step(&mut bus);
        assert_eq!(cpu.regs.a, 0x99);
        assert_eq!(t, 8);
    }

    #[test]
    fn inc_b_sets_zero_and_half_carry() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        cpu.regs.b = 0xFF;
        bus.mem[0] = 0x04; // INC B
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.b, 0x00);
        assert!(cpu.regs.zero());
        assert!(cpu.regs.half_carry());
        assert!(!cpu.regs.subtract());
    }

    #[test]
    fn dec_b_sets_subtract_flag() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        cpu.regs.b = 0x01;
        bus.mem[0] = 0x05; // DEC B
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.b, 0x00);
        assert!(cpu.regs.zero());
        assert!(cpu.regs.subtract());
    }

    #[test]
    fn add_a_b_sets_carry_and_half_carry() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        cpu.regs.a = 0xFF;
        cpu.regs.b = 0x01;
        bus.mem[0] = 0x80; // ADD A, B
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.a, 0x00);
        assert!(cpu.regs.zero());
        assert!(cpu.regs.carry());
        assert!(cpu.regs.half_carry());
        assert!(!cpu.regs.subtract());
    }

    #[test]
    fn sub_a_b_sets_subtract_and_no_borrow() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        cpu.regs.a = 0x10;
        cpu.regs.b = 0x01;
        bus.mem[0] = 0x90; // SUB B
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.a, 0x0F);
        assert!(cpu.regs.subtract());
        assert!(cpu.regs.half_carry()); // 0x0 < 0x1 nibble borrow
        assert!(!cpu.regs.carry());
    }

    #[test]
    fn cp_does_not_modify_a() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        cpu.regs.a = 0x10;
        cpu.regs.b = 0x10;
        bus.mem[0] = 0xB8; // CP B
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.a, 0x10);
        assert!(cpu.regs.zero());
    }

    #[test]
    fn and_sets_half_carry_always() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        cpu.regs.a = 0xF0;
        cpu.regs.b = 0x0F;
        bus.mem[0] = 0xA0; // AND B
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.a, 0x00);
        assert!(cpu.regs.zero());
        assert!(cpu.regs.half_carry());
        assert!(!cpu.regs.carry());
    }

    #[test]
    fn jp_unconditional() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        bus.mem[0] = 0xC3; // JP 0x1234
        bus.mem[1] = 0x34;
        bus.mem[2] = 0x12;
        let t = cpu.step(&mut bus);
        assert_eq!(cpu.regs.pc, 0x1234);
        assert_eq!(t, 16);
    }

    #[test]
    fn jr_z_not_taken_when_zero_clear() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        cpu.regs.set_flag(FLAG_Z, false);
        bus.mem[0] = 0x28; // JR Z, +5
        bus.mem[1] = 0x05;
        let t = cpu.step(&mut bus);
        assert_eq!(cpu.regs.pc, 2);
        assert_eq!(t, 8);
    }

    #[test]
    fn jr_z_taken_when_zero_set() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        cpu.regs.set_flag(FLAG_Z, true);
        bus.mem[0] = 0x28; // JR Z, +5
        bus.mem[1] = 0x05;
        let t = cpu.step(&mut bus);
        assert_eq!(cpu.regs.pc, 7); // 2 (after operand) + 5
        assert_eq!(t, 12);
    }

    #[test]
    fn call_and_ret_round_trip() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        cpu.regs.sp = 0xFFFE;
        bus.mem[0] = 0xCD; // CALL 0x0010
        bus.mem[1] = 0x10;
        bus.mem[2] = 0x00;
        bus.mem[0x0010] = 0xC9; // RET
        let t1 = cpu.step(&mut bus);
        assert_eq!(cpu.regs.pc, 0x0010);
        assert_eq!(t1, 24);
        let t2 = cpu.step(&mut bus);
        assert_eq!(cpu.regs.pc, 0x0003);
        assert_eq!(t2, 16);
    }

    #[test]
    fn push_pop_round_trip() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        cpu.regs.sp = 0xFFFE;
        cpu.regs.set_bc(0xBEEF);
        bus.mem[0] = 0xC5; // PUSH BC
        bus.mem[1] = 0xD1; // POP DE
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.sp, 0xFFFC);
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.de(), 0xBEEF);
        assert_eq!(cpu.regs.sp, 0xFFFE);
    }

    #[test]
    fn halt_sets_flag_and_stalls() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        bus.mem[0] = 0x76; // HALT
        cpu.step(&mut bus);
        assert!(cpu.halted);
        assert_eq!(cpu.regs.pc, 1);
        let t = cpu.step(&mut bus); // second step: no-op while halted
        assert_eq!(t, 4);
        assert_eq!(cpu.regs.pc, 1); // PC doesn't advance while halted
    }

    #[test]
    fn di_is_immediate_ei_is_delayed_by_one_instruction() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        bus.mem[0] = 0xF3; // DI
        bus.mem[1] = 0xFB; // EI
        bus.mem[2] = 0x00; // NOP - EI's effect lands after this executes
        cpu.step(&mut bus); // DI
        assert!(!cpu.ime);
        cpu.step(&mut bus); // EI: not yet enabled
        assert!(!cpu.ime);
        cpu.step(&mut bus); // NOP: now enabled
        assert!(cpu.ime);
    }

    #[test]
    fn rlca_rotates_and_sets_carry() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        cpu.regs.a = 0x85; // 1000_0101
        bus.mem[0] = 0x07; // RLCA
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.a, 0x0B); // 0000_1011
        assert!(cpu.regs.carry());
    }

    #[test]
    fn cb_bit_sets_zero_when_bit_clear() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        cpu.regs.b = 0x00;
        bus.mem[0] = 0xCB;
        bus.mem[1] = 0x40; // BIT 0, B
        let t = cpu.step(&mut bus);
        assert!(cpu.regs.zero());
        assert_eq!(t, 8);
    }

    #[test]
    fn cb_bit_clear_when_bit_set() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        cpu.regs.b = 0x01;
        bus.mem[0] = 0xCB;
        bus.mem[1] = 0x40; // BIT 0, B
        cpu.step(&mut bus);
        assert!(!cpu.regs.zero());
    }

    #[test]
    fn cb_set_and_res_bit() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        cpu.regs.b = 0x00;
        bus.mem[0] = 0xCB;
        bus.mem[1] = 0xC0; // SET 0, B
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.b, 0x01);

        bus.mem[2] = 0xCB;
        bus.mem[3] = 0x80; // RES 0, B
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.b, 0x00);
    }

    #[test]
    fn cb_swap_nibbles() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        cpu.regs.a = 0x12;
        bus.mem[0] = 0xCB;
        bus.mem[1] = 0x37; // SWAP A
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.a, 0x21);
    }

    #[test]
    fn cb_rlc_hl_costs_16_cycles() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        cpu.regs.set_hl(0x1000);
        bus.mem[0x1000] = 0x80;
        bus.mem[0] = 0xCB;
        bus.mem[1] = 0x06; // RLC (HL)
        let t = cpu.step(&mut bus);
        assert_eq!(t, 16);
        assert_eq!(bus.mem[0x1000], 0x01);
        assert!(cpu.regs.carry());
    }

    #[test]
    fn daa_after_bcd_addition() {
        let mut cpu = Cpu::new();
        let mut bus = FlatBus::new();
        cpu.regs.a = 0x45;
        cpu.regs.b = 0x38;
        bus.mem[0] = 0x80; // ADD A, B -> 0x7D
        bus.mem[1] = 0x27; // DAA -> should become 0x83 (BCD 45+38=83)
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.a, 0x7D);
        assert_eq!(cpu.regs.a, 0x7D);
        cpu.step(&mut bus);
        assert_eq!(cpu.regs.a, 0x83);
    }
}
