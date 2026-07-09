//! Joypad register and button state. Implemented in M4.

/// Placeholder joypad.
#[derive(Debug, Default, Clone)]
pub struct Joypad {
    _placeholder: (),
}

impl Joypad {
    pub fn new() -> Self {
        Self::default()
    }
}
