//! SM83 CPU core. Implemented in M1.

/// Placeholder CPU state. Real register set/flags land in M1.
#[derive(Debug, Default, Clone)]
pub struct Cpu {
    _placeholder: (),
}

impl Cpu {
    pub fn new() -> Self {
        Self::default()
    }
}
