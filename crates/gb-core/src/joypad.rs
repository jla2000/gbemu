//! Joypad register (`JOYP`/`P1`, 0xFF00) and button state.
//!
//! Real hardware multiplexes 8 buttons onto 4 bus lines (P10-P13) via two
//! active-low select lines (P14 = direction buttons, P15 = action
//! buttons); a game reads the register twice, once per select line, to
//! see all 8 buttons. The joypad interrupt fires on any 1->0 (pressed)
//! transition of a *currently selected* line — which can happen either
//! from a button press while its row is selected, or from selecting a row
//! whose button is already held down. Both are modeled by comparing the
//! selected-lines nibble before/after each state change.

/// Joypad interrupt request bit (bit 4) in `IF`/`IE`.
pub const JOYPAD_INT_BIT: u8 = 1 << 4;

const SELECT_DIRECTIONS: u8 = 1 << 4;
const SELECT_ACTIONS: u8 = 1 << 5;
const SELECT_MASK: u8 = SELECT_DIRECTIONS | SELECT_ACTIONS;
const UNUSED_BITS: u8 = 0b1100_0000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Button {
    Right,
    Left,
    Up,
    Down,
    A,
    B,
    Select,
    Start,
}

#[derive(Debug, Default, Clone)]
pub struct Joypad {
    /// Raw `JOYP` bits 4-5 as last written (0 = that row is selected).
    select_bits: u8,
    right: bool,
    left: bool,
    up: bool,
    down: bool,
    a: bool,
    b: bool,
    select: bool,
    start: bool,
    interrupt_pending: bool,
}

impl Joypad {
    pub fn new() -> Self {
        Self::default()
    }

    /// Bits 6-7 unused (read 1), bits 4-5 the last-written select lines,
    /// bits 0-3 the (active-low) state of whichever row(s) are selected.
    pub fn read_joyp(&self) -> u8 {
        UNUSED_BITS | self.select_bits | self.selected_lines()
    }

    pub fn write_joyp(&mut self, val: u8) {
        let before = self.selected_lines();
        self.select_bits = val & SELECT_MASK;
        self.request_interrupt_on_falling_edge(before, self.selected_lines());
    }

    pub fn set_button(&mut self, button: Button, pressed: bool) {
        let before = self.selected_lines();
        match button {
            Button::Right => self.right = pressed,
            Button::Left => self.left = pressed,
            Button::Up => self.up = pressed,
            Button::Down => self.down = pressed,
            Button::A => self.a = pressed,
            Button::B => self.b = pressed,
            Button::Select => self.select = pressed,
            Button::Start => self.start = pressed,
        }
        self.request_interrupt_on_falling_edge(before, self.selected_lines());
    }

    /// Consumes and returns whether a selected line had a falling
    /// (pressed) edge since the last call.
    pub fn take_interrupt(&mut self) -> bool {
        std::mem::take(&mut self.interrupt_pending)
    }

    fn request_interrupt_on_falling_edge(&mut self, before: u8, after: u8) {
        if before & !after & 0x0F != 0 {
            self.interrupt_pending = true;
        }
    }

    /// Active-low P10-P13 state (0 = pressed) for whichever row(s) are
    /// currently selected; unselected/both-selected rows are OR'd
    /// together, matching how real hardware wires the two rows onto the
    /// same 4 lines.
    fn selected_lines(&self) -> u8 {
        let mut lines = 0x0F;
        if self.select_bits & SELECT_DIRECTIONS == 0 {
            if self.right {
                lines &= !0x01;
            }
            if self.left {
                lines &= !0x02;
            }
            if self.up {
                lines &= !0x04;
            }
            if self.down {
                lines &= !0x08;
            }
        }
        if self.select_bits & SELECT_ACTIONS == 0 {
            if self.a {
                lines &= !0x01;
            }
            if self.b {
                lines &= !0x02;
            }
            if self.select {
                lines &= !0x04;
            }
            if self.start {
                lines &= !0x08;
            }
        }
        lines
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unselected_rows_read_all_lines_high() {
        let mut jp = Joypad::new();
        jp.write_joyp(SELECT_MASK); // neither row selected
        jp.set_button(Button::A, true);
        assert_eq!(jp.read_joyp() & 0x0F, 0x0F);
    }

    #[test]
    fn selecting_directions_reports_direction_buttons_only() {
        let mut jp = Joypad::new();
        jp.set_button(Button::Right, true);
        jp.set_button(Button::A, true); // action row not selected, shouldn't show
        jp.write_joyp(SELECT_ACTIONS); // select_bits: directions row selected (bit4=0)
        assert_eq!(jp.read_joyp() & 0x0F, 0b1110); // bit0 (Right) low
    }

    #[test]
    fn selecting_actions_reports_action_buttons_only() {
        let mut jp = Joypad::new();
        jp.set_button(Button::Start, true);
        jp.set_button(Button::Up, true); // direction row not selected
        jp.write_joyp(SELECT_DIRECTIONS); // actions row selected (bit5=0)
        assert_eq!(jp.read_joyp() & 0x0F, 0b0111); // bit3 (Start) low
    }

    #[test]
    fn button_press_while_row_selected_requests_interrupt() {
        let mut jp = Joypad::new();
        jp.write_joyp(SELECT_ACTIONS); // directions row selected
        assert!(!jp.take_interrupt());
        jp.set_button(Button::Left, true);
        assert!(jp.take_interrupt());
        assert!(!jp.take_interrupt()); // consumed
    }

    #[test]
    fn button_press_while_row_unselected_does_not_request_interrupt() {
        let mut jp = Joypad::new();
        jp.write_joyp(SELECT_MASK); // neither row selected
        jp.set_button(Button::A, true);
        assert!(!jp.take_interrupt());
    }

    #[test]
    fn selecting_a_row_with_a_button_already_held_requests_interrupt() {
        let mut jp = Joypad::new();
        jp.write_joyp(SELECT_MASK); // neither row selected
        jp.set_button(Button::B, true); // no interrupt: not selected yet
        assert!(!jp.take_interrupt());
        jp.write_joyp(SELECT_DIRECTIONS); // now select the action row
        assert!(jp.take_interrupt()); // B's line just fell
    }

    #[test]
    fn release_does_not_request_interrupt() {
        let mut jp = Joypad::new();
        jp.write_joyp(SELECT_ACTIONS);
        jp.set_button(Button::Down, true);
        jp.take_interrupt(); // drain the press's interrupt
        jp.set_button(Button::Down, false);
        assert!(!jp.take_interrupt());
    }
}
