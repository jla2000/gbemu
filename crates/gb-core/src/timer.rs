//! Timer: DIV/TIMA/TMA/TAC and the timer interrupt. Implemented in M4.

/// Placeholder timer.
#[derive(Debug, Default, Clone)]
pub struct Timer {
    _placeholder: (),
}

impl Timer {
    pub fn new() -> Self {
        Self::default()
    }
}
