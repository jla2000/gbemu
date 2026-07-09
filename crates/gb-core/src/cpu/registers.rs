//! SM83 register file: 8-bit registers (viewable as 16-bit pairs) + flags.

/// Zero flag bit in `F`.
pub const FLAG_Z: u8 = 0b1000_0000;
/// Subtract/negative flag bit in `F`.
pub const FLAG_N: u8 = 0b0100_0000;
/// Half-carry flag bit in `F`.
pub const FLAG_H: u8 = 0b0010_0000;
/// Carry flag bit in `F`.
pub const FLAG_C: u8 = 0b0001_0000;

/// SM83 register set: A/B/C/D/E/H/L + flags (F), SP, PC.
///
/// `F`'s low nibble is always zero — enforced by [`Registers::set_f`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Registers {
    pub a: u8,
    pub f: u8,
    pub b: u8,
    pub c: u8,
    pub d: u8,
    pub e: u8,
    pub h: u8,
    pub l: u8,
    pub sp: u16,
    pub pc: u16,
}

impl Registers {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    pub fn af(&self) -> u16 {
        (self.a as u16) << 8 | self.f as u16
    }

    #[inline]
    pub fn set_af(&mut self, v: u16) {
        self.a = (v >> 8) as u8;
        self.f = (v as u8) & 0xF0;
    }

    #[inline]
    pub fn bc(&self) -> u16 {
        (self.b as u16) << 8 | self.c as u16
    }

    #[inline]
    pub fn set_bc(&mut self, v: u16) {
        self.b = (v >> 8) as u8;
        self.c = v as u8;
    }

    #[inline]
    pub fn de(&self) -> u16 {
        (self.d as u16) << 8 | self.e as u16
    }

    #[inline]
    pub fn set_de(&mut self, v: u16) {
        self.d = (v >> 8) as u8;
        self.e = v as u8;
    }

    #[inline]
    pub fn hl(&self) -> u16 {
        (self.h as u16) << 8 | self.l as u16
    }

    #[inline]
    pub fn set_hl(&mut self, v: u16) {
        self.h = (v >> 8) as u8;
        self.l = v as u8;
    }

    #[inline]
    pub fn set_f(&mut self, v: u8) {
        self.f = v & 0xF0;
    }

    #[inline]
    pub fn flag(&self, mask: u8) -> bool {
        self.f & mask != 0
    }

    #[inline]
    pub fn set_flag(&mut self, mask: u8, set: bool) {
        if set {
            self.f |= mask;
        } else {
            self.f &= !mask;
        }
        self.f &= 0xF0;
    }

    #[inline]
    pub fn zero(&self) -> bool {
        self.flag(FLAG_Z)
    }
    #[inline]
    pub fn subtract(&self) -> bool {
        self.flag(FLAG_N)
    }
    #[inline]
    pub fn half_carry(&self) -> bool {
        self.flag(FLAG_H)
    }
    #[inline]
    pub fn carry(&self) -> bool {
        self.flag(FLAG_C)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_round_trip() {
        let mut r = Registers::new();
        r.set_af(0x1234);
        assert_eq!(r.a, 0x12);
        assert_eq!(r.f, 0x30); // low nibble masked off
        assert_eq!(r.af(), 0x1230);

        r.set_bc(0xBEEF);
        assert_eq!(r.bc(), 0xBEEF);
        r.set_de(0xCAFE);
        assert_eq!(r.de(), 0xCAFE);
        r.set_hl(0xABCD);
        assert_eq!(r.hl(), 0xABCD);
    }

    #[test]
    fn flags_set_and_read() {
        let mut r = Registers::new();
        r.set_flag(FLAG_Z, true);
        r.set_flag(FLAG_C, true);
        assert!(r.zero());
        assert!(r.carry());
        assert!(!r.subtract());
        assert!(!r.half_carry());
        r.set_flag(FLAG_Z, false);
        assert!(!r.zero());
    }
}
