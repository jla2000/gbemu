//! APU (audio processing unit). Implemented in M5.

/// Placeholder APU state. Channel/mixer implementation lands in M5.
#[derive(Debug, Default, Clone)]
pub struct Apu {
    _placeholder: (),
}

impl Apu {
    pub fn new() -> Self {
        Self::default()
    }
}
