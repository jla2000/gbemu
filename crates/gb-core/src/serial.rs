//! Serial port (link cable) stub. Implemented in M1 — used as a loopback
//! target so Blargg test ROMs can report pass/fail via serial output.

/// Placeholder serial port.
#[derive(Debug, Default, Clone)]
pub struct Serial {
    _placeholder: (),
}

impl Serial {
    pub fn new() -> Self {
        Self::default()
    }
}
