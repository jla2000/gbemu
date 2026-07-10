//! Serial port (link cable): `SB` (0xFF01)/`SC` (0xFF02) registers.
//!
//! No link cable is emulated — this is a "loopback" stub: writing `SC` with
//! the transfer-start bit (7) set and the internal-clock bit (0) set
//! immediately captures the current `SB` byte into an output buffer (used
//! by the Blargg test harness to read back ASCII "Passed"/"Failed" text),
//! requests the serial interrupt, and clears the transfer-start bit — all
//! without modeling the real ~8-cycle-per-bit shift timing, since no test
//! ROM this project targets depends on that timing, only on the resulting
//! byte stream and the interrupt firing at all.
//!
//! With no device attached, real hardware shifts in `0xFF` (idle line) as
//! it shifts `SB` out; that's what `sb` becomes after a transfer here too.

/// Serial interrupt request bit (bit 3) in `IF`/`IE`.
pub const SERIAL_INT_BIT: u8 = 1 << 3;

const SC_TRANSFER_START: u8 = 0b1000_0000;
const SC_INTERNAL_CLOCK: u8 = 0b0000_0001;
/// Unused `SC` bits read back as 1 on DMG.
const SC_UNUSED_MASK: u8 = 0b0111_1110;

#[derive(Debug, Default, Clone, serde::Serialize, serde::Deserialize)]
pub struct Serial {
    sb: u8,
    sc: u8,
    /// Bytes captured off completed transfers, in order. The Blargg
    /// harness drains this (via [`Serial::take_output`]) and decodes it as
    /// ASCII to look for "Passed"/"Failed".
    output: Vec<u8>,
    /// Set for exactly one `read`/check cycle after a transfer completes;
    /// the caller (MMU) ORs this into `IF` bit 3, mirroring how a real
    /// serial-complete interrupt request would be raised.
    interrupt_pending: bool,
}

impl Serial {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn read_sb(&self) -> u8 {
        self.sb
    }

    pub fn write_sb(&mut self, val: u8) {
        self.sb = val;
    }

    pub fn read_sc(&self) -> u8 {
        self.sc | SC_UNUSED_MASK
    }

    /// Writing `SC` with bits 7 and 0 both set triggers an immediate
    /// loopback "transfer": `sb` is captured to `output`, the interrupt is
    /// requested, and the transfer-start bit reads back cleared.
    pub fn write_sc(&mut self, val: u8) {
        self.sc = val;
        if val & (SC_TRANSFER_START | SC_INTERNAL_CLOCK) == (SC_TRANSFER_START | SC_INTERNAL_CLOCK)
        {
            self.output.push(self.sb);
            self.sb = 0xFF; // idle line shifted in, no device attached
            self.sc &= !SC_TRANSFER_START;
            self.interrupt_pending = true;
        }
    }

    /// Consumes and returns whether a transfer completed since the last
    /// call — the MMU calls this once per write to fold it into `IF`.
    pub fn take_interrupt(&mut self) -> bool {
        std::mem::take(&mut self.interrupt_pending)
    }

    /// Drains all captured output bytes as a `String` (lossy — test ROM
    /// output is plain ASCII).
    pub fn take_output(&mut self) -> String {
        let bytes = std::mem::take(&mut self.output);
        String::from_utf8_lossy(&bytes).into_owned()
    }

    /// Non-destructive peek, for polling loops that just want to check for
    /// "Passed"/"Failed" without losing bytes across calls.
    pub fn output_so_far(&self) -> String {
        String::from_utf8_lossy(&self.output).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transfer_captures_byte_and_requests_interrupt() {
        let mut s = Serial::new();
        s.write_sb(b'P');
        s.write_sc(0x81); // start + internal clock
        assert_eq!(s.output_so_far(), "P");
        assert!(s.take_interrupt());
        assert!(!s.take_interrupt()); // consumed
        assert_eq!(s.read_sc() & 0x80, 0); // transfer-start bit cleared
        assert_eq!(s.read_sb(), 0xFF); // idle line shifted in
    }

    #[test]
    fn write_without_internal_clock_does_not_transfer() {
        let mut s = Serial::new();
        s.write_sb(b'X');
        s.write_sc(0x80); // start bit only, external clock
        assert_eq!(s.output_so_far(), "");
        assert!(!s.take_interrupt());
    }

    #[test]
    fn take_output_drains_accumulated_bytes() {
        let mut s = Serial::new();
        for &b in b"Passed" {
            s.write_sb(b);
            s.write_sc(0x81);
        }
        assert_eq!(s.take_output(), "Passed");
        assert_eq!(s.take_output(), ""); // drained
    }
}
