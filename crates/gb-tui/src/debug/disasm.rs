//! SM83 disassembler for the debugger's disassembly panel. Display-only —
//! decodes a byte sequence into a mnemonic and instruction length, using
//! the same opcode encoding `gb-core`'s `cpu::execute` dispatches on (the
//! standard `x`/`y`/`z`/`p`/`q` bit-field decomposition), cross-checked
//! against it rather than transcribed from memory.

const R: [&str; 8] = ["B", "C", "D", "E", "H", "L", "(HL)", "A"];
const RP: [&str; 4] = ["BC", "DE", "HL", "SP"];
const RP2: [&str; 4] = ["BC", "DE", "HL", "AF"];
const CC: [&str; 4] = ["NZ", "Z", "NC", "C"];
const ALU: [&str; 8] = ["ADD A,", "ADC A,", "SUB ", "SBC A,", "AND ", "XOR ", "OR ", "CP "];
const ROT: [&str; 8] = ["RLC", "RRC", "RL", "RR", "SLA", "SRA", "SWAP", "SRL"];

/// One decoded instruction: its mnemonic text and length in bytes (1-3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instruction {
    pub mnemonic: String,
    pub len: u16,
}

fn imm8(bytes: &[u8]) -> u8 {
    bytes.get(1).copied().unwrap_or(0)
}

fn imm16(bytes: &[u8]) -> u16 {
    let lo = bytes.get(1).copied().unwrap_or(0) as u16;
    let hi = bytes.get(2).copied().unwrap_or(0) as u16;
    (hi << 8) | lo
}

/// Decodes the instruction at `bytes[0..]` (which must be non-empty;
/// shorter-than-needed operand bytes read as 0). `pc` is the address of
/// `bytes[0]`, used to compute `JR`'s absolute jump target for display.
pub fn disassemble_one(bytes: &[u8], pc: u16) -> Instruction {
    let mk = |mnemonic: String, len: u16| Instruction { mnemonic, len };
    let Some(&op) = bytes.first() else {
        return mk("??".to_string(), 1);
    };

    if op == 0xCB {
        let op2 = bytes.get(1).copied().unwrap_or(0);
        return mk(disassemble_cb(op2), 2);
    }

    // GB-specific / irregular opcodes that don't fit the generic x/y/z
    // table below (deviations from the base Z80 encoding, and the
    // handful of officially unused/illegal opcodes).
    match op {
        0x00 => return mk("NOP".into(), 1),
        0x08 => return mk(format!("LD (${:04X}),SP", imm16(bytes)), 3),
        0x10 => return mk("STOP".into(), 2),
        0x76 => return mk("HALT".into(), 1),
        0xC3 => return mk(format!("JP ${:04X}", imm16(bytes)), 3),
        0xC9 => return mk("RET".into(), 1),
        0xD9 => return mk("RETI".into(), 1),
        0xE9 => return mk("JP HL".into(), 1),
        0xF9 => return mk("LD SP,HL".into(), 1),
        0xCD => return mk(format!("CALL ${:04X}", imm16(bytes)), 3),
        0xF3 => return mk("DI".into(), 1),
        0xFB => return mk("EI".into(), 1),
        0xE0 => return mk(format!("LDH (${:02X}),A", imm8(bytes)), 2),
        0xF0 => return mk(format!("LDH A,(${:02X})", imm8(bytes)), 2),
        0xE2 => return mk("LD (C),A".into(), 1),
        0xF2 => return mk("LD A,(C)".into(), 1),
        0xE8 => return mk(format!("ADD SP,{}", imm8(bytes) as i8), 2),
        0xF8 => return mk(format!("LD HL,SP{:+}", imm8(bytes) as i8), 2),
        0xEA => return mk(format!("LD (${:04X}),A", imm16(bytes)), 3),
        0xFA => return mk(format!("LD A,(${:04X})", imm16(bytes)), 3),
        0xD3 | 0xDB | 0xDD | 0xE3 | 0xE4 | 0xEB | 0xEC | 0xED | 0xF4 | 0xFC | 0xFD => {
            return mk(format!("DB ${op:02X}"), 1); // unused/illegal on SM83
        }
        _ => {}
    }

    let x = op >> 6;
    let y = (op >> 3) & 0x07;
    let z = op & 0x07;
    let p = (y >> 1) as usize;
    let q = y & 1;

    match x {
        0 => match z {
            0 => {
                if y == 3 {
                    mk(format!("JR ${:04X}", jr_target(pc, imm8(bytes))), 2)
                } else {
                    mk(format!("JR {},${:04X}", CC[(y - 4) as usize], jr_target(pc, imm8(bytes))), 2)
                }
            }
            1 => {
                if q == 0 {
                    mk(format!("LD {},${:04X}", RP[p], imm16(bytes)), 3)
                } else {
                    mk(format!("ADD HL,{}", RP[p]), 1)
                }
            }
            2 => {
                let text = match (q, p) {
                    (0, 0) => "LD (BC),A",
                    (0, 1) => "LD (DE),A",
                    (0, 2) => "LD (HL+),A",
                    (0, 3) => "LD (HL-),A",
                    (1, 0) => "LD A,(BC)",
                    (1, 1) => "LD A,(DE)",
                    (1, 2) => "LD A,(HL+)",
                    _ => "LD A,(HL-)",
                };
                mk(text.into(), 1)
            }
            3 => {
                if q == 0 {
                    mk(format!("INC {}", RP[p]), 1)
                } else {
                    mk(format!("DEC {}", RP[p]), 1)
                }
            }
            4 => mk(format!("INC {}", R[y as usize]), 1),
            5 => mk(format!("DEC {}", R[y as usize]), 1),
            6 => mk(format!("LD {},${:02X}", R[y as usize], imm8(bytes)), 2),
            _ => mk(["RLCA", "RRCA", "RLA", "RRA", "DAA", "CPL", "SCF", "CCF"][y as usize].into(), 1),
        },
        1 => mk(format!("LD {},{}", R[y as usize], R[z as usize]), 1),
        2 => mk(format!("{}{}", ALU[y as usize], R[z as usize]), 1),
        _ => match z {
            0 => mk(format!("RET {}", CC[y as usize]), 1),
            1 => mk(format!("POP {}", RP2[p]), 1), // q==1 cases handled above as literal opcodes
            2 => mk(format!("JP {},${:04X}", CC[y as usize], imm16(bytes)), 3),
            4 => mk(format!("CALL {},${:04X}", CC[y as usize], imm16(bytes)), 3),
            5 => mk(format!("PUSH {}", RP2[p]), 1), // q==1 (0xCD) handled above
            6 => mk(format!("{}${:02X}", ALU[y as usize], imm8(bytes)), 2),
            _ => mk(format!("RST ${:02X}", y * 8), 1),
        },
    }
}

