//! Top-level render entry point: lays out the screen area and debug panels,
//! and draws each frame. Debugger panels land in M6.

mod layout;
mod video;

use ratatui::Frame;

use crate::app::App;

pub fn draw(frame: &mut Frame, app: &App) {
    layout::draw(frame, app);
}
