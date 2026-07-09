//! Top-level render entry point: lays out the screen area and debug panels,
//! and draws each frame. Full half-block video widget lands in M2; debugger
//! panels land in M6. For M0 this only draws a placeholder shell.

mod layout;
mod video;

use ratatui::Frame;

use crate::app::App;

pub fn draw(frame: &mut Frame, app: &App) {
    layout::draw(frame, app);
}