fn jr_target(pc: u16, offset: u8) -> u16 {
    pc.wrapping_add(2).wrapping_add(offset as i8 as u16)
}

fn disassemble_cb(op2: u8) -> String {
    let r = R[(op2 & 0x07) as usize];
    let y = (op2 >> 3) & 0x07;
    match op2 >> 6 {
        0 => format!("{} {r}", ROT[y as usize]),
        1 => format!("BIT {y},{r}"),
        2 => format!("RES {y},{r}"),
        _ => format!("SET {y},{r}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mnem(bytes: &[u8]) -> String {
        disassemble_one(bytes, 0x0100).mnemonic
    }
    fn len(bytes: &[u8]) -> u16 {
        disassemble_one(bytes, 0x0100).len
    }

    #[test]
    fn decodes_nop_and_simple_loads() {
        assert_eq!(mnem(&[0x00]), "NOP");
        assert_eq!(mnem(&[0x06, 0x42]), "LD B,$42");
        assert_eq!(mnem(&[0x21, 0x34, 0x12]), "LD HL,$1234");
        assert_eq!(len(&[0x21, 0x34, 0x12]), 3);
    }

    #[test]
    fn decodes_register_to_register_loads_and_halt() {
        assert_eq!(mnem(&[0x78]), "LD A,B"); // x=1,y=7,z=0
        assert_eq!(mnem(&[0x76]), "HALT"); // the one x=1 slot that isn't LD (HL),(HL)
    }

    #[test]
    fn decodes_alu_ops_against_registers_and_immediates() {
        assert_eq!(mnem(&[0x80]), "ADD A,B");
        assert_eq!(mnem(&[0xA8]), "XOR B");
        assert_eq!(mnem(&[0xFE, 0x10]), "CP $10");
    }

    #[test]
    fn decodes_jr_with_absolute_target_and_conditions() {
        // JR $05 at PC=0x0100: target = 0x0100 + 2 + 5 = 0x0107
        assert_eq!(mnem(&[0x18, 0x05]), "JR $0107");
        // JR NZ, -2 (0xFE as i8 = -2): target = 0x0100+2-2 = 0x0100
        assert_eq!(mnem(&[0x20, 0xFE]), "JR NZ,$0100");
    }

    #[test]
    fn decodes_calls_jumps_rst_push_pop() {
        assert_eq!(mnem(&[0xCD, 0x00, 0x40]), "CALL $4000");
        assert_eq!(mnem(&[0xC4, 0x00, 0x40]), "CALL NZ,$4000");
        assert_eq!(mnem(&[0xC3, 0xAD, 0xDE]), "JP $DEAD");
        assert_eq!(mnem(&[0xFF]), "RST $38");
        assert_eq!(mnem(&[0xC5]), "PUSH BC");
        assert_eq!(mnem(&[0xF1]), "POP AF");
    }

    #[test]
    fn decodes_gb_specific_opcodes() {
        assert_eq!(mnem(&[0xE0, 0x44]), "LDH ($44),A");
        assert_eq!(mnem(&[0xF0, 0x44]), "LDH A,($44)");
        assert_eq!(mnem(&[0xE2]), "LD (C),A");
        assert_eq!(mnem(&[0x22]), "LD (HL+),A");
        assert_eq!(mnem(&[0x3A]), "LD A,(HL-)");
        assert_eq!(mnem(&[0x10, 0x00]), "STOP");
        assert_eq!(len(&[0x10, 0x00]), 2);
    }

    #[test]
    fn decodes_cb_prefixed_rotate_bit_res_set() {
        assert_eq!(mnem(&[0xCB, 0x00]), "RLC B");
        assert_eq!(mnem(&[0xCB, 0x7F]), "BIT 7,A");
        assert_eq!(mnem(&[0xCB, 0x86]), "RES 0,(HL)");
        assert_eq!(mnem(&[0xCB, 0xFF]), "SET 7,A");
        assert_eq!(len(&[0xCB, 0x00]), 2);
    }

    #[test]
    fn decodes_illegal_opcodes_as_db() {
        assert_eq!(mnem(&[0xD3]), "DB $D3");
        assert_eq!(mnem(&[0xFD]), "DB $FD");
    }

    #[test]
    fn all_256_base_and_cb_opcodes_decode_without_panicking() {
        for op in 0u16..256 {
            let _ = disassemble_one(&[op as u8, 0, 0], 0);
        }
        for op2 in 0u16..256 {
            let _ = disassemble_one(&[0xCB, op2 as u8], 0);
        }
    }
}
